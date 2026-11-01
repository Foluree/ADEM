//! Глобальное состояние игры (Bevy-ресурсы) и вспомогательные типы.

use bevy::prelude::*;

use crate::simulation::commands::CommandQueue;
use crate::simulation::dirty::DirtyTiles;
use crate::simulation::entities::EntityKind;
use crate::simulation::objects::{BuildingKind, ObjectKind};
use crate::simulation::player::{Player, WorldPos};
use crate::simulation::scheduler::Scheduler;
use crate::simulation::tile::TileType;
use crate::simulation::time::{GameClock, TICKS_PER_SEC};
use crate::simulation::{World, WorldSize};

/// Скорость симуляции.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimSpeed {
    X0_5,
    X1,
    X2,
    X3,
    X4,
    X5,
    X7_5,
    X10,
    X12_5,
    X15,
    X20,
}

impl SimSpeed {
    /// Множитель скорости (для планировщика симуляции).
    pub fn multiplier(self) -> f32 {
        match self {
            SimSpeed::X0_5 => 0.5,
            SimSpeed::X1 => 1.0,
            SimSpeed::X2 => 2.0,
            SimSpeed::X3 => 3.0,
            SimSpeed::X4 => 4.0,
            SimSpeed::X5 => 5.0,
            SimSpeed::X7_5 => 7.5,
            SimSpeed::X10 => 10.0,
            SimSpeed::X12_5 => 12.5,
            SimSpeed::X15 => 15.0,
            SimSpeed::X20 => 20.0,
        }
    }

    /// Подпись для UI.
    pub fn label(self) -> &'static str {
        match self {
            SimSpeed::X0_5 => "x0.5",
            SimSpeed::X1 => "x1",
            SimSpeed::X2 => "x2",
            SimSpeed::X3 => "x3",
            SimSpeed::X4 => "x4",
            SimSpeed::X5 => "x5",
            SimSpeed::X7_5 => "x7.5",
            SimSpeed::X10 => "x10",
            SimSpeed::X12_5 => "x12.5",
            SimSpeed::X15 => "x15",
            SimSpeed::X20 => "x20",
        }
    }

    /// Все доступные скорости в порядке UI.
    pub fn all() -> &'static [SimSpeed] {
        &[
            SimSpeed::X0_5,
            SimSpeed::X1,
            SimSpeed::X2,
            SimSpeed::X3,
            SimSpeed::X4,
            SimSpeed::X5,
            SimSpeed::X7_5,
            SimSpeed::X10,
            SimSpeed::X12_5,
            SimSpeed::X15,
            SimSpeed::X20,
        ]
    }
}

/// Роль игрока (GAME_DESIGN 3.1). Наблюдатель — он же редактор мира;
/// режим игрока — управление одним существом.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GameMode {
    /// Камера, время, инструменты редактора. Игрок по умолчанию здесь.
    #[default]
    Observer,
    /// Управление существом: WASD, взаимодействие с миром, инвентарь (E).
    Player,
}

/// Категория инструментов редактора в нижней панели.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ToolCategory {
    /// Размещение тайлов рельефа.
    #[default]
    Tiles,
    /// Размещение объектов над тайлами (деревья, кусты, валуны...).
    Objects,
    /// Размещение построек (костёр...). Строятся и игроком — через меню
    /// строительства, в редакторе ставятся бесплатно.
    Buildings,
    /// Размещение существ (пока только волк).
    Entities,
}

impl ToolCategory {
    /// Все категории в порядке UI.
    pub const ALL: [ToolCategory; 4] = [
        ToolCategory::Tiles,
        ToolCategory::Objects,
        ToolCategory::Buildings,
        ToolCategory::Entities,
    ];
}

/// Выбранный инструмент редактора: что ставится кликом по карте.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Тайл рельефа.
    Tile(TileType),
    /// Природный объект над тайлом (деревья, кусты, валуны).
    Object(ObjectKind),
    /// Постройка (рукотворное сооружение).
    Building(BuildingKind),
    /// Существо в тайле.
    Entity(EntityKind),
}

impl Tool {
    /// Категория инструмента: в каком подменю панели он живёт. Постройки
    /// выделены в собственную категорию, объекты — в Objects.
    pub fn category(self) -> ToolCategory {
        match self {
            Tool::Tile(_) => ToolCategory::Tiles,
            Tool::Object(_) => ToolCategory::Objects,
            Tool::Building(_) => ToolCategory::Buildings,
            Tool::Entity(_) => ToolCategory::Entities,
        }
    }
}

/// Открытое модальное меню.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuKind {
    Tech,
    Social,
    /// Творческое меню (I): все предметы, клик — полный стак в инвентарь.
    Creative,
    /// ESC-меню (пауза + настройки). Пока открыто, игра стоит на паузе.
    Pause,
}

/// Главное состояние игры.
#[derive(Resource)]
pub struct GameState {
    pub world: Option<World>,
    pub paused: bool,
    pub speed: SimSpeed,
    pub world_size: WorldSize,
    pub seed: u64,
    pub active_menu: Option<MenuKind>,
    /// Флаг: мир перегенерирован и рендер надо обновить.
    pub pending_rerender: bool,
    /// Роль игрока: наблюдатель (редактор) или существо в мире.
    pub mode: GameMode,
    /// Выбранный инструмент (None — клик по карте ничего не ставит).
    pub selected_tool: Option<Tool>,
    /// Тайлы, изменённые с прошлого кадра, по каналам причины (рельеф /
    /// декор): рендер пересобирает terrain-меш блока или только дочерние
    /// спрайты. Наполняется применением команд и задачами растительности,
    /// изымается рендером.
    pub dirty_tiles: DirtyTiles,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            world: None,
            paused: false,
            speed: SimSpeed::X1,
            world_size: WorldSize::new(256),
            seed: 42,
            active_menu: None,
            pending_rerender: false,
            mode: GameMode::Observer,
            selected_tool: None,
            dirty_tiles: DirtyTiles::default(),
        }
    }
}

/// Bevy-обёртка над игровыми часами.
#[derive(Resource, Default)]
pub struct ClockRes(pub GameClock);

/// Bevy-обёртка над планировщиком симуляции.
#[derive(Resource, Default)]
pub struct SchedulerRes(pub Scheduler);

/// Данные игрока: Some — существо существует. В режиме наблюдателя игрок
/// «припаркован» (данные и инвентарь сохраняются), в режиме игрока он
/// управляется вводом. Позиция переносится между входами в режим.
#[derive(Resource, Default)]
pub struct PlayerRes(pub Option<Player>);

/// Bevy-обёртка над очередью команд симуляции.
#[derive(Resource, Default)]
pub struct CommandQueueRes(pub CommandQueue);

/// Аккумулятор реального времени для фикс-степ тика.
#[derive(Resource, Default)]
pub struct TickAccumulator {
    pub seconds: f32,
}

/// Метка тиковой системы мира: всё, что показывает состояние симуляции
/// (камера, спрайты), запускается строго после неё в том же кадре.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SimTick;


/// Позиция для рендера в этом кадре: интерполяция между позициями
/// прошлого и текущего тиков по заполнению аккумулятора фикс-степа.
/// Общая для всех подвижных сущностей (игрок, животные).
pub fn interpolate_pos(prev: WorldPos, pos: WorldPos, acc: &TickAccumulator) -> WorldPos {
    let step = 1.0 / TICKS_PER_SEC as f32;
    let alpha = (acc.seconds / step).clamp(0.0, 1.0);
    WorldPos {
        x: prev.x + (pos.x - prev.x) * alpha,
        y: prev.y + (pos.y - prev.y) * alpha,
    }
}

/// Позиция игрока для рендера в этом кадре: интерполяция между позициями
/// прошлого и текущего тиков по заполнению аккумулятора фикс-степа.
/// Позиция меняется дискретно по тикам (20 раз в секунду), без
/// интерполяции спрайт и камера двигались бы рывками.
pub fn player_render_pos(player: &Player, acc: &TickAccumulator) -> WorldPos {
    interpolate_pos(player.prev_pos, player.pos, acc)
}

/// Сглаженное время исполнения одного тика (мс) — для FPS-панели
/// (GAME_DESIGN 9: время тика должно быть измеримой метрикой).
#[derive(Resource, Default)]
pub struct TickTiming {
    pub avg_ms: f32,
}

/// Состояние окна: в фокусе и не перекрыто ли. Обновляется из событий
/// окна; используется лимитером FPS и паузой при потере фокуса.
///
/// Дефолт «в фокусе»: окно создается активным, событие фокуса приходит
/// в первые кадры и при необходимости исправит значение.
#[derive(Resource, Debug)]
pub struct WindowFocusState {
    pub focused: bool,
    pub occluded: bool,
}

impl Default for WindowFocusState {
    fn default() -> Self {
        Self {
            focused: true,
            occluded: false,
        }
    }
}

/// Пауза, поставленная потерей фокуса. При возврате фокуса игра
/// снимает паузу только если она была поставлена именно этим
/// механизмом, — иначе вернувшийся из меню игрок попал бы на бегущий мир.
#[derive(Resource, Default)]
pub struct FocusPause {
    pub active: bool,
}
