//! Чанк — блок 16×16 тайлов. Несколько независимых слоёв: рельеф, ресурсы,
//! объекты, предметы на земле, культуры, контейнеры станций.
//! Слои хранятся и сжимаются независимо (GAME_DESIGN 3.2, ARCHITECTURE 2.2).

use std::collections::HashMap;

use super::containers::Container;
use super::crops::Crop;
use super::items::ItemStack;
use super::objects::WorldObject;
use super::tile::{ResourceKind, TileType};

pub const CHUNK_SIZE: usize = 16;
pub const CHUNK_TILES: usize = CHUNK_SIZE * CHUNK_SIZE;

/// Слой рельефа: одно значение на весь чанк или полный массив тайлов.
/// Uniform — основной случай для однородных областей (океан, пустыня):
/// 1 байт вместо 256, обход без чтения массива.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerrainTiles {
    Uniform(TileType),
    Full(Box<[TileType; CHUNK_TILES]>),
}

impl TerrainTiles {
    /// Однородный слой заданного вида.
    pub fn uniform(kind: TileType) -> Self {
        Self::Uniform(kind)
    }
    /// Вид тайла по линейному индексу (0..256).
    pub fn get(&self, idx: usize) -> TileType {
        match self {
            Self::Uniform(kind) => *kind,
            Self::Full(arr) => arr[idx],
        }
    }

    /// Записывает вид тайла; первый отличающийся тайл разворачивает Uniform в Full.
    pub fn set(&mut self, idx: usize, kind: TileType) {
        match self {
            Self::Uniform(current) if *current == kind => {}
            Self::Uniform(current) => {
                let mut arr = Box::new([*current; CHUNK_TILES]);
                arr[idx] = kind;
                *self = Self::Full(arr);
            }
            Self::Full(arr) => arr[idx] = kind,
        }
    }

    /// Схлопывает полный слой в Uniform, если все тайлы одинаковы.
    /// Вызывается редко: после генерации, при «засыпании» чанка, при сохранении.
    pub fn try_compress(&mut self) {
        if let Self::Full(arr) = self {
            let first = arr[0];
            if arr.iter().all(|kind| *kind == first) {
                *self = Self::Uniform(first);
            }
        }
    }
}

/// Залежь ресурса на тайле.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResourceDeposit {
    pub kind: ResourceKind,
    pub amount: u16,
}

/// Чанк 16×16. Рельеф — плотный слой со сжатием; остальные слои —
/// разреженные: они занимают малую долю тайлов, и плотный массив на каждый
/// слой раздувал бы память на порядки (ARCHITECTURE 2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub terrain: TerrainTiles,
    /// Ресурсы по линейному индексу тайла 0..256.
    pub resources: HashMap<u8, ResourceDeposit>,
    /// Объекты по линейному индексу тайла 0..256. Правило: один объект на тайл.
    pub objects: HashMap<u8, WorldObject>,
    /// Предметы, лежащие на тайлах (дроп). Разреженный слой, как ресурсы.
    pub items: HashMap<u8, ItemStack>,
    /// Культуры на пашне по линейному индексу тайла (одна культура на тайл).
    pub crops: HashMap<u8, Crop>,
    /// Контейнеры станций и складов (печи, бочка, склад, костёр).
    /// Существуют только у тайлов со станцией-объектом.
    pub containers: HashMap<u8, Container>,
}

impl Chunk {
    /// Пустой чанк: рельеф Empty, остальных слоёв нет.
    pub fn empty() -> Self {
        Self {
            terrain: TerrainTiles::uniform(TileType::Empty),
            resources: HashMap::new(),
            objects: HashMap::new(),
            items: HashMap::new(),
            crops: HashMap::new(),
            containers: HashMap::new(),
        }
    }
}
