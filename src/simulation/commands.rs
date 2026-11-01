//! Команды контроллеров (UI, игрок, «бог», в будущем ИИ) — единый тип
//! для всех источников (GAME_DESIGN 3.1, ARCHITECTURE 2.6). UI не меняет
//! мир напрямую: он ставит команду в очередь, очередь применяется по порядку.

use std::collections::VecDeque;

use super::actions;
use super::combat;
use super::containers;
use super::crafts::CraftBook;
use super::crops::CropKind;
use super::dirty::DirtyTiles;
use super::entities::{EntityKind, HumanMind};
use super::items::{ItemKind, SlotRef, ItemStack, MAX_STACK, MISC_SLOTS};
use super::objects::{BuildingKind, WorldObject};
use super::scheduler::{Scheduler, TaskKind};
use super::tile::TileType;
use super::vegetation;
use super::world::{TilePos, World, WorldPos, split_tile};

/// Команда контроллера. Значения — данные, детерминированные; для
/// повторяемости порядок применения = порядок постановки в очередь.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Command {
    /// Инструмент «бога»: сменить рельеф тайла.
    SetTerrain { pos: TilePos, kind: TileType },
    /// Инструмент «бога»: поставить объект на тайл (None — убрать).
    SetObject { pos: TilePos, object: Option<WorldObject> },
    /// Инструмент «бога»: выпустить существо на тайл.
    SpawnEntity { kind: EntityKind, pos: TilePos },
    /// Инструмент «бога»: убрать существо по id (ПКМ в редакторе).
    /// Id, а не тайл: на одном тайле может стоять несколько существ,
    /// и удалять нужно то, по которому кликнули.
    RemoveEntity { id: u64 },
    /// Инструмент «бога»: перенести существо в точку (перетаскивание
    /// колёсиком в редакторе). Позиция непрерывная — существо следует за
    /// курсором, а не прыгает по центрам тайлов.
    MoveEntity { id: u64, pos: WorldPos },
    /// Действие игрока: взаимодействие с тайлом (удар по существу, если
    /// оно под прицелом, иначе добыча).
    PlayerInteract { pos: TilePos },
    /// Действие игрока: укус захваченного неразумного существа (волк) по
    /// тайлу под курсором — ближайшее существо в радиусе попадания и
    /// досягаемости укуса. Только бой: ломать и добывать волк не умеет.
    PlayerBite { pos: TilePos },
    /// Действие игрока: собрать яблоки с яблони (ПКМ).
    PlayerHarvestApple { pos: TilePos },
    /// Действие игрока: положить предмет из руки на тайл.
    PlayerDropItem { pos: TilePos },
    /// Действие игрока: положить конкретный стек на землю (остатки сетки
    /// крафта при закрытии инвентаря). Указанный тайл занят — стек ищет
    /// место на соседних тайлах.
    PlayerDropStack { pos: TilePos, stack: ItemStack },
    /// Действие игрока: подобрать лежащую стопку предметов с тайла.
    PlayerPickupItem { pos: TilePos },
    /// Действие игрока: бросить один предмет из выбранного слота в сторону
    /// тайла (бинд «Бросить предмет»). Предмет «пролетает» до
    /// `THROW_DISTANCE_TILES` и лежит с задержкой автоподбора.
    PlayerThrowItem { target: TilePos },
    /// Действие UI: бросить весь стек из слота misc игрока в сторону тайла
    /// (перетаскивание за пределы окна GUI). Стек списывается из слота.
    PlayerThrowStack { target: TilePos, slot: u8 },
    /// Действие игрока: построить постройку на тайле (меню строительства).
    /// Плата списывается симуляцией; вид, не являющийся постройкой,
    /// просто игнорируется.
    PlayerBuild { pos: TilePos, kind: BuildingKind },
    /// Действие игрока: бросить копьё из выбранного слота в сторону тайла.
    /// Дальность ограничена (см. `combat`); копьё списывается при запуске.
    PlayerThrowSpear { target: TilePos },
    /// Действие игрока: выстрелить из лука в сторону тайла. Тратит одну
    /// стрелу из инвентаря; без стрел выстрела нет.
    PlayerShootBow { target: TilePos },
    /// Действие игрока: посеять культуру в пашню предметом из выбранного
    /// слота (семена, корнеплоды, вишня). Не сажаемый предмет в руке или
    /// непашня — команда игнорируется.
    PlayerSow { pos: TilePos },
    /// Действие игрока: удар по постройке при зажатой ЛКМ. Команда уходит
    /// каждый кадр зажатия; тики непрерывности и разрушение считает
    /// `actions::break_building` (постройка рушится на 80-м тике зажатия).
    PlayerBreakBuilding { pos: TilePos },
    /// Действие игрока/UI: перенос стека между слотами станции и misc
    /// игрока. Адреса: (0xFF, index) — misc игрока, (0..=N, index) — слот
    /// контейнера станции. `count = 0` — весь стак. Станции с сеткой
    /// (верстак, кузнечный стол) контейнера не имеют — их сетка временная
    /// и живёт в UI.
    StationMoveItem {
        pos: TilePos,
        from: (u8, u8),
        to: (u8, u8),
        count: u8,
    },
    /// Действие игрока: съесть предмет из выбранного слота хотбара
    /// (ПКМ с едой в руке). Не еда в руке — команда игнорируется.
    PlayerEat,
    /// Инструмент «бога» (окно инспектора): перенос стека внутри
    /// инвентаря существа. Правила слотов — `Inventory::transfer`
    /// (слияние/обмен/мусорка).
    EntityMoveItem { id: u64, from: SlotRef, to: SlotRef },
    /// Инструмент «бога» (окно инспектора): выбросить стек из слота
    /// существа на землю под ним (дроп-сущностью с разбросом).
    EntityDropItem { id: u64, from: SlotRef },
}

/// Очередь команд FIFO. Применяется app-слоем каждый кадр; порядок
/// поступления сохраняется (детерминизм, ARCHITECTURE 2.6/2.8).
#[derive(Default)]
pub struct CommandQueue {
    queue: VecDeque<Command>,
}

impl CommandQueue {
    /// Ставит команду в конец очереди.
    pub fn push(&mut self, command: Command) {
        self.queue.push_back(command);
    }
    /// Пуста ли очередь.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Применяет все команды по порядку поступления и возвращает их число.
    ///
    /// Команды `Player*` — действия контроллера: они исполняются сущностью
    /// `actor` (захваченным существом, GAME_DESIGN 4.1). `actor = None`
    /// (наблюдатель без захвата) — действия игрока пропускаются, команды
    /// «бога» работают как всегда.
    ///
    /// `dirty` наполняется позициями изменённых тайлов с разделением по
    /// каналу (рельеф / декор) — рендер по ним пересобирает меш блока или
    /// только дочерние спрайты. После каждого изменения запускаются
    /// цепочки растительности, которых коснулось изменение (новая земля
    /// рядом с травой, новый берег у воды и т.п.). Команды с тайлами вне
    /// мира пропускаются: контроллер мог посчитать по устаревшим данным.
    pub fn apply_all(
        &mut self,
        world: &mut World,
        actor: Option<u64>,
        dirty: &mut DirtyTiles,
        sched: &mut Scheduler,
        tick: u64,
        book: &CraftBook,
    ) -> usize {
        let mut applied = 0;
        while let Some(command) = self.queue.pop_front() {
            match command {
                Command::SetTerrain { pos, kind } => {
                    if in_bounds(world, pos) {
                        world.set_terrain(pos, kind);
                        // Рельеф перестал быть пашней (или тайл перекрашен) —
                        // культура на нём не выживает; её цепочка роста
                        // погаснет сама, не найдя культуры.
                        if kind != TileType::Farmland && world.crop_at(pos).is_some() {
                            world.set_crop(pos, None);
                            dirty.decor(pos);
                        }
                        dirty.terrain(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                        // Новая пашня редактором — месячная проверка пустоты.
                        schedule_farmland_revert(world, sched, tick, pos);
                    }
                }
                Command::SetObject { pos, object } => {
                    if in_bounds(world, pos) {
                        // Объект занимает тайл целиком: культура под ним
                        // погибает; контейнер станции появляется/удаляется
                        // вместе с объектом.
                        if object.is_some() && world.crop_at(pos).is_some() {
                            world.set_crop(pos, None);
                        }
                        // Прогресс ломания привязан к постройке на тайле:
                        // новая/убранная постройка начинается с нуля.
                        world.building_breaks.remove(&pos);
                        world.set_object_with_container(pos, object);
                        dirty.decor(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                        // Объект убран, а пашня осталась — пустая пашня
                        // возвращается в землю (с объектом не считается пустой).
                        schedule_farmland_revert(world, sched, tick, pos);
                    }
                }
                Command::SpawnEntity { kind, pos } => {
                    if in_bounds(world, pos) {
                        // Существо рисуется отдельным динамическим спрайтом,
                        // поэтому terrain-блок не помечается грязным.
                        world
                            .entities
                            .spawn(kind, WorldPos::of_tile_center(pos));
                    }
                }
                Command::RemoveEntity { id } => {
                    // Спрайт удалённого существа рендер убирает сам: он
                    // сверяет список живых существ с картой спрайтов.
                    world.entities.remove(id);
                }
                Command::MoveEntity { id, pos } => {
                    // Перетаскивание в редакторе: спрайт существа читает
                    // позицию каждый кадр, отдельная пометка тайлов не нужна.
                    world.entities.move_to(id, pos);
                }
                Command::PlayerInteract { pos } => {
                    let Some(actor) = actor else { continue };
                    // Под прицелом существо — удар (кулдаун и дальность
                    // проверяет боевая система). Мимо существа — обычная
                    // добыча: оружие не мешает собирать траву и камни.
                    // Исключение — лук: им нельзя ничего ломать.
                    let can_harvest =
                        actions::selected_item(world, actor) != Some(ItemKind::Bow);
                    // Добыча меняет либо объект (куст, валун), либо рельеф
                    // (сбор травы превращает тайл в землю) — но не оба
                    // сразу. Культура меняет только декор. Канал dirty
                    // определяется по тому, что было на тайле до действия.
                    let had_object = world.object_at(pos).is_some();
                    let had_crop = world.crop_at(pos).is_some();
                    if !combat::try_melee_attack(world, actor, pos, tick)
                        && can_harvest && actions::interact(world, actor, pos)
                    {
                        if had_object || had_crop {
                            dirty.decor(pos);
                        } else {
                            dirty.terrain(pos);
                        }
                        // Пахота или уборка урожая: пашня может остаться
                        // пустой — ставим месячную проверку.
                        schedule_farmland_revert(world, sched, tick, pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                    }
                }
                Command::PlayerBite { pos } => {
                    let Some(actor) = actor else { continue };
                    // Укус — единственное действие ЛКМ неразумного существа:
                    // только бой, никаких ломаний и добычи.
                    combat::wolf_bite(world, actor, pos, tick);
                }
                Command::PlayerHarvestApple { pos } => {
                    let Some(actor) = actor else { continue };
                    if actions::harvest_apple(world, actor, pos) {
                        dirty.decor(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                    }
                }
                Command::PlayerDropItem { pos } => {
                    let Some(actor) = actor else { continue };
                    if actions::drop_item(world, actor, pos) {
                        dirty.decor(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                    }
                }
                Command::PlayerDropStack { pos, stack } => {
                    if !in_bounds(world, pos) {
                        // вне мира — пропускаем.
                    } else if let Some(target) = find_drop_tile(world, pos, stack) {
                        // Свободный тайл нашёлся: предмет не исчезает «в никуда».
                        try_place_item(world, target, stack);
                        dirty.decor(target);
                        vegetation::after_tile_changed(world, sched, tick, target);
                    }
                }
                Command::PlayerThrowItem { target } => {
                    let Some(actor) = actor else { continue };
                    actions::throw_selected(world, actor, target, tick);
                }
                Command::PlayerThrowStack { target, slot } => {
                    let Some(actor) = actor else { continue };
                    actions::throw_stack(world, actor, slot, target, tick);
                }
                Command::PlayerPickupItem { pos } => {
                    let Some(actor) = actor else { continue };
                    if actions::pickup_item(world, actor, pos) {
                        dirty.decor(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                    }
                }
                Command::PlayerBuild { pos, kind } => {
                    let Some(actor) = actor else { continue };
                    if actions::build(world, actor, pos, kind) {
                        dirty.decor(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                    }
                }
                Command::PlayerSow { pos } => {
                    let Some(actor) = actor else { continue };
                    if !in_bounds(world, pos) {
                        continue;
                    }
                    // Вид культуры берём из предмета в руке до посева:
                    // после списания слот может опустеть.
                    let Some(crop_kind) =
                        actions::selected_item(world, actor).and_then(CropKind::from_seed_item)
                    else {
                        continue;
                    };
                    if actions::sow(world, actor, pos) {
                        dirty.decor(pos);
                        // Цепочка роста: событие следующей стадии.
                        let (chunk, local_index) = split_tile(pos);
                        sched.schedule_at(
                            tick + crop_kind.stage_interval_ticks(),
                            TaskKind::CropGrowth { chunk, local_index },
                        );
                    }
                }
                Command::PlayerBreakBuilding { pos } => {
                    let Some(actor) = actor else { continue };
                    if actions::break_building(world, actor, pos, tick) {
                        dirty.decor(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                    }
                }
                Command::StationMoveItem { pos, from, to, count } => {
                    if !in_bounds(world, pos) {
                        continue;
                    }
                    if station_move_item(world, actor, pos, from, to, count) {
                        // Слоты изменились: декор (состояние печи) и поджиг.
                        dirty.decor(pos);
                        containers::wake_station(world, sched, tick, book, pos);
                    }
                }
                Command::PlayerThrowSpear { target } => {
                    let Some(actor) = actor else { continue };
                    let Some(origin) = actions::pos_of(world, actor) else { continue };
                    let slot = mind_of(world, actor)
                        .and_then(|m| {
                            let slot = m.selected_slot.min(MISC_SLOTS - 1);
                            m.inventory.items[slot].map(|stack| (slot, stack))
                        });
                    // Бросается копьё из руки: пустой слот или другой предмет
                    // — команда бессмысленна.
                    let Some((slot, _)) = slot.filter(|(_, stack)| stack.kind == ItemKind::Spear)
                    else {
                        continue;
                    };
                    if !in_bounds(world, target) {
                        continue;
                    }
                    if let Some(m) = mind_of(world, actor) {
                        m.inventory.items[slot] = None;
                    }
                    // Точка прицела за пределами дальности — цель ужимается
                    // к максимальной дальности по тому же направлению.
                    let target = clamp_target(
                        origin,
                        WorldPos::of_tile_center(target),
                        combat::SPEAR_THROW_RANGE_TILES,
                    );
                    world.spawn_projectile(combat::ProjectileKind::ThrownSpear, origin, target, actor);
                }
                Command::PlayerShootBow { target } => {
                    let Some(actor) = actor else { continue };
                    let Some(origin) = actions::pos_of(world, actor) else { continue };
                    if actions::selected_item(world, actor) != Some(ItemKind::Bow) {
                        continue;
                    }
                    if !in_bounds(world, target) {
                        continue;
                    }
                    // Кулдаун выстрела — общий с ближним боем.
                    if tick < mind_of(world, actor).map_or(u64::MAX, |m| m.attack_ready_tick) {
                        continue;
                    }
                    // Стрелы пока не расходуются (решение автора на время
                    // отладки): экономика боеприпасов появится вместе с
                    // производством. `take_items(Arrow, 1)` вернётся сюда.
                    if let Some(m) = mind_of(world, actor) {
                        m.attack_ready_tick = tick + combat::BOW_COOLDOWN_TICKS;
                    }
                    let target = clamp_target(
                        origin,
                        WorldPos::of_tile_center(target),
                        combat::BOW_RANGE_TILES,
                    );
                    world.spawn_projectile(combat::ProjectileKind::Arrow, origin, target, actor);
                }
                Command::PlayerEat => {
                    let Some(actor) = actor else { continue };
                    // Кулдаун поедания: ранняя команда игнорируется целиком.
                    if tick < mind_of(world, actor).map_or(u64::MAX, |m| m.eat_ready_tick) {
                        continue;
                    }
                    // Еда только из выбранного слота: пусто или не еда — нет.
                    // Кулдаун тикает только от удачного поедания.
                    if actions::eat_selected(world, actor)
                        && let Some(m) = mind_of(world, actor)
                    {
                        m.eat_ready_tick = tick + super::hunger::EAT_COOLDOWN_TICKS;
                    }
                }
                Command::EntityMoveItem { id, from, to } => {
                    // Перенос внутри инвентаря существа: правила слотов
                    // (слияние/обмен/мусорка) решает `Inventory::transfer`.
                    if let Some(m) = world.entities.get_mut(id).and_then(|e| e.mind.as_mut()) {
                        m.inventory.transfer(from, to);
                    }
                }
                Command::EntityDropItem { id, from } => {
                    // Позиция существа снимается до мутации инвентаря.
                    let Some(pos) = world.entities.get(id).map(|e| e.pos) else {
                        continue;
                    };
                    let Some(stack) = world
                        .entities
                        .get_mut(id)
                        .and_then(|e| e.mind.as_mut())
                        .and_then(|m| m.inventory.take(from))
                    else {
                        continue;
                    };
                    world.item_entities.drop_stack(world.seed, pos, stack, tick);
                }
            }
            applied += 1;
        }
        applied
    }
}

/// Ум актора (мутабельно); `None` — захвата нет или существо неразумное.
fn mind_of(world: &mut World, actor: u64) -> Option<&mut HumanMind> {
    world.entities.get_mut(actor)?.mind.as_mut()
}

/// Лежит ли тайл внутри мира.
fn in_bounds(world: &World, pos: TilePos) -> bool {
    let size = world.size.tiles() as i32;
    pos.x >= 0 && pos.y >= 0 && pos.x < size && pos.y < size
}

/// Прицел дальше максимальной дальности — точка падения ужимается к ней
/// по тому же направлению: снаряд всегда летит «в сторону курсора».
fn clamp_target(origin: WorldPos, target: WorldPos, max_range: f32) -> WorldPos {
    let distance = origin.dist(target);
    if distance <= max_range || distance <= f32::EPSILON {
        return target;
    }
    let k = max_range / distance;
    WorldPos {
        x: origin.x + (target.x - origin.x) * k,
        y: origin.y + (target.y - origin.y) * k,
    }
}

/// Можно ли занять тайл постройкой: тайл внутри мира, рельеф не водный,
/// на тайле нет объекта и лежащих предметов. Общая проверка команды
/// строительства и голограммы в рендере.
/// Ставит месячную задачу возврата пустой пашни в землю: пашня без
/// культуры и без объекта считается неиспользуемой. Повторная постановка
/// безопасна: задача идемпотентна (гаснет, если состояние не то).
fn schedule_farmland_revert(world: &World, sched: &mut Scheduler, tick: u64, pos: TilePos) {
    if world.terrain_kind(pos) != TileType::Farmland
        || world.crop_at(pos).is_some()
        || world.object_at(pos).is_some()
    {
        return;
    }
    let (chunk, local) = split_tile(pos);
    sched.schedule_at(
        tick + super::time::TICKS_PER_MONTH,
        TaskKind::FarmlandRevert { chunk, local_index: local },
    );
}

pub fn buildable_at(world: &World, pos: TilePos) -> bool {
    if !in_bounds(world, pos) {
        return false;
    }
    if world.terrain_kind(pos).is_water() {
        return false;
    }
    world.object_at(pos).is_none()
        && world.item_at(pos).is_none()
        && world.crop_at(pos).is_none()
}

/// Перенос стека между слотами станции и misc захваченного существа
/// (команда `StationMoveItem`). Целевой слот занят тем же видом — доливает
/// до предела стака, остаток возвращается в источник; другим предметом —
/// меняет местами, если предмет источника допустим в целевом слоте и
/// наоборот. `true` — содержимое изменилось.
///
/// Контейнер и инвентарь существа — разные поля `World`, поэтому заём
/// берётся заново на каждую мутацию (`container_at_mut` / `get_mut`):
/// долгоживущий заём контейнера блокировал бы доступ к существам.
fn station_move_item(
    world: &mut World,
    actor: Option<u64>,
    pos: TilePos,
    from: (u8, u8),
    to: (u8, u8),
    count: u8,
) -> bool {
    // Действия с misc возможны только у существа с «умом».
    let Some(actor) = actor else {
        return false;
    };
    let Some(kind) = world.object_at(pos).map(|o| o.kind) else {
        return false;
    };
    let Some(container) = containers::container_at(world, pos) else {
        // Станции с контейнером нет (верстак/кузнечный стол — их сетка
        // живёт в UI), перенос мимо контейнера невозможен.
        return false;
    };
    // Снимок входов до мутаций: изменение входа сбрасывает идущую готовку.
    let inputs_before = containers::input_snapshot(kind, container);
    // Заём `container` заканчивается на этом утверждении: дальше контейнер
    // берётся заново на каждую мутацию, чтобы не блокировать существ. 

    // Читаем источник.
    let source = if from.0 == containers::SLOT_AREA_PLAYER_MISC {
        let slot = usize::from(from.1);
        if slot >= MISC_SLOTS {
            return false;
        }
        let Some(m) = mind_of(world, actor) else {
            return false;
        };
        m.inventory.items[slot]
    } else {
        match containers::container_at(world, pos) {
            Some(c) => c.get(usize::from(from.0)),
            None => return false,
        }
    };
    let Some(source) = source else {
        return false;
    };
    // Число переносимых единиц: 0 — весь стак.
    let amount = if count == 0 {
        source.count
    } else {
        (count as u32).min(source.count)
    };
    if amount == 0 {
        return false;
    }
    let moving = ItemStack { kind: source.kind, count: amount };
    let left_in_source = source.count - amount;

    // Правило целевого слота. Кодировка адреса: misc актора — (0xFF, индекс),
    // слот контейнера — (индекс слота, 0).
    let target_is_player = to.0 == containers::SLOT_AREA_PLAYER_MISC;
    let target_slot = if target_is_player {
        usize::from(to.1)
    } else {
        usize::from(to.0)
    };
    if !target_is_player {
        if !containers::can_put_into(kind, target_slot, moving.kind) {
            return false;
        }
    } else if target_slot >= MISC_SLOTS {
        return false;
    }

    // Применяем: сначала снимаем источник (полностью, долив вернём).
    if from.0 == containers::SLOT_AREA_PLAYER_MISC {
        let slot = usize::from(from.1);
        let Some(m) = mind_of(world, actor) else {
            return false;
        };
        m.inventory.items[slot] = None;
    } else if let Some(container) = containers::container_at_mut(world, pos) {
        container.take(usize::from(from.0));
    }
    let mut back_to_source = (left_in_source > 0).then_some(ItemStack {
        kind: source.kind,
        count: left_in_source,
    });
    let mut changed = true;

    if target_is_player {
        let Some(m) = mind_of(world, actor) else {
            return false;
        };
        let slot = &mut m.inventory.items[target_slot];
        match *slot {
            None => *slot = Some(moving),
            Some(mut existing) if existing.kind == moving.kind => {
                let cap = existing.kind.max_stack();
                let free = cap.saturating_sub(existing.count).min(moving.count);
                existing.count += free;
                *slot = Some(existing);
                let unplaced = moving.count - free;
                if unplaced > 0 {
                    back_to_source = Some(ItemStack { kind: moving.kind, count: unplaced });
                    changed = free > 0;
                }
            }
            // Обмен: предмет источника всегда допустим в misc.
            Some(existing) => {
                if left_in_source == 0 {
                    back_to_source = Some(existing);
                } else {
                    // Обмен при частичном переносе невозможен: возвращаем.
                    back_to_source = Some(source);
                    changed = false;
                }
            }
        }
    } else {
        let Some(container) = containers::container_at_mut(world, pos) else {
            return false;
        };
        let target_index = target_slot;
        match container.get(target_index) {
            None => {
                container.put(target_index, moving);
            }
            Some(existing) if existing.kind == moving.kind => {
                let cap = existing.kind.max_stack();
                let free = cap.saturating_sub(existing.count).min(moving.count);
                let mut merged = existing;
                merged.count += free;
                container.replace(target_index, merged);
                let unplaced = moving.count - free;
                if unplaced > 0 {
                    back_to_source = Some(ItemStack { kind: moving.kind, count: unplaced });
                    changed = free > 0;
                }
            }
            Some(existing) => {
                // Обмен: предмет из станции должен быть допустим в источник.
                let source_ok = if from.0 == containers::SLOT_AREA_PLAYER_MISC {
                    true
                } else {
                    containers::can_put_into(kind, usize::from(from.0), existing.kind)
                };
                if source_ok && left_in_source == 0 {
                    // Слот занят: именно replace, а не put — put кладёт только
                    // в пустой слот, и стек молча терялся (предмет актора
                    // исчезал, а на его место приезжал предмет контейнера).
                    container.replace(target_index, moving);
                    back_to_source = Some(existing);
                } else {
                    back_to_source = Some(source);
                    changed = false;
                }
            }
        }
    }

    // Вход изменился (забрали/заменили) — прогресс жарки сбрасывается;
    // поджиг с новым предметом сделает wake_station вызывающей стороны.
    if let Some(container) = containers::container_at_mut(world, pos) {
        containers::reset_cooking_if_inputs_changed(kind, container, &inputs_before);
    }

    // Возврат остатка в источник.
    if let Some(rest) = back_to_source {
        if from.0 == containers::SLOT_AREA_PLAYER_MISC {
            let slot = usize::from(from.1);
            let Some(m) = mind_of(world, actor) else {
                return false;
            };
            m.inventory.items[slot] = Some(rest);
        } else if let Some(container) = containers::container_at_mut(world, pos) {
            container.put(usize::from(from.0), rest);
        }
    }
    changed
}

/// Кладёт стек предметов на тайл: в пустой — целиком, в стек того же вида —
/// доливает. `false` — тайл вне мира или занят другим предметом/переполнен.
pub(crate) fn try_place_item(world: &mut World, pos: TilePos, stack: ItemStack) -> bool {
    if !in_bounds(world, pos) || stack.count == 0 || stack.count > MAX_STACK {
        return false;
    }
    match world.item_at(pos).copied() {
        None => {
            world.set_item(pos, Some(stack));
            true
        }
        Some(existing) if existing.kind == stack.kind
            && existing.count + stack.count <= stack.kind.max_stack() =>
        {
            let mut merged = existing;
            merged.count += stack.count;
            world.set_item(pos, Some(merged));
            true
        }
        _ => false,
    }
}

/// Ищет тайл, куда стек помещается целиком: указанный, затем кольца
/// соседних тайлов в порядке удаления (кольцо построчно — детерминированно).
/// Нужно для сброса остатков сетки крафта при закрытии инвентаря: тайл под
/// игроком может быть занят. Поиск идёт до края мира, поэтому стек
/// теряется только если занята вся карта.
fn find_drop_tile(world: &World, center: TilePos, stack: ItemStack) -> Option<TilePos> {
    let size = world.size.tiles() as i32;
    let max_radius = center.x.max(center.y).max(size - 1 - center.x).max(size - 1 - center.y);
    for radius in 0..=max_radius {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                // Внутренность кольца покрыта меньшими радиусами.
                if dx.abs() != radius && dy.abs() != radius {
                    continue;
                }
                let pos = TilePos { x: center.x + dx, y: center.y + dy };
                // Предел стака на тайле — тоже по виду предмета: инструменты
                // не сливаются в кучу, каждый ищет свой тайл.
                let fits = in_bounds(world, pos)
                    && match world.item_at(pos) {
                        None => true,
                        Some(existing) => {
                            existing.kind == stack.kind
                                && existing.count + stack.count <= stack.kind.max_stack()
                        }
                    };
                if fits {
                    return Some(pos);
                }
            }
        }
    }
    None
}
