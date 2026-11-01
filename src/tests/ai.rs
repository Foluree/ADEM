//! Тесты автономного поведения людей (ИИ выживания, simulation/ai.rs):
//! голодный человек ищет еду, ночью идёт к костру.

use crate::simulation::entities::{EntityKind, HumanGoal};
use crate::simulation::hunger::Hunger;
use crate::simulation::items::ItemKind;
use crate::simulation::objects::{BuildingKind, WorldObject};
use crate::simulation::tile::TileType;
use crate::simulation::world::{TilePos, World, WorldPos, WorldSize};

/// Тик, соответствующий началу часа `hour` первых суток (1 игровой час
/// = 300 тиков: 3600 игровых секунд / 12 секунд на тик).
fn hour_tick(hour: u32) -> u64 {
    u64::from(hour) * 300
}

/// Голодный человек рядом с яблоней: ИИ ставит цель «еда», собирает яблоки
/// в инвентарь и ест их — сытость растёт.
#[test]
fn hungry_human_seeks_and_harvests_apples() {
    let mut world = World::new(WorldSize::new(256), 1);
    let human_pos = TilePos { x: 10, y: 10 };
    let tree_pos = TilePos { x: 11, y: 10 };
    // Суша вокруг: сгенерированный мир пуст, рельеф выставляем явно.
    for pos in [
        human_pos,
        tree_pos,
        TilePos { x: 12, y: 10 },
        TilePos { x: 10, y: 11 },
        TilePos { x: 11, y: 11 },
    ] {
        world.set_terrain(pos, TileType::Grass);
    }
    world.set_object(tree_pos, Some(WorldObject::apple_tree_with_fruit(7)));
    let id = world
        .entities
        .spawn(EntityKind::Human, WorldPos::of_tile_center(human_pos));
    world
        .entities
        .get_mut(id)
        .unwrap()
        .hunger = Hunger::with_value(30.0);

    // Дневные тики (старт мира — 8:00): ИИ ищет еду, собирает яблоки и ест.
    let start = crate::simulation::time::start_tick() + 10;
    let mut saw_food_goal = false;
    let mut saw_apples = false;
    for tick in start..start + 200 {
        world.tick_entities(tick, None);
        let entity = world.entities.get(id).unwrap();
        saw_food_goal |= matches!(
            entity.mind.and_then(|m| m.goal),
            Some(HumanGoal::Food(_))
        );
        saw_apples |= entity
            .mind
            .map(|m| m.inventory.count_of(ItemKind::Apple) > 0)
            .unwrap_or(false);
    }

    assert!(
        saw_food_goal || saw_apples,
        "голодный человек должен был искать еду (яблоня в радиусе поиска)"
    );
    let hunger = world
        .entities
        .get(id)
        .unwrap()
        .hunger
        .value();
    assert!(hunger > 30.0, "человек должен был поесть, hunger = {hunger}");
}

/// Ночью (час ≥ 21) человек в радиусе поиска выбирает цель «костёр»,
/// доходит и стоит рядом до утра.
#[test]
fn human_walks_to_campfire_at_night() {    let mut world = World::new(WorldSize::new(256), 1);
    let human_pos = TilePos { x: 20, y: 20 };
    let fire_pos = TilePos { x: 25, y: 20 };
    for x in 18..=27 {
        world.set_terrain(TilePos { x, y: 20 }, TileType::Grass);
    }
    world.set_object(fire_pos, Some(WorldObject::building(BuildingKind::Campfire)));
    let id = world
        .entities
        .spawn(EntityKind::Human, WorldPos::of_tile_center(human_pos));

    // Тики 22:00 (ночь): ИИ находит костёр и идёт к нему.
    let start = hour_tick(22);
    let mut saw_fire_goal = false;
    for tick in start..start + 200 {
        world.tick_entities(tick, None);
        saw_fire_goal |= matches!(
            world.entities.get(id).unwrap().mind.and_then(|m| m.goal),
            Some(HumanGoal::Campfire(_))
        );
    }
    assert!(saw_fire_goal, "ночью человек должен выбрать цель «костёр»");

    // За ночь человек дошёл и стоит у огня (в кольце прибытия).
    let dist = world
        .entities
        .get(id)
        .unwrap()
        .pos
        .dist(WorldPos::of_tile_center(fire_pos));
    assert!(dist <= 1.5, "человек должен дойти до костра, дистанция {dist}");
}

/// Сытый человек днём без еды вокруг не стоит на месте: ИИ отправляет его
/// в брожение. Регрессия на вечное сброс стояния FSM'ом, из-за которого
/// люди простаивали.
#[test]
fn idle_human_wanders_during_day() {
    let mut world = World::new(WorldSize::new(256), 1);
    let human_pos = TilePos { x: 20, y: 20 };
    for y in 12..=28 {
        for x in 12..=28 {
            world.set_terrain(TilePos { x, y }, TileType::Grass);
        }
    }
    let id = world
        .entities
        .spawn(EntityKind::Human, WorldPos::of_tile_center(human_pos));

    // Дневные тики, еды в радиусе поиска нет: человек должен сместиться.
    let start = crate::simulation::time::start_tick() + 10;
    for tick in start..start + 600 {
        world.tick_entities(tick, None);
    }
    let dist = world
        .entities
        .get(id)
        .unwrap()
        .pos
        .dist(WorldPos::of_tile_center(human_pos));
    assert!(dist > 1.0, "сытый человек должен бродить, сместился на {dist}");
}
