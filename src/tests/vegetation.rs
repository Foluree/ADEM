//! Тесты растительности: самозапланированные цепочки — расползание травы,
//! рост кустов, эрозия берегов, перезапуск от инструментов редактора.

use crate::simulation::dirty::DirtyTiles;
use crate::simulation::world::{TilePos, World};

/// Прогоняет планировщик до `until` тика, применяя события растительности
/// и собирая позиции изменённых тайлов по каналам (dirty-флаги для рендера).
/// Возвращает число исполненных событий. Вспомогательная для тестов
/// самозапланированных цепочек.
fn run_vegetation(
    sched: &mut crate::simulation::Scheduler,
    world: &mut World,
    until: u64,
    dirty: &mut DirtyTiles,
) -> usize {
    let mut applied = 0;
    for tick in 1..=until {
        let mut fired = Vec::new();
        sched.run_tick(tick, |task| fired.push(task));
        for task in fired {
            crate::simulation::vegetation::apply_task(&task.kind, world, sched, tick, dirty);
            applied += 1;
        }
    }
    applied
}

/// События растительности отмечают dirty-тайлы: рендер получает адреса
/// изменившихся тайлов без ручной правки карты.
#[test]
fn vegetation_changes_report_dirty_tiles() {
    use crate::simulation::tile::TileType;
    use crate::simulation::vegetation;

    let mut world = World::new(crate::simulation::WorldSize::new(256), 1);
    let mut sched = crate::simulation::Scheduler::default();
    // Полоса травы рядом с полосой земли (как в тесте расползания).
    for y in 4..8 {
        world.set_terrain(TilePos { x: 10, y }, TileType::Grass);
        world.set_terrain(TilePos { x: 11, y }, TileType::Dirt);
    }
    vegetation::seed_all(&world, &mut sched, 0);

    let mut dirty = DirtyTiles::default();
    run_vegetation(&mut sched, &mut world, 20_000, &mut dirty);

    assert!(!dirty.is_empty(), "растительные события должны отмечать dirty-тайлы");
    // Расползание травы меняет рельеф — канал terrain; каналу decor здесь
    // взяться неоткуда (кубы растут только на лесной траве).
    assert!(dirty.decor.is_empty(), "расползание травы не меняет декор");
    for pos in &dirty.terrain {
        assert_eq!(
            world.terrain_kind(*pos),
            TileType::Grass,
            "изменённый тайл ({}, {}) должен быть травой после расползания",
            pos.x,
            pos.y
        );
    }
}

/// Трава расползается на землю через самозапланированные события: цепочки
/// гаснут, когда земля кончается, и мир остаётся без лишней работы.
#[test]
fn grass_spreads_via_scheduler() {
    use crate::simulation::tile::TileType;
    use crate::simulation::vegetation;

    let mut world = World::new(crate::simulation::WorldSize::new(256), 1);
    let mut sched = crate::simulation::Scheduler::default();
    // Полоса травы рядом с полосой земли.
    for y in 4..8 {
        world.set_terrain(TilePos { x: 10, y }, TileType::Grass);
        world.set_terrain(TilePos { x: 11, y }, TileType::Dirt);
    }
    vegetation::seed_all(&world, &mut sched, 0);

    // Четыре тайла травы по 0.1%/такт: первое расползание в среднем за ~250
    // тиков; 20 000 тиков хватает с огромным запасом.
    run_vegetation(&mut sched, &mut world, 20_000, &mut DirtyTiles::default());

    for y in 4..8 {
        assert_eq!(
            world.terrain_kind(TilePos { x: 11, y }),
            TileType::Grass,
            "земля по x=11, y={y} не заросла"
        );
    }
}

/// Кусты вырастают в лесу сами, но не больше предела на чанк.
#[test]
fn bushes_grow_up_to_chunk_cap() {
    use crate::simulation::chunk::CHUNK_SIZE;
    use crate::simulation::objects::ObjectKind;
    use crate::simulation::tile::TileType;
    use crate::simulation::vegetation::{BUSH_CAP_PER_CHUNK, seed_all};

    let mut world = World::new(crate::simulation::WorldSize::new(256), 1);
    let mut sched = crate::simulation::Scheduler::default();
    // Весь чанк (0,0) — сплошной лес.
    for y in 0..CHUNK_SIZE as i32 {
        for x in 0..CHUNK_SIZE as i32 {
            world.set_terrain(TilePos { x, y }, TileType::ForestGrass);
        }
    }
    seed_all(&world, &mut sched, 0);

    // 0.05%/такт на тайл × 256 тайлов: куст каждые ~8 тиков, предел
    // достигается за сотни тиков.
    run_vegetation(&mut sched, &mut world, 100_000, &mut DirtyTiles::default());

    let chunk = world.chunk(crate::simulation::ChunkPos { x: 0, y: 0 });
    let bushes = chunk
        .objects
        .values()
        .filter(|obj| obj.kind == ObjectKind::Bush)
        .count() as u32;
    assert_eq!(bushes, BUSH_CAP_PER_CHUNK, "кусты должны дорасти до предела");
}

/// Вода постепенно превращает лесную траву в траву, а траву и землю — в песок.
#[test]
fn water_erodes_shores_via_scheduler() {
    use crate::simulation::tile::TileType;
    use crate::simulation::vegetation;

    let mut world = World::new(crate::simulation::WorldSize::new(256), 1);
    let mut sched = crate::simulation::Scheduler::default();
    // Вода с двумя эродируемыми соседями: лес (две стадии) и земля (одна).
    world.set_terrain(TilePos { x: 10, y: 5 }, TileType::Water);
    world.set_terrain(TilePos { x: 11, y: 5 }, TileType::ForestGrass);
    world.set_terrain(TilePos { x: 9, y: 5 }, TileType::Dirt);
    vegetation::seed_all(&world, &mut sched, 0);

    // 0.02%/такт: событие в среднем за 5000 тиков, нужно три превращения —
    // 300 000 тиков дают запас.
    run_vegetation(&mut sched, &mut world, 300_000, &mut DirtyTiles::default());

    assert_eq!(
        world.terrain_kind(TilePos { x: 11, y: 5 }),
        TileType::Sand,
        "лесная трава должна пройти лес → трава → песок"
    );
    assert_eq!(
        world.terrain_kind(TilePos { x: 9, y: 5 }),
        TileType::Sand,
        "земля должна стать песком"
    );
}

/// Редактор запускает цепочки: земля, поставленная рядом с травой,
/// со временем зарастает без повторного посева.
#[test]
fn editor_placed_dirt_gets_overgrown() {
    use crate::simulation::commands::{Command, CommandQueue};
    use crate::simulation::tile::TileType;

    let mut world = World::new(crate::simulation::WorldSize::new(256), 1);
    let mut queue = CommandQueue::default();
    let mut sched = crate::simulation::Scheduler::default();
    // Готовая трава, затем команда ставит землю рядом (как инструмент «бога»).
    world.set_terrain(TilePos { x: 10, y: 5 }, TileType::Grass);
    queue.push(Command::SetTerrain { pos: TilePos { x: 11, y: 5 }, kind: TileType::Dirt });

    let mut dirty = DirtyTiles::default();
    queue.apply_all(&mut world, None, &mut dirty, &mut sched, 0);

    run_vegetation(&mut sched, &mut world, 20_000, &mut dirty);
    assert_eq!(
        world.terrain_kind(TilePos { x: 11, y: 5 }),
        TileType::Grass,
        "поставленная земля должна зарасти травой"
    );
}
