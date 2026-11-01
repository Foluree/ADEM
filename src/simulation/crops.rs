//! Культуры на пашне: посев, рост по стадиям, уборка урожая.
//!
//! Культура — разреженный слой чанка (`Chunk.crops`): на тайле пашни лежит
//! не более одной культуры. Рост — цепочка самозапланированных событий
//! планировщика (по образцу `vegetation.rs`): событие advancing стадию
//! планирует следующее; зимой цепочка замирает до весны. Состояние RNG не
//! хранится — урожай сеется от (seed, тик, координаты), поэтому детерминирован
//! и воспроизводим после загрузки сохранения.

use super::items::ItemKind;
use super::dirty::DirtyTiles;
use super::scheduler::{Scheduler, TaskKind};
use super::vegetation;
use super::tile::TileType;
use super::time::{self, Season};
use super::world::{ChunkPos, World, tile_pos};

/// Число видов культур (текстуры, массивы отображения).
pub const CROP_KIND_COUNT: usize = 5;

/// Максимум стадий роста по всем культурам (у пшеницы их 8).
pub const MAX_CROP_STAGES: usize = 8;

/// Полный цикл роста культуры: примерно 3 игровых месяца
/// (`TICKS_PER_MONTH * 3`). Интервал стадии = цикл / (стадий − 1).
pub const GROWTH_CYCLE_TICKS: u64 = super::time::TICKS_PER_MONTH * 3;

/// Вид культуры. Значения — стабильные идентификаторы (repr(u8)); новые
/// добавляются только в конец.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum CropKind {
    Wheat = 0,
    Beetroot,
    Carrot,
    Potato,
    Cherry,
}

impl CropKind {
    /// Все виды культур в стабильном порядке.
    pub const ALL: [CropKind; CROP_KIND_COUNT] = [
        CropKind::Wheat,
        CropKind::Beetroot,
        CropKind::Carrot,
        CropKind::Potato,
        CropKind::Cherry,
    ];
    /// Вид культуры по стабильному значению u8 (сохранения).
    pub fn from_discriminant(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Wheat),
            1 => Some(Self::Beetroot),
            2 => Some(Self::Carrot),
            3 => Some(Self::Potato),
            4 => Some(Self::Cherry),
            _ => None,
        }
    }
    /// Число стадий роста вида: стадия 0 — всходы, последняя — зрелость.
    pub fn stage_count(self) -> usize {
        match self {
            // У пшеницы текстур стадий вдвое больше — растёт визуально плавнее.
            CropKind::Wheat => 8,
            CropKind::Beetroot | CropKind::Carrot | CropKind::Potato | CropKind::Cherry => 4,
        }
    }
    /// Интервал между стадиями роста в тиках: полный цикл делится на
    /// переходы между стадиями.
    pub fn stage_interval_ticks(self) -> u64 {
        GROWTH_CYCLE_TICKS / (self.stage_count() as u64 - 1)
    }
    /// Предмет, которым культура сажается (семена у пшеницы и свёклы,
    /// корнеплоды и ягода — сами плодом, как в Minecraft).
    pub fn seed_item(self) -> ItemKind {
        match self {
            CropKind::Wheat => ItemKind::WheatSeeds,
            CropKind::Beetroot => ItemKind::BeetrootSeeds,
            CropKind::Carrot => ItemKind::Carrot,
            CropKind::Potato => ItemKind::Potato,
            CropKind::Cherry => ItemKind::Cherry,
        }
    }
    /// Культура по сажаемому предмету (`CropKind::seed_item`).
    pub fn from_seed_item(kind: ItemKind) -> Option<Self> {
        Self::ALL.into_iter().find(|crop| crop.seed_item() == kind)
    }
    /// Основной предмет урожая.
    #[allow(dead_code)] // читается UI станции (тултип «урожай») в будущем
    pub fn product_item(self) -> ItemKind {
        match self {
            CropKind::Wheat => ItemKind::Wheat,
            CropKind::Beetroot => ItemKind::Beetroot,
            CropKind::Carrot => ItemKind::Carrot,
            CropKind::Potato => ItemKind::Potato,
            CropKind::Cherry => ItemKind::Cherry,
        }
    }
    /// Урожай зрелой культуры: пары (предмет, количество). Детерминированный
    /// (без бросков — одинаков для тайла и стадии): зерно/свёкла возвращают
    /// семена на повторный посев, корнеплоды и вишня — сам плод.
    pub fn harvest_yield(self) -> [(ItemKind, u32); 2] {
        match self {
            CropKind::Wheat => [
                (ItemKind::Wheat, 1),
                (ItemKind::WheatSeeds, 2),
            ],
            CropKind::Beetroot => [
                (ItemKind::Beetroot, 1),
                (ItemKind::BeetrootSeeds, 2),
            ],
            CropKind::Carrot => [(ItemKind::Carrot, 2), (ItemKind::Carrot, 0)],
            CropKind::Potato => [(ItemKind::Potato, 2), (ItemKind::Potato, 0)],
            CropKind::Cherry => [(ItemKind::Cherry, 2), (ItemKind::Cherry, 0)],
        }
    }
}

/// Культура на тайле пашни: вид + текущая стадия роста.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Crop {
    pub kind: CropKind,
    pub stage: u8,
}

impl Crop {
    /// Всходы посаженной культуры.
    pub fn planted(kind: CropKind) -> Self {
        Self { kind, stage: 0 }
    }
    /// Зрелая ли культура (последняя стадия — урожайная).
    pub fn is_mature(&self) -> bool {
        self.stage as usize >= self.kind.stage_count() - 1
    }
}

/// Событие роста культуры: следующая стадия, если культура ещё на тайле
/// и не зрелая. Зимой цепочка замирает — событие переносится на начало
/// весны (сезонность, ROADMAP «Земледелие»). Вызывается из app-слоя
/// по исполненной задаче `TaskKind::CropGrowth`.
pub fn apply_growth(
    world: &mut World,
    sched: &mut Scheduler,
    tick: u64,
    dirty: &mut DirtyTiles,
    chunk: ChunkPos,
    local: u8,
) {
    let pos = tile_pos(chunk, local);
    let Some(crop) = world.crop_at(pos).copied() else {
        return; // культуру убрали (урожай, редактор) — цепочка гаснет
    };
    if crop.is_mature() {
        return;
    }
    // Пашня под культурой исчезла (редактор) — культура не выживает.
    if world.terrain_kind(pos) != TileType::Farmland {
        world.set_crop(pos, None);
        dirty.decor(pos);
        return;
    }

    // Зима: роста нет, ждём весну.
    let month = time::month_of_tick(tick);
    if Season::from_month(month) == Season::Winter {
        sched.schedule_at(
            time::next_season_tick(tick),
            TaskKind::CropGrowth { chunk, local_index: local },
        );
        return;
    }

    let next_stage = crop.stage + 1;
    world.set_crop(
        pos,
        Some(Crop { kind: crop.kind, stage: next_stage }),
    );
    dirty.decor(pos);
    if (next_stage as usize) < crop.kind.stage_count() - 1 {
        sched.schedule_at(
            tick + crop.kind.stage_interval_ticks(),
            TaskKind::CropGrowth { chunk, local_index: local },
        );
    }
}

/// Месячная проверка пашни: тайл без культуры («пустая пашня») возвращается
/// в землю. Тайл с культурой используется — задача гаснет, новую поставит
/// уборка урожая. Вызывается из app-слоя по задаче `FarmlandRevert`.
pub fn apply_farmland_revert(
    world: &mut World,
    sched: &mut Scheduler,
    tick: u64,
    dirty: &mut DirtyTiles,
    chunk: ChunkPos,
    local: u8,
) {
    let pos = tile_pos(chunk, local);
    if world.terrain_kind(pos) != TileType::Farmland || world.crop_at(pos).is_some() {
        return;
    }
    world.set_terrain(pos, TileType::Dirt);
    dirty.terrain(pos);
    // Рельеф изменился: перезапускаем растительные цепочки (трава может
    // занять освободившийся тайл).
    vegetation::after_tile_changed(world, sched, tick, pos);
}

// Все тесты должны быть в tests, чтобы н засорять код.
