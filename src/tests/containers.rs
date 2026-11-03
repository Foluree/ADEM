//! Контейнеры станций: рабочий цикл печи (поджиг → готовка → результат),
//! топливные правила плавильни, правила слотов и ёмкости хранилищ.

use crate::simulation::commands::Command;
use crate::simulation::containers::{
    self, capacity, can_put_into, container_at, wake_station, BLAST_FUEL, BLAST_INPUT,
    CAMPFIRE_INPUT, CAMPFIRE_OUTPUT, FURNACE_FUEL, FURNACE_INPUT, FURNACE_OUTPUT,
    FURNACE_WASTE, SLOT_AREA_PLAYER_MISC,
};
use crate::simulation::crafts::CraftBook;
use crate::simulation::dirty::DirtyTiles;
use crate::simulation::entities::EntityKind;
use crate::simulation::items::{ItemKind, ItemStack};
use crate::simulation::objects::{BuildingKind, ObjectKind, WorldObject};
use crate::simulation::scheduler::TaskKind;
use crate::simulation::world::{TilePos, World, WorldPos, WorldSize};

/// Мир со станцией-постройкой на тайле (10, 10) и игроком рядом.
fn furnace_world(kind: BuildingKind) -> (World, CraftBook) {
    let mut world = World::new(WorldSize::new(256), 7);
    let pos = TilePos { x: 10, y: 10 };
    world.set_object_with_container(pos, Some(WorldObject::building(kind)));
    let load = CraftBook::load_dir("data/crafts");
    assert!(load.errors.is_empty());
    (world, load.book)
}

/// Человек рядом со станцией: инвентарь существует у существа с «умом».
fn spawn_human_nearby(world: &mut World) -> u64 {
    world.entities.spawn(
        EntityKind::Human,
        WorldPos::of_tile_center(TilePos { x: 11, y: 10 }),
    )
}

/// Ум существа (мутабельно): инвентарь, голод, выбранный слот.
fn mind_mut(world: &mut World, actor: u64) -> &mut crate::simulation::HumanMind {
    world
        .entities
        .get_mut(actor)
        .unwrap()
        .mind
        .as_mut()
        .unwrap()
}

/// Ставит печь в слот `slot` со стеком `stack`.
fn put(world: &mut World, pos: TilePos, slot: usize, stack: ItemStack) {
    assert!(containers::container_at_mut(world, pos)
        .unwrap()
        .put(slot, stack));
}

/// Прогоняет цикл печи: поджиг, исполнение задач планировщика с вызовом
/// `apply_station_tick` (как в app-слое) до тика `until`.
fn run_stations(world: &mut World, sched: &mut crate::simulation::Scheduler, book: &CraftBook, until: u64) {
    let mut dirty = DirtyTiles::default();
    let mut fired = Vec::new();
    sched.run_tick(until, |task| fired.push(task));
    for task in fired {
        if let TaskKind::StationTick { chunk, local_index } = task.kind {
            containers::apply_station_tick(world, sched, until, &mut dirty, book, chunk, local_index);
        }
    }
}

#[test]
fn furnace_cooks_food_with_fuel() {
    let (mut world, book) = furnace_world(BuildingKind::Furnace);
    let pos = TilePos { x: 10, y: 10 };
    put(&mut world, pos, FURNACE_INPUT, ItemStack { kind: ItemKind::MeatHerbivoreRaw, count: 3 });
    put(&mut world, pos, FURNACE_FUEL, ItemStack { kind: ItemKind::Coal, count: 2 });

    // Поджиг: горит, вход ещё на месте, задача на тик готовки.
    let mut sched = crate::simulation::Scheduler::default();
    wake_station(&mut world, &mut sched, 0, &book, pos);
    let container = container_at(&world, pos).unwrap();
    assert!(container.is_lit());
    assert_eq!(container.get(FURNACE_INPUT).unwrap().count, 3);

    // Готовка завершается на 100-м тике: результат в выходе, вход списан.
    run_stations(&mut world, &mut sched, &book, containers::FURNACE_COOK_TICKS);
    let container = container_at(&world, pos).unwrap();
    assert_eq!(
        container.get(FURNACE_OUTPUT),
        Some(ItemStack { kind: ItemKind::MeatHerbivoreCooked, count: 1 })
    );
    assert_eq!(container.get(FURNACE_INPUT).unwrap().count, 2);

    // Цикл перезапустился сам: топливо ещё есть, вход остался.
    assert!(container.cook_finish_tick.is_some());
}

#[test]
fn furnace_stops_without_fuel() {
    let (mut world, book) = furnace_world(BuildingKind::Furnace);
    let pos = TilePos { x: 10, y: 10 };
    put(&mut world, pos, FURNACE_INPUT, ItemStack { kind: ItemKind::Potato, count: 1 });
    let mut sched = crate::simulation::Scheduler::default();
    wake_station(&mut world, &mut sched, 0, &book, pos);
    // Топлива нет — печь не загорелась.
    assert!(!container_at(&world, pos).unwrap().is_lit());
}

#[test]
fn blast_furnace_accepts_only_coal() {
    let (mut world, book) = furnace_world(BuildingKind::BlastFurnace);
    let pos = TilePos { x: 10, y: 10 };
    // Правила слотов: уголь — да, дровяной уголь и палки — нет.
    assert!(can_put_into(ObjectKind::Building(BuildingKind::BlastFurnace), BLAST_FUEL[0], ItemKind::Coal));
    assert!(!can_put_into(ObjectKind::Building(BuildingKind::BlastFurnace), BLAST_FUEL[0], ItemKind::Charcoal));
    assert!(!can_put_into(ObjectKind::Building(BuildingKind::BlastFurnace), BLAST_FUEL[0], ItemKind::Stick));
    // Выход и отходы — только забирать.
    assert!(!can_put_into(ObjectKind::Building(BuildingKind::BlastFurnace), containers::BLAST_OUTPUT, ItemKind::Coal));

    put(&mut world, pos, BLAST_INPUT[0], ItemStack { kind: ItemKind::RawCopper, count: 1 });
    put(&mut world, pos, BLAST_FUEL[0], ItemStack { kind: ItemKind::Charcoal, count: 5 });
    let mut sched = crate::simulation::Scheduler::default();
    wake_station(&mut world, &mut sched, 0, &book, pos);
    assert!(!container_at(&world, pos).unwrap().is_lit());

    // С углём плавка идёт до 200-го тика.
    put(&mut world, pos, BLAST_FUEL[1], ItemStack { kind: ItemKind::Coal, count: 1 });
    wake_station(&mut world, &mut sched, 0, &book, pos);
    assert!(container_at(&world, pos).unwrap().is_lit());
    run_stations(&mut world, &mut sched, &book, containers::BLAST_COOK_TICKS);
    assert_eq!(
        container_at(&world, pos).unwrap().get(containers::BLAST_OUTPUT),
        Some(ItemStack { kind: ItemKind::IngotCopper, count: 1 })
    );
}

#[test]
fn campfire_cooks_without_fuel() {
    let (mut world, book) = furnace_world(BuildingKind::Campfire);
    let pos = TilePos { x: 10, y: 10 };
    put(&mut world, pos, CAMPFIRE_INPUT, ItemStack { kind: ItemKind::Egg, count: 1 });
    let mut sched = crate::simulation::Scheduler::default();
    wake_station(&mut world, &mut sched, 0, &book, pos);
    // Костёр горит вечно: burn_out нет, но готовка идёт.
    let container = container_at(&world, pos).unwrap();
    assert!(!container.is_lit());
    assert!(container.cook_finish_tick.is_some());
    run_stations(&mut world, &mut sched, &book, containers::CAMPFIRE_COOK_TICKS);
    assert_eq!(
        container_at(&world, pos).unwrap().get(CAMPFIRE_OUTPUT),
        Some(ItemStack { kind: ItemKind::EggRoasted, count: 1 })
    );
}

#[test]
fn container_rules_and_capacities() {
    // Ёмкости по видам; верстак и кузнечный стол контейнера не имеют.
    assert_eq!(capacity(ObjectKind::Building(BuildingKind::Furnace)), Some(5));
    assert_eq!(capacity(ObjectKind::Building(BuildingKind::BlastFurnace)), Some(7));
    assert_eq!(capacity(ObjectKind::Building(BuildingKind::Barrel)), Some(containers::BARREL_SLOTS));
    assert_eq!(capacity(ObjectKind::Building(BuildingKind::Storage)), Some(containers::STORAGE_SLOTS));
    assert_eq!(capacity(ObjectKind::Building(BuildingKind::CraftingTable)), None);
    assert_eq!(capacity(ObjectKind::Building(BuildingKind::SmithingTable)), None);

    // Печь: вход принимает всё, топливо — только горючее.
    assert!(can_put_into(ObjectKind::Building(BuildingKind::Furnace), FURNACE_INPUT, ItemKind::Rock));
    assert!(!can_put_into(ObjectKind::Building(BuildingKind::Furnace), FURNACE_FUEL, ItemKind::Rock));
    assert!(can_put_into(ObjectKind::Building(BuildingKind::Furnace), FURNACE_FUEL, ItemKind::Stick));
    assert!(!can_put_into(ObjectKind::Building(BuildingKind::Furnace), FURNACE_WASTE[0], ItemKind::Rock));

    // Хранилище принимает любые предметы в любой слот.
    assert!(can_put_into(ObjectKind::Building(BuildingKind::Barrel), 17, ItemKind::Rock));
}

#[test]
fn station_move_item_between_container_and_player() {
    let (mut world, book) = furnace_world(BuildingKind::Furnace);
    let pos = TilePos { x: 10, y: 10 };
    let cooked = ItemKind::MeatHerbivoreCooked;
    put(&mut world, pos, FURNACE_OUTPUT, ItemStack { kind: cooked, count: 2 });

    let human = spawn_human_nearby(&mut world);
    let mut sched = crate::simulation::Scheduler::default();
    let mut queue = crate::simulation::CommandQueue::default();
    let mut dirty = DirtyTiles::default();

    // Из выхода (только забирать) в misc игрока — весь стак.
    queue.push(Command::StationMoveItem {
        pos,
        from: (FURNACE_OUTPUT as u8, 0),
        to: (SLOT_AREA_PLAYER_MISC, 0),
        count: 0,
    });
    queue.apply_all(&mut world, Some(human), &mut dirty, &mut sched, 0, &book);
    assert_eq!(mind_mut(&mut world, human).inventory.count_of(cooked), 2);
    assert_eq!(container_at(&world, pos).unwrap().get(FURNACE_OUTPUT), None);

    // Обратный перенос камня в топливный слот невозможен (не топливо):
    // камень остаётся в инвентаре, слот пуст.
    mind_mut(&mut world, human).inventory.add(ItemKind::Rock, 1);
    queue.push(Command::StationMoveItem {
        pos,
        from: (SLOT_AREA_PLAYER_MISC, 1),
        to: (FURNACE_FUEL as u8, 0),
        count: 0,
    });
    queue.apply_all(&mut world, Some(human), &mut dirty, &mut sched, 0, &book);
    assert_eq!(container_at(&world, pos).unwrap().get(FURNACE_FUEL), None);
    assert_eq!(mind_mut(&mut world, human).inventory.count_of(ItemKind::Rock), 1);

    // Уголь в топливный слот проходит.
    mind_mut(&mut world, human).inventory.add(ItemKind::Coal, 1);
    queue.push(Command::StationMoveItem {
        pos,
        from: (SLOT_AREA_PLAYER_MISC, 2),
        to: (FURNACE_FUEL as u8, 0),
        count: 0,
    });
    queue.apply_all(&mut world, Some(human), &mut dirty, &mut sched, 0, &book);
    assert_eq!(
        container_at(&world, pos).unwrap().get(FURNACE_FUEL).map(|s| s.count),
        Some(1)
    );
    assert_eq!(mind_mut(&mut world, human).inventory.count_of(ItemKind::Coal), 0);
}

/// Обмен занятых слотов: предмет игрока уезжает в контейнер, предмет
/// контейнера — к игроку. Регрессия: обмен использовал `put` (только
/// пустые слоты) и молча терял предмет игрока.
#[test]
fn station_move_swaps_occupied_slots() {
    let (mut world, book) = furnace_world(BuildingKind::Barrel);
    let pos = TilePos { x: 10, y: 10 };
    put(
        &mut world,
        pos,
        0,
        ItemStack { kind: ItemKind::Rock, count: 3 },
    );

    let human = spawn_human_nearby(&mut world);
    mind_mut(&mut world, human).inventory.items[0] =
        Some(ItemStack { kind: ItemKind::Apple, count: 5 });

    let mut sched = crate::simulation::Scheduler::default();
    let mut queue = crate::simulation::CommandQueue::default();
    let mut dirty = DirtyTiles::default();
    queue.push(Command::StationMoveItem {
        pos,
        from: (SLOT_AREA_PLAYER_MISC, 0),
        to: (0, 0),
        count: 0,
    });
    queue.apply_all(&mut world, Some(human), &mut dirty, &mut sched, 0, &book);

    // Обмен состоялся в обе стороны.
    assert_eq!(
        container_at(&world, pos).unwrap().get(0),
        Some(ItemStack { kind: ItemKind::Apple, count: 5 })
    );
    assert_eq!(
        mind_mut(&mut world, human).inventory.items[0],
        Some(ItemStack { kind: ItemKind::Rock, count: 3 })
    );
}
