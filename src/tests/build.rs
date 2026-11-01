//! Тесты строительства: списание предметов (`Inventory::take_items`) и
//! применение команды `PlayerBuild` (плата, проверка тайла). Игрок —
//! захваченное существо: команды исполняются человеком мира по id.

use crate::simulation::actions;
use crate::simulation::crafts::CraftBook;
use crate::simulation::commands::{Command, CommandQueue, buildable_at};
use crate::simulation::dirty::DirtyTiles;
use crate::simulation::entities::EntityKind;
use crate::simulation::items::{ItemKind, ItemStack};
use crate::simulation::objects::{BuildingKind, WorldObject};
use crate::simulation::scheduler::Scheduler;
use crate::simulation::tile::TileType;
use crate::simulation::world::{TilePos, World, WorldPos, WorldSize};

/// Мир 256×256 с травяным центром и человеком в его центре.
fn setup() -> (World, u64, TilePos) {
    let mut world = World::new(WorldSize::new(256), 1);
    let pos = TilePos { x: 128, y: 128 };
    world.set_terrain(pos, TileType::Grass);
    let actor = world
        .entities
        .spawn(EntityKind::Human, WorldPos::of_tile_center(pos));
    (world, actor, pos)
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

/// Сколько единиц предмета в инвентаре существа.
fn count_of(world: &World, actor: u64, kind: ItemKind) -> u32 {
    world
        .entities
        .get(actor)
        .unwrap()
        .mind
        .as_ref()
        .unwrap()
        .inventory
        .count_of(kind)
}

/// Применяет очередь команд к миру от имени существа `actor`.
fn apply(queue: &mut CommandQueue, world: &mut World, actor: u64) {
    let mut dirty = DirtyTiles::default();
    queue.apply_all(world, Some(actor), &mut dirty, &mut Scheduler::default(), 0, &CraftBook::default());
}

/// Списание раскладывается по стекам: 16 палок берутся из двух стеков.
#[test]
fn take_items_spreads_across_stacks() {
    let mut inv = crate::simulation::Inventory::default();
    inv.items[0] = Some(ItemStack { kind: ItemKind::Stick, count: 10 });
    inv.items[3] = Some(ItemStack { kind: ItemKind::Stick, count: 10 });

    assert!(inv.take_items(ItemKind::Stick, 16));
    assert_eq!(inv.count_of(ItemKind::Stick), 4);
    // Первый стек опустел и освободился, во втором осталось 4.
    assert!(inv.items[0].is_none());
    assert_eq!(inv.items[3], Some(ItemStack { kind: ItemKind::Stick, count: 4 }));
}

/// Нехватка предметов списывает ничего: стройка не должна «есть» частями.
#[test]
fn take_items_insufficient_takes_nothing() {
    let mut inv = crate::simulation::Inventory::default();
    inv.items[0] = Some(ItemStack { kind: ItemKind::Stick, count: 10 });

    assert!(!inv.take_items(ItemKind::Stick, 16));
    assert_eq!(inv.count_of(ItemKind::Stick), 10);
    assert_eq!(
        inv.items[0],
        Some(ItemStack { kind: ItemKind::Stick, count: 10 })
    );
}

/// Строительство списывает плату и ставит костёр на тайл.
#[test]
fn build_deducts_cost_and_places_campfire() {
    let (mut world, actor, pos) = setup();
    mind_mut(&mut world, actor).inventory.add(ItemKind::Stick, 20);

    let mut queue = CommandQueue::default();
    queue.push(Command::PlayerBuild { pos, kind: BuildingKind::Campfire });
    apply(&mut queue, &mut world, actor);

    assert_eq!(world.object_at(pos), Some(&WorldObject::building(BuildingKind::Campfire)));
    assert_eq!(count_of(&world, actor, ItemKind::Stick), 4);
}

/// Без нужного количества предметов постройка не появляется и плата не тратится.
#[test]
fn build_requires_enough_items() {
    let (mut world, actor, pos) = setup();
    mind_mut(&mut world, actor).inventory.add(ItemKind::Stick, 10);

    let mut queue = CommandQueue::default();
    queue.push(Command::PlayerBuild { pos, kind: BuildingKind::Campfire });
    apply(&mut queue, &mut world, actor);

    assert_eq!(world.object_at(pos), None);
    assert_eq!(count_of(&world, actor, ItemKind::Stick), 10);
}

/// Тайл постройки: не вода, без объекта и без лежащих предметов.
#[test]
fn buildable_tile_rules() {
    let (mut world, _actor, pos) = setup();

    assert!(buildable_at(&world, pos));

    world.set_terrain(pos, TileType::Water);
    assert!(!buildable_at(&world, pos), "на воде строить нельзя");
    world.set_terrain(pos, TileType::Grass);

    world.set_object(pos, Some(WorldObject::tree()));
    assert!(!buildable_at(&world, pos), "тайл занят объектом");
    world.set_object(pos, None);

    world.set_item(pos, Some(ItemStack { kind: ItemKind::Stick, count: 1 }));
    assert!(!buildable_at(&world, pos), "тайл занят предметами");
}

/// Постройка вне радиуса взаимодействия невозможна.
#[test]
fn build_out_of_reach_fails() {
    let (mut world, actor, pos) = setup();
    mind_mut(&mut world, actor).inventory.add(ItemKind::Stick, 20);
    // 10 тайлов в сторону — за пределами INTERACT_REACH.
    let far = TilePos { x: pos.x + 10, y: pos.y };

    let mut queue = CommandQueue::default();
    queue.push(Command::PlayerBuild { pos: far, kind: BuildingKind::Campfire });
    apply(&mut queue, &mut world, actor);

    assert_eq!(world.object_at(far), None);
    assert_eq!(count_of(&world, actor, ItemKind::Stick), 20);
}

// --- Ломание построек зажатием ЛКМ ---

/// Мир с костром на тайле `pos`, человеком рядом и предметом в контейнере
/// костра (дроп содержимого проверяется на нём).
fn setup_building() -> (World, u64, TilePos) {
    let (mut world, actor, pos) = setup();
    world.set_object_with_container(pos, Some(WorldObject::building(BuildingKind::Campfire)));
    let mut container = crate::simulation::containers::Container::new(
        crate::simulation::ObjectKind::Building(BuildingKind::Campfire),
    );
    container.put(0, ItemStack { kind: ItemKind::Rock, count: 3 });
    world.set_container(pos, Some(container));
    (world, actor, pos)
}

/// 80 тиков непрерывного зажатия ломают постройку; на землю падают цена
/// постройки и содержимое контейнера.
#[test]
fn building_breaks_after_80_ticks_of_holding() {
    let (mut world, actor, pos) = setup_building();

    for tick in 0..actions::BUILDING_BREAK_TICKS {
        assert!(!actions::break_building(&mut world, actor, pos, tick));
        assert!(world.object_at(pos).is_some(), "постройка рухнула раньше времени");
    }
    assert!(actions::break_building(&mut world, actor, pos, actions::BUILDING_BREAK_TICKS));

    assert!(world.object_at(pos).is_none());
    assert!(world.container_at(pos).is_none(), "контейнер уходит вместе с постройкой");
    // Дроп: 16 палок цены + 3 камня из контейнера.
    assert_eq!(world.item_entities.len(), 2);
}

/// Пауза в зажатии дольше нескольких тиков сбрасывает прогресс.
#[test]
fn building_break_progress_resets_after_gap() {
    let (mut world, actor, pos) = setup_building();

    // 39 тиков зажатия, затем разрыв в 10 тиков.
    for tick in 0..39 {
        assert!(!actions::break_building(&mut world, actor, pos, tick));
    }
    // Зажатие с тика 49: прогресс с нуля — рухнет на тике 49 + 80.
    for tick in 49..129 {
        assert!(!actions::break_building(&mut world, actor, pos, tick));
    }
    assert!(actions::break_building(&mut world, actor, pos, 129));
    assert!(world.object_at(pos).is_none());
}

/// Удары по постройке вне радиуса не копят прогресс (и после долгой
/// паузы вне радиуса старый прогресс сгорает).
#[test]
fn building_break_out_of_reach_resets() {
    let (mut world, actor, pos) = setup_building();
    let far = TilePos { x: pos.x + 10, y: pos.y };

    for tick in 0..39 {
        assert!(!actions::break_building(&mut world, actor, pos, tick));
    }
    // Долгий «замах» вне радиуса: прогресс не двигается.
    for tick in 39..100 {
        assert!(!actions::break_building(&mut world, actor, far, tick));
        assert!(world.object_at(pos).is_some());
    }
    // Возврат к постройке — заново: первый удар даёт 0 тиков, рухнет
    // на тике 100 + 80.
    for tick in 100..180 {
        assert!(!actions::break_building(&mut world, actor, pos, tick));
    }
    assert!(actions::break_building(&mut world, actor, pos, 180));
    assert!(world.object_at(pos).is_none());
}

/// Природные объекты ломанием построек не трогаются.
#[test]
fn building_break_ignores_natural_objects() {
    let (mut world, actor, pos) = setup();
    world.set_object(pos, Some(WorldObject::tree()));

    for tick in 0..actions::BUILDING_BREAK_TICKS + 10 {
        assert!(!actions::break_building(&mut world, actor, pos, tick));
    }
    assert_eq!(world.object_at(pos), Some(&WorldObject::tree()));
}

