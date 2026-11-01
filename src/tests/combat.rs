use super::*;
use crate::simulation::entities::EntityKind;
use crate::simulation::generation::generate_world;
use crate::simulation::world::WorldSize;

/// Мир без воды и существ для боевых тестов.
fn flat_world() -> World {
    let mut world = generate_world(WorldSize::new(256), 7);
    world.entities = Default::default();
    world
}

#[test]
fn melee_attack_damages_and_kills_wolf() {
    let mut world = flat_world();
    let id = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.0, y: 10.0 });
    let mut player = Player::new(WorldPos { x: 10.0, y: 11.0 });

    // Нож в выбранном слоте: 5 урона.
    player.inventory.add(ItemKind::Knife, 1);
    player.selected_slot = 0;
    assert!(try_melee_attack(
        &mut world,
        &mut player,
        TilePos { x: 10, y: 10 },
        100
    ));
    assert_eq!(world.entities.iter().find(|e| e.id == id).unwrap().hp, 75.0);
    // Кулдаун 6 тиков: на том же тике удар невозможен.
    assert!(!try_melee_attack(
        &mut world,
        &mut player,
        TilePos { x: 10, y: 10 },
        101
    ));
    assert!(try_melee_attack(
        &mut world,
        &mut player,
        TilePos { x: 10, y: 10 },
        106
    ));
    // Третий удар (ровно на границе кулдауна) выбивает волка до 65 HP.
    assert!(try_melee_attack(
        &mut world,
        &mut player,
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
    let mut player = Player::new(WorldPos { x: 20.0, y: 20.0 });

    // Волк дальше базового радиуса — удар невозможен, кулдаун не тратится.
    assert!(!try_melee_attack(
        &mut world,
        &mut player,
        TilePos { x: 10, y: 10 },
        50
    ));
    assert_eq!(player.attack_ready_tick, 0);
}

#[test]
fn spear_throw_flies_sticks_and_is_picked_up() {
    let mut world = flat_world();
    let mut player = Player::new(WorldPos { x: 100.0, y: 100.0 });
    player.inventory.add(ItemKind::Spear, 1);
    player.selected_slot = 0;

    // Бросок на 8 тайлов (копьё списывается командой — здесь
    // симулируем списание вручную, т.к. spawn_projectile вызван напрямую).
    let id = world.spawn_projectile(
        ProjectileKind::ThrownSpear,
        player.pos,
        WorldPos { x: 108.5, y: 100.0 },
    );
    player.inventory.items[0] = None;
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
    player.pos = WorldPos { x: 108.0, y: 100.5 };
    assert_eq!(pickup_stuck_spears(&mut world, &mut player), 1);
    assert!(world.stuck_spears.is_empty());
    assert_eq!(player.inventory.count_of(ItemKind::Spear), 1);
}

#[test]
fn arrow_hits_wolf_and_vanishes() {
    let mut world = flat_world();
    let id = world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 110.0, y: 100.0 });
    world.spawn_projectile(
        ProjectileKind::Arrow,
        WorldPos { x: 100.0, y: 100.0 },
        WorldPos { x: 120.5, y: 100.0 },
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
