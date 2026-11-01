//! Процедурная генерация карты: тайлы, ресурсы, объекты, реки.
//!
//! Используем собственный value-noise с seeded hash, чтобы не тащить
//! внешние шумовые крейты. Детерминизм: одинаковый seed → одинаковая
//! карта в пределах одной платформы.
//!
//! Схема генерации:
//! 1. Рельеф — континентальный шум, искривлённый domain warp'ом, со спадом
//!    к краям карты: вода у границы читается как море, а карта не выглядит
//!    обрезанным прямоугольником. Глубины воды — три полосы по высоте,
//!    горы — по гребневому шуму на возвышенностях, биомы — по влажности.
//! 2. Классификация водоёмов: вода, соединённая с краем карты, — море;
//!    внутренние водоёмы — озёра, «океаническая» глубина в них понижается.
//!    Море отличается от озера и размером, и самой тёмной краской глубины.
//! 3. Реки: истоки на возвышенностях, русло — вниз по склону до воды;
//!    в локальных минимумах река устраивает пруд, так что каждая река
//!    куда-то впадает.
//! 4. Объекты поверх тайлов — отдельным проходом со своим потоком RNG.

use std::collections::{HashSet, VecDeque};

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng}; // Bevy 0.20 / rand 0.10: RngExt вместо Rng

use super::entities::EntityKind;
use super::objects::WorldObject;
use super::player::WorldPos;
use super::tile::{ResourceKind, TileType};
use super::world::{TilePos, World, WorldSize};

/// Ниже этой высоты — океан (самая глубокая вода).
const ELEV_OCEAN: f32 = 0.24;
/// Ниже этой высоты — глубокая вода.
const ELEV_DEEP: f32 = 0.33;
/// Уровень моря: ниже — мелководье, выше — суша.
const ELEV_SEA: f32 = 0.42;
/// Ниже уровня моря и до этой высоты — пляжи (песок).
const ELEV_BEACH: f32 = 0.44;

/// Минимальная высота истока реки: реки рождаются на возвышенностях.
const ELEV_RIVER_SOURCE: f32 = 0.55;

/// Масштаб континентального шума, тайлов на клетку: задаёт размер
/// материков и крупных островов.
const CONTINENT_SCALE: f32 = 80.0;
/// Сила domain warp: насколько смещаются координаты шума (в клетках).
/// Искривляет линии уровня, чтобы берега и хребты не выглядели сеткой.
const WARP_STRENGTH: f32 = 0.9;
/// Доля стороны карты, на которой высота спадает к краю, и потолок спада
/// в тайлах: на огромных мирах прибрежная зона не должна съедать карту.
const EDGE_FALLOFF_FRACTION: f32 = 0.15;
const EDGE_FALLOFF_MAX: f32 = 320.0;
/// Минимальный множитель высоты у самого края карты: край гарантированно
/// уходит под океан, но прибрежная полоса не «вдавливается» до нуля.
const EDGE_FALLOFF_FLOOR: f32 = 0.2;

/// Масштаб шума мха, тайлов: мелкий масштаб даёт пятна-области, а не крап.
const MOSS_SCALE: f32 = 20.0;
/// Порог мшистости: доля лесной травы, зарастающей мхом.
const MOSS_THRESHOLD: f32 = 0.62;

/// Соседи тайла по 8 направлениям в фиксированном порядке: при равных
/// высотах побеждает первый — путь спуска детерминирован.
const NEIGHBORS_8: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];
/// Соседи по 4 направлениям: с такой связностью размечаются регионы воды.
const NEIGHBORS_4: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// Хэш пары (x, y) с seed → float в [0, 1).
fn hash2(x: i32, y: i32, seed: u64) -> f32 {
    let mut h = (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ seed;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 27;
    h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    ((h & 0xFFFFFF) as f32) / (0xFFFFFF as f32)
}

/// 2D value noise с bilinear interpolation и smoothstep.
fn value_noise(x: f32, y: f32, seed: u64) -> f32 {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;

    let v00 = hash2(x0, y0, seed);
    let v10 = hash2(x0 + 1, y0, seed);
    let v01 = hash2(x0, y0 + 1, seed);
    let v11 = hash2(x0 + 1, y0 + 1, seed);

    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);

    let v0 = v00 + (v10 - v00) * sx;
    let v1 = v01 + (v11 - v01) * sx;
    v0 + (v1 - v0) * sy
}

/// Фрактальный шум (сумма октав value noise, нормированная).
fn fbm(x: f32, y: f32, seed: u64, octaves: u32) -> f32 {
    let mut total = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for i in 0..octaves {
        let s = seed.wrapping_add((i as u64).wrapping_mul(1000));
        total += value_noise(x * freq, y * freq, s) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    total / norm
}

/// Гребневый шум для горных хребтов: узкие вытянутые гребни вместо бугров.
/// Каждая октава — инвертированный модуль value noise в квадрате (заостряет
/// вершины), сумма нормируется в [0, 1].
fn ridge_noise(x: f32, y: f32, seed: u64, octaves: u32) -> f32 {
    let mut total = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for i in 0..octaves {
        let s = seed.wrapping_add((i as u64).wrapping_mul(1000));
        let v = 1.0 - (2.0 * value_noise(x * freq, y * freq, s) - 1.0).abs();
        total += v * v * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    total / norm
}

/// Smoothstep в [0, 1] — плавная кривая без изломов.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Высота тайла в [0, 1]: континентальный шум, искривлённый domain warp'ом
/// и умноженный на спад к краям карты. Спад прижимает воду к границам —
/// карта читается как континенты и острова, а не обрезанный прямоугольник.
pub(crate) fn elevation_at(x: i32, y: i32, size: i32, seed: u64) -> f32 {
    let fx = x as f32 / CONTINENT_SCALE;
    let fy = y as f32 / CONTINENT_SCALE;
    // Domain warp: смещаем координаты низкочастотным шумом.
    let wx = fx + (fbm(fx * 0.5 + 37.2, fy * 0.5 + 11.7, seed ^ 0x5EED_0001, 2) - 0.5) * WARP_STRENGTH;
    let wy = fy + (fbm(fx * 0.5 + 91.3, fy * 0.5 + 53.9, seed ^ 0x5EED_0002, 2) - 0.5) * WARP_STRENGTH;
    let continents = fbm(wx, wy, seed, 5);

    // Спад к краям: у границы множитель EDGE_FALLOFF_FLOOR (край всегда
    // под водой), дальше `span` тайлов — 1 (рельеф без искажений).
    let span = (size as f32 * EDGE_FALLOFF_FRACTION).clamp(1.0, EDGE_FALLOFF_MAX);
    let dx = (x.min(size - 1 - x) as f32 / span).clamp(0.0, 1.0);
    let dy = (y.min(size - 1 - y) as f32 / span).clamp(0.0, 1.0);
    let edge = EDGE_FALLOFF_FLOOR + (1.0 - EDGE_FALLOFF_FLOOR) * smoothstep(dx) * smoothstep(dy);
    continents * edge
}

/// Влажность тайла в [0, 1]: отдельный шум для распределения биомов.
fn moisture_at(x: i32, y: i32, seed: u64) -> f32 {
    let fx = x as f32 / 64.0;
    let fy = y as f32 / 64.0;
    fbm(fx * 1.7 + 100.0, fy * 1.7 + 100.0, seed ^ 0xDEAD_BEEF, 3)
}

/// «Гребневость» тайла в [0, 1]: высока вдоль вытянутых горных хребтов.
fn ridge_at(x: i32, y: i32, seed: u64) -> f32 {
    let fx = x as f32 / 48.0;
    let fy = y as f32 / 48.0;
    ridge_noise(fx, fy, seed ^ 0x1D6E_5EED, 4)
}

/// Мшистость тайла в [0, 1]: отдельный шум мелкого масштаба — пятна мха
/// в лесу получаются областями, а не россыпью одиночных тайлов.
fn moss_at(x: i32, y: i32, seed: u64) -> f32 {
    let fx = x as f32 / MOSS_SCALE;
    let fy = y as f32 / MOSS_SCALE;
    fbm(fx + 211.0, fy + 137.0, seed ^ 0x0DD5_1DEA, 3)
}

/// Вид тайла по высоте, гребневости и влажности.
/// Вода — три глубины по высоте; берег — песок; горы и камень — по
/// гребневости на возвышенностях (камень ещё и на голом высокогорье);
/// остальная суша — биом по влажности.
fn terrain_kind_for(elevation: f32, ridge: f32, moisture: f32) -> TileType {
    if elevation < ELEV_OCEAN {
        TileType::Ocean
    } else if elevation < ELEV_DEEP {
        TileType::DeepWater
    } else if elevation < ELEV_SEA {
        TileType::Water
    } else if elevation < ELEV_BEACH {
        TileType::Sand
    } else if elevation > 0.56 && ridge > 0.64 {
        TileType::Mountain
    } else if (elevation > 0.50 && ridge > 0.56) || elevation > 0.70 {
        TileType::Stone
    } else if moisture < 0.35 {
        TileType::Dirt
    } else if moisture < 0.62 {
        TileType::Grass
    } else {
        TileType::ForestGrass
    }
}

/// Сгенерировать мир: тайлы + водоёмы + реки + ресурсы + объекты.
pub fn generate_world(size: WorldSize, seed: u64) -> World {
    let mut world = World::new(size, seed);
    let mut rng = StdRng::seed_from_u64(seed);
    let n = size.tiles() as i32;

    // Проход 1: рельеф и ресурсы в тайлах.
    for y in 0..n {
        for x in 0..n {
            let elevation = elevation_at(x, y, n, seed);
            let ridge = ridge_at(x, y, seed);
            let moisture = moisture_at(x, y, seed);
            let kind = terrain_kind_for(elevation, ridge, moisture);
            let pos = TilePos { x, y };
            world.set_terrain(pos, kind);

            match kind {
                TileType::Stone if rng.random::<f32>() < 0.10 => {
                    world.set_resource(
                        pos,
                        ResourceKind::Copper,
                        20 + rng.random_range(0..80),
                    );
                }
                TileType::Grass | TileType::Dirt if rng.random::<f32>() < 0.02 => {
                    world.set_terrain(pos, TileType::Clay);
                    world.set_resource(pos, ResourceKind::Clay, 30 + rng.random_range(0..50));
                }
                TileType::ForestGrass
                    if moss_at(x, y, seed) > MOSS_THRESHOLD =>
                {
                    // Мох пятнами в лесу: тайл остаётся на уровне лесной травы.
                    world.set_terrain(pos, TileType::Moss);
                }
                _ => {}
            }
        }
    }

    // Вода у края карты — море; внутренние водоёмы — озёра с пониженной
    // глубиной, чтобы их можно было отличить на взгляд.
    classify_water(&mut world);

    // Реки: от возвышенностей вниз по склону, впадают в озёра и моря.
    carve_rivers(&mut world);

    // Проход 2: объекты поверх тайлов. Отдельный проход, чтобы не смешивать
    // RNG-последовательность тайлов с объектами (каждый проход детерминирован).
    for y in 0..n {
        for x in 0..n {
            let pos = TilePos { x, y };
            match world.terrain_kind(pos) {
                // Мох — лесная подстилка: растительность та же, что на лесной траве.
                TileType::ForestGrass | TileType::Moss => {
                    let r = rng.random::<f32>();
                    if r < 0.18 {
                        world.set_object(pos, Some(WorldObject::tree()));
                    } else if r < 0.22 {
                        // Плодов на дереве — 4–10 (GAME_DESIGN, приложение A).
                        world.set_object(
                            pos,
                            Some(WorldObject::apple_tree_with_fruit(rng.random_range(4..=10))),
                        );
                    } else if r < 0.25 {
                        world.set_object(pos, Some(WorldObject::bush()));
                    } else if r < 0.253 {
                        // Валуны в лесу редки (GAME_DESIGN, приложение A).
                        world.set_object(pos, Some(WorldObject::rock(5 + rng.random_range(0..10))));
                    }
                }
                TileType::Grass => {
                    if rng.random::<f32>() < 0.03 {
                        world.set_object(pos, Some(WorldObject::bush()));
                    }
                }
                TileType::Stone if rng.random::<f32>() < 0.05 => {
                    world.set_object(pos, Some(WorldObject::rock(5 + rng.random_range(0..10))));
                }
                TileType::Mountain if rng.random::<f32>() < 0.03 => {
                    world.set_object(pos, Some(WorldObject::rock(5 + rng.random_range(0..10))));
                }
                _ => {}
            }
        }
    }

    // Существа — после рельефа и объектов: им нужна готовая карта,
    // чтобы не появиться в воде.
    spawn_wildlife(&mut world, &mut rng);

    // Однородные чанки (океаны, пустыни) схлопываем в Uniform —
    // экономия ОЗУ сразу после генерации.
    for chunk in world.chunks.iter_mut() {
        chunk.terrain.try_compress();
    }

    world
}

/// Плотность заселения: одно существо на столько тайлов площади.
/// На стартовом мире 256×256 это восемь волков; на больших картах
/// количество растёт, но пока ограничено сверху, чтобы не перегружать
/// рендер и ИИ до появления LOD существ.
const TILES_PER_ANIMAL: u32 = 8192;
const MAX_ANIMALS: u32 = 64;

/// Заселяет мир волками на суше. Мир маленький (256×256) — восемь особей;
/// количество масштабируется от площади, но не больше `MAX_ANIMALS`.
fn spawn_wildlife(world: &mut World, rng: &mut StdRng) {
    let tiles = world.size.tiles();
    let count = (tiles * tiles / TILES_PER_ANIMAL).clamp(1, MAX_ANIMALS);
    let side = tiles as i32;
    let mut placed = 0_u32;
    let mut attempts = 0_u32;

    while placed < count && attempts < count * 100 {
        attempts += 1;
        let pos = TilePos {
            x: rng.random_range(0..side),
            y: rng.random_range(0..side),
        };
        // Волки живут на суше: мелководье проходимо при ходьбе, но
        // появляться в воде им незачем.
        if matches!(
            world.terrain_kind(pos),
            TileType::Water | TileType::DeepWater | TileType::Ocean
        ) {
            continue;
        }
        world
            .entities
            .spawn(EntityKind::Wolf, WorldPos::of_tile_center(pos));
        placed += 1;
    }
}

/// Битовая карта тайлов n×n для отметки обработанных позиций при обходе
/// водоёмов. Биты вместо `Vec<bool>`: на максимальном мире это ~33 МБ
/// вместо 268 МБ.
struct BitGrid {
    words: Vec<u64>,
    side: u32,
}

impl BitGrid {
    /// Карта нулей на `side × side` тайлов.
    fn new(side: u32) -> Self {
        let tiles = side as usize * side as usize;
        Self {
            words: vec![0; tiles.div_ceil(64)],
            side,
        }
    }

    /// Линейный индекс тайла (row-major).
    fn index(&self, x: i32, y: i32) -> usize {
        (y as usize) * self.side as usize + x as usize
    }

    /// Координаты тайла по линейному индексу.
    fn coords(&self, idx: usize) -> (i32, i32) {
        (
            (idx as u32 % self.side) as i32,
            (idx as u32 / self.side) as i32,
        )
    }

    /// Ставит бит тайла.
    fn set(&mut self, idx: usize) {
        self.words[idx / 64] |= 1 << (idx % 64);
    }

    /// Читает бит тайла.
    fn get(&self, idx: usize) -> bool {
        self.words[idx / 64] & (1 << (idx % 64)) != 0
    }
}

/// Размечает водоёмы по соединённости. Вода, достижимая от границы мира,
/// — море: глубины остаются как сгенерированы. Остальная вода — озёра:
/// «океаническая» глубина понижается до глубокой воды. Обходы детерминиро-
/// ваны: BFS с очередью и фиксированным порядком соседей.
fn classify_water(world: &mut World) {
    let n = world.size.tiles() as i32;
    let mut grid = BitGrid::new(n as u32);
    let mut queue: VecDeque<usize> = VecDeque::new();

    // Истоки моря — водные тайлы периметра карты.
    for i in 0..n {
        for pos in [
            TilePos { x: i, y: 0 },
            TilePos { x: i, y: n - 1 },
            TilePos { x: 0, y: i },
            TilePos { x: n - 1, y: i },
        ] {
            if world.terrain_kind(pos).is_water() {
                let idx = grid.index(pos.x, pos.y);
                if !grid.get(idx) {
                    grid.set(idx);
                    queue.push_back(idx);
                }
            }
        }
    }

    // BFS по воде от границы: всё достижимое — море.
    while let Some(idx) = queue.pop_front() {
        let (x, y) = grid.coords(idx);
        for (dx, dy) in NEIGHBORS_4 {
            let nx = x + dx;
            let ny = y + dy;
            if nx < 0 || ny < 0 || nx >= n || ny >= n {
                continue;
            }
            if !world.terrain_kind(TilePos { x: nx, y: ny }).is_water() {
                continue;
            }
            let next = grid.index(nx, ny);
            if !grid.get(next) {
                grid.set(next);
                queue.push_back(next);
            }
        }
    }

    // Озёра: остальная вода. Регионы собираются целиком: маленькие лужи
    // от шума высот осушаются в песок, крупные остаются озёрами с
    // пониженной глубиной. Пометка идёт тем же битовым набором: помеченный
    // тайл — уже море или разобранное озеро, повторный заход исключён.
    let tiles = n as usize * n as usize;
    for idx in 0..tiles {
        if grid.get(idx) {
            continue;
        }
        let (x, y) = grid.coords(idx);
        if !world.terrain_kind(TilePos { x, y }).is_water() {
            continue;
        }
        grid.set(idx);
        queue.push_back(idx);
        let mut region: Vec<TilePos> = Vec::new();
        while let Some(i) = queue.pop_front() {
            let (x, y) = grid.coords(i);
            region.push(TilePos { x, y });
            for (dx, dy) in NEIGHBORS_4 {
                let nx = x + dx;
                let ny = y + dy;
                if nx < 0 || ny < 0 || nx >= n || ny >= n {
                    continue;
                }
                if !world.terrain_kind(TilePos { x: nx, y: ny }).is_water() {
                    continue;
                }
                let next = grid.index(nx, ny);
                if !grid.get(next) {
                    grid.set(next);
                    queue.push_back(next);
                }
            }
        }
        if region.len() < BASIN_MIN_TILES {
            // Лужа от шума: осушаем в песок — на карте остаются только
            // водоёмы заметного размера.
            for pos in region {
                world.set_terrain(pos, TileType::Sand);
            }
        } else {
            // Озеро: «океанической» глубины в нём не бывает.
            for pos in region {
                if world.terrain_kind(pos) == TileType::Ocean {
                    world.set_terrain(pos, TileType::DeepWater);
                }
            }
        }
    }
}

/// Сколько попыток провести реки: пропорционально площади мира, с нижней
/// и верхней границей — маленькие миры не остаются без рек, огромные
/// не генерируются вечность.
fn river_attempts(n: i32) -> u32 {
    (((n / 64) * (n / 64) * 2) as u32).clamp(8, 256)
}

/// Проводит реки. Истоки выбираются случайными точками на возвышенностях
/// (отдельный RNG-поток от seed, чтобы проходы генерации не зависели друг
/// от друга), русло идёт вниз по склону до воды.
fn carve_rivers(world: &mut World) {
    let n = world.size.tiles() as i32;
    let seed = world.seed;
    let mut rng = StdRng::seed_from_u64(seed ^ 0x51CE_5EED_5A1D);
    let mut visited: HashSet<u32> = HashSet::new();
    for _ in 0..river_attempts(n) {
        let pos = TilePos {
            x: rng.random_range(0..n),
            y: rng.random_range(0..n),
        };
        if elevation_at(pos.x, pos.y, n, seed) < ELEV_RIVER_SOURCE {
            continue;
        }
        if world.terrain_kind(pos).is_water() {
            continue;
        }
        carve_river(world, pos, n, seed, &mut visited);
    }
}

/// Чем длиннее река, тем шире её русло у низовьев: 1–3 тайла. Пороги
/// подобраны так, чтобы на карте были и ручьи, и полноводные реки.
fn max_river_width(length: usize) -> usize {
    if length < 64 {
        1
    } else if length < 160 {
        2
    } else {
        3
    }
}

/// Ширина русла на позиции `i` пути длиной `length`: река растёт от
/// истока к устью, но не шире максимума для своей длины.
fn river_width_at(i: usize, length: usize) -> u32 {
    let grown = 1 + (3 * i / length.max(1));
    grown.min(max_river_width(length)) as u32
}

/// Проводит одну реку от истока вниз по склону.
///
/// Сначала путь трассируется по исходной высоте (самый низкий из восьми
/// соседей; при равенстве — первый в фиксированном порядке `NEIGHBORS_8`,
/// поэтому путь детерминирован), затем красится: русло помечается
/// мелководьем, ширина растёт от истока к устью. Любое завершение пути —
/// водоём: существующая вода, край карты (океан у границы) или залитая
/// котловина локального минимума. Обрывков посреди суши не остаётся.
fn carve_river(world: &mut World, start: TilePos, n: i32, seed: u64, visited: &mut HashSet<u32>) {
    // Предохранитель по длине: путь не может идти вечно.
    let max_steps = (n as usize / 2).clamp(256, 4096);
    let mut path: Vec<TilePos> = Vec::with_capacity(64);
    let mut pos = start;
    // Путь закончился в существующей воде (или у края-океана): озеро не нужно.
    let mut reached_water = false;
    for _ in 0..max_steps {
        if world.terrain_kind(pos).is_water() {
            reached_water = true; // впала в существующую воду
            break;
        }
        path.push(pos);
        let elevation = elevation_at(pos.x, pos.y, n, seed);

        let mut best: Option<(TilePos, f32)> = None;
        for (dx, dy) in NEIGHBORS_8 {
            let nx = pos.x + dx;
            let ny = pos.y + dy;
            if nx < 0 || ny < 0 || nx >= n || ny >= n {
                continue;
            }
            let ne = elevation_at(nx, ny, n, seed);
            if best.map(|(_, best_e)| ne < best_e).unwrap_or(true) {
                best = Some((TilePos { x: nx, y: ny }, ne));
            }
        }

        let Some((next, next_e)) = best else {
            // Край мира: тайлы границы всегда под океаном.
            reached_water = true;
            break;
        };
        if next_e > elevation || visited.contains(&linear_index(next, n)) {
            break; // локальный минимум: ниже зальётся котловина-озеро
        }
        visited.insert(linear_index(pos, n));
        pos = next;
    }

    // Локальный минимум или предохранитель длины: заливаем котловину
    // вокруг конечной точки — река заканчивается озером.
    if !reached_water {
        flood_basin(world, pos, n, seed);
    }

    // Красим русло с ростом ширины от истока к устью. Уже водные тайлы
    // (включая свежезалитое озеро и устье) не перекрашиваются: река не
    // должна обмелять океан.
    let length = path.len();
    for (i, &tile) in path.iter().enumerate() {
        let width = river_width_at(i, length);
        // Направление течения в точке пути: к следующему тайлу, у устья —
        // от предыдущего. Перпендикуляр достраивает русло в стороны.
        let dir = if i + 1 < length {
            (path[i + 1].x - tile.x, path[i + 1].y - tile.y)
        } else if i > 0 {
            (tile.x - path[i - 1].x, tile.y - path[i - 1].y)
        } else {
            (1, 0)
        };
        let size = world.size.tiles() as i32;
        let paint = |p: TilePos, world: &mut World| {
            if p.x >= 0
                && p.y >= 0
                && p.x < size
                && p.y < size
                && !world.terrain_kind(p).is_water()
            {
                world.set_terrain(p, TileType::Water);
            }
        };
        paint(tile, world);
        if width >= 2 {
            paint(
                TilePos { x: tile.x + dir.1, y: tile.y - dir.0 },
                world,
            );
        }
        if width >= 3 {
            paint(
                TilePos { x: tile.x - dir.1, y: tile.y + dir.0 },
                world,
            );
        }
    }
}

/// Минимальный размер озера-стока: меньше — река «дорезает» чашу до этого
/// размера, чтобы конец русла не выглядел обрывком.
const BASIN_MIN_TILES: usize = 12;

/// Потолок заливки котловины: плато не должно заливаться целиком.
const BASIN_MAX_TILES: usize = 512;

/// Допуск высоты при заливке котловины вокруг локального минимума.
const BASIN_EPS: f32 = 0.01;

/// Заливает котловину вокруг локального минимума водой.
///
/// Сначала собираются все соединённые с центром тайлы не выше уровня
/// минимума (+допуск) — это естественная чаша. Если рядом нашлась вода
/// (сток в море/озеро), заливка заканчивается: река уже куда-то впадает.
/// Если чаша вышла меньше минимального размера, к ней добавляются самые
/// низкие соседние тайлы, пока озеро не станет заметным на карте.
fn flood_basin(world: &mut World, center: TilePos, n: i32, seed: u64) {
    let level = elevation_at(center.x, center.y, n, seed);
    let mut lake: Vec<TilePos> = vec![center];
    let mut in_lake: HashSet<u32> = HashSet::new();
    in_lake.insert(linear_index(center, n));
    let mut connected = false; // котловина примыкает к существующей воде

    let mut queue: VecDeque<TilePos> = VecDeque::new();
    queue.push_back(center);
    while let Some(pos) = queue.pop_front() {
        if lake.len() >= BASIN_MAX_TILES {
            break;
        }
        for (dx, dy) in NEIGHBORS_4 {
            let nx = pos.x + dx;
            let ny = pos.y + dy;
            if nx < 0 || ny < 0 || nx >= n || ny >= n {
                continue;
            }
            let neighbor = TilePos { x: nx, y: ny };
            let idx = linear_index(neighbor, n);
            if in_lake.contains(&idx) {
                continue;
            }
            if world.terrain_kind(neighbor).is_water() {
                connected = true; // сток найден — расширять озеро не нужно
                break;
            }
            if elevation_at(nx, ny, n, seed) <= level + BASIN_EPS {
                in_lake.insert(idx);
                lake.push(neighbor);
                queue.push_back(neighbor);
            }
        }
        if connected {
            break;
        }
    }

    // Чаша меньше минимума и без стока: дорезаем самыми низкими соседями.
    while !connected && lake.len() < BASIN_MIN_TILES {
        let mut best: Option<(TilePos, f32)> = None;
        for pos in &lake {
            for (dx, dy) in NEIGHBORS_4 {
                let nx = pos.x + dx;
                let ny = pos.y + dy;
                if nx < 0 || ny < 0 || nx >= n || ny >= n {
                    continue;
                }
                let neighbor = TilePos { x: nx, y: ny };
                let idx = linear_index(neighbor, n);
                if in_lake.contains(&idx) {
                    continue;
                }
                if world.terrain_kind(neighbor).is_water() {
                    connected = true;
                    break;
                }
                let e = elevation_at(nx, ny, n, seed);
                if best.map(|(_, best_e)| e < best_e).unwrap_or(true) {
                    best = Some((neighbor, e));
                }
            }
            if connected {
                break;
            }
        }
        let Some((neighbor, _)) = best else {
            break; // вся окружность — вода: сток уже есть
        };
        in_lake.insert(linear_index(neighbor, n));
        lake.push(neighbor);
    }

    for pos in lake {
        world.set_terrain(pos, TileType::Water);
    }
}

/// Линейный индекс тайла (row-major) — ключ множества посещённых позиций.
fn linear_index(pos: TilePos, n: i32) -> u32 {
    (pos.y as u32) * (n as u32) + pos.x as u32
}
