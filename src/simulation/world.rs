//! Мир: размер, чанки со слоями, доступ к тайлам и объектам.

use super::chunk::{Chunk, ResourceDeposit, TerrainTiles, CHUNK_SIZE};
use super::combat::{Projectile, StuckSpear};
use super::entities::EntityStore;
use super::items::ItemStack;
use super::objects::WorldObject;
use super::tile::{ResourceKind, Tile, TileType};

/// Размер мира в тайлах по стороне.
///
/// Значение произвольное (задаётся полем ввода в UI), а не пресетом.
/// Конструктор зажимает его в допустимые пределы и выравнивает на чанк,
/// чтобы сетка чанков покрывала мир без «хвостов».
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WorldSize {
    side: u32,
}

impl WorldSize {
    /// Минимальная сторона мира (GAME_DESIGN.md, раздел 3.2).
    pub const MIN: u32 = 256;
    /// Максимальная сторона мира (GAME_DESIGN.md, раздел 3.2).
    pub const MAX: u32 = 16384;

    /// Создаёт размер мира из произвольного числа: зажимает в [MIN, MAX]
    /// и округляет вниз до кратности чанку (16 тайлов).
    pub fn new(side: u32) -> Self {
        let side = side.clamp(Self::MIN, Self::MAX);
        let side = side - side % CHUNK_SIZE as u32;
        Self { side }
    }

    /// Сторона мира в тайлах.
    pub fn tiles(self) -> u32 {
        self.side
    }

    /// Подпись для UI.
    pub fn label(self) -> String {
        format!("{}×{}", self.side, self.side)
    }
}

/// Размер одного тайла в мировых единицах (пикселях).
pub const TILE_SIZE: f32 = 16.0;

/// Позиция тайла в мире (в тайлах).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TilePos {
    pub x: i32,
    pub y: i32,
}

/// Позиция чанка (в чанках).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ChunkPos {
    pub x: u32,
    pub y: u32,
}

/// Мир целиком.
pub struct World {
    pub size: WorldSize,
    /// Seed, которым мир сгенерирован; понадобится сохранениям.
    #[allow(dead_code)]
    pub seed: u64,
    pub chunks_per_side: u32,
    pub chunks: Vec<Chunk>,
    /// Существа мира: самостоятельные сущности, не свойства тайлов.
    pub entities: EntityStore,
    /// Температура окружения, ℃. Считается из календаря (месяц + время
    /// суток, `simulation/climate.rs`) и обновляется app-слоем каждый тик.
    /// Локальный климат появится вместе с погодой. Читают UI и будущие
    /// потребности.
    #[allow(dead_code)]
    pub ambient_temperature_c: f32,
    /// Снаряды в полёте (стрелы, брошенные копья) — см. `combat`.
    pub projectiles: Vec<Projectile>,
    /// Копья, воткнувшиеся в землю; живут до подбора игроком.
    pub stuck_spears: Vec<StuckSpear>,
    /// Следующий идентификатор снаряда (для рендера); 0 — «нет id».
    pub next_projectile_id: u64,
}

impl World {
    /// Сколько чанков по стороне для данного размера мира.
    pub fn chunks_per_side(size: WorldSize) -> u32 {
        size.tiles() / CHUNK_SIZE as u32
    }

    /// Оценка расхода ОЗУ миром в байтах (для F3-панели): чанки с рельефом
    /// и разреженными слоями, существа, снаряды, планировщик. Приближение
    /// без учёта аллокатора: ёмкости × размер записи, hashbrown с запасом
    /// на байт управления и загрузку 7/8. Итог делить на байты/тайл —
    /// основной показатель бюджета памяти (ARCHITECTURE 3.1).
    pub fn memory_bytes(&self) -> u64 {
        const fn size_of<T>() -> u64 {
            std::mem::size_of::<T>() as u64
        }
        // Оценка разреженного слоя: (ключ, значение) + байт управления, ×8/7.
        const fn map_bytes<K, V>(len: usize) -> u64 {
            (len as u64) * (size_of::<(K, V)>() + 2) * 8 / 7
        }

        let mut bytes = (self.chunks.capacity() as u64) * size_of::<Chunk>();
        for chunk in &self.chunks {
            if matches!(chunk.terrain, TerrainTiles::Full(_)) {
                // Отдельная аллокация Box<[TileType; 256]>.
                bytes += 256 * size_of::<TileType>();
            }
            bytes += map_bytes::<u8, ResourceDeposit>(chunk.resources.len());
            bytes += map_bytes::<u8, WorldObject>(chunk.objects.len());
            bytes += map_bytes::<u8, ItemStack>(chunk.items.len());
        }
        bytes += (self.entities.len() as u64) * size_of::<super::entities::SimEntity>();
        bytes += (self.projectiles.capacity() as u64) * size_of::<Projectile>();
        bytes += (self.stuck_spears.capacity() as u64) * size_of::<StuckSpear>();
        bytes
    }

    /// Создать пустой мир заданного размера.
    pub fn new(size: WorldSize, seed: u64) -> Self {
        let cps = Self::chunks_per_side(size);
        let chunks = (0..cps * cps).map(|_| Chunk::empty()).collect();
        Self {
            size,
            seed,
            chunks_per_side: cps,
            chunks,
            entities: EntityStore::default(),
            ambient_temperature_c: 24.0,
            projectiles: Vec::new(),
            stuck_spears: Vec::new(),
            next_projectile_id: 0,
        }
    }

    /// Ссылка на чанк по его позиции.
    pub fn chunk(&self, pos: ChunkPos) -> &Chunk {
        &self.chunks[(pos.y * self.chunks_per_side + pos.x) as usize]
    }

    /// Мутабельная ссылка на чанк по его позиции.
    pub fn chunk_mut(&mut self, pos: ChunkPos) -> &mut Chunk {
        let idx = (pos.y * self.chunks_per_side + pos.x) as usize;
        &mut self.chunks[idx]
    }

    /// Плоский вид тайла (рельеф + ресурс), собираемый из слоёв на лету.
    /// View-доступ для UI и тестов; для горячего обхода рельефа используй
    /// `terrain_kind` — он не трогает разреженные слои.
    #[allow(dead_code)] // view-доступ по ARCHITECTURE 2.3, активные пользователи — впереди
    pub fn tile(&self, pos: TilePos) -> Tile {
        let (cpos, idx) = split_tile_pos(pos);
        let chunk = self.chunk(cpos);
        let (resource, amount) = chunk
            .resources
            .get(&idx)
            .map(|r| (r.kind, r.amount))
            .unwrap_or((ResourceKind::None, 0));
        Tile {
            kind: chunk.terrain.get(idx as usize),
            resource,
            amount,
        }
    }

    /// Вид тайла без ресурса — только рельеф. Горячий путь для меша и генерации.
    pub fn terrain_kind(&self, pos: TilePos) -> TileType {
        let (cpos, idx) = split_tile_pos(pos);
        self.chunk(cpos).terrain.get(idx as usize)
    }

    /// Меняет рельеф тайла.
    pub fn set_terrain(&mut self, pos: TilePos, kind: TileType) {
        let (cpos, idx) = split_tile_pos(pos);
        self.chunk_mut(cpos).terrain.set(idx as usize, kind);
    }

    /// Меняет ресурс тайла; `None` или нулевое количество убирает запись —
    /// слой разреженный, пустые тайлы в нём не хранятся.
    pub fn set_resource(&mut self, pos: TilePos, kind: ResourceKind, amount: u16) {
        let (cpos, idx) = split_tile_pos(pos);
        let chunk = self.chunk_mut(cpos);
        if kind == ResourceKind::None || amount == 0 {
            chunk.resources.remove(&idx);
        } else {
            chunk.resources.insert(idx, ResourceDeposit { kind, amount });
        }
    }

    /// Ставит объект на тайл или убирает его (один объект на тайл:
    /// повторная запись перезаписывает).
    pub fn set_object(&mut self, pos: TilePos, obj: Option<WorldObject>) {
        let (cpos, idx) = split_tile_pos(pos);
        let chunk = self.chunk_mut(cpos);
        match obj {
            Some(obj) => {
                chunk.objects.insert(idx, obj);
            }
            None => {
                chunk.objects.remove(&idx);
            }
        }
    }

    /// Объект на тайле, если есть.
    pub fn object_at(&self, pos: TilePos) -> Option<&WorldObject> {
        let (cpos, idx) = split_tile_pos(pos);
        self.chunk(cpos).objects.get(&idx)
    }

    /// Ставит стек предметов на тайл или убирает его.
    pub fn set_item(&mut self, pos: TilePos, stack: Option<ItemStack>) {
        let (cpos, idx) = split_tile_pos(pos);
        let chunk = self.chunk_mut(cpos);
        match stack {
            Some(s) => {
                chunk.items.insert(idx, s);
            }
            None => {
                chunk.items.remove(&idx);
            }
        }
    }

    /// Предмет на тайле, если есть.
    pub fn item_at(&self, pos: TilePos) -> Option<&ItemStack> {
        let (cpos, idx) = split_tile_pos(pos);
        self.chunk(cpos).items.get(&idx)
    }
}

/// Раскладывает мировую позицию тайла на (позиция чанка, линейный индекс в чанке).
fn split_tile_pos(pos: TilePos) -> (ChunkPos, u8) {
    let cx = pos.x.div_euclid(CHUNK_SIZE as i32) as u32;
    let cy = pos.y.div_euclid(CHUNK_SIZE as i32) as u32;
    let tx = pos.x.rem_euclid(CHUNK_SIZE as i32) as usize;
    let ty = pos.y.rem_euclid(CHUNK_SIZE as i32) as usize;
    (ChunkPos { x: cx, y: cy }, (ty * CHUNK_SIZE + tx) as u8)
}

/// Раскладывает позицию тайла на (чанк, линейный индекс) — публичная
/// обёртка над внутренним разбиением: задачи планировщика адресуются
/// парой «чанк + индекс» (simulation/vegetation.rs).
pub fn split_tile(pos: TilePos) -> (ChunkPos, u8) {
    split_tile_pos(pos)
}

/// Собирает позицию тайла из чанка и линейного индекса — обратная
/// операция к `split_tile`.
pub fn tile_pos(chunk: ChunkPos, local_index: u8) -> TilePos {
    let local = local_index as usize;
    TilePos {
        x: chunk.x as i32 * CHUNK_SIZE as i32 + (local % CHUNK_SIZE) as i32,
        y: chunk.y as i32 * CHUNK_SIZE as i32 + (local / CHUNK_SIZE) as i32,
    }
}
