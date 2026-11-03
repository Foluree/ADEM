//! Бинарное сохранение и загрузка состояния симуляции (GAME_DESIGN 8).
//!
//! Формат little-endian, без внешних крейтов:
//!
//! ```text
//! Заголовок:
//!   magic              4 байта  b"ADEM"
//!   format_version     u32
//!   длина версии игры  u8 + байты UTF-8
//!   seed               u64
//!   tick               u64
//!   world_side         u32   (тайлов по стороне)
//! Тело:
//!   мир:
//!     ambient_temperature_c  f32 (биты)
//!     chunks_per_side        u32
//!     чанки по порядку (row-major):
//!       рельеф:   tag u8: 0 = Uniform + TileType u8 | 1 = Full + 256 байт
//!       ресурсы:  u16 count + [index u8, kind u8, amount u16] по индексам
//!       объекты:  u16 count + [index u8, kind u8, stage u8, charge u16]
//!       предметы: u16 count + [index u8, kind u8, count u32] по индексам
//!       культуры: u16 count + [index u8, kind u8, stage u8] по индексам
//!       контейнеры: u16 count + [index u8, kind u8, cook u8+u64,
//!                     burn u8+u64, fuel u8, u16 slots + слоты]
//!       слот контейнера: tag u8: 0 = пусто | 1 = kind u8 + count u32
//!   существа:
//!     next_id u64
//!     count   u32 + [id u64, kind u8, x f32, y f32, prev_x f32, prev_y f32,
//!                    state u8, facing u8, hp f32, hunger f32,
//!                    water_ticks u16,
//!                    state_ticks u64, state_duration u64,
//!                    target_x f32, target_y f32, stuck_ticks u16,
//!                    land_search_failed_at u64,
//!                    mind u8: 0 = нет (неразумное) | 1 = запись ниже]
//!                    mind:  selected_slot u8,
//!                           слоты misc / брони / одежды / косметики /
//!                           мусорки (слот: tag u8: 0 = пусто | 1 =
//!                           kind u8 + count u32)]
//!                    по возрастанию id
//!   снаряды:
//!     next_projectile_id u64
//!     count   u32 + [id u64, kind u8, ox f32, oy f32, tx f32, ty f32,
//!                    t f32, duration u64, arc f32, damage f32]
//!   воткнутые копья:
//!     count   u32 + [x f32, y f32, heading f32, pitch f32]
//!   item-сущности (дроп):
//!     count   u32 + [id u64, x f32, y f32, kind u8, count u32,
//!                    spawn_tick u64, pickup_after_tick u64] по возрастанию id
//!   планировщик:
//!     current_tick     u64 (должен совпадать с tick из заголовка)
//!     next_id          u64
//!     отменённые:      u64 count + отсортированные id u64
//!     ближнее окно:    u64 число бакетов + бакеты
//!                      [u64 count + [id u64, вид задачи] на бакет]
//!     дальние:         u64 count + [tick u64, id u64, вид задачи] по (tick, id)
//! ```
//!
//! Обход разреженных слоёв идёт по возрастанию индексов, списки
//! планировщика отсортированы — одинаковые состояния дают одинаковые байты.
//! Файл должен читаться обратно без потерь: счёт скорости, пауза и прочее
//! transient-состояние UI сюда не входят — это выбор игрока, а не мира.
//! Кулдауны ума существа (атака, еда) и цель ИИ при загрузке сбрасываются:
//! тики мира могли уйти вперёд.
//!
//! Состав модуля: `write` — сериализация (`save_game`), `read` — разбор
//! (`load_game`/`read_header`), `validate` — структурные проверки формата.

use std::fmt;

/// Версия формата сохранения. При несовместимых изменениях — увеличить
/// и решить, поддерживать ли чтение старых версий. Версия 1: основа.
/// Версия 2: слои культур и контейнеров станций, item-сущности дропа,
/// задачи CropGrowth/StationTick. Версия 3: игрок стал сущностью мира —
/// у существ появился «ум» (инвентарь/голод/слот хотбара), секция игрока
/// удалена, у снаряда появилось поле стрелка. Версия 4: голод перенесён
/// из «ума» на уровень существа — голодают все виды, включая волка.
/// Сейвы v3 и старше не читаются.
pub const SAVE_FORMAT_VERSION: u32 = 4;

/// Магия в начале файла.
pub(super) const MAGIC: [u8; 4] = *b"ADEM";

/// Ошибки чтения сохранения.
#[derive(Debug)]
pub enum SaveError {
    /// Файл не начинается с магии `ADEM` — это не сохранение этой игры.
    BadMagic,
    /// Версия формата не поддерживается.
    UnsupportedFormat { found: u32, expected: u32 },
    /// Данные кончились раньше, чем ожидалось.
    Truncated,
    /// Структура данных не соответствует формату (битый или подменённый файл).
    BadData(&'static str),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::BadMagic => write!(f, "это не файл сохранения ADEM"),
            SaveError::UnsupportedFormat { found, expected } => {
                write!(f, "версия формата {found}, поддерживается {expected}")
            }
            SaveError::Truncated => write!(f, "файл обрезан"),
            SaveError::BadData(what) => write!(f, "повреждённые данные: {what}"),
        }
    }
}

impl std::error::Error for SaveError {}

/// Данные заголовка сохранения. Достаточно для списка слотов без
/// чтения тела файла (дата, размер мира, seed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveHeader {
    pub format_version: u32,
    pub game_version: String,
    pub seed: u64,
    pub tick: u64,
    pub world_side: u32,
}

/// Полностью разобранное сохранение.
pub struct SavedGame {
    pub seed: u64,
    pub tick: u64,
    pub world: super::world::World,
    pub scheduler: super::scheduler::Scheduler,
}

mod read;
mod validate;
mod write;

pub use read::{load_game, read_header};
pub use write::save_game;
