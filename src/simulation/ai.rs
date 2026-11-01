//! Автономное поведение людей (ИИ выживания).
//!
//! Объём этой итерации — «выживание» (решение автора): голод → еда
//! (свой инвентарь → яблоня/зрелая культура → охота), ночь → сбор у костра.
//! Полный набор возможностей (добыча, крафт, строительство) доступен
//! человеку при захвате контроллером — через те же функции `actions`,
//! которыми управляется и захваченное существо.
//!
//! Архитектура: движение и воды-паника — общий FSM существ
//! (`entities.rs`), ИИ только решает, куда идти и что делать, выставляя
//! состояние/цель существа. Решения детерминированы: RNG события сеётся
//! от (seed, id, тик) — `decision_rng`, состояние не сохраняется.

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::combat;
use super::entities::{EntityKind, EntityState, HumanGoal};
use super::hunger::EAT_COOLDOWN_TICKS;
use super::items::MISC_SLOTS;
use super::objects::ObjectKind;
use super::time::{self, TICKS_PER_SEC};
use super::world::{World, WorldPos};

/// Час начала ночи: люди возвращаются к костру.
const NIGHT_START_HOUR: u32 = 21;
/// Час конца ночи: люди расходятся по своим делам.
const NIGHT_END_HOUR: u32 = 6;

/// Радиус поиска костра, тайлов. Небольшой квадрат по замыслу «ходить и
/// искать»: найдя костёр, человек запоминает его в `known_campfire` и
/// больше не сканирует (пока костёр на месте).
const CAMPFIRE_SEARCH_TILES: f32 = 12.0;
/// Радиус поиска еды, тайлов (найденное запоминается в `known_food`).
const FOOD_SEARCH_TILES: f32 = 10.0;

/// Порог голода: ниже человек начинает искать еду.
const HUNGRY_THRESHOLD: f32 = 40.0;
/// Порог поедания из инвентаря: ниже человек ест запасы (с запасом выше
/// порога поиска еды, чтобы утолить голод до её добычи).
const EAT_WHEN_BELOW: f32 = 60.0;
/// Порог пополнения запасов: сытый человек днём может идти за едой впрок,
/// пока сытость не близка к максимуму.
const STOCKPILE_THRESHOLD: f32 = 85.0;

/// Дистанция «дошёл до костра», тайлов (встать в кольце вокруг огня).
const CAMPFIRE_ARRIVE_TILES: f32 = 1.4;
/// Дистанция атаки на охоте, тайлов (руки/нож достают).
const HUNT_ATTACK_TILES: f32 = 1.3;

/// Интервал «обдумывания» ИИ, тиков: переоценка цели и скан окрестностей
/// не чаще раза в 5 секунд (при x1). Между обдумываниями ИИ только
/// исполняет текущую цель — сканы объектов не совершаются.
const THINK_INTERVAL_TICKS: u64 = 5 * TICKS_PER_SEC as u64;

/// Ночь ли сейчас по тику (часы считаются так же, как в календаре).
fn is_night(tick: u64) -> bool {
    let hour = time::hour_of_tick(tick);
    // Диапазон ночи «переходит через полночь»: 21:00..06:00.
    !(NIGHT_END_HOUR..NIGHT_START_HOUR).contains(&hour)
}

impl World {
    /// Продвигает автономное поведение людей на один тик. Вызывается из
    /// `tick_entities` после общего FSM-тикания: ИИ только выставляет
    /// состояния и цели, движение делает общий код существ. Голод тикает
    /// в общем FSM (`EntityStore::update`), смерти от голода — там же.
    ///
    /// `controlled` — захваченное существо: его поведение — контроллер,
    /// ИИ его не трогает.
    pub(crate) fn tick_human_ai(&mut self, controlled: Option<u64>, tick: u64) {
        // Снимок id автономных людей: мутации внутри цикла не ломают обход.
        let ids: Vec<u64> = self
            .entities
            .iter()
            .filter(|e| e.kind == EntityKind::Human && Some(e.id) != controlled)
            .map(|e| e.id)
            .collect();
        if ids.is_empty() {
            return;
        }
        let night = is_night(tick);

        for id in ids {
            // В воде ИИ молчит: работает паника/выход на сушу общего FSM.
            let swimming = self
                .entities
                .get(id)
                .is_some_and(|e| super::actions::terrain_is_water(self, e.pos));
            if swimming {
                continue;
            }
            self.human_tick(id, tick, night);
        }
    }

    /// Один тик поведения человека: исполнение текущей цели, по
    /// «обдумыванию» — её переоценка (ночь, голод, запасы, сбор, брожение).
    fn human_tick(&mut self, id: u64, tick: u64, night: bool) {
        // Голодную еду из инвентаря человек ест при любой цели — это дёшево
        // и не требует сканов. Кулдаун поедания общий с игроком.
        self.try_eat_from_inventory(id, tick);

        let think_due = self
            .entities
            .get(id)
            .and_then(|e| e.mind.as_ref())
            .is_some_and(|m| tick >= m.next_think_tick);
        if think_due {
            if let Some(m) = self.entities.get_mut(id).and_then(|e| e.mind.as_mut()) {
                m.next_think_tick = tick + THINK_INTERVAL_TICKS;
            }
            // Ночь важнее всего: костёр (если известен) перебивает другие
            // цели. Костёр ищется ходьбой и малым сканом: найдя — человек
            // помнит его; если запомненный исчез — забывает.
            if night {
                let have_fire_goal = self
                    .entities
                    .get(id)
                    .and_then(|e| e.mind.as_ref())
                    .and_then(|m| m.goal)
                    .is_some_and(|g| matches!(g, HumanGoal::Campfire(_)));
                if !have_fire_goal
                    && let Some(fire) = self.known_campfire(id)
                {
                    self.set_goal(id, Some(HumanGoal::Campfire(fire)));
                }
            } else {
                // Утром цель «костёр» гаснет — человек возвращается к делам.
                let was_fire = self
                    .entities
                    .get(id)
                    .and_then(|e| e.mind.as_ref())
                    .and_then(|m| m.goal)
                    .is_some_and(|g| matches!(g, HumanGoal::Campfire(_)));
                if was_fire {
                    self.set_goal(id, None);
                }
            }
            // Дневные занятия — только без цели (не перебиваем дорогу к
            // еде, охоту и костру): запас еды, сбор палок, взгляд по сторонам.
            let has_goal = self
                .entities
                .get(id)
                .and_then(|e| e.mind.as_ref())
                .is_some_and(|m| m.goal.is_some());
            if !night && !has_goal {
                self.pick_day_activity(id, tick);
            }
            // Голод: цель-еда, если уже не идём к костру.
            let going_to_fire = self
                .entities
                .get(id)
                .and_then(|e| e.mind.as_ref())
                .and_then(|m| m.goal)
                .is_some_and(|g| matches!(g, HumanGoal::Campfire(_)));
            if !going_to_fire && !night {
                let hungry = self
                    .entities
                    .get(id)
                    .is_some_and(|e| e.hunger.value() < HUNGRY_THRESHOLD);
                let has_food_goal = self
                    .entities
                    .get(id)
                    .and_then(|e| e.mind.as_ref())
                    .and_then(|m| m.goal)
                    .is_some_and(|g| matches!(g, HumanGoal::Food(_) | HumanGoal::Hunt(_)));
                if hungry && !has_food_goal
                    && let Some(goal) = self.known_food_goal(id)
                {
                    self.set_goal(id, Some(goal));
                }
            }
        }

        let Some(goal) = self
            .entities
            .get(id)
            .and_then(|e| e.mind.as_ref())
            .and_then(|m| m.goal)
        else {
            // Без цели — брожение (как у животных, но без лежания).
            self.human_wander(id, tick);
            return;
        };
        match goal {
            HumanGoal::Campfire(fire) => self.tick_campfire_goal(id, fire, tick),
            HumanGoal::Food(tile) => self.tick_food_goal(id, tile),
            HumanGoal::Hunt(victim) => self.tick_hunt_goal(id, victim, tick),
            HumanGoal::Gather(tile) => self.tick_gather_goal(id, tile),
        }
    }

    /// Дневные занятия свободного человека (без цели): пополнить запас еды
    /// впрок, собрать палки с куста или осмотреться. Один бросок RNG на
    /// «обдумывании» — детерминировано от (seed, id, тик). Остальное время
    /// человек стоит или бродит (общее брожение ИИ).
    fn pick_day_activity(&mut self, id: u64, tick: u64) {
        let mut rng = wander_rng(self.seed, id, tick);
        let roll = rng.random::<f32>();
        // Запас еды впрок: та же цель-еда, что и при голоде, но по сытости
        // ещё далеко до порога поиска. Не находится еда — цель не ставится.
        if roll < 0.3 {
            let not_full = self
                .entities
                .get(id)
                .is_some_and(|e| e.hunger.value() < STOCKPILE_THRESHOLD);
            if not_full
                && let Some(goal) = self.known_food_goal(id)
            {
                self.set_goal(id, Some(goal));
                return;
            }
        }
        // Сбор палок с куста: задел под крафт и строительство.
        if roll < 0.5
            && let Some(bush) = self.nearest_bush(id)
        {
            self.set_goal(id, Some(HumanGoal::Gather(bush)));
            return;
        }
        // Осмотреться: повернуться в случайную сторону (живость покоя).
        if roll < 0.7
            && let Some(entity) = self.entities.get_mut(id)
        {
            entity.facing = if rng.random::<f32>() < 0.5 {
                super::entities::Facing::Left
            } else {
                super::entities::Facing::Right
            };
        }
    }

    /// Ставит сущности цель (и сбрасывает устаревший путь, если цель
    /// сменилась на другую).
    fn set_goal(&mut self, id: u64, goal: Option<HumanGoal>) {
        let Some(entity) = self.entities.get_mut(id) else {
            return;
        };
        let changed = entity.mind.as_ref().and_then(|m| m.goal) != goal;
        if let Some(m) = entity.mind.as_mut() {
            m.goal = goal;
        }
        if changed {
            // Новая цель — старый путь больше не имеет смысла: сброс в
            // стояние, следующий тик цели отправит в путь.
            entity.state = EntityState::Standing;
            entity.state_ticks = 0;
            entity.stuck_ticks = 0;
        }
    }

    /// Еда из своего инвентаря: находит слот с едой, выбирает его и ест
    /// с общим кулдауном поедания. Сытый (выше порога) еду не тратит —
    /// запас нужен на дорогу и ночь.
    fn try_eat_from_inventory(&mut self, id: u64, tick: u64) {
        // Сытость читается до заёма ума: существо и ум — разные поля.
        let satiated = self
            .entities
            .get(id)
            .is_some_and(|e| e.hunger.value() >= EAT_WHEN_BELOW);
        let Some(m) = self.entities.get_mut(id).and_then(|e| e.mind.as_mut()) else {
            return;
        };
        if tick < m.eat_ready_tick || satiated {
            return;
        }
        let Some(slot) = m
            .inventory
            .items
            .iter()
            .position(|s| s.is_some_and(|stack| stack.kind.food_value() > 0.0))
        else {
            return;
        };
        m.selected_slot = slot.min(MISC_SLOTS - 1);
        if super::actions::eat_selected(self, id)
            && let Some(m) = self.entities.get_mut(id).and_then(|e| e.mind.as_mut())
        {
            m.eat_ready_tick = tick + EAT_COOLDOWN_TICKS;
        }
    }

    /// Цель «костёр»: дойти и стоять рядом до утра; погасший/убранный
    /// костёр гасит цель и забывается.
    fn tick_campfire_goal(&mut self, id: u64, fire: super::world::TilePos, tick: u64) {
        // Костёр исчез (сгорел дотла, убран редактором) — цель недостижима.
        let fire_exists = self
            .object_at(fire)
            .is_some_and(|o| o.kind == ObjectKind::Building(super::objects::BuildingKind::Campfire));
        if !fire_exists {
            self.set_goal(id, None);
            if let Some(m) = self.entities.get_mut(id).and_then(|e| e.mind.as_mut())
                && m.known_campfire == Some(fire)
            {
                m.known_campfire = None;
            }
            return;
        }
        let center = WorldPos::of_tile_center(fire);
        let Some(entity) = self.entities.get(id) else {
            return;
        };
        let near = entity.pos.dist(center) <= CAMPFIRE_ARRIVE_TILES;
        if near {
            let (pos, standing) = (entity.pos, entity.state == EntityState::Standing);
            // Стоит у огня до утра: длинное стояние (ИИ обновит утром).
            if !standing {
                let Some(entity) = self.entities.get_mut(id) else {
                    return;
                };
                entity.state = EntityState::Standing;
                entity.state_ticks = 0;
                entity.state_duration = TICKS_PER_SEC as u64;
                entity.target = entity.pos;
                entity.stuck_ticks = 0;
            }
            // Стоя у костра, смотрит на огонь, а не куда пришлось.
            if (center.x - pos.x).abs() > f32::EPSILON
                && let Some(entity) = self.entities.get_mut(id)
            {
                entity.facing = if center.x > pos.x {
                    super::entities::Facing::Right
                } else {
                    super::entities::Facing::Left
                };
            }
            return;
        }
        // Ещё в пути: поддерживаем ходьбу (FSM мог закончить состояние).
        if entity.state != EntityState::Walking {
            let ticks_left = ticks_until_morning(tick);
            self.walk_to(id, center, ticks_left.max(TICKS_PER_SEC as u64));
        }
    }

    /// Цель «еда»: дойти до тайла и собрать (яблоня или зрелая культура).
    fn tick_food_goal(&mut self, id: u64, tile: super::world::TilePos) {
        let center = WorldPos::of_tile_center(tile);
        let Some(entity) = self.entities.get(id) else {
            return;
        };
        if entity.pos.dist(center) <= super::actions::INTERACT_REACH {
            // На месте: сначала яблоня, затем культура (порядок не важен —
            // на тайле есть что-то одно). Успех или пусто — цель исполнена,
            // голод сам решит, нужна ли следующая.
            let _ = super::actions::harvest_apple(self, id, tile);
            let _ = super::actions::harvest_crop(self, id, tile);
            // Собранная/исчезнувшая еда из памяти тоже уходит.
            if let Some(m) = self.entities.get_mut(id).and_then(|e| e.mind.as_mut())
                && m.known_food == Some(tile)
            {
                m.known_food = None;
            }
            self.set_goal(id, None);
            return;
        }
        if entity.state != EntityState::Walking {
            self.walk_to(id, center, 20 * TICKS_PER_SEC as u64);
        }
    }

    /// Цель «сбор»: дойти до куста и облупить его (палки в инвентарь,
    /// куст исчезает — он отрастает цепочкой BushGrowth). Куст уже не
    /// куст — цель исполнена или отменена.
    fn tick_gather_goal(&mut self, id: u64, tile: super::world::TilePos) {
        let center = WorldPos::of_tile_center(tile);
        let Some(entity) = self.entities.get(id) else {
            return;
        };
        if entity.pos.dist(center) <= super::actions::INTERACT_REACH {
            let is_bush = self
                .object_at(tile)
                .is_some_and(|o| o.kind == ObjectKind::Bush);
            if is_bush {
                let _ = super::actions::interact(self, id, tile);
            }
            self.set_goal(id, None);
            return;
        }
        if entity.state != EntityState::Walking {
            self.walk_to(id, center, 20 * TICKS_PER_SEC as u64);
        }
    }

    /// Цель «охота»: догнать животное и бить, пока не падёт. Мясо падает
    /// на землю и подбирается автоподбором.
    fn tick_hunt_goal(&mut self, id: u64, victim: u64, tick: u64) {
        let Some(victim_pos) = self.entities.get(victim).map(|e| e.pos) else {
            // Жертва мертва или убрана — охота окончена.
            self.set_goal(id, None);
            return;
        };
        let Some(entity) = self.entities.get(id) else {
            return;
        };
        if entity.pos.dist(victim_pos) <= HUNT_ATTACK_TILES {
            combat::attack_entity(self, id, victim, tick);
            return;
        }
        if entity.state != EntityState::Walking {
            self.walk_to(id, victim_pos, 30 * TICKS_PER_SEC as u64);
        } else {
            // Жертва движется: обновляем точку пути, чтобы не бегать по
            // старым координатам.
            let Some(entity) = self.entities.get_mut(id) else {
                return;
            };
            entity.target = victim_pos;
        }
    }

    /// Брожение без цели: из стояния, когда его срок истёк, выбрать
    /// случайную точку рядом (как у животных, детерминированно).
    fn human_wander(&mut self, id: u64, tick: u64) {
        let Some(entity) = self.entities.get(id) else {
            return;
        };
        if entity.state != EntityState::Standing || entity.state_ticks < entity.state_duration {
            return;
        }
        let seed = self.seed;
        let mut rng = wander_rng(seed, id, tick);
        let side = self.size.tiles() as f32;
        for _ in 0..16 {
            let angle = rng.random::<f32>() * std::f32::consts::TAU;
            let radius = 8.0 * rng.random::<f32>().sqrt();
            let target = WorldPos {
                x: (entity.pos.x + angle.cos() * radius).clamp(0.0, side),
                y: (entity.pos.y + angle.sin() * radius).clamp(0.0, side),
            };
            // Цель — только сухопутный тайл, и по пути не больше пары
            // проб воды (узкая река). Озеро по прямой — цель отбрасывается:
            // человек не «плывёт» в брожении. Не нашли чистую цель — стоим,
            // на следующем обдумывании попробует снова.
            if !super::actions::terrain_at(self, target).is_some_and(|k| k.is_water())
                && path_water_crossings(self, entity.pos, target) <= MAX_WATER_CROSSINGS
            {
                self.walk_to(id, target, 10 * TICKS_PER_SEC as u64);
                return;
            }
        }
    }

    /// Переводит существо в ходьбу к точке с заданным лимитом тиков.
    fn walk_to(&mut self, id: u64, target: WorldPos, duration_ticks: u64) {
        let Some(entity) = self.entities.get_mut(id) else {
            return;
        };
        entity.state = EntityState::Walking;
        entity.state_ticks = 0;
        entity.state_duration = duration_ticks;
        entity.target = target;
        entity.stuck_ticks = 0;
        if (target.x - entity.pos.x).abs() > f32::EPSILON {
            entity.facing = if target.x > entity.pos.x {
                super::entities::Facing::Right
            } else {
                super::entities::Facing::Left
            };
        }
    }

    /// Известный костёр: проверяет память человека (`known_campfire`),
    /// при неактуальности (костёр убран) забывает её и делает малый скан
    /// окрестностей — найдя, запоминает. Логика «ходить и искать»:
    /// бродящий человек время от времени сканирует новый квадрат и
    /// пополняет память, вместо дальних сканов каждый тик.
    fn known_campfire(&mut self, id: u64) -> Option<super::world::TilePos> {
        let remembered = self
            .entities
            .get(id)
            .and_then(|e| e.mind.as_ref())
            .and_then(|m| m.known_campfire);
        if let Some(fire) = remembered {
            let valid = self.object_at(fire)
                .is_some_and(|o| o.kind == ObjectKind::Building(super::objects::BuildingKind::Campfire));
            if valid {
                return Some(fire);
            }
            // Запомненное устарело — забыть.
            if let Some(m) = self.entities.get_mut(id).and_then(|e| e.mind.as_mut()) {
                m.known_campfire = None;
            }
        }
        let found = self.nearest_campfire(id);
        if let Some(fire) = found
            && let Some(m) = self.entities.get_mut(id).and_then(|e| e.mind.as_mut())
        {
            m.known_campfire = Some(fire);
        }
        found
    }

    /// Цель-еда по памяти (`known_food`): неактуальная (собрана, съедена
    /// кем-то) забывается; малый скан окрестностей пополняет память.
    /// В remembered-еде нет — пробует охоту: ближайший волк в радиусе.
    fn known_food_goal(&mut self, id: u64) -> Option<HumanGoal> {
        let remembered = self
            .entities
            .get(id)
            .and_then(|e| e.mind.as_ref())
            .and_then(|m| m.known_food);
        if let Some(tile) = remembered {
            if self.food_still_there(tile) {
                return Some(HumanGoal::Food(tile));
            }
            // Запомненное устарело — забыть.
            if let Some(m) = self.entities.get_mut(id).and_then(|e| e.mind.as_mut()) {
                m.known_food = None;
            }
        }
        if let Some(tile) = self.nearest_food_tile(id) {
            if let Some(m) = self.entities.get_mut(id).and_then(|e| e.mind.as_mut()) {
                m.known_food = Some(tile);
            }
            return Some(HumanGoal::Food(tile));
        }
        self.find_hunt_goal(id)
    }

    /// Еда ли ещё на тайле: яблоня с плодами или зрелая культура.
    fn food_still_there(&self, tile: super::world::TilePos) -> bool {
        match self.object_at(tile) {
            Some(obj) => matches!(obj.kind, ObjectKind::AppleTree) && obj.charge > 0,
            None => self.crop_at(tile).is_some_and(|crop| crop.is_mature()),
        }
    }

    /// Охота: ближайший волк в радиусе поиска еды.
    fn find_hunt_goal(&mut self, id: u64) -> Option<HumanGoal> {
        let origin = self.entities.get(id)?.pos;
        let mut best: Option<(f32, u64)> = None;
        for entity in self.entities.iter() {
            if entity.kind != EntityKind::Wolf {
                continue;
            }
            let dist = origin.dist(entity.pos);
            if dist <= FOOD_SEARCH_TILES && best.is_none_or(|(d, _)| dist < d) {
                best = Some((dist, entity.id));
            }
        }
        best.map(|(_, victim)| HumanGoal::Hunt(victim))
    }

    /// Ближайший костёр в радиусе `CAMPFIRE_SEARCH_TILES` от человека.
    /// Скан идёт по чанкам в квадрате радиуса — только на «обдумывании».
    fn nearest_campfire(&self, id: u64) -> Option<super::world::TilePos> {
        let origin = self.entities.get(id)?.pos;
        let radius = CAMPFIRE_SEARCH_TILES as i32;
        let size = self.size.tiles() as i32;
        let tx0 = (origin.x.floor() as i32).clamp(0, size - 1);
        let ty0 = (origin.y.floor() as i32).clamp(0, size - 1);
        let x0 = (tx0 - radius).max(0);
        let y0 = (ty0 - radius).max(0);
        let x1 = (tx0 + radius).min(size - 1);
        let y1 = (ty0 + radius).min(size - 1);

        let mut best: Option<(f32, super::world::TilePos)> = None;
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let pos = super::world::TilePos { x: tx, y: ty };
                let Some(obj) = self.object_at(pos) else {
                    continue;
                };
                if obj.kind != ObjectKind::Building(super::objects::BuildingKind::Campfire) {
                    continue;
                }
                let dist = origin.dist(WorldPos::of_tile_center(pos));
                if best.is_none_or(|(d, _)| dist < d) {
                    best = Some((dist, pos));
                }
            }
        }
        best.map(|(_, pos)| pos)
    }

    /// Цель-еда по памяти: см. `known_food_goal`. Старый скан-выбор
    /// заменён памятью с малым сканом окрестностей.
    /// Ближайший тайл с готовой едой: яблоня с плодами или зрелая культура.
    /// Скан по чанкам квадрата радиуса `FOOD_SEARCH_TILES`.
    fn nearest_food_tile(&self, id: u64) -> Option<super::world::TilePos> {
        let origin = self.entities.get(id)?.pos;
        let radius = FOOD_SEARCH_TILES as i32;
        let size = self.size.tiles() as i32;
        let tx0 = (origin.x.floor() as i32).clamp(0, size - 1);
        let ty0 = (origin.y.floor() as i32).clamp(0, size - 1);
        let x0 = (tx0 - radius).max(0);
        let y0 = (ty0 - radius).max(0);
        let x1 = (tx0 + radius).min(size - 1);
        let y1 = (ty0 + radius).min(size - 1);

        let mut best: Option<(f32, super::world::TilePos)> = None;
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let pos = super::world::TilePos { x: tx, y: ty };
                let food = match self.object_at(pos) {
                    Some(obj) => {
                        // Яблоня: плоды есть, пока заряд не нулевой.
                        matches!(obj.kind, ObjectKind::AppleTree) && obj.charge > 0
                    }
                    None => self.crop_at(pos).is_some_and(|crop| crop.is_mature()),
                };
                if !food {
                    continue;
                }
                // Еда за озером по прямой не рассматривается: человек в
                // брожении туда не идёт (узкая река — можно).
                if path_water_crossings(self, origin, WorldPos::of_tile_center(pos))
                    > MAX_WATER_CROSSINGS
                {
                    continue;
                }
                let dist = origin.dist(WorldPos::of_tile_center(pos));
                if best.is_none_or(|(d, _)| dist < d) {
                    best = Some((dist, pos));
                }
            }
        }
        best.map(|(_, pos)| pos)
    }
}

/// Тиков до конца ночи (час `NIGHT_END_HOUR`) — лимит ходьбы к костру.
fn ticks_until_morning(tick: u64) -> u64 {
    let hour = time::hour_of_tick(tick);
    let hours_left = if hour >= NIGHT_START_HOUR {
        24 - hour + NIGHT_END_HOUR
    } else {
        NIGHT_END_HOUR - hour
    };
    hours_left as u64 * time::SECS_PER_HOUR / time::GAME_SECS_PER_TICK as u64
}

/// Сколько проб воды допустимо на прямом пути человека: 1–2 — узкая река,
/// переходится вброд; больше — озеро, человек в брожении туда не идёт.
const MAX_WATER_CROSSINGS: usize = 2;

/// Число проб воды на прямой «от—до» (8 проб по отрезку). Для выбора
/// целей брожения и добычи: озеро на пути — повод искать цель с другой
/// стороны или подождать.
fn path_water_crossings(world: &World, from: WorldPos, to: WorldPos) -> usize {
    const SAMPLES: usize = 8;
    let mut crossings = 0;
    for step in 1..=SAMPLES {
        let t = step as f32 / SAMPLES as f32;
        let point = WorldPos {
            x: from.x + (to.x - from.x) * t,
            y: from.y + (to.y - from.y) * t,
        };
        if super::actions::terrain_at(world, point).is_some_and(|kind| kind.is_water()) {
            crossings += 1;
        }
    }
    crossings
}

/// RNG одного решения брожения: состояние не хранится.
fn wander_rng(seed: u64, id: u64, tick: u64) -> StdRng {
    let mut x = seed
        ^ id.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ tick.wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ 0x0D1E_5EED;
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    StdRng::seed_from_u64(x ^ (x >> 31))
}

impl World {
    /// Ближайший куст в радиусе поиска сбора (источник палок). Скан по
    /// квадрату радиуса `FOOD_SEARCH_TILES` — как у поиска еды, только
    /// на «обдумывании».
    fn nearest_bush(&self, id: u64) -> Option<super::world::TilePos> {
        let origin = self.entities.get(id)?.pos;
        let radius = FOOD_SEARCH_TILES as i32;
        let size = self.size.tiles() as i32;
        let tx0 = (origin.x.floor() as i32).clamp(0, size - 1);
        let ty0 = (origin.y.floor() as i32).clamp(0, size - 1);
        let x0 = (tx0 - radius).max(0);
        let y0 = (ty0 - radius).max(0);
        let x1 = (tx0 + radius).min(size - 1);
        let y1 = (ty0 + radius).min(size - 1);

        let mut best: Option<(f32, super::world::TilePos)> = None;
        for ty in y0..=y1 {
            for tx in x0..=x1 {
                let pos = super::world::TilePos { x: tx, y: ty };
                if !self
                    .object_at(pos)
                    .is_some_and(|obj| obj.kind == ObjectKind::Bush)
                {
                    continue;
                }
                // Куст за озером по прямой не рассматривается.
                if path_water_crossings(self, origin, WorldPos::of_tile_center(pos))
                    > MAX_WATER_CROSSINGS
                {
                    continue;
                }
                let dist = origin.dist(WorldPos::of_tile_center(pos));
                if best.is_none_or(|(d, _)| dist < d) {
                    best = Some((dist, pos));
                }
            }
        }
        best.map(|(_, pos)| pos)
    }
}
