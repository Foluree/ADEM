//! Описание тайла и его ресурсов.

/// Вид тайла. Значения — стабильные идентификаторы (repr(u8)),
/// чтобы позже использовать в сохранениях и при обмене с рендером.
/// Новые варианты добавляются только в конец, чтобы не сдвигать
/// значения существующих (правило то же, что у `ResourceKind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum TileType {
    Empty = 0,
    Water,
    Sand,
    Dirt,
    Grass,
    ForestGrass,
    Stone,
    Clay,
    Farmland,
    /// Глубокая вода: середина морей и глубокие озёра.
    DeepWater,
    /// Океан: самые глубокие воды у границ карты; в озёрах не встречается.
    Ocean,
    /// Гора: высокий рельеф над камнем.
    Mountain,
    /// Мох: пятнами в лесу на уровне лесной травы.
    Moss,
    /// Грязь: задел под будущие биомы, пока не генерируется.
    Mud,
    /// Подзол: задел под будущие биомы, пока не генерируется.
    Podzol,
}

impl TileType {
    /// Вид тайла по стабильному значению u8 (сохранения, обмен с рендером).
    pub fn from_discriminant(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Empty),
            1 => Some(Self::Water),
            2 => Some(Self::Sand),
            3 => Some(Self::Dirt),
            4 => Some(Self::Grass),
            5 => Some(Self::ForestGrass),
            6 => Some(Self::Stone),
            7 => Some(Self::Clay),
            8 => Some(Self::Farmland),
            9 => Some(Self::DeepWater),
            10 => Some(Self::Ocean),
            11 => Some(Self::Mountain),
            12 => Some(Self::Moss),
            13 => Some(Self::Mud),
            14 => Some(Self::Podzol),
            _ => None,
        }
    }

    /// Цвет-плейсхолдер (RGBA) для рендера, пока нет текстур.
    /// Имена вариантов уже соответствуют «будущим» текстурам.
    pub fn placeholder_color(self) -> [f32; 4] {
        match self {
            TileType::Empty => [0.0, 0.0, 0.0, 0.0],
            TileType::Water => [0.14, 0.33, 0.62, 1.0],
            TileType::Sand => [0.84, 0.79, 0.54, 1.0],
            TileType::Dirt => [0.42, 0.31, 0.20, 1.0],
            TileType::Grass => [0.34, 0.58, 0.24, 1.0],
            TileType::ForestGrass => [0.20, 0.42, 0.15, 1.0],
            TileType::Stone => [0.53, 0.53, 0.56, 1.0],
            TileType::Clay => [0.62, 0.39, 0.27, 1.0],
            TileType::Farmland => [0.39, 0.29, 0.17, 1.0],
            TileType::DeepWater => [0.10, 0.24, 0.50, 1.0],
            TileType::Ocean => [0.05, 0.15, 0.38, 1.0],
            TileType::Mountain => [0.36, 0.35, 0.34, 1.0],
            TileType::Moss => [0.24, 0.45, 0.20, 1.0],
            TileType::Mud => [0.31, 0.25, 0.18, 1.0],
            TileType::Podzol => [0.44, 0.32, 0.18, 1.0],
        }
    }

    /// Водный ли тайл: любая из трёх глубин воды непроходима с суши.
    pub fn is_water(self) -> bool {
        matches!(self, TileType::Water | TileType::DeepWater | TileType::Ocean)
    }
}

/// Вид ресурса, лежащего в тайле.
/// Варианты — стабильные идентификаторы (repr(u8)); новые добавляются
/// только в конец, чтобы не сдвигать значения существующих.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ResourceKind {
    None = 0,
    Copper,
    Clay,
    /// Дерево из тайла лесной травы; появится с добычей дерева.
    #[allow(dead_code)]
    Wood,
    /// Камень, откалываемый от валунов.
    Stone,
    /// Палка (из кустов, побочно из деревьев).
    Stick,
}

impl ResourceKind {
    /// Вид ресурса по стабильному значению u8 (сохранения).
    pub fn from_discriminant(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::None),
            1 => Some(Self::Copper),
            2 => Some(Self::Clay),
            3 => Some(Self::Wood),
            4 => Some(Self::Stone),
            5 => Some(Self::Stick),
            _ => None,
        }
    }
}

/// Тайл: вид + ресурс (вид + количество).
/// Плоский view для UI и тестов (ARCHITECTURE 2.3); горячие пути симуляции
/// работают со слоями чанка напрямую, поэтому в сборке без тестов view
/// временно без читателей.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub kind: TileType,
    pub resource: ResourceKind,
    pub amount: u16,
}

#[allow(dead_code)]
impl Tile {
    pub const EMPTY: Self = Self {
        kind: TileType::Empty,
        resource: ResourceKind::None,
        amount: 0,
    };

    /// Пустой тайл заданного вида.
    pub fn new(kind: TileType) -> Self {
        Self {
            kind,
            resource: ResourceKind::None,
            amount: 0,
        }
    }

    /// Тайл вида `kind` с ресурсом.
    pub fn with_resource(kind: TileType, resource: ResourceKind, amount: u16) -> Self {
        Self { kind, resource, amount }
    }
}
