//! Тесты существ: спавн и HP волков, автомат состояний, анимация ходьбы,
//! поведение в воде, команды Spawn/RemoveEntity.

use crate::simulation::crafts::CraftBook;
use crate::simulation::dirty::DirtyTiles;
use crate::simulation::tile::TileType;
use crate::simulation::world::{TilePos, World, WorldSize};

/// Волк появляется с HP вида (80) и стабильным ненулевым id.
#[test]
fn wolf_spawns_with_hp_and_unique_id() {
    use crate::simulation::entities::EntityKind;
    use crate::simulation::world::WorldPos;

    let mut world = World::new(WorldSize::new(256), 1);
    let first = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.5, y: 10.5 });
    let second = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 20.5, y: 20.5 });

    assert_ne!(first, 0);
    assert_ne!(first, second);
    let wolf = &world.entities.entities[0];
    assert_eq!(wolf.hp, 80.0);
    assert_eq!(wolf.kind.max_hp(), 80.0);
    assert_eq!(wolf.hp, wolf.kind.max_hp());
}

/// Автомат волка никогда не переходит между ходьбой и лежанием напрямую:
/// между ними обязательно есть стояние. За длинную симуляцию волк должен
/// побывать во всех трёх состояниях.
#[test]
fn wolf_always_stands_between_walking_and_lying() {
    use crate::simulation::entities::{EntityKind, EntityState};
    use crate::simulation::world::WorldPos;

    let mut world = World::new(WorldSize::new(256), 7);
    world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 100.5, y: 100.5 });

    let mut previous = world.entities.entities[0].state;
    let mut seen_standing = false;
    let mut seen_walking = false;
    let mut seen_lying = false;

    for tick in 1..=30_000_u64 {
        world.tick_entities(tick, None);
        let state = world.entities.entities[0].state;
        assert!(
            !matches!(
                (previous, state),
                (EntityState::Walking, EntityState::Lying)
                    | (EntityState::Lying, EntityState::Walking)
            ),
            "прямой переход {previous:?} → {state:?} на тике {tick}"
        );
        match state {
            EntityState::Standing => seen_standing = true,
            EntityState::Walking => seen_walking = true,
            EntityState::Lying => seen_lying = true,
        }
        previous = state;
    }

    assert!(seen_standing, "волк ни разу не стоял");
    assert!(seen_walking, "волк ни разу не ходил");
    assert!(seen_lying, "волк ни разу не лежал");
}

/// Волк не заходит в глубокую воду, даже если гуляет рядом с ней.
#[test]
fn wolf_never_enters_deep_water() {
    use crate::simulation::entities::{EntityKind, EntityState};
    use crate::simulation::world::WorldPos;

    let mut world = World::new(WorldSize::new(256), 3);
    for y in 0..256 {
        world.set_terrain(TilePos { x: 20, y }, TileType::DeepWater);
    }
    world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 15.5, y: 128.5 });

    let mut seen_walking = false;
    for tick in 1..=5000_u64 {
        world.tick_entities(tick, None);
        let wolf = &world.entities.entities[0];
        assert!(
            wolf.pos.x.floor() < 20.0,
            "волк вошёл в глубокую воду: x={} на тике {tick}",
            wolf.pos.x
        );
        seen_walking |= wolf.state == EntityState::Walking;
    }
    assert!(seen_walking, "волк должен был хотя бы раз пойти");
}

/// Анимация ходьбы меняет кадр раз в 0.5 секунды (10 тиков при 20 tps).
#[test]
fn wolf_walk_animation_frame_changes_every_half_second() {
    use crate::simulation::entities::{EntityKind, EntityState};
    use crate::simulation::world::WorldPos;

    let mut world = World::new(WorldSize::new(256), 1);
    world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 50.5, y: 50.5 });
    let wolf = &mut world.entities.entities[0];
    wolf.state = EntityState::Walking;

    for tick in 0..25_u64 {
        wolf.state_ticks = tick;
        let expected = ((tick / 10) % 2) as u8;
        assert_eq!(wolf.walk_frame(), expected, "кадр на тике {tick}");
    }
}

/// Волк идёт к цели со скоростью вида и разворачивается по направлению
/// движения: влево — `Facing::Left`, вправо — `Facing::Right`.
#[test]
fn wolf_walks_toward_target_and_turns() {
    use crate::simulation::entities::{EntityKind, EntityState, Facing};
    use crate::simulation::world::WorldPos;

    let mut world = World::new(WorldSize::new(256), 1);
    world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 100.5, y: 100.5 });
    {
        let wolf = &mut world.entities.entities[0];
        wolf.state = EntityState::Walking;
        wolf.state_ticks = 0;
        wolf.state_duration = 1000;
        wolf.target = WorldPos { x: 90.5, y: 100.5 };
    }

    world.tick_entities(1, None);
    let wolf = &world.entities.entities[0];
    assert_eq!(wolf.facing, Facing::Left);
    assert!(
        (wolf.pos.x - (100.5 - 0.2)).abs() < 1e-5,
        "волк должен пройти 0.2 тайла за тик, x={}",
        wolf.pos.x
    );

    {
        let wolf = &mut world.entities.entities[0];
        wolf.target = WorldPos { x: 110.5, y: 100.5 };
    }
    world.tick_entities(2, None);
    let wolf = &world.entities.entities[0];
    assert_eq!(wolf.facing, Facing::Right);
}

/// Спавн существ командой редактора создаёт существо в центре тайла.
#[test]
fn spawn_entity_command_places_wolf_at_tile_center() {
    use crate::simulation::entities::EntityKind;

    let mut world = World::new(WorldSize::new(256), 1);
    let mut queue = crate::simulation::CommandQueue::default();
    let pos = TilePos { x: 7, y: 9 };
    queue.push(crate::simulation::Command::SpawnEntity { kind: EntityKind::Wolf, pos });

    let mut dirty = DirtyTiles::default();
    let mut sched = crate::simulation::Scheduler::default();
    queue.apply_all(&mut world, None, &mut dirty, &mut sched, 0, &CraftBook::default());

    assert_eq!(world.entities.entities.len(), 1);
    let wolf = &world.entities.entities[0];
    assert_eq!(wolf.kind, EntityKind::Wolf);
    assert_eq!(wolf.pos.x, 7.5);
    assert_eq!(wolf.pos.y, 9.5);
    assert!(dirty.is_empty(), "спавн существа не меняет terrain-блок");
}

/// Удаление существа командой редактора убирает ровно его: остальные
/// существа мира остаются нетронутыми, повторное удаление безопасно.
#[test]
fn remove_entity_command_deletes_by_id() {
    use crate::simulation::commands::{Command, CommandQueue};
    use crate::simulation::entities::EntityKind;
    use crate::simulation::world::WorldPos;

    let mut world = World::new(WorldSize::new(256), 1);
    let mut queue = CommandQueue::default();
    let mut sched = crate::simulation::Scheduler::default();
    let first = world.entities.spawn(EntityKind::Wolf, WorldPos { x: 3.5, y: 4.5 });
    let second = world.entities.spawn(EntityKind::Wolf, WorldPos { x: 8.5, y: 9.5 });

    queue.push(Command::RemoveEntity { id: first });
    let mut dirty = DirtyTiles::default();
    queue.apply_all(&mut world, None, &mut dirty, &mut sched, 0, &CraftBook::default());

    let ids: Vec<u64> = world.entities.iter().map(|entity| entity.id).collect();
    assert_eq!(ids, vec![second], "удалено должно быть только существо first");
    assert!(dirty.is_empty(), "удаление существа не меняет terrain-блок");
    assert!(!world.entities.remove(first), "повторное удаление — no-op");
    assert_eq!(world.entities.len(), 1);
}

/// Перетаскивание существа командой редактора переносит его в точку,
/// выравнивает интерполяцию и сбрасывает состояние в стояние.
#[test]
fn move_entity_command_places_and_stands() {
    use crate::simulation::commands::{Command, CommandQueue};
    use crate::simulation::entities::{EntityKind, EntityState};
    use crate::simulation::world::WorldPos;

    let mut world = World::new(WorldSize::new(256), 1);
    let mut queue = CommandQueue::default();
    let mut sched = crate::simulation::Scheduler::default();
    let id = world.entities.spawn(EntityKind::Wolf, WorldPos { x: 3.5, y: 4.5 });

    let target = WorldPos { x: 12.25, y: 9.75 };
    queue.push(Command::MoveEntity { id, pos: target });
    let mut dirty = DirtyTiles::default();
    queue.apply_all(&mut world, None, &mut dirty, &mut sched, 0, &CraftBook::default());

    let wolf = &world.entities.entities[0];
    assert_eq!(wolf.pos, target);
    assert_eq!(wolf.prev_pos, target, "рендер не должен догонять старое место");
    assert_eq!(wolf.target, target);
    assert_eq!(wolf.state, EntityState::Standing);
    assert!(dirty.is_empty(), "перенос существа не меняет terrain-блок");
}

/// Волк не стоит и не лежит в воде: оказавшись в пруду, он сразу идёт
/// к берегу, а не отдыхает на воде.
#[test]
fn wolf_never_rests_in_water() {
    use crate::simulation::entities::{EntityKind, EntityState};
    use crate::simulation::world::WorldPos;

    let mut world = World::new(WorldSize::new(256), 5);
    for y in 50..55 {
        for x in 50..55 {
            world.set_terrain(TilePos { x, y }, TileType::Water);
        }
    }
    world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 52.5, y: 52.5 });

    let mut left_water = false;
    for tick in 1..=3000_u64 {
        world.tick_entities(tick, None);
        let wolf = &world.entities.entities[0];
        let tile = TilePos {
            x: wolf.pos.x.floor() as i32,
            y: wolf.pos.y.floor() as i32,
        };
        if world.terrain_kind(tile).is_water() {
            assert_eq!(
                wolf.state,
                EntityState::Walking,
                "волк отдыхает в воде на тике {tick}"
            );
        } else {
            left_water = true;
        }
    }
    assert!(left_water, "волк так и не выбрался на сушу");
}

/// После долгого плавания волк бросает прежнюю водную цель и идёт
/// кратчайшим путём к ближайшей суше.
#[test]
fn wolf_water_panic_retargets_nearest_land() {
    use crate::simulation::entities::{EntityKind, EntityState};
    use crate::simulation::world::WorldPos;

    let mut world = World::new(WorldSize::new(256), 5);
    for y in 50..55 {
        for x in 50..55 {
            world.set_terrain(TilePos { x, y }, TileType::Water);
        }
    }
    world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 52.5, y: 52.5 });
    {
        let wolf = &mut world.entities.entities[0];
        wolf.state = EntityState::Walking;
        wolf.state_ticks = 0;
        wolf.state_duration = 10_000;
        wolf.target = WorldPos { x: 53.5, y: 53.5 };
        wolf.water_ticks = u16::MAX;
    }

    world.tick_entities(1, None);
    let wolf = &world.entities.entities[0];
    let target = TilePos {
        x: wolf.target.x.floor() as i32,
        y: wolf.target.y.floor() as i32,
    };
    assert!(!world.terrain_kind(target).is_water(), "цель должна быть на суше");
    assert_eq!(wolf.state, EntityState::Walking, "волк должен плыть к берегу");
}

/// Модификаторы рельефа — общие правила мира: существа замедляются на горе
/// вдвое и на мелководье втрое, как и игрок.
#[test]
fn wolf_slows_down_on_mountain_and_in_shallow_water() {
    use crate::simulation::entities::{EntityKind, EntityState};
    use crate::simulation::world::WorldPos;

    // Гора: шаг 0.2 / MOUNTAIN_SLOWDOWN = 0.1 тайла за тик.
    let mut world = World::new(WorldSize::new(256), 1);
    let mountain = TilePos { x: 100, y: 100 };
    world.set_terrain(mountain, TileType::Mountain);
    world
        .entities
        .spawn(EntityKind::Wolf, WorldPos::of_tile_center(mountain));
    {
        let wolf = &mut world.entities.entities[0];
        wolf.state = EntityState::Walking;
        wolf.state_ticks = 0;
        wolf.state_duration = 1000;
        wolf.target = WorldPos { x: 110.5, y: 100.5 };
    }
    world.tick_entities(1, None);
    let wolf = &world.entities.entities[0];
    assert!(
        (wolf.pos.x - 100.6).abs() < 1e-5,
        "на горе шаг должен быть вдвое короче, x = {}",
        wolf.pos.x
    );

    // Мелководье: шаг 0.2 / WADE_SLOWDOWN ≈ 0.0667 тайла за тик.
    let mut world = World::new(WorldSize::new(256), 1);
    let water = TilePos { x: 100, y: 110 };
    world.set_terrain(water, TileType::Water);
    world
        .entities
        .spawn(EntityKind::Wolf, WorldPos::of_tile_center(water));
    {
        let wolf = &mut world.entities.entities[0];
        wolf.state = EntityState::Walking;
        wolf.state_ticks = 0;
        wolf.state_duration = 1000;
        wolf.target = WorldPos { x: 110.5, y: 110.5 };
    }
    world.tick_entities(1, None);
    let wolf = &world.entities.entities[0];
    let expected = 100.5 + 0.2 / 3.0;
    assert!(
        (wolf.pos.x - expected).abs() < 1e-5,
        "вброд шаг должен быть втрое короче, x = {}",
        wolf.pos.x
    );
}
