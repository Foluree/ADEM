//! Объекты над тайлами. Один объект на тайл; хранятся разреженно в слое
//! чанка, а не как Bevy-сущности — сущности рендер создаёт только для
//! видимых объектов.
//!
//! Два класса видов:
//! - **Объекты** (`ObjectKind`) — всё, что выросло/возникло само: деревья,
//!   кусты, валуны, яблони. Генерируются проходом генерации.
//! - **Постройки** (`BuildingKind`) — рукотворные сооружения: костёр,
//!   верстак, печи, бочка, склад, кузнечный стол. Не генерируются,
//!   строятся игроком (меню строительства) или редактором.
//!
//! `WorldObject` хранит вид как `ObjectKind`: вариант `Building` несёт
//! конкретную постройку, поэтому слой чанка и сохранения остаются одним
//! слоем с одной кодировкой дискриминантов.

use super::items::ItemKind;
use super::tile::ResourceKind;

/// Вид объекта над тайлом: природный объект или постройка.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ObjectKind {
    Tree,
    Bush,
    Rock,
    AppleTree,
    /// Постройка (рукотворное сооружение) — см. `BuildingKind`.
    Building(BuildingKind),
}

/// Постройка: рукотворное сооружение. Не генерируется, ставится игроком
/// или редактором. Новые виды — только в конец (стабильные дискриминанты).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BuildingKind {
    /// Костёр (строится игроком, пока горит вечно).
    Campfire,
    /// Рабочая станция: верстак (сетка 3×3, крафты manual + свои).
    CraftingTable,
    /// Рабочая станция: печь (готовка на топливе, 100 тиков).
    Furnace,
    /// Рабочая станция: бочка (24 слота хранения).
    Barrel,
    /// Рабочая станция: плавильная печь (руда → слитки, 200 тиков).
    BlastFurnace,
    /// Склад: плоская постройка-декаль, 16 слотов хранения.
    Storage,
    /// Рабочая станция: кузнечный стол (сетка 3×3 + 4 слота инструментов).
    SmithingTable,
}

impl ObjectKind {
    /// Природные объекты в стабильном порядке (генерация, UI редактора).
    /// Новые виды добавляются в конец — стабильные значения не сдвигаются.
    pub const ALL: [ObjectKind; 4] = [
        ObjectKind::Tree,
        ObjectKind::Bush,
        ObjectKind::Rock,
        ObjectKind::AppleTree,
    ];

    /// Является ли вид постройкой (у кнопок редактора, меню строительства).
    pub fn is_building(self) -> bool {
        matches!(self, ObjectKind::Building(_))
    }

    /// Постройка под видом, если это постройка.
    pub fn building_kind(self) -> Option<BuildingKind> {
        match self {
            ObjectKind::Building(kind) => Some(kind),
            _ => None,
        }
    }

    /// Рабочая станция ли вид: открывается по ПКМ, имеет контейнер
    /// (или сетку крафта) со своей механикой. Пока все постройки —
    /// станции; решение за видом постройки (`BuildingKind::is_station`).
    pub fn is_station(self) -> bool {
        self.building_kind().is_some_and(BuildingKind::is_station)
    }

    /// Стабильное значение u8 (сохранения): природные виды 0–3, постройки
    /// 4 + дискриминант вида постройки.
    pub fn discriminant(self) -> u8 {
        match self {
            ObjectKind::Tree => 0,
            ObjectKind::Bush => 1,
            ObjectKind::Rock => 2,
            ObjectKind::AppleTree => 3,
            ObjectKind::Building(kind) => 4 + kind as u8,
        }
    }

    /// Вид объекта по стабильному значению u8 (сохранения).
    pub fn from_discriminant(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Tree),
            1 => Some(Self::Bush),
            2 => Some(Self::Rock),
            3 => Some(Self::AppleTree),
            4..=10 => BuildingKind::from_discriminant(value - 4).map(Self::Building),
            _ => None,
        }
    }
}

impl BuildingKind {
    /// Все постройки в стабильном порядке (меню строительства, редактор).
    pub const ALL: [BuildingKind; 7] = [
        BuildingKind::Campfire,
        BuildingKind::CraftingTable,
        BuildingKind::Furnace,
        BuildingKind::Barrel,
        BuildingKind::BlastFurnace,
        BuildingKind::Storage,
        BuildingKind::SmithingTable,
    ];

    /// Рабочая станция ли постройка: открывается по ПКМ, имеет контейнер
    /// (или сетку крафта). Сейчас станции все постройки; match без
    /// wildcard заставит решить это для каждой новой постройки.
    pub fn is_station(self) -> bool {
        match self {
            BuildingKind::Campfire
            | BuildingKind::CraftingTable
            | BuildingKind::Furnace
            | BuildingKind::Barrel
            | BuildingKind::BlastFurnace
            | BuildingKind::Storage
            | BuildingKind::SmithingTable => true,
        }
    }

    /// Вид постройки по стабильному значению u8 (сохранения).
    pub fn from_discriminant(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Campfire),
            1 => Some(Self::CraftingTable),
            2 => Some(Self::Furnace),
            3 => Some(Self::Barrel),
            4 => Some(Self::BlastFurnace),
            5 => Some(Self::Storage),
            6 => Some(Self::SmithingTable),
            _ => None,
        }
    }
}

/// Стадия роста (у деревьев и кустов; валуны рождаются зрелыми).
/// Варианты Sapling/Young включит механика роста (ROADMAP, этап «Мир и активность»).
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ObjectStage {
    Sapling,
    Young,
    Mature,
}

impl ObjectStage {
    /// Стадия роста по стабильному значению u8 (сохранения).
    pub fn from_discriminant(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Sapling),
            1 => Some(Self::Young),
            2 => Some(Self::Mature),
            _ => None,
        }
    }
}

/// Объект на тайле: лёгкая копируемая запись без аллокаций.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WorldObject {
    pub kind: ObjectKind,
    /// Стадия включится механикой роста объектов (ROADMAP, этап «Мир и активность»).
    #[allow(dead_code)]
    pub stage: ObjectStage,
    /// Семантика зависит от вида: запас камня у валуна, счётчик плодов
    /// у яблони, 0 у остальных.
    pub charge: u16,
}

impl WorldObject {
    /// Взрослое дерево — постоянный объект лесной травы.
    pub fn tree() -> Self {
        Self { kind: ObjectKind::Tree, stage: ObjectStage::Mature, charge: 0 }
    }

    /// Взрослый куст — источник палок, ломается при сборе.
    pub fn bush() -> Self {
        Self { kind: ObjectKind::Bush, stage: ObjectStage::Mature, charge: 0 }
    }

    /// Валун с запасом камня (GAME_DESIGN, приложение A: 5–15 единиц).
    pub fn rock(amount: u16) -> Self {
        Self { kind: ObjectKind::Rock, stage: ObjectStage::Mature, charge: amount }
    }

    /// Взрослая яблоня со стандартным запасом плодов.
    pub fn apple_tree() -> Self {
        Self::apple_tree_with_fruit(6)
    }

    /// Объект для постройки по виду постройки.
    pub fn building(kind: BuildingKind) -> Self {
        Self {
            kind: ObjectKind::Building(kind),
            stage: ObjectStage::Mature,
            charge: 0,
        }
    }

    /// Взрослая яблоня с заданным числом плодов (при генерации — 4–10).
    pub fn apple_tree_with_fruit(fruit: u16) -> Self {
        Self {
            kind: ObjectKind::AppleTree,
            stage: ObjectStage::Mature,
            charge: fruit,
        }
    }

    /// Что даёт объект при добыче/сборе. None — руками не добывается.
    pub fn harvest_yield(&self) -> Option<(ResourceKind, u16)> {
        match self.kind {
            // Куст при сборе ломается и даёт две палки (решение автора).
            ObjectKind::Bush => Some((ResourceKind::Stick, 2)),
            // От валуна откалывают камень, пока есть запас.
            ObjectKind::Rock if self.charge > 0 => Some((ResourceKind::Stone, 1)),
            _ => None,
        }
    }

    /// Сколько яблок можно собрать с дерева. None — это не плодоносящая
    /// яблоня (пустая яблоня остаётся объектом, но больше не даёт плодов).
    pub fn apple_yield(&self) -> Option<u16> {
        (self.kind == ObjectKind::AppleTree && self.charge > 0).then_some(self.charge)
    }
}

/// Стоимость строительства постройки: предмет и количество. `None` — цена
/// не задана (постройка ставится редактором и в меню строительства не
/// показывается). Цены построек собраны здесь, чтобы симуляция (команда
/// строительства) и UI (голограмма, меню) читали одни и те же данные.
pub fn build_cost(kind: BuildingKind) -> Option<(ItemKind, u32)> {
    match kind {
        BuildingKind::Campfire => Some((ItemKind::Stick, 16)),
        // Крафтов рабочих станций пока нет: строятся редактором, в меню
        // строительства не показываются (цена не задана).
        _ => None,
    }
}
