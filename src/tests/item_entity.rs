//! Дроп-сущности предметов: разброс, слияние стеков, автоподбор человеком,
//! исчезновение по возрасту. Слой предметов на тайлах (`Chunk.items`)
//! этими сущностями не затрагивается. Автоподбор выполняет полный тик мира
//! (`World::tick_item_entities`): подбирает ближайший человек в радиусе.

use crate::simulation::entities::EntityKind;
use crate::simulation::item_entity::{ItemEntities, PICKUP_DELAY_TICKS, PICKUP_RADIUS};
use crate::simulation::items::{ItemStack, MAX_STACK, MISC_SLOTS};
use crate::simulation::world::{World, WorldPos, WorldSize};

const SEED: u64 = 7;

/// Мир с человеком в точке (10, 10) и его id.
fn world_with_human_at_drop_point() -> (World, u64) {
    let mut world = World::new(WorldSize::new(256), SEED);
    let human = world
        .entities
        .spawn(EntityKind::Human, WorldPos { x: 10.0, y: 10.0 });
    (world, human)
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
fn human_picks_up_nearby_drops_after_delay() {
    let (mut world, human) = world_with_human_at_drop_point();
    let kind = crate::simulation::items::ItemKind::MeatPredatorRaw;
    world.item_entities.drop_stack(
        SEED,
        WorldPos { x: 10.2, y: 10.2 },
        ItemStack { kind, count: 3 },
        0,
    );

    // Раньше задержки — предмет лежит.
    world.tick_item_entities(PICKUP_DELAY_TICKS - 1);
    assert_eq!(world.item_entities.len(), 1);
    assert_eq!(
        mind_mut(&mut world, human).inventory.count_of(kind),
        0
    );

    // После задержки — подобран целиком.
    world.tick_item_entities(PICKUP_DELAY_TICKS);
    assert!(world.item_entities.is_empty());
    assert_eq!(mind_mut(&mut world, human).inventory.count_of(kind), 3);
}

#[test]
fn pickup_keeps_leftover_when_inventory_full() {
    let (mut world, human) = world_with_human_at_drop_point();
    let kind = crate::simulation::items::ItemKind::MeatPredatorRaw;
    // Инвентарь забит: влезает только 5 единиц (долив в стек того же вида).
    {
        let m = mind_mut(&mut world, human);
        m.inventory.add(kind, MISC_SLOTS as u32 * MAX_STACK);
        m.inventory.items[0] = Some(ItemStack { kind, count: MAX_STACK - 5 });
        for slot in m.inventory.items.iter_mut().skip(1) {
            *slot = Some(ItemStack { kind: crate::simulation::items::ItemKind::Rock, count: MAX_STACK });
        }
    }
    world.item_entities.drop_stack(
        SEED,
        WorldPos { x: 10.1, y: 10.1 },
        ItemStack { kind, count: 8 },
        0,
    );

    world.tick_item_entities(PICKUP_DELAY_TICKS);
    // Взято 5, остаток 3 лежит на земле с новой задержкой подбора.
    assert_eq!(
        mind_mut(&mut world, human).inventory.count_of(kind),
        MAX_STACK
    );
    assert_eq!(world.item_entities.len(), 1);
    assert_eq!(world.item_entities.items[0].stack.count, 3);
    assert!(world.item_entities.items[0].pickup_after_tick > PICKUP_DELAY_TICKS);
}

#[test]
fn distant_drops_are_not_picked_up() {
    let (mut world, human) = world_with_human_at_drop_point();
    let kind = crate::simulation::items::ItemKind::MeatPredatorRaw;
    world.item_entities.drop_stack(
        SEED,
        WorldPos { x: 10.0 + PICKUP_RADIUS + 1.0, y: 10.0 },
        ItemStack { kind, count: 1 },
        0,
    );
    world.tick_item_entities(PICKUP_DELAY_TICKS * 10);
    assert_eq!(world.item_entities.len(), 1);
    assert_eq!(mind_mut(&mut world, human).inventory.count_of(kind), 0);
}

#[test]
fn old_drops_despawn() {
    let mut items = ItemEntities::default();
    let kind = crate::simulation::items::ItemKind::MeatPredatorRaw;
    items.drop_stack(SEED, WorldPos { x: 10.0, y: 10.0 }, ItemStack { kind, count: 1 }, 0);
    // Без людей despawn продолжается (наблюдатель).
    items.tick(7201);
    assert!(items.is_empty());
}

/// Волк ест лёжащее мясо с земли: единица за тик, без инвентаря.
#[test]
fn wolf_eats_meat_drop_on_ground() {
    let mut world = World::new(WorldSize::new(256), SEED);
    let wolf = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.0, y: 10.0 });
    world.entities.get_mut(wolf).unwrap().hunger =
        crate::simulation::Hunger::with_value(50.0);
    let kind = crate::simulation::items::ItemKind::MeatHerbivoreRaw;
    world.item_entities.drop_stack(
        SEED,
        WorldPos { x: 10.2, y: 10.0 },
        ItemStack { kind, count: 3 },
        100,
    );

    // До задержки подбора волк не ест; после — съедает единицу за тик.
    world.tick_item_entities(110);
    assert_eq!(
        world.entities.get(wolf).unwrap().hunger.value(),
        50.0,
        "до окончания задержки дропа мясо не съедается"
    );
    world.tick_item_entities(130);
    let hunger = world.entities.get(wolf).unwrap().hunger.value();
    assert!(hunger > 50.0, "волк должен был съесть мясо, сытость {hunger}");
    assert_eq!(world.item_entities.len(), 1, "стек мяса не доеден за тик");
    assert_eq!(world.item_entities.items[0].stack.count, 2);
}

/// Волк не ест немясные предметы: лежащие рядом палки остаются на месте.
#[test]
fn wolf_ignores_non_meat_drops() {
    let mut world = World::new(WorldSize::new(256), SEED);
    let wolf = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.0, y: 10.0 });
    world.entities.get_mut(wolf).unwrap().hunger =
        crate::simulation::Hunger::with_value(50.0);
    let kind = crate::simulation::items::ItemKind::Stick;
    world.item_entities.drop_stack(
        SEED,
        WorldPos { x: 10.1, y: 10.0 },
        ItemStack { kind, count: 3 },
        0,
    );
    world.tick_item_entities(PICKUP_DELAY_TICKS * 10);
    assert_eq!(world.item_entities.len(), 1);
    assert_eq!(world.item_entities.items[0].stack.count, 3);
    assert_eq!(world.entities.get(wolf).unwrap().hunger.value(), 50.0);
}
