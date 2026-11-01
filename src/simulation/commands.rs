//! Команды контроллеров (UI, игрок, «бог», в будущем ИИ) — единый тип
//! для всех источников (GAME_DESIGN 3.1, ARCHITECTURE 2.6). UI не меняет
//! мир напрямую: он ставит команду в очередь, очередь применяется по порядку.

use std::collections::VecDeque;

use super::combat;
use super::containers;
use super::item_entity::THROW_DISTANCE_TILES;
use super::crafts::CraftBook;
use super::crops::CropKind;
use super::dirty::DirtyTiles;
use super::entities::EntityKind;
use super::items::{ItemKind, ItemStack, MAX_STACK, MISC_SLOTS};
use super::objects::{BuildingKind, WorldObject, build_cost};
use super::player::{Player, WorldPos};
use super::scheduler::{Scheduler, TaskKind};
use super::tile::TileType;
use super::vegetation;
use super::world::{TilePos, World, split_tile};

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
    /// `dirty` наполняется позициями изменённых тайлов с разделением по
    /// каналу (рельеф / декор) — рендер по ним пересобирает меш блока или
    /// только дочерние спрайты. После каждого изменения запускаются
    /// цепочки растительности, которых коснулось изменение (новая земля
    /// рядом с травой, новый берег у воды и т.п.). Команды с тайлами вне
    /// мира пропускаются: контроллер мог посчитать по устаревшим данным.
    pub fn apply_all(
        &mut self,
        world: &mut World,
        mut player: Option<&mut Player>,
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
                    if let Some(player) = player.as_deref_mut() {
                        // Под прицелом существо — удар (кулдаун и дальность
                        // проверяет боевая система). Мимо существа — обычная
                        // добыча: оружие не мешает собирать траву и камни.
                        // Исключение — лук: им нельзя ничего ломать.
                        let can_harvest = player.selected_item() != Some(ItemKind::Bow);
                        // Добыча меняет либо объект (куст, валун), либо рельеф
                        // (сбор травы превращает тайл в землю) — но не оба
                        // сразу. Культура меняет только декор. Канал dirty
                        // определяется по тому, что было на тайле до действия.
                        let had_object = world.object_at(pos).is_some();
                        let had_crop = world.crop_at(pos).is_some();
                        if !combat::try_melee_attack(world, player, pos, tick)
                            && can_harvest && player.interact(pos, world)
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
                }
                Command::PlayerHarvestApple { pos } => {
                    if let Some(player) = player.as_deref_mut() && player.harvest_apple(pos, world) {
                        dirty.decor(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                    }
                }
                Command::PlayerDropItem { pos } => {
                    if !in_bounds(world, pos) {
                        // вне мира — пропускаем.
                    } else if let Some(player) = player.as_deref_mut()
                        // Выкладывать можно только в радиусе взаимодействия
                        // (как для добычи): «телепорт» предметов на дальние
                        // тайлы через курсор невозможен.
                        && player.reachable(pos, world)
                    {
                        let slot_index = player.selected_slot.min(super::items::MISC_SLOTS - 1);
                        let current = player.inventory.items[slot_index];
                        if let Some(stack) = current && stack.count > 0 {
                            // Снимаем весь стек из слота.
                            player.inventory.items[slot_index] = None;
                            if try_place_item(world, pos, stack) {
                                dirty.decor(pos);
                                vegetation::after_tile_changed(world, sched, tick, pos);
                            } else {
                                // Не поместилось: возвращаем стек в инвентарь.
                                player.inventory.items[slot_index] = Some(stack);
                            }
                        }
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
                    let Some(player) = player.as_deref_mut() else {
                        continue;
                    };
                    let slot = player.selected_slot.min(super::items::MISC_SLOTS - 1);
                    let Some(stack) = player.inventory.items[slot] else {
                        continue;
                    };
                    // Бросается один предмет; слот опустевает полностью.
                    player.inventory.items[slot] = (stack.count > 1)
                        .then_some(ItemStack { count: stack.count - 1, ..stack });
                    throw_items(
                        world,
                        player.pos,
                        ItemStack { kind: stack.kind, count: 1 },
                        target,
                        tick,
                    );
                }
                Command::PlayerThrowStack { target, slot } => {
                    let Some(player) = player.as_deref_mut() else {
                        continue;
                    };
                    let slot = usize::from(slot);
                    if slot >= super::items::MISC_SLOTS {
                        continue;
                    }
                    let Some(stack) = player.inventory.items[slot] else {
                        continue;
                    };
                    player.inventory.items[slot] = None;
                    throw_items(world, player.pos, stack, target, tick);
                }
                Command::PlayerPickupItem { pos } => {
                    if !in_bounds(world, pos) {
                        // вне мира — пропускаем.
                    } else if let Some(stack) = world.item_at(pos).copied()
                        && let Some(player) = player.as_deref_mut()
                        // Подобрать можно только в радиусе взаимодействия,
                        // как и всё остальное: «телепорт» стека к игроку
                        // с дальнего края экрана запрещён.
                        && player.reachable(pos, world)
                        && player.inventory.can_fit(stack.kind, stack.count)
                    {
                        player.inventory.add(stack.kind, stack.count);
                        world.set_item(pos, None);
                        dirty.decor(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                    }
                }
                Command::PlayerBuild { pos, kind } => {
                    let Some((item, count)) = build_cost(kind) else {
                        // У постройки нет цены (строятся редактором) —
                        // команда игнорируется.
                        continue;
                    };
                    let Some(player) = player.as_deref_mut() else {
                        continue;
                    };
                    if !player.reachable(pos, world) || !buildable_at(world, pos) {
                        continue;
                    }
                    // Платим только после проверки места: постройка не может
                    // «съесть» предметы и не появиться.
                    if !player.inventory.take_items(item, count) {
                        continue;
                    }
                    // Контейнер станции появляется вместе с постройкой.
                    if world.crop_at(pos).is_some() {
                        world.set_crop(pos, None);
                    }
                    world.set_object_with_container(pos, Some(WorldObject::building(kind)));
                    dirty.decor(pos);
                    vegetation::after_tile_changed(world, sched, tick, pos);
                }
                Command::PlayerSow { pos } => {
                    let Some(player) = player.as_deref_mut() else {
                        continue;
                    };
                    if !in_bounds(world, pos) {
                        continue;
                    }
                    // Вид культуры берём из предмета в руке до посева:
                    // после списания слот может опустеть.
                    let Some(crop_kind) =
                        player.selected_item().and_then(CropKind::from_seed_item)
                    else {
                        continue;
                    };
                    if player.sow(pos, world) {
                        dirty.decor(pos);
                        // Цепочка роста: событие следующей стадии.
                        let (chunk, local_index) = split_tile(pos);
                        sched.schedule_at(
                            tick + crop_kind.stage_interval_ticks(),
                            TaskKind::CropGrowth { chunk, local_index },
                        );
                    }
                }
                Command::StationMoveItem { pos, from, to, count } => {
                    if !in_bounds(world, pos) {
                        continue;
                    }
                    if station_move_item(world, player.as_deref_mut(), pos, from, to, count) {
                        // Слоты изменились: декор (состояние печи) и поджиг.
                        dirty.decor(pos);
                        containers::wake_station(world, sched, tick, book, pos);
                    }
                }
                Command::PlayerThrowSpear { target } => {
                    let Some(player) = player.as_deref_mut() else {
                        continue;
                    };
                    let slot = player.selected_slot.min(MISC_SLOTS - 1);
                    // Бросается копьё из руки: пустой слот или другой предмет
                    // — команда бессмысленна.
                    match player.inventory.items[slot] {
                        Some(stack) if stack.kind == ItemKind::Spear => {}
                        _ => continue,
                    }
                    if !in_bounds(world, target) {
                        continue;
                    }
                    player.inventory.items[slot] = None;
                    // Точка прицела за пределами дальности — цель ужимается
                    // к максимальной дальности по тому же направлению.
                    let target = clamp_target(
                        player.pos,
                        WorldPos::of_tile_center(target),
                        combat::SPEAR_THROW_RANGE_TILES,
                    );
                    world.spawn_projectile(combat::ProjectileKind::ThrownSpear, player.pos, target);
                }
                Command::PlayerShootBow { target } => {
                    let Some(player) = player.as_deref_mut() else {
                        continue;
                    };
                    if player.selected_item() != Some(ItemKind::Bow) {
                        continue;
                    }
                    if !in_bounds(world, target) {
                        continue;
                    }
                    // Кулдаун выстрела — общий с ближним боем.
                    if tick < player.attack_ready_tick {
                        continue;
                    }
                    // Стрелы пока не расходуются (решение автора на время
                    // отладки): экономика боеприпасов появится вместе с
                    // производством. `take_items(Arrow, 1)` вернётся сюда.
                    player.attack_ready_tick = tick + combat::BOW_COOLDOWN_TICKS;
                    let target = clamp_target(
                        player.pos,
                        WorldPos::of_tile_center(target),
                        combat::BOW_RANGE_TILES,
                    );
                    world.spawn_projectile(combat::ProjectileKind::Arrow, player.pos, target);
                }
                Command::PlayerEat => {
                    let Some(player) = player.as_deref_mut() else {
                        continue;
                    };
                    // Кулдаун поедания: ранняя команда игнорируется целиком.
                    if tick < player.eat_ready_tick {
                        continue;
                    }
                    // Еда только из выбранного слота: пусто или не еда — нет.
                    // Кулдаун тикает только от удачного поедания.
                    if player.eat_selected() {
                        player.eat_ready_tick = tick + super::hunger::EAT_COOLDOWN_TICKS;
                    }
                }
            }
            applied += 1;
        }
        applied
    }
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

/// Бросок стека предметов в сторону тайла: предмет «пролетает» до
/// `THROW_DISTANCE_TILES` (или до цели, если она ближе) и остаётся лежать
/// дроп-сущностью с задержкой автоподбора. Позиция зажимается в мир.
fn throw_items(world: &mut World, from: WorldPos, stack: ItemStack, target: TilePos, tick: u64) {
    let aim = WorldPos::of_tile_center(target);
    let (dx, dy) = (aim.x - from.x, aim.y - from.y);
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < f32::EPSILON {
        // Цель под ногами: направление неизвестно — бросок не имеет смысла.
        return;
    }
    let fly = dist.min(THROW_DISTANCE_TILES);
    let side = world.size.tiles() as f32;
    let landing = WorldPos {
        x: (from.x + dx / dist * fly).clamp(0.0, side),
        y: (from.y + dy / dist * fly).clamp(0.0, side),
    };
    world.item_entities.throw(from, stack, tick, landing);
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

/// Перенос стека между слотами станции и misc игрока (команда
/// `StationMoveItem`). Целевой слот занят тем же видом — доливает до
/// предела стака, остаток возвращается в источник; другим предметом —
/// меняет местами, если предмет источника допустим в целевом слоте и
/// наоборот. `true` — содержимое изменилось.
fn station_move_item(
    world: &mut World,
    mut player: Option<&mut Player>,
    pos: TilePos,
    from: (u8, u8),
    to: (u8, u8),
    count: u8,
) -> bool {
    let Some(kind) = world.object_at(pos).map(|o| o.kind) else {
        return false;
    };
    let Some(container) = containers::container_at_mut(world, pos) else {
        // Станции с контейнером нет (верстак/кузнечный стол — их сетка
        // живёт в UI), перенос мимо контейнера невозможен.
        return false;
    };
    // Снимок входов до мутаций: изменение входа сбрасывает идущую готовку.
    let inputs_before = containers::input_snapshot(kind, container);

    // Читаем источник.
    let source = if from.0 == containers::SLOT_AREA_PLAYER_MISC {
        let Some(player) = player.as_deref_mut() else {
            return false;
        };
        let slot = usize::from(from.1);
        if slot >= MISC_SLOTS {
            return false;
        }
        player.inventory.items[slot]
    } else {
        container.get(usize::from(from.0))
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

    // Правило целевого слота. Кодировка адреса: misc игрока — (0xFF, индекс),
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
        player.as_deref_mut().unwrap().inventory.items[slot] = None;
    } else {
        container.take(usize::from(from.0));
    }
    let mut back_to_source = (left_in_source > 0).then_some(ItemStack {
        kind: source.kind,
        count: left_in_source,
    });
    let mut changed = true;

    if target_is_player {
        let player = player.as_deref_mut().unwrap();
        let slot = &mut player.inventory.items[target_slot];
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
                    // в пустой слот, и стек молча терялся (предмет игрока
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
    containers::reset_cooking_if_inputs_changed(kind, container, &inputs_before);

    // Возврат остатка в источник.
    if let Some(rest) = back_to_source {
        if from.0 == containers::SLOT_AREA_PLAYER_MISC {
            let slot = usize::from(from.1);
            player.unwrap().inventory.items[slot] = Some(rest);
        } else {
            container.put(usize::from(from.0), rest);
        }
    }
    changed
}

/// Кладёт стек предметов на тайл: в пустой — целиком, в стек того же вида —
/// доливает. `false` — тайл вне мира или занят другим предметом/переполнен.
fn try_place_item(world: &mut World, pos: TilePos, stack: ItemStack) -> bool {
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
