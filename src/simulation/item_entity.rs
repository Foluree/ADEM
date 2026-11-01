//! Сущности-предметы на земле: дроп в стиле Minecraft.
//!
//! Параллельный механизм к слою `Chunk.items` (предметы, выложенные
//! игроком на тайлы — тот слой не трогается). Item-сущности живут в
//! непрерывных координатах: дроп с разбросом, слияние близких стеков
//! одного вида, автоподбор игроком при приближении, исчезновение по
//! возрасту. Хранилище — плоский `Vec` (дропа немного, O(n) на тик
//! дёшево); рендер читает только видимые.

use super::items::ItemStack;
use super::world::{World, WorldPos};

/// Радиус автоподбора: игрок подбирает предмет при приближении, тайлов.
pub const PICKUP_RADIUS: f32 = 0.75;
/// Задержка автоподбора после дропа, тиков (иначе предмет подбирается
/// в момент выпадения).
pub const PICKUP_DELAY_TICKS: u64 = 20;
/// Время жизни дропа, тиков (6 минут при 20 тик/с), после — исчезает.
pub const DESPAWN_TICKS: u64 = 7200;
/// Радиус слияния с ближайшим стеком того же вида, тайлов.
pub const MERGE_RADIUS: f32 = 0.8;
/// Задержка автоподбора брошенного предмета (клавиша/перетаскивание за
/// пределы GUI), тиков: предмет не подбирается мгновенно после броска.
pub const THROW_PICKUP_DELAY_TICKS: u64 = 50;
/// Дальность броска предмета игроком, тайлов.
pub const THROW_DISTANCE_TILES: f32 = 3.0;
/// Длительность полёта брошенного предмета: тиков на тайл дистанции
/// (полёт замедляется к цели — квадратичный ease-out).
pub const THROW_FLY_TICKS_PER_TILE: f32 = 8.0;
/// Границы длительности полёта, тиков.
pub const THROW_FLY_MIN_TICKS: u64 = 6;
pub const THROW_FLY_MAX_TICKS: u64 = 60;
/// Разброс позиции при дропе, тайлов (в каждую сторону).
pub const SCATTER_TILES: f32 = 0.3;
/// Радиус поедания мяса волком (как автоподбор — «наступил и съел»), тайлов.
pub const WOLF_EAT_RADIUS: f32 = 0.75;
/// Волк ест, пока сытость не близка к максимуму: упитанный хищник
/// добирает еду до отвала.
pub const WOLF_EAT_BELOW: f32 = super::hunger::HUNGER_MAX - 0.5;
/// Радиус поиска лежащего мяса голодным волком, тайлов.
pub const WOLF_MEAT_SEEK_RADIUS: f32 = 16.0;
/// Порог голода, ниже которого волк ищет мясо, тайлов сытости.
pub const WOLF_SEEK_BELOW: f32 = 60.0;

/// Полёт брошенного предмета: интерполяция от точки броска к цели
/// с замедлением. Транзиентное состояние — в сейв не пишется (предмет
/// сохраняется в текущей точке пути).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ItemFly {
    /// Точка броска.
    pub from: WorldPos,
    /// Точка приземления.
    pub to: WorldPos,
    /// Тик начала полёта.
    pub start_tick: u64,
    /// Полная длительность полёта, тиков.
    pub ticks: u64,
}

/// Сущность-предмет на земле.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ItemEntity {
    /// Уникальный id (для рендера); 0 не выдаётся.
    pub id: u64,
    /// Позиция в тайлах (непрерывная).
    pub pos: WorldPos,
    /// Предмет.
    pub stack: ItemStack,
    /// Тик появления (возраст для despawn).
    pub spawn_tick: u64,
    /// С этого тика разрешён автоподбор.
    pub pickup_after_tick: u64,
    /// Идущий полёт (бросок игроком); `None` — предмет лежит.
    pub fly: Option<ItemFly>,
}

/// Хранилище item-сущностей мира: плоский Vec, id с 1, не переиспользуются.
#[derive(Default, Debug)]
pub struct ItemEntities {
    pub items: Vec<ItemEntity>,
    next_id: u64,
}

impl ItemEntities {
    /// Восстанавливает счётчик id при загрузке сохранения (save.rs).
    pub(crate) fn restore_next_id(&mut self, next_id: u64) {
        self.next_id = next_id;
    }
}

/// Детерминированный бросок от (seed, тик, соль): mini splitmix-хэш,
/// возвращает [0, 1).
fn unit_roll(seed: u64, tick: u64, salt: u64) -> f32 {
    let mut h = seed
        ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ salt.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = h.wrapping_mul(0xD6E8_FEB8_6659_7B93);
    h ^= h >> 32;
    (h >> 40) as f32 / (1u32 << 24) as f32
}

impl ItemEntities {
    /// Число сущностей.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Пуст ли склад.
    #[allow(dead_code)] // читается тестами
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Дроп стека у `pos` с разбросом и слиянием с ближайшим стеком того
    /// же вида. Возвращает id сущности (0 — влит в соседний стек).
    /// Детерминирован: разброс сеется от (seed, тик).
    pub fn drop_stack(&mut self, seed: u64, pos: WorldPos, stack: ItemStack, tick: u64) -> u64 {
        self.drop_stack_delayed(seed, pos, stack, tick, PICKUP_DELAY_TICKS)
    }

    /// Дроп с заданной задержкой автоподбора (бросок предмета игроком).
    pub fn drop_stack_delayed(
        &mut self,
        seed: u64,
        pos: WorldPos,
        stack: ItemStack,
        tick: u64,
        pickup_delay: u64,
    ) -> u64 {
        if stack.count == 0 {
            return 0;
        }
        // Слияние с ближайшим стеком того же вида в радиусе MERGE_RADIUS.
        if let Some(target) = self.items.iter_mut().find(|entity| {
            entity.stack.kind == stack.kind && entity.pos.dist(pos) <= MERGE_RADIUS
        }) {
            target.stack.count += stack.count;
            return 0;
        }

        let id = self.alloc_id();
        // Разброс: два независимых броска по осям, центр — точка дропа.
        let salt = id;
        let dx = (unit_roll(seed, tick, salt) - 0.5) * 2.0 * SCATTER_TILES;
        let dy = (unit_roll(seed, tick, salt ^ 0x5EED_C0DE) - 0.5) * 2.0 * SCATTER_TILES;
        self.items.push(ItemEntity {
            id,
            pos: WorldPos { x: pos.x + dx, y: pos.y + dy },
            stack,
            spawn_tick: tick,
            pickup_after_tick: tick + pickup_delay,
            fly: None,
        });
        id
    }

    /// Бросок стека: предмет летит к цели с замедлением (ease-out) и
    /// лежит с задержкой автоподбора. Длительность полёта — от дистанции.
    /// Без слияния и разброса: траектория задана целью.
    pub fn throw(
        &mut self,
        from: WorldPos,
        stack: ItemStack,
        tick: u64,
        to: WorldPos,
    ) -> u64 {
        if stack.count == 0 {
            return 0;
        }
        let id = self.alloc_id();
        let dist = from.dist(to);
        let ticks = ((dist * THROW_FLY_TICKS_PER_TILE) as u64)
            .clamp(THROW_FLY_MIN_TICKS, THROW_FLY_MAX_TICKS);
        self.items.push(ItemEntity {
            id,
            pos: from,
            stack,
            spawn_tick: tick,
            pickup_after_tick: tick + THROW_PICKUP_DELAY_TICKS,
            fly: Some(ItemFly { from, to, start_tick: tick, ticks }),
        });
        id
    }

    /// Тик дропа без подбора: despawn по возрасту и полёт брошенных.
    /// Автоподбор выполняет `World::tick_item_entities` (ему нужны люди).
    pub fn tick(&mut self, tick: u64) {
        // Возрастные предметы исчезают; порядок Vec сохраняется (детерминизм).
        self.items
            .retain(|entity| entity.spawn_tick + DESPAWN_TICKS > tick);

        // Полёт брошенных предметов: позиция по квадратичному ease-out
        // (быстрый старт, замедление к цели), приземление — точная цель.
        for entity in &mut self.items {
            let Some(fly) = entity.fly else {
                continue;
            };
            let t = ((tick.saturating_sub(fly.start_tick)) as f32 / fly.ticks.max(1) as f32)
                .clamp(0.0, 1.0);
            let ease = 1.0 - (1.0 - t) * (1.0 - t);
            entity.pos = WorldPos {
                x: fly.from.x + (fly.to.x - fly.from.x) * ease,
                y: fly.from.y + (fly.to.y - fly.from.y) * ease,
            };
            if t >= 1.0 {
                entity.pos = fly.to;
                entity.fly = None;
            }
        }
    }

    /// Сущность по id (для рендера).
    pub fn get(&self, id: u64) -> Option<&ItemEntity> {
        self.items.iter().find(|entity| entity.id == id)
    }

    fn alloc_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

impl World {
    /// Полный тик дропа: despawn, полёт, автоподбор людьми и поедание
    /// мяса волками.
    ///
    /// Автоподбор: предмет подбирает ближайший человек в радиусе
    /// `PICKUP_RADIUS` (люди единицы — O(люди) на предмет; за весь тик
    /// O(дроп × едоки)). Если влезает целиком — забирается сущность, иначе
    /// остаётся остаток (инвентарь полон).
    ///
    /// Волк не подбирает в инвентарь: мясо его вида съедается на месте —
    /// одна единица за тик в радиусе `WOLF_EAT_RADIUS` (тот же проход,
    /// без второго скана мира).
    pub fn tick_item_entities(&mut self, tick: u64) {
        self.item_entities.tick(tick);

        // Позиции едоков снимаются один раз за тик: люди (подбор в
        // инвентарь) и голодные волки (поедание мяса с земли).
        let humans: Vec<(u64, super::world::WorldPos)> = self
            .entities
            .iter()
            .filter(|e| e.mind.is_some())
            .map(|e| (e.id, e.pos))
            .collect();
        let wolves: Vec<(u64, super::world::WorldPos)> = self
            .entities
            .iter()
            .filter(|e| e.kind == super::entities::EntityKind::Wolf && e.hunger.value() < WOLF_EAT_BELOW)
            .map(|e| (e.id, e.pos))
            .collect();

        let mut leftover_ids = Vec::new();
        for entity in self.item_entities.items.iter_mut() {
            if tick < entity.pickup_after_tick || entity.fly.is_some() {
                continue;
            }
            let stack = entity.stack;

            // Мясо — еда волков: ближайший голодный волк съедает единицу
            // за тик (глотает на месте, без инвентаря). При равенстве
            // дистанций — меньший id (детерминизм).
            if stack.kind.is_wolf_food()
                && let Some((wolf_id, _)) = wolves
                    .iter()
                    .filter(|(_, pos)| pos.dist(entity.pos) <= WOLF_EAT_RADIUS)
                    .min_by(|a, b| {
                        a.1.dist(entity.pos)
                            .total_cmp(&b.1.dist(entity.pos))
                            .then_with(|| a.0.cmp(&b.0))
                    })
            {
                let Some(wolf) = self.entities.get_mut(*wolf_id) else {
                    continue;
                };
                wolf.hunger.eat(stack.kind.food_value());
                entity.stack.count -= 1;
                if entity.stack.count == 0 {
                    leftover_ids.push(entity.id);
                }
                continue;
            }

            // Ближайший человек в радиусе подбора (при равенстве — меньший id:
            // выбор стабильный и детерминированный).
            let Some((picker, _)) = humans
                .iter()
                .filter(|(_, pos)| pos.dist(entity.pos) <= PICKUP_RADIUS)
                .min_by(|a, b| {
                    a.1.dist(entity.pos)
                        .total_cmp(&b.1.dist(entity.pos))
                        .then_with(|| a.0.cmp(&b.0))
                })
            else {
                continue;
            };
            let Some(m) = self
                .entities
                .get_mut(*picker)
                .and_then(|e| e.mind.as_mut())
            else {
                continue;
            };
            let leftover = m.inventory.add(stack.kind, stack.count);
            if leftover == 0 {
                leftover_ids.push(entity.id);
            } else if leftover < stack.count {
                entity.stack.count = leftover;
                // Пока инвентарь полон, не пинаем предмет каждый тик.
                entity.pickup_after_tick = tick + PICKUP_DELAY_TICKS;
            }
        }
        if !leftover_ids.is_empty() {
            self.item_entities
                .items
                .retain(|entity| !leftover_ids.contains(&entity.id));
        }
    }
}
