//! Действия существа в мире: движение, добыча, посев, еда, строительство.
//!
//! По целевому формату (GAME_DESIGN 4.1, ARCHITECTURE 2.7) игрок — это
//! захваченная сущность мира, а не отдельная структура: интерфейс отдаёт
//! команды управляемому существу, оно исполняет их здесь. Поэтому все
//! действия адресованы `SimEntity` по id и работают только у существ
//! с «умом» (`HumanMind`); движение доступно и неразумным (захват волка).
//! Команды `Command::Player*` и ИИ человека (simulation/ai.rs) вызывают
//! одни и те же функции — правила действия едины для игрока и NPC.

use super::crops::{Crop, CropKind};
use super::entities::{EntityState, HumanMind};
use super::hunger::Activity;
use super::items::{ItemKind, ItemStack, MISC_SLOTS, ToolClass};
use super::movement;
use super::objects::{BuildingKind, ObjectKind, WorldObject};
use super::tile::TileType;
use super::world::{TilePos, World, WorldPos};

/// Радиус взаимодействия с тайлом: от существа до центра тайла, в тайлах.
pub const INTERACT_REACH: f32 = 2.0;

/// Сколько тиков непрерывного зажатия ЛКМ ломают постройку.
pub const BUILDING_BREAK_TICKS: u64 = 80;
/// Разрыв в тиках между ударами, при котором прогресс ломания сбрасывается:
/// команда ломания приходит каждый кадр зажатия, зазор между кадрами —
/// единицы тиков; пауза зажатия дольше — зажатие прервано.
const BUILDING_BREAK_RESET_TICKS: u64 = 5;

/// Сколько пучков травы даёт один травяной тайл: единица, как и у камня.
const GRASS_YIELD_PER_TILE: u32 = 1;

/// Шанс найти странный клубень при вскапывании тайла мотыгой.
const STRANGE_TUBER_CHANCE: f32 = 0.04;

/// Ум существа по id (мутабельно). `None` — существа нет или оно
/// неразумное (волк): действия с инвентарём ему недоступны.
fn mind_mut(world: &mut World, actor: u64) -> Option<&mut HumanMind> {
    world.entities.get_mut(actor)?.mind.as_mut()
}

/// Ум существа по id (только чтение).
pub fn mind(world: &World, actor: u64) -> Option<&HumanMind> {
    world.entities.get(actor)?.mind.as_ref()
}

/// Предмет в выбранном слоте хотбара существа.
pub fn selected_item(world: &World, actor: u64) -> Option<ItemKind> {
    mind(world, actor).and_then(|m| m.selected_item())
}

/// Позиция существа; `None` — существа с таким id нет.
pub fn pos_of(world: &World, actor: u64) -> Option<WorldPos> {
    Some(world.entities.get(actor)?.pos)
}

/// Тайл в пределах мира и радиуса взаимодействия от существа. Публичная
/// функция: правило «радиус как у добычи» применяется ко всем действиям
/// (выкладывание предметов, открытие станций, строительство).
pub fn reachable(world: &World, actor: u64, pos: TilePos) -> bool {
    let size = world.size.tiles() as i32;
    if pos.x < 0 || pos.y < 0 || pos.x >= size || pos.y >= size {
        return false;
    }
    let Some(from) = pos_of(world, actor) else {
        return false;
    };
    let center = WorldPos::of_tile_center(pos);
    let dx = center.x - from.x;
    let dy = center.y - from.y;
    (dx * dx + dy * dy).sqrt() <= INTERACT_REACH
}

/// Вид рельефа под позицией. `None` — вне мира (считается сушей:
/// clamp в движении не даёт выйти за границы, а для тестов это честный
/// «не вода»).
pub fn terrain_at(world: &World, pos: WorldPos) -> Option<TileType> {
    let size = world.size.tiles() as i32;
    let tx = pos.x.floor() as i32;
    let ty = pos.y.floor() as i32;
    if tx < 0 || ty < 0 || tx >= size || ty >= size {
        return None;
    }
    Some(world.terrain_kind(TilePos { x: tx, y: ty }))
}

/// Проходим ли тайл для движения. Глубокая вода и океан не проходимы —
/// ни с суши, ни вплавь с мелководья. Исключение — существо, уже стоящее
/// в глубине («бог» перетащило его в океан): движение позволяет выбраться
/// на берег. Вне мира — суша.
fn passable(world: &World, pos: WorldPos, on_deep_water: bool) -> bool {
    !matches!(
        terrain_at(world, pos),
        Some(TileType::DeepWater | TileType::Ocean) if !on_deep_water
    )
}

/// Шаг движения существа за один тик по направлению ввода контроллера.
/// Направление `dir` — экранные оси (X вправо, Y вверх); нормализуется,
/// чтобы диагональ не была быстрее. Работает и для неразумных (захваченный
/// волк идёт на своей скорости вида).
///
/// По осям движение проверяется отдельно: упираясь в глубокую воду по
/// одной оси, существо скользит вдоль берега по другой. Скорость — по
/// общим правилам мира (`simulation/movement.rs`).
///
/// Контроллер ведёт и внешний вид существа: взгляд по горизонтальной оси
/// ввода и состояния Walking/Standing (FSM захваченного не тикает — см.
/// `EntityStore::update`), чтобы спрайт зеркалился и анимировался.
pub fn step_direction(world: &mut World, actor: u64, dir: (f32, f32)) {
    let len = (dir.0 * dir.0 + dir.1 * dir.1).sqrt();
    let Some(entity) = world.entities.get_mut(actor) else {
        return;
    };
    if len <= f32::EPSILON {
        // Стоим на месте: сегмент интерполяции схлопывается, иначе спрайт
        // каждый тик «доезжал» бы последний сделанный шаг.
        entity.prev_pos = entity.pos;
        // Шаг был, ввод кончился — переход в стояние (анимация покоя).
        if entity.state == EntityState::Walking {
            entity.state = EntityState::Standing;
            entity.state_ticks = 0;
            entity.stuck_ticks = 0;
        }
        return;
    }
    let pos = entity.pos;
    let base_speed = entity.walk_speed();

    // Взгляд следует за горизонтальной осью ввода (в осях сетки), как у
    // ходьбы FSM: текстуры нарисованы смотрящими вправо, влево зеркалятся.
    if dir.0.abs() > f32::EPSILON {
        entity.facing = if dir.0 > 0.0 {
            super::entities::Facing::Right
        } else {
            super::entities::Facing::Left
        };
    }
    // Движение по вводу — ходьба без срока давности: пока контроллер
    // ведёт, существо идёт (FSM захваченного состояние не меняет).
    if entity.state != EntityState::Walking {
        entity.state = EntityState::Walking;
        entity.state_ticks = 0;
        entity.state_duration = u64::MAX;
    }
    // Кадры анимации ходьбы листаются общим `walk_frame` (по state_ticks).
    entity.state_ticks += 1;
    entity.stuck_ticks = 0;

    // Запоминаем позицию до шага: интерполяция рендера идёт между
    // прошлым и текущим тиком.
    entity.prev_pos = entity.pos;

    let max = world.size.tiles() as f32;
    let on_deep = matches!(
        terrain_at(world, pos),
        Some(TileType::DeepWater | TileType::Ocean)
    );
    let speed = movement::effective_walk_speed(base_speed, terrain_at(world, pos));
    let dx = dir.0 / len * speed;
    let dy = dir.1 / len * speed;

    let try_x = WorldPos {
        x: (pos.x + dx).clamp(0.0, max),
        y: pos.y,
    };
    if passable(world, try_x, on_deep)
        && let Some(entity) = world.entities.get_mut(actor)
    {
        entity.pos = try_x;
    }
    // Шаг по Y идёт от уже сдвинутой по X позиции: иначе проход по Y
    // (в том числе нулевой) затирал бы сделанный шаг по X.
    let current_x = world
        .entities
        .get(actor)
        .map(|entity| entity.pos.x)
        .unwrap_or(pos.x);
    let try_y = WorldPos {
        x: current_x,
        y: (pos.y + dy).clamp(0.0, max),
    };
    if passable(world, try_y, on_deep)
        && let Some(entity) = world.entities.get_mut(actor)
    {
        entity.pos = try_y;
    }
}

/// Тик потребности голода управляемого существа: списывает сытость по
/// активности и применяет урон от голода к здоровью. Вызывается app-слоем
/// на каждом тике для захваченного существа (автономные существа тикают
/// голод в общем FSM, `EntityStore::update`). Голод есть у всех видов —
/// захваченный волк тоже голодает. `moving` — двигалось ли существо.
/// `false` — существо погибло от голода.
pub fn tick_hunger(world: &mut World, actor: u64, moving: bool) -> bool {
    let swimming = matches!(
        pos_of(world, actor).and_then(|pos| terrain_at(world, pos)),
        Some(TileType::Water | TileType::DeepWater | TileType::Ocean)
    );
    let activity = if swimming {
        Activity::Swimming
    } else if moving {
        Activity::Walking
    } else {
        Activity::Idle
    };
    let Some(entity) = world.entities.get_mut(actor) else {
        // Несуществующий актор не «умирает»: проверка существования
        // остаётся за вызывающей стороной.
        return true;
    };
    let damage = entity.hunger.tick(activity);
    if damage > 0.0 {
        entity.hp -= damage;
    }
    // Сытый организм подлечивается (полное HP — скип внутри).
    let kind = entity.kind;
    entity.hunger.regen(&mut entity.hp, kind.max_hp());
    entity.hp > 0.0
}

/// Съесть одну единицу предмета из выбранного слота хотбара: восстанавливает
/// сытость на питательность (`ItemKind::food_value`). `false` — в руке не
/// еда. Есть можно и при полной сытости: излишек пропадает. Кулдаун
/// поедания проверяет вызывающая сторона (команда `PlayerEat`).
pub fn eat_selected(world: &mut World, actor: u64) -> bool {
    // Слота хотбара нет без «ума»: неразумное существо не ест из
    // инвентаря (еда для волка — будущая механика).
    let Some(m) = mind_mut(world, actor) else {
        return false;
    };
    let slot = m.selected_slot.min(MISC_SLOTS - 1);
    let Some(stack) = m.inventory.items[slot] else {
        return false;
    };
    let food = stack.kind.food_value();
    if food <= 0.0 {
        return false;
    }
    if stack.count == 1 {
        m.inventory.items[slot] = None;
    } else {
        m.inventory.items[slot] = Some(ItemStack { count: stack.count - 1, ..stack });
    }
    // Сытость — свойство существа, не ума: восстанавливаем напрямую.
    if let Some(entity) = world.entities.get_mut(actor) {
        entity.hunger.eat(food);
    }
    true
}

/// Взаимодействие с тайлом (добыча). `false` — действие невозможно:
/// тайл вне мира, дальше радиуса, объект руками не добывается или
/// в инвентаре нет места.
///
/// Порядок: объект на тайле (куст, валун) добывается как раньше;
/// зрелая культура на пашне убирается; мотыга в руке пашет траву/землю
/// в пашню; пустой травяной тайл даёт пучок травы пустой рукой или ножом.
pub fn interact(world: &mut World, actor: u64, pos: TilePos) -> bool {
    if !reachable(world, actor, pos) {
        return false;
    }
    if let Some(obj) = world.object_at(pos).copied() {
        return harvest_object(world, actor, pos, obj);
    }
    if world.crop_at(pos).is_some() {
        return harvest_crop(world, actor, pos);
    }
    if selected_item(world, actor).is_some_and(|kind| kind.tool_class() == Some(ToolClass::Hoe)) {
        return plow(world, actor, pos);
    }
    gather_grass(world, actor, pos)
}

/// Уборка зрелой культуры: урожай в инвентарь, пашня остаётся.
/// Незрелая культура не убирается. `false` — культуры нет, она ещё
/// растёт или в инвентаре нет места (тогда культура не теряется).
/// Публично: ИИ человека добывает еду с грядок тем же действием.
pub fn harvest_crop(world: &mut World, actor: u64, pos: TilePos) -> bool {
    let Some(crop) = world.crop_at(pos).copied() else {
        return false;
    };
    if !crop.is_mature() {
        return false;
    }
    let yields = crop.kind.harvest_yield();
    // Место проверяется до уборки: урожай не исчезает «в никуда».
    let fits = mind(world, actor).is_some_and(|m| {
        yields.iter().all(|(kind, count)| {
            *count == 0 || m.inventory.can_fit(*kind, *count)
        })
    });
    if !fits {
        return false;
    }
    for (kind, count) in yields {
        if count > 0
            && let Some(m) = mind_mut(world, actor)
        {
            m.inventory.add(kind, count);
        }
    }
    world.set_crop(pos, None);
    true
}

/// Посев культуры в пашню предметом из выбранного слота хотбара
/// (семена пшеницы/свёклы, корнеплоды, вишня). Списывает один предмет,
/// кладёт всходы. Задачу роста ставит команда `PlayerSow` — ей доступен
/// планировщик.
pub fn sow(world: &mut World, actor: u64, pos: TilePos) -> bool {
    let Some(crop_kind) = selected_item(world, actor).and_then(CropKind::from_seed_item) else {
        return false;
    };
    if !reachable(world, actor, pos) {
        return false;
    }
    // Пашня без объекта, культуры и лежащих предметов.
    if world.terrain_kind(pos) != TileType::Farmland
        || world.object_at(pos).is_some()
        || world.crop_at(pos).is_some()
        || world.item_at(pos).is_some()
    {
        return false;
    }
    let Some(m) = mind_mut(world, actor) else {
        return false;
    };
    let slot = m.selected_slot.min(MISC_SLOTS - 1);
    let Some(stack) = m.inventory.items[slot] else {
        return false;
    };
    if stack.count == 1 {
        m.inventory.items[slot] = None;
    } else {
        m.inventory.items[slot] = Some(ItemStack { count: stack.count - 1, ..stack });
    }
    world.set_crop(pos, Some(Crop::planted(crop_kind)));
    true
}

/// Пахота мотыгой: трава, лесная трава, земля или мох становятся пашней.
/// Тайл с объектом сюда не попадает (перехвачен `interact`). Копание
/// с редким шансом выкапывает странный клубень. `false` — тайл не пахотный.
fn plow(world: &mut World, actor: u64, pos: TilePos) -> bool {
    if !matches!(
        world.terrain_kind(pos),
        TileType::Grass | TileType::ForestGrass | TileType::Dirt | TileType::Moss
    ) {
        return false;
    }
    world.set_terrain(pos, TileType::Farmland);
    if dig_roll(world.seed, pos) < STRANGE_TUBER_CHANCE
        && let Some(m) = mind_mut(world, actor)
        && m.inventory.can_fit(ItemKind::StrangeTuber, 1)
    {
        m.inventory.add(ItemKind::StrangeTuber, 1);
    }
    true
}

/// Сбор яблок с яблони (ПКМ). За одно действие дерево отдаёт одно яблоко,
/// как валун — один камень. Последнее яблоко оставляет дерево пустым
/// навсегда: `charge = 0` и текстура `apple_tree_empty`. `false` — не
/// яблоня, нет плодов, далеко или в инвентаре нет места.
pub fn harvest_apple(world: &mut World, actor: u64, pos: TilePos) -> bool {
    if !reachable(world, actor, pos) {
        return false;
    }
    let Some(obj) = world.object_at(pos).copied() else {
        return false;
    };
    let Some(amount) = obj.apple_yield() else {
        return false;
    };
    let fits = mind(world, actor).is_some_and(|m| m.inventory.can_fit(ItemKind::Apple, 1));
    if !fits {
        return false;
    }
    if let Some(m) = mind_mut(world, actor) {
        m.inventory.add(ItemKind::Apple, 1);
    }

    // Плодов на дереве на единицу меньше; пустая яблоня остаётся объектом.
    let mut tree = obj;
    tree.charge = amount - 1;
    world.set_object(pos, Some(tree));
    true
}

/// Добыча объекта над тайлом (куст, валун). Отдельная функция, чтобы
/// `interact` оставался коротким и не смешивал правила объектов и травы.
fn harvest_object(world: &mut World, actor: u64, pos: TilePos, obj: WorldObject) -> bool {
    let Some((resource, amount)) = obj.harvest_yield() else {
        return false;
    };
    // Добыча камня требует класс pickaxe (любая кирка) или по-старому
    // палку/камень в руке (выбранный слот хотбара).
    if obj.kind == ObjectKind::Rock {
        let selected = mind(world, actor)
            .and_then(|m| m.selected_item());
        let ok = matches!(
            selected,
            Some(ItemKind::Stick) | Some(ItemKind::Rock)
        ) || selected
            .and_then(|kind| kind.tool_class())
            .is_some_and(|class| class == ToolClass::Pickaxe);
        if !ok {
            return false;
        }
    }
    let Some(item) = ItemKind::from_resource(resource) else {
        return false;
    };
    let fits = mind(world, actor)
        .is_some_and(|m| m.inventory.can_fit(item, u32::from(amount)));
    if !fits {
        return false;
    }
    if let Some(m) = mind_mut(world, actor) {
        m.inventory.add(item, u32::from(amount));
    }

    // Применяем добычу к объекту: куст ломается, у валуна падает запас.
    match obj.kind {
        ObjectKind::Bush => world.set_object(pos, None),
        ObjectKind::Rock => {
            let mut rock = obj;
            rock.charge = rock.charge.saturating_sub(amount);
            world.set_object(pos, (rock.charge > 0).then_some(rock));
        }
        // Остальные объекты руками не добываются (harvest_yield = None).
        _ => return false,
    }
    true
}

/// Сбор травы пустой рукой или ножом: тайл даёт один пучок и превращается
/// в землю. Другие предметы в руке траву не берут.
fn gather_grass(world: &mut World, actor: u64, pos: TilePos) -> bool {
    if !matches!(
        world.terrain_kind(pos),
        TileType::Grass | TileType::ForestGrass
    ) {
        return false;
    }
    let selected = mind(world, actor).and_then(|m| m.selected_item());
    let allowed = matches!(selected, None | Some(ItemKind::Knife));
    if !allowed {
        return false;
    }
    let amount = GRASS_YIELD_PER_TILE;
    let fits = mind(world, actor)
        .is_some_and(|m| m.inventory.can_fit(ItemKind::GrassBunch, amount));
    if !fits {
        return false;
    }
    if let Some(m) = mind_mut(world, actor) {
        m.inventory.add(ItemKind::GrassBunch, amount);
    }
    world.set_terrain(pos, TileType::Dirt);
    true
}

/// Выкладывает весь стек из выбранного слота на тайл. `false` — тайл вне
/// мира, далеко или занят; стек остаётся в инвентаре.
pub fn drop_item(world: &mut World, actor: u64, pos: TilePos) -> bool {
    if !in_bounds(world, pos) {
        // вне мира — пропускаем.
        return false;
    }
    if !reachable(world, actor, pos) {
        // Выкладывать можно только в радиусе взаимодействия (как для
        // добычи): «телепорт» предметов на дальние тайлы невозможен.
        return false;
    }
    let stack = mind_mut(world, actor)
        .and_then(|m| {
            let slot = m.selected_slot.min(MISC_SLOTS - 1);
            let stack = m.inventory.items[slot];
            if stack.is_some() {
                m.inventory.items[slot] = None;
            }
            stack
        });
    let Some(stack) = stack else {
        return false;
    };
    if super::commands::try_place_item(world, pos, stack) {
        true
    } else {
        // Не поместилось: возвращаем стек в инвентарь.
        if let Some(m) = mind_mut(world, actor) {
            let slot = m.selected_slot.min(MISC_SLOTS - 1);
            m.inventory.items[slot] = Some(stack);
        }
        false
    }
}

/// Подбирает лежащую стопку предметов с тайла в инвентарь. `false` —
/// далеко, не влезает или подбирать нечего.
pub fn pickup_item(world: &mut World, actor: u64, pos: TilePos) -> bool {
    if !in_bounds(world, pos) {
        return false;
    }
    let Some(stack) = world.item_at(pos).copied() else {
        return false;
    };
    if !reachable(world, actor, pos) {
        // Подобрать можно только в радиусе взаимодействия, как и всё
        // остальное: «телепорт» стека с дальнего края экрана запрещён.
        return false;
    }
    let fits = mind(world, actor)
        .is_some_and(|m| m.inventory.can_fit(stack.kind, stack.count));
    if !fits {
        return false;
    }
    if let Some(m) = mind_mut(world, actor) {
        m.inventory.add(stack.kind, stack.count);
    }
    world.set_item(pos, None);
    true
}

/// Строит постройку на тайле: проверяет место и радиус, списывает плату.
/// Контейнер станции создаётся вместе с постройкой. `false` — нет цены,
/// места, далеко или не хватает предметов.
pub fn build(world: &mut World, actor: u64, pos: TilePos, kind: BuildingKind) -> bool {
    let Some((item, count)) = super::objects::build_cost(kind) else {
        // У постройки нет цены (строятся редактором).
        return false;
    };
    if !reachable(world, actor, pos) || !super::commands::buildable_at(world, pos) {
        return false;
    }
    // Платим только после проверки места: постройка не может «съесть»
    // предметы и не появиться.
    let paid = mind_mut(world, actor)
        .is_some_and(|m| m.inventory.take_items(item, count));
    if !paid {
        return false;
    }
    if world.crop_at(pos).is_some() {
        world.set_crop(pos, None);
    }
    world.set_object_with_container(pos, Some(WorldObject::building(kind)));
    true
}

/// Ломание постройки зажатием ЛКМ (любым предметом, включая лук —
/// «разбирать» можно чем угодно). Ввод шлёт команду каждый кадр зажатия;
/// прогресс копит тики непрерывного зажатия (зазор больше
/// `BUILDING_BREAK_RESET_TICKS` — зажатие прервано, счёт с нуля).
/// Набралось `BUILDING_BREAK_TICKS` — постройка разрушается: на землю
/// падают предметы цены (если задана) и всё содержимое контейнера станции.
/// `false` — ломать нечего, далеко или прогресс ещё не набран.
pub fn break_building(world: &mut World, actor: u64, pos: TilePos, tick: u64) -> bool {
    let Some(obj) = world.object_at(pos) else {
        return false;
    };
    let ObjectKind::Building(kind) = obj.kind else {
        return false;
    };
    if !reachable(world, actor, pos) {
        return false;
    }

    // Непрерывность зажатия: накопленное живёт, пока удары идут подряд.
    let progress = match world.building_breaks.get_mut(&pos) {
        Some(break_state) if tick - break_state.last_tick <= BUILDING_BREAK_RESET_TICKS => {
            break_state.progress as u64 + (tick - break_state.last_tick)
        }
        _ => 0,
    };
    if progress < BUILDING_BREAK_TICKS {
        world.building_breaks.insert(
            pos,
            super::world::BuildingBreak {
                progress: progress.min(u16::MAX as u64) as u16,
                last_tick: tick,
            },
        );
        return false;
    }
    world.building_breaks.remove(&pos);

    // Дроп: цена постройки (если задана) и содержимое контейнера.
    // Стеки снимаются до удаления объекта: контейнер исчезает вместе с ним.
    let center = WorldPos::of_tile_center(pos);
    let contents: Vec<ItemStack> = world
        .container_at(pos)
        .map(|c| c.slots.iter().flatten().copied().collect())
        .unwrap_or_default();
    if let Some((item, count)) = super::objects::build_cost(kind) {
        world
            .item_entities
            .drop_stack(world.seed, center, ItemStack { kind: item, count }, tick);
    }
    for stack in contents {
        world.item_entities.drop_stack(world.seed, center, stack, tick);
    }
    world.set_object_with_container(pos, None);
    true
}

/// Бросает один предмет из выбранного слота в сторону тайла (бинд
/// «Бросить предмет»). Предмет «пролетает» до `THROW_DISTANCE_TILES`.
pub fn throw_selected(world: &mut World, actor: u64, target: TilePos, tick: u64) {
    let Some(origin) = pos_of(world, actor) else {
        return;
    };
    let stack = mind_mut(world, actor)
        .and_then(|m| {
            let slot = m.selected_slot.min(MISC_SLOTS - 1);
            let stack = m.inventory.items[slot];
            // Бросается один предмет; слот опустевает полностью.
            if let Some(s) = stack {
                m.inventory.items[slot] =
                    (s.count > 1).then_some(ItemStack { count: s.count - 1, ..s });
            }
            stack
        });
    let Some(stack) = stack else {
        return;
    };
    throw_items(world, origin, ItemStack { kind: stack.kind, count: 1 }, target, tick);
}

/// Бросает весь стек из слота misc в сторону тайла (перетаскивание за
/// пределы окна GUI). Стек списывается из слота.
pub fn throw_stack(world: &mut World, actor: u64, slot: u8, target: TilePos, tick: u64) {
    let Some(origin) = pos_of(world, actor) else {
        return;
    };
    let slot = usize::from(slot);
    if slot >= MISC_SLOTS {
        return;
    }
    let stack = mind_mut(world, actor).and_then(|m| {
        let stack = m.inventory.items[slot];
        m.inventory.items[slot] = None;
        stack
    });
    let Some(stack) = stack else {
        return;
    };
    throw_items(world, origin, stack, target, tick);
}

/// Бросок стека предметов в сторону тайла: предмет «пролетает» до
/// `THROW_DISTANCE_TILES` (или до цели, если она ближе) и остаётся лежать
/// дроп-сущностью с задержкой автоподбора. Позиция зажимается в мир.
pub fn throw_items(world: &mut World, from: WorldPos, stack: ItemStack, target: TilePos, tick: u64) {
    let aim = WorldPos::of_tile_center(target);
    let (dx, dy) = (aim.x - from.x, aim.y - from.y);
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < f32::EPSILON {
        // Цель под ногами: направление неизвестно — бросок не имеет смысла.
        return;
    }
    let fly = dist.min(super::item_entity::THROW_DISTANCE_TILES);
    let side = world.size.tiles() as f32;
    let landing = WorldPos {
        x: (from.x + dx / dist * fly).clamp(0.0, side),
        y: (from.y + dy / dist * fly).clamp(0.0, side),
    };
    world.item_entities.throw(from, stack, tick, landing);
}

/// Вода ли под позицией (любая глубина). Для ИИ и общего FSM.
pub fn terrain_is_water(world: &World, pos: WorldPos) -> bool {
    matches!(
        terrain_at(world, pos),
        Some(TileType::Water | TileType::DeepWater | TileType::Ocean)
    )
}

/// Лежит ли тайл внутри мира.
fn in_bounds(world: &World, pos: TilePos) -> bool {
    let size = world.size.tiles() as i32;
    pos.x >= 0 && pos.y >= 0 && pos.x < size && pos.y < size
}

/// Детерминированный бросок [0, 1) от (seed, координаты тайла). Пахота
/// случается с тайлом один раз, поэтому тик не нужен: бросок стабильный
/// и воспроизводимый после загрузки сохранения.
pub(crate) fn dig_roll(seed: u64, pos: TilePos) -> f32 {
    let mut h = seed
        ^ (pos.x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (pos.y as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = h.wrapping_mul(0xD6E8_FEB8_6659_7B93);
    h ^= h >> 32;
    (h >> 40) as f32 / (1u32 << 24) as f32
}
