//! Команды контроллеров (UI, игрок, «бог», в будущем ИИ) — единый тип
//! для всех источников (GAME_DESIGN 3.1, ARCHITECTURE 2.6). UI не меняет
//! мир напрямую: он ставит команду в очередь, очередь применяется по порядку.

use std::collections::VecDeque;

use super::combat;
use super::dirty::DirtyTiles;
use super::entities::EntityKind;
use super::items::{ItemKind, ItemStack, MAX_STACK, MISC_SLOTS};
use super::objects::{ObjectKind, WorldObject, build_cost};
use super::player::{Player, WorldPos};
use super::scheduler::Scheduler;
use super::tile::TileType;
use super::vegetation;
use super::world::{TilePos, World};

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
    /// Действие игрока: построить постройку на тайле (меню строительства).
    /// Плата списывается симуляцией; вид, не являющийся постройкой,
    /// просто игнорируется.
    PlayerBuild { pos: TilePos, kind: ObjectKind },
    /// Действие игрока: бросить копьё из выбранного слота в сторону тайла.
    /// Дальность ограничена (см. `combat`); копьё списывается при запуске.
    PlayerThrowSpear { target: TilePos },
    /// Действие игрока: выстрелить из лука в сторону тайла. Тратит одну
    /// стрелу из инвентаря; без стрел выстрела нет.
    PlayerShootBow { target: TilePos },
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
    ) -> usize {
        let mut applied = 0;
        while let Some(command) = self.queue.pop_front() {
            match command {
                Command::SetTerrain { pos, kind } => {
                    if in_bounds(world, pos) {
                        world.set_terrain(pos, kind);
                        dirty.terrain(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
                    }
                }
                Command::SetObject { pos, object } => {
                    if in_bounds(world, pos) {
                        world.set_object(pos, object);
                        dirty.decor(pos);
                        vegetation::after_tile_changed(world, sched, tick, pos);
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
                        // сразу. Канал dirty определяется по тому, был ли на
                        // тайле объект до действия.
                        let had_object = world.object_at(pos).is_some();
                        if !combat::try_melee_attack(world, player, pos, tick)
                            && can_harvest && player.interact(pos, world)
                        {
                            if had_object {
                                dirty.decor(pos);
                            } else {
                                dirty.terrain(pos);
                            }
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
                        // Вид не является постройкой (строится только через
                        // меню строительства) — команда игнорируется.
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
                    let Some(object) = WorldObject::building(kind) else {
                        continue;
                    };
                    world.set_object(pos, Some(object));
                    dirty.decor(pos);
                    vegetation::after_tile_changed(world, sched, tick, pos);
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
pub fn buildable_at(world: &World, pos: TilePos) -> bool {
    if !in_bounds(world, pos) {
        return false;
    }
    if world.terrain_kind(pos).is_water() {
        return false;
    }
    world.object_at(pos).is_none() && world.item_at(pos).is_none()
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
