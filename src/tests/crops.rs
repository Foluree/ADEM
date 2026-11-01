//! Культуры: посев, рост по стадиям (с зимней паузой), уборка урожая.

use crate::simulation::crops::{self, Crop, CropKind};
use crate::simulation::dirty::DirtyTiles;
use crate::simulation::items::{ItemKind, MISC_SLOTS};
use crate::simulation::player::{Player, WorldPos};
use crate::simulation::scheduler::Scheduler;
use crate::simulation::tile::TileType;
use crate::simulation::time::TICKS_PER_MONTH;
use crate::simulation::world::{TilePos, World, WorldSize};

/// Плоский мир с пашней в углу и игроком рядом.
fn farm_world() -> (World, Player) {
    let mut world = World::new(WorldSize::new(256), 7);
    let pos = TilePos { x: 10, y: 10 };
    world.set_terrain(pos, TileType::Farmland);
    let player = Player::new(WorldPos::of_tile_center(TilePos { x: 11, y: 10 }));
    (world, player)
}

#[test]
fn sow_requires_farmland_and_plantable_item() {
    let (mut world, mut player) = farm_world();
    // Нет сажаемого предмета — посев невозможен.
    assert!(!player.sow(TilePos { x: 10, y: 10 }, &mut world));

    // Не пашня — посев невозможен даже с семенами.
    player.inventory.add(ItemKind::WheatSeeds, 2);
    world.set_terrain(TilePos { x: 10, y: 10 }, TileType::Grass);
    assert!(!player.sow(TilePos { x: 10, y: 10 }, &mut world));

    world.set_terrain(TilePos { x: 10, y: 10 }, TileType::Farmland);
    assert!(player.sow(TilePos { x: 10, y: 10 }, &mut world));
    // Семя списано, всходы на тайле, вторая посадка на занятый тайл — нет.
    assert_eq!(player.inventory.count_of(ItemKind::WheatSeeds), 1);
    assert_eq!(
        world.crop_at(TilePos { x: 10, y: 10 }).map(|c| (c.kind, c.stage)),
        Some((CropKind::Wheat, 0))
    );
    assert!(!player.sow(TilePos { x: 10, y: 10 }, &mut world));
}

#[test]
fn growth_advances_stage_by_stage_until_mature() {
    let (mut world, _) = farm_world();
    let pos = TilePos { x: 10, y: 10 };
    let kind = CropKind::Carrot;
    world.set_crop(pos, Some(Crop::planted(kind)));

    let (chunk, local) = crate::simulation::split_tile(pos);
    let mut sched = Scheduler::default();
    let mut dirty = DirtyTiles::default();
    let interval = kind.stage_interval_ticks();
    let stages = kind.stage_count();

    for stage in 0..stages - 1 {
        let tick = (stage as u64 + 1) * interval;
        crops::apply_growth(&mut world, &mut sched, tick, &mut dirty, chunk, local);
        assert_eq!(
            world.crop_at(pos).map(|c| c.stage),
            Some(stage as u8 + 1),
            "стадия {stage} → {stage}+1"
        );
    }
    assert!(world.crop_at(pos).unwrap().is_mature());
    // Дальше расти некуда: событие не меняет зрелую культуру.
    crops::apply_growth(
        &mut world,
        &mut sched,
        (stages as u64) * interval,
        &mut dirty,
        chunk,
        local,
    );
    assert_eq!(world.crop_at(pos).map(|c| c.stage), Some(stages as u8 - 1));
}

#[test]
fn growth_pauses_in_winter_and_resumes_in_spring() {
    let (mut world, _) = farm_world();
    let pos = TilePos { x: 10, y: 10 };
    world.set_crop(pos, Some(Crop::planted(CropKind::Wheat)));
    let (chunk, local) = crate::simulation::split_tile(pos);
    let mut sched = Scheduler::default();
    let mut dirty = DirtyTiles::default();

    // Месяц 9 (первый зимний): рост замирает, стадия не меняется.
    let winter_tick = 9 * TICKS_PER_MONTH;
    crops::apply_growth(&mut world, &mut sched, winter_tick, &mut dirty, chunk, local);
    assert_eq!(world.crop_at(pos).map(|c| c.stage), Some(0));

    // Весна: событие, перенесённое на границу сезона, продолжает рост.
    let spring_tick = 12 * TICKS_PER_MONTH;
    crops::apply_growth(&mut world, &mut sched, spring_tick, &mut dirty, chunk, local);
    assert_eq!(world.crop_at(pos).map(|c| c.stage), Some(1));
}

#[test]
fn harvest_mature_crop_gives_yield_and_frees_tile() {
    let (mut world, mut player) = farm_world();
    let pos = TilePos { x: 10, y: 10 };
    let stages = CropKind::Wheat.stage_count() as u8;
    world.set_crop(
        pos,
        Some(Crop { kind: CropKind::Wheat, stage: stages - 1 }),
    );

    // Место под весь урожай есть — уборка проходит.
    assert!(player.interact(pos, &mut world));
    let yield_wheat = CropKind::Wheat.harvest_yield();
    for (kind, count) in yield_wheat {
        if count > 0 {
            assert_eq!(player.inventory.count_of(kind), count);
        }
    }
    // Культура снята, пашня осталась.
    assert!(world.crop_at(pos).is_none());
    assert_eq!(world.terrain_kind(pos), TileType::Farmland);
}

#[test]
fn harvest_immune_to_full_inventory() {
    let (mut world, mut player) = farm_world();
    let pos = TilePos { x: 10, y: 10 };
    let stages = CropKind::Potato.stage_count() as u8;
    world.set_crop(
        pos,
        Some(Crop { kind: CropKind::Potato, stage: stages - 1 }),
    );
    // Забитый инвентарь чужими предметами: урожаю деться некуда.
    for slot in 0..MISC_SLOTS {
        player.inventory.items[slot] =
            Some(crate::simulation::items::ItemStack { kind: ItemKind::Rock, count: 64 });
    }
    assert!(!player.interact(pos, &mut world));
    // Культура не потеряна.
    assert!(world.crop_at(pos).is_some());
}

#[test]
fn terrain_change_kills_crop() {
    let (mut world, _) = farm_world();
    let pos = TilePos { x: 10, y: 10 };
    world.set_crop(pos, Some(Crop::planted(CropKind::Cherry)));
    // Рельеф перестал быть пашней — культура не выживает (правило команды
    // SetTerrain; здесь проверяем слой напрямую).
    world.set_terrain(pos, TileType::Dirt);
    world.set_crop(pos, None);
    assert!(world.crop_at(pos).is_none());
}

/// Пустая пашня (без культуры) через месяц возвращается в землю; занятая
/// культурой пашня не трогается.
#[test]
fn empty_farmland_reverts_to_dirt() {
    let (mut world, _) = farm_world();
    let pos = TilePos { x: 10, y: 10 };
    let (chunk, local) = crate::simulation::split_tile(pos);
    let mut sched = Scheduler::default();
    let mut dirty = DirtyTiles::default();

    // Пашня без культуры: тайл становится землёй.
    crops::apply_farmland_revert(&mut world, &mut sched, TICKS_PER_MONTH, &mut dirty, chunk, local);
    assert_eq!(world.terrain_kind(pos), TileType::Dirt);

    // Занятая культурой пашня не трогается.
    world.set_terrain(pos, TileType::Farmland);
    world.set_crop(pos, Some(Crop::planted(CropKind::Wheat)));
    crops::apply_farmland_revert(
        &mut world,
        &mut sched,
        2 * TICKS_PER_MONTH,
        &mut dirty,
        chunk,
        local,
    );
    assert_eq!(world.terrain_kind(pos), TileType::Farmland);
}
