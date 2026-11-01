//! Тесты строительства: списание предметов (`Inventory::take_items`) и
//! применение команды `PlayerBuild` (плата, проверка тайла).

use crate::simulation::crafts::CraftBook;
use crate::simulation::commands::{Command, CommandQueue, buildable_at};
use crate::simulation::dirty::DirtyTiles;
use crate::simulation::items::{ItemKind, ItemStack};
use crate::simulation::objects::{BuildingKind, WorldObject};
use crate::simulation::player::{Player, WorldPos};
use crate::simulation::scheduler::Scheduler;
use crate::simulation::tile::TileType;
use crate::simulation::world::{TilePos, World, WorldSize};

/// Мир 256×256 с травяным центром и игрок в его центре.
fn setup() -> (World, Player, TilePos) {
    let mut world = World::new(WorldSize::new(256), 1);
    let pos = TilePos { x: 128, y: 128 };
    world.set_terrain(pos, TileType::Grass);
    let player = Player::new(WorldPos::of_tile_center(pos));
    (world, player, pos)
}

/// Применяет очередь команд к миру с игроком.
fn apply(queue: &mut CommandQueue, world: &mut World, player: &mut Player) {
    let mut dirty = DirtyTiles::default();
    queue.apply_all(world, Some(player), &mut dirty, &mut Scheduler::default(), 0, &CraftBook::default());
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
    let (mut world, mut player, pos) = setup();
    player.inventory.add(ItemKind::Stick, 20);

    let mut queue = CommandQueue::default();
    queue.push(Command::PlayerBuild { pos, kind: BuildingKind::Campfire });
    apply(&mut queue, &mut world, &mut player);

    assert_eq!(world.object_at(pos), Some(&WorldObject::building(BuildingKind::Campfire)));
    assert_eq!(player.inventory.count_of(ItemKind::Stick), 4);
}

/// Без нужного количества предметов постройка не появляется и плата не тратится.
#[test]
fn build_requires_enough_items() {
    let (mut world, mut player, pos) = setup();
    player.inventory.add(ItemKind::Stick, 10);

    let mut queue = CommandQueue::default();
    queue.push(Command::PlayerBuild { pos, kind: BuildingKind::Campfire });
    apply(&mut queue, &mut world, &mut player);

    assert_eq!(world.object_at(pos), None);
    assert_eq!(player.inventory.count_of(ItemKind::Stick), 10);
}

/// Тайл постройки: не вода, без объекта и без лежащих предметов.
#[test]
fn buildable_tile_rules() {
    let (mut world, _player, pos) = setup();

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
    let (mut world, mut player, pos) = setup();
    player.inventory.add(ItemKind::Stick, 20);
    // 10 тайлов в сторону — за пределами INTERACT_REACH.
    let far = TilePos { x: pos.x + 10, y: pos.y };

    let mut queue = CommandQueue::default();
    queue.push(Command::PlayerBuild { pos: far, kind: BuildingKind::Campfire });
    apply(&mut queue, &mut world, &mut player);

    assert_eq!(world.object_at(far), None);
    assert_eq!(player.inventory.count_of(ItemKind::Stick), 20);
}


