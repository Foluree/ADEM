//! Дроп-сущности предметов: разброс, слияние стеков, автоподбор игроком,
//! исчезновение по возрасту. Слой предметов на тайлах (`Chunk.items`)
//! этими сущностями не затрагивается.

use crate::simulation::item_entity::{ItemEntities, PICKUP_DELAY_TICKS, PICKUP_RADIUS};
use crate::simulation::items::{ItemStack, MAX_STACK, MISC_SLOTS};
use crate::simulation::player::{Player, WorldPos};

const SEED: u64 = 7;

#[test]
fn drop_scatters_within_radius_and_assigns_ids() {
    let mut items = ItemEntities::default();
    let pos = WorldPos { x: 10.0, y: 10.0 };
    let stack = ItemStack { kind: crate::simulation::items::ItemKind::Bone, count: 1 };
    let id = items.drop_stack(SEED, pos, stack, 100);
    assert!(id > 0);
    let entity = items.get(id).unwrap();
    // Разброс не дальше SCATTER_TILES от точки дропа.
    assert!(entity.pos.dist(pos) <= 0.31);
    // Подбор разрешён не сразу.
    assert_eq!(entity.pickup_after_tick, 100 + PICKUP_DELAY_TICKS);
}

#[test]
fn nearby_same_kind_stacks_merge() {
    let mut items = ItemEntities::default();
    let pos = WorldPos { x: 10.0, y: 10.0 };
    let kind = crate::simulation::items::ItemKind::Bone;
    let first = items.drop_stack(SEED, pos, ItemStack { kind, count: 2 }, 100);
    let second = items.drop_stack(SEED, pos, ItemStack { kind, count: 3 }, 101);
    // Первый дроп получил id, второй влился в него.
    assert!(first > 0);
    assert_eq!(second, 0);
    assert_eq!(items.len(), 1);
    assert_eq!(items.items[0].stack.count, 5);
    // Далёкий дроп того же вида не сливается.
    let far = WorldPos { x: 12.0, y: 12.0 };
    assert!(items.drop_stack(SEED, far, ItemStack { kind, count: 1 }, 102) > 0);
    assert_eq!(items.len(), 2);
}

#[test]
fn player_picks_up_nearby_drops_after_delay() {
    let mut items = ItemEntities::default();
    let mut player = Player::new(WorldPos { x: 10.0, y: 10.0 });
    let kind = crate::simulation::items::ItemKind::MeatPredatorRaw;
    items.drop_stack(SEED, WorldPos { x: 10.2, y: 10.2 }, ItemStack { kind, count: 3 }, 0);

    // Раньше задержки — предмет лежит.
    items.tick(PICKUP_DELAY_TICKS - 1, Some(&mut player));
    assert_eq!(items.len(), 1);
    assert_eq!(player.inventory.count_of(kind), 0);

    // После задержки — подобран целиком.
    items.tick(PICKUP_DELAY_TICKS, Some(&mut player));
    assert!(items.is_empty());
    assert_eq!(player.inventory.count_of(kind), 3);
}

#[test]
fn pickup_keeps_leftover_when_inventory_full() {
    let mut items = ItemEntities::default();
    let mut player = Player::new(WorldPos { x: 10.0, y: 10.0 });
    let kind = crate::simulation::items::ItemKind::MeatPredatorRaw;
    // Инвентарь забит: влезает только 5 единиц (долив в стек того же вида).
    player.inventory.add(kind, MISC_SLOTS as u32 * MAX_STACK);
    player.inventory.items[0] = Some(ItemStack { kind, count: MAX_STACK - 5 });
    for slot in player.inventory.items.iter_mut().skip(1) {
        *slot = Some(ItemStack { kind: crate::simulation::items::ItemKind::Rock, count: MAX_STACK });
    }
    items.drop_stack(SEED, WorldPos { x: 10.1, y: 10.1 }, ItemStack { kind, count: 8 }, 0);

    items.tick(PICKUP_DELAY_TICKS, Some(&mut player));
    // Взято 5, остаток 3 лежит на земле с новой задержкой подбора.
    assert_eq!(player.inventory.count_of(kind), MAX_STACK);
    assert_eq!(items.len(), 1);
    assert_eq!(items.items[0].stack.count, 3);
    assert!(items.items[0].pickup_after_tick > PICKUP_DELAY_TICKS);
}

#[test]
fn distant_drops_are_not_picked_up() {
    let mut items = ItemEntities::default();
    let mut player = Player::new(WorldPos { x: 10.0, y: 10.0 });
    let kind = crate::simulation::items::ItemKind::MeatPredatorRaw;
    items.drop_stack(
        SEED,
        WorldPos { x: 10.0 + PICKUP_RADIUS + 1.0, y: 10.0 },
        ItemStack { kind, count: 1 },
        0,
    );
    items.tick(PICKUP_DELAY_TICKS * 10, Some(&mut player));
    assert_eq!(items.len(), 1);
    assert_eq!(player.inventory.count_of(kind), 0);
}

#[test]
fn old_drops_despawn() {
    let mut items = ItemEntities::default();
    let kind = crate::simulation::items::ItemKind::MeatPredatorRaw;
    items.drop_stack(SEED, WorldPos { x: 10.0, y: 10.0 }, ItemStack { kind, count: 1 }, 0);
    // Без игрока despawn продолжается (наблюдатель).
    items.tick(7201, None);
    assert!(items.is_empty());
}
