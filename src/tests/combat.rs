use super::*;
use crate::simulation::entities::{EntityKind, HUMAN_MAX_HP, WOLF_BITE_DAMAGE};
use crate::simulation::generation::generate_world;
use crate::simulation::world::WorldSize;

/// Мир без воды и существ для боевых тестов.
fn flat_world() -> World {
    let mut world = generate_world(WorldSize::new(256), 7);
    world.entities = Default::default();
    world
}

/// Человек с предметом в выбранном слоте (ум создаётся при спавне).
fn human_with_item(world: &mut World, pos: WorldPos, kind: ItemKind) -> u64 {
    let human = world.entities.spawn(EntityKind::Human, pos);
    world
        .entities
        .get_mut(human)
        .unwrap()
        .mind
        .as_mut()
        .unwrap()
        .inventory
        .add(kind, 1);
    human
}

/// Укус захваченного волка: урон волчьими характеристиками, кулдаун
/// на сущности (общий с ответными кусами ИИ).
#[test]
fn wolf_bite_damages_and_cooldowns() {
    let mut world = flat_world();
    let wolf = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.0, y: 10.0 });
    let victim = world
        .entities
        .spawn(EntityKind::Human, WorldPos { x: 10.8, y: 10.0 });

    // Укус по тайлу жертвы: 8 урона, кулдаун — 20 тиков на сущности.
    assert!(wolf_bite(&mut world, wolf, TilePos { x: 10, y: 10 }, 100));
    assert_eq!(
        world.entities.get(victim).unwrap().hp,
        HUMAN_MAX_HP - WOLF_BITE_DAMAGE
    );
    assert_eq!(world.entities.get(wolf).unwrap().next_bite_tick, 120);

    // Кулдаун общий: на том же тике второй укус невозможен.
    assert!(!wolf_bite(&mut world, wolf, TilePos { x: 10, y: 10 }, 101));
    // Ровно на границе кулдауна волк снова кусает.
    assert!(wolf_bite(&mut world, wolf, TilePos { x: 10, y: 10 }, 120));
    assert_eq!(
        world.entities.get(victim).unwrap().hp,
        HUMAN_MAX_HP - 2.0 * WOLF_BITE_DAMAGE
    );
}

/// Мимо существа или вне досягаемости укуса — укуса нет, кулдаун не
/// списывается (как у удара оружием).
#[test]
fn wolf_bite_miss_and_range() {
    let mut world = flat_world();
    let wolf = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.0, y: 10.0 });

    // Пустой тайл: кусать нечего.
    assert!(!wolf_bite(&mut world, wolf, TilePos { x: 10, y: 10 }, 100));
    assert_eq!(world.entities.get(wolf).unwrap().next_bite_tick, 0);

    // Человек дальше досягаемости укуса (1.4 тайла).
    world
        .entities
        .spawn(EntityKind::Human, WorldPos { x: 12.0, y: 10.0 });
    assert!(!wolf_bite(&mut world, wolf, TilePos { x: 12, y: 10 }, 100));
    assert_eq!(world.entities.get(wolf).unwrap().next_bite_tick, 0);
}

/// Смертельный укус убивает жертву общим путём урона: инвентарь падает
/// на землю дроп-сущностями.
#[test]
fn wolf_bite_kills_and_drops_inventory() {
    let mut world = flat_world();
    let wolf = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.0, y: 10.0 });
    let victim = human_with_item(&mut world, WorldPos { x: 10.8, y: 10.0 }, ItemKind::Apple);
    world.entities.get_mut(victim).unwrap().hp = WOLF_BITE_DAMAGE - 1.0;

    assert!(wolf_bite(&mut world, wolf, TilePos { x: 10, y: 10 }, 100));
    assert!(world.entities.get(victim).is_none(), "жертва погибла");
    assert!(
        world
            .item_entities
            .items
            .iter()
            .any(|drop| drop.stack.kind == ItemKind::Apple),
        "инвентарь жертвы выпал на землю"
    );
}

/// Человек кусаться не может: у него удар оружием/рукой (PlayerInteract).
#[test]
fn wolf_bite_rejected_for_human() {
    let mut world = flat_world();
    let human = human_with_item(&mut world, WorldPos { x: 10.0, y: 10.0 }, ItemKind::KnifePrimal);
    let wolf = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.8, y: 10.0 });

    assert!(!wolf_bite(&mut world, human, TilePos { x: 10, y: 10 }, 100));
    assert_eq!(world.entities.get(wolf).unwrap().hp, EntityKind::Wolf.max_hp());
}

#[test]
fn melee_attack_damages_and_kills_wolf() {
    let mut world = flat_world();
    let id = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.0, y: 10.0 });
    // Нож в выбранном слоте: 5 урона.
    let human = human_with_item(&mut world, WorldPos { x: 10.0, y: 11.0 }, ItemKind::KnifePrimal);

    assert!(try_melee_attack(
        &mut world,
        human,
        TilePos { x: 10, y: 10 },
        100
    ));
    assert_eq!(world.entities.iter().find(|e| e.id == id).unwrap().hp, 75.0);
    // Кулдаун 6 тиков: на том же тике удар невозможен.
    assert!(!try_melee_attack(
        &mut world,
        human,
        TilePos { x: 10, y: 10 },
        101
    ));
    assert!(try_melee_attack(
        &mut world,
        human,
        TilePos { x: 10, y: 10 },
        106
    ));
    // Третий удар (ровно на границе кулдауна) выбивает волка до 65 HP.
    assert!(try_melee_attack(
        &mut world,
        human,
        TilePos { x: 10, y: 10 },
        112
    ));
    assert_eq!(
        world.entities.iter().find(|e| e.id == id).unwrap().hp,
        65.0
    );
}

#[test]
fn melee_out_of_range_and_cooldown() {
    let mut world = flat_world();
    world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.0, y: 10.0 });
    let human = human_with_item(&mut world, WorldPos { x: 20.0, y: 20.0 }, ItemKind::KnifePrimal);

    // Волк дальше базового радиуса — удар невозможен, кулдаун не тратится.
    assert!(!try_melee_attack(
        &mut world,
        human,
        TilePos { x: 10, y: 10 },
        50
    ));
    assert_eq!(
        world
            .entities
            .get(human)
            .and_then(|e| e.mind.as_ref())
            .unwrap()
            .attack_ready_tick,
        0
    );
}

#[test]
fn spear_throw_flies_sticks_and_is_picked_up() {
    let mut world = flat_world();
    let human = human_with_item(&mut world, WorldPos { x: 100.0, y: 100.0 }, ItemKind::SpearPrimal);

    // Бросок на 8 тайлов (копьё списывается командой — здесь
    // симулируем списание вручную, т.к. spawn_projectile вызван напрямую).
    let origin = world.entities.get(human).unwrap().pos;
    let id = world.spawn_projectile(
        ProjectileKind::ThrownSpear,
        origin,
        WorldPos { x: 108.5, y: 100.0 },
        human,
    );
    world
        .entities
        .get_mut(human)
        .unwrap()
        .mind
        .as_mut()
        .unwrap()
        .inventory
        .items[0] = None;
    let spear = world.projectiles.iter().find(|p| p.id == id).unwrap();
    let duration = spear.duration_ticks;
    for tick in 0..duration {
        world.tick_projectiles(tick);
    }
    // Полёт закончился: снаряд исчез, копьё воткнулось в цели.
    assert!(world.projectiles.iter().all(|p| p.id != id));
    assert_eq!(world.stuck_spears.len(), 1);
    assert!((world.stuck_spears[0].pos.x - 108.5).abs() < 0.01);

    // Подход в радиус подбора — копьё в инвентаре, из земли исчезло.
    world.entities.get_mut(human).unwrap().pos = WorldPos { x: 108.0, y: 100.5 };
    assert_eq!(pickup_stuck_spears(&mut world, human), 1);
    assert!(world.stuck_spears.is_empty());
    assert_eq!(
        world
            .entities
            .get(human)
            .unwrap()
            .mind
            .as_ref()
            .unwrap()
            .inventory
            .count_of(ItemKind::SpearPrimal),
        1
    );
}

#[test]
fn arrow_hits_wolf_and_vanishes() {
    let mut world = flat_world();
    let id = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 110.0, y: 100.0 });
    // Стрелок не указан (0): снаряд может попасть в любого.
    world.spawn_projectile(
        ProjectileKind::Arrow,
        WorldPos { x: 100.0, y: 100.0 },
        WorldPos { x: 120.5, y: 100.0 },
        0,
    );

    for tick in 0..40 {
        world.tick_projectiles(tick);
        if world.entities.iter().all(|e| e.id != id) {
            break;
        }
    }
    // Волк получил 10 урона от стрелы (80 → 70) и жив.
    let hp = world.entities.iter().find(|e| e.id == id).map(|e| e.hp);
    assert_eq!(hp, Some(70.0));
    // Стрела долетела и пропала.
    for tick in 40..80 {
        world.tick_projectiles(tick);
    }
    assert!(world.projectiles.is_empty());
}

#[test]
fn projectile_flight_is_bounded() {
    let mut world = flat_world();
    world.spawn_projectile(
        ProjectileKind::Arrow,
        WorldPos { x: 50.0, y: 50.0 },
        WorldPos { x: 60.5, y: 50.0 },
        0,
    );
    let projectile = &world.projectiles[0];
    // Высота дуги: 0 на концах, максимум в середине.
    let start = projectile.height();
    let middle = {
        let mut p = *projectile;
        p.t = 0.5;
        p.height()
    };
    assert_eq!(start, 0.0);
    assert!(middle > 0.0 && middle <= projectile.arc_height + f32::EPSILON);
}

// Волк, убитый человеком в ближнем бою, роняет мясо хищника дроп-сущностью,
// и человек рядом подбирает его автоподбором полного тика мира.
//#[test] deleted
