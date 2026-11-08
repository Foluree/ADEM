//! Глобальное состояние игры (Bevy-ресурсы) и вспомогательные типы.

use bevy::prelude::*;

use crate::simulation::commands::CommandQueue;
use crate::simulation::dirty::DirtyTiles;
use crate::simulation::entities::{EntityKind, HumanMind, SimEntity};
use crate::simulation::objects::{BuildingKind, ObjectKind};
use crate::simulation::scheduler::Scheduler;
use crate::simulation::tile::TileType;
use crate::simulation::time::{GameClock, TICKS_PER_SEC};
use crate::simulation::{World, WorldPos, WorldSize};

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
    X25,
    X30,
    X35,
    X40,
    X45,
    X50,
    X60,
    X70,
    X80,
    X90,
    X100,
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
            SimSpeed::X25 => 25.0,
            SimSpeed::X30 => 30.0,
            SimSpeed::X35 => 35.0,
            SimSpeed::X40 => 40.0,
            SimSpeed::X45 => 45.0,
            SimSpeed::X50 => 50.0,
            SimSpeed::X60 => 60.0,
            SimSpeed::X70 => 70.0,
            SimSpeed::X80 => 80.0,
            SimSpeed::X90 => 90.0,
            SimSpeed::X100 => 100.0,
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
            SimSpeed::X25 => "x25",
            SimSpeed::X30 => "x30",
            SimSpeed::X35 => "x35",
            SimSpeed::X40 => "x40",
            SimSpeed::X45 => "x45",
            SimSpeed::X50 => "x50",
            SimSpeed::X60 => "x60",
            SimSpeed::X70 => "x70",
            SimSpeed::X80 => "x80",
            SimSpeed::X90 => "x90",
            SimSpeed::X100 => "x100",
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
            SimSpeed::X25,
            SimSpeed::X30,
            SimSpeed::X35,
            SimSpeed::X40,
            SimSpeed::X45,
            SimSpeed::X50,
            SimSpeed::X60,
            SimSpeed::X70,
            SimSpeed::X80,
            SimSpeed::X90,
            SimSpeed::X100,
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

/// Вид линзы карты (редактор): что подсвечивается на чанках.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LensKind {
    /// Чанки с разумными существами (люди).
    Sentients,
    /// Чанки с рукотворными постройками.
    Buildings,
}

/// Включённые линзы карты (обновление раз в секунду — в rendering/lenses).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MapLenses {
    /// Чанки с разумными существами.
    pub sentients: bool,
    /// Чанки с постройками.
    pub buildings: bool,
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
    /// Захваченное существо (игра за существо, GAME_DESIGN 4.1): id в
    /// `world.entities`. `None` — наблюдатель. Инвариант: `mode == Player`
    /// ⇔ `controlled.is_some()`, менять только через `set_controlled`.
    pub controlled: Option<u64>,
    /// Автономность захваченного существа (кнопка «ИИ» в HUD): `true` —
    /// существо живёт само (FSM брожения у животных, ИИ выживания у людей),
    /// `false` (по умолчанию) — полностью под контроллером: ни FSM, ни ИИ
    /// его не ведут. Сбрасывается при каждом захвате.
    pub controlled_ai: bool,
    /// Выбранный инструмент (None — клик по карте ничего не ставит).
    pub selected_tool: Option<Tool>,
    /// Включённые линзы карты (категория «Линзы» в панели редактора).
    pub lenses: MapLenses,
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
            controlled: None,
            controlled_ai: false,
            selected_tool: None,
            lenses: MapLenses::default(),
            dirty_tiles: DirtyTiles::default(),
        }
    }
}

impl GameState {
    /// Захватывает существо (режим игрока) или отпускает его
    /// (`None` — наблюдатель). Единственная точка смены режима.
    /// Захват всегда начинается под полным контролем: ИИ выключен.
    pub fn set_controlled(&mut self, id: Option<u64>) {
        if id.is_some() {
            self.selected_tool = None;
        }
        self.controlled = id;
        self.controlled_ai = false;
        self.mode = if id.is_some() {
            GameMode::Player
        } else {
            GameMode::Observer
        };
    }

    /// Захваченное существо (если существует в мире).
    pub fn controlled_entity(&self) -> Option<&SimEntity> {
        self.world
            .as_ref()
            .and_then(|world| world.entities.get(self.controlled?))
    }

    /// Захваченное существо (мутабельно): UI меняет инвентарь напрямую,
    /// как раньше менял `PlayerRes`.
    pub fn controlled_entity_mut(&mut self) -> Option<&mut SimEntity> {
        self.world
            .as_mut()
            .and_then(|world| world.entities.get_mut(self.controlled?))
    }

    /// Ум захваченного существа: инвентарь/голод. `None` — захвата нет или
    /// существо неразумное (волк): хотбар/инвентарь ему недоступны.
    pub fn controlled_mind(&self) -> Option<&HumanMind> {
        self.controlled_entity()?.mind.as_ref()
    }

    /// Ум захваченного существа (мутабельно).
    pub fn controlled_mind_mut(&mut self) -> Option<&mut HumanMind> {
        self.controlled_entity_mut()?.mind.as_mut()
    }
}

/// Bevy-обёртка над игровыми часами.
#[derive(Resource, Default)]
pub struct ClockRes(pub GameClock);

/// Bevy-обёртка над планировщиком симуляции.
#[derive(Resource, Default)]
pub struct SchedulerRes(pub Scheduler);

/// Bevy-обёртка над очередью команд симуляции.
#[derive(Resource, Default)]
pub struct CommandQueueRes(pub CommandQueue);

/// Выделение существа в редакторе (задача «инспектор»): подсветка под
/// курсором и существо с открытым окном-инспектором. Заполняется вводом
/// (app/input.rs), читается рендером (white outline) и окном инспектора.
#[derive(Resource, Default)]
pub struct EditorSelection {
    /// Существо под курсором (наблюдатель, радиус выбора — в input.rs).
    pub hovered: Option<u64>,
    /// Существо с открытым окном-инспектором (СКМ-клик).
    pub selected: Option<u64>,
}

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

/// Позиция захваченного существа для рендера в этом кадре: интерполяция
/// между позициями прошлого и текущего тиков по заполнению аккумулятора
/// фикс-степа. Позиция меняется дискретно по тикам (20 раз в секунду),
/// без интерполяции спрайт и камера двигались бы рывками.
pub fn controlled_render_pos(entity: &SimEntity, acc: &TickAccumulator) -> WorldPos {
    interpolate_pos(entity.prev_pos, entity.pos, acc)
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
