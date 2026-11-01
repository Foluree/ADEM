//! Растительность и эрозия берегов: самозапланированные процессы мира.
//!
//! Никаких обходов мира каждый тик (GAME_DESIGN 4.4): каждое активное
//! «место» — тайл травы, тайл воды, чанк леса — живёт собственной цепочкой
//! событий в планировщике. Событие исполняется → применяет правило →
//! сразу планирует следующее событие той же цепочки. Когда правило
//! становится неприменимо (соседи кончились, предел кустов достигнут),
//! цепочка гаснет и больше не тратит ни операций, ни памяти.
//!
//! Вероятности правил заданы «за такт»; интервал до следующего события
//! рисуется из геометрического распределения со средним 1/p, так что
//! средняя частота совпадает с вероятностной формулировкой.
//!
//! Детерминизм: RNG каждого события сеется от (seed мира, тик события,
//! вид задачи, координаты) — поток не зависит от порядка прочих событий;
//! обходы при посеве идут row-major, порядок регистрации задач стабилен.

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use super::chunk::CHUNK_TILES;
use super::dirty::DirtyTiles;
use super::objects::{ObjectKind, WorldObject};
use super::scheduler::{Scheduler, TaskKind};
use super::tile::TileType;
use super::world::{ChunkPos, TilePos, World, split_tile, tile_pos};

/// Вероятность за такт, что тайл травы займёт соседний тайл земли.
pub const GRASS_SPREAD_P: f32 = 0.0007;

/// Вероятность за такт, что один тайл лесной травы вырастит куст.
pub const BUSH_GROW_P: f32 = 0.0001;

/// Максимум кустов в одном чанке: после предела цепочка чанка гаснет.
pub const BUSH_CAP_PER_CHUNK: u32 = 16;

/// Вероятность за такт, что тайл воды подточит одного из эродируемых соседей.
pub const EROSION_P: f32 = 0.0001;

/// Теги видов задач для посева RNG событий (стабильны, часть детерминизма).
const TAG_GRASS: u8 = 1;
const TAG_BUSH: u8 = 2;
const TAG_EROSION: u8 = 3;

/// Соседи тайла на расстоянии 1 тайла (ортогональные) — все правила
/// растительности действуют только на таких соседей.
const NEIGHBORS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// Эродируемый водой тайл: лесная трава и трава постепенно превращаются
/// водой в следующую стадию, земля — сразу в песок.
fn erodible(kind: TileType) -> bool {
    matches!(kind, TileType::ForestGrass | TileType::Grass | TileType::Dirt)
}

/// Во что вода превращает эродируемый сосед: лесная трава → трава,
/// трава и земля → песок.
fn eroded(kind: TileType) -> TileType {
    match kind {
        TileType::ForestGrass => TileType::Grass,
        _ => TileType::Sand,
    }
}

/// Соседи `pos` в границах мира, в фиксированном порядке (детерминизм).
/// Итератор без аллокаций: соседи ≤ 4, функция вызывается на каждый тайл
/// при посеве и на каждое событие цепочек.
fn neighbors<'a>(world: &'a World, pos: TilePos) -> impl Iterator<Item = TilePos> + 'a {
    let size = world.size.tiles() as i32;
    NEIGHBORS.iter().filter_map(move |&(dx, dy)| {
        let (nx, ny) = (pos.x + dx, pos.y + dy);
        (nx >= 0 && ny >= 0 && nx < size && ny < size).then_some(TilePos { x: nx, y: ny })
    })
}

/// Детерминированный RNG события: сеется от seed мира, тика события,
/// тега вида задачи и координат. Поток не зависит от порядка прочих
/// событий и воспроизводится при загрузке сохранения.
fn event_rng(seed: u64, tick: u64, tag: u8, chunk: ChunkPos, local: u8) -> StdRng {
    let mut h = seed
        ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (tag as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ (chunk.x as u64).wrapping_mul(0x94D0_49BB_1331_11EB)
        ^ (chunk.y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (local as u64).wrapping_mul(0x2545_F491_4F6C_DD1D);
    h = h.wrapping_mul(0xD6E8_FEB8_6659_7B93);
    h ^= h >> 32;
    StdRng::seed_from_u64(h)
}

/// Интервал до следующего события цепочки: геометрическое распределение
/// с вероятностью `p` за такт (среднее 1/p тиков), минимум 1 тик.
fn interval_ticks(rng: &mut StdRng, p: f32) -> u64 {
    let p = p.clamp(1e-6, 0.999);
    let u = 1.0 - rng.random::<f32>(); // (0, 1] — логарифм определён
    (u.ln() / (1.0 - p).ln()).floor().max(1.0) as u64
}

/// Запускает первичные цепочки процессов по всему миру: вызывается один
/// раз после генерации (или пересоздания) мира. Сами функции планирования
/// проверяют применимость правила, поэтому тайлы без соседей-целей цепочек
/// не получают.
pub fn seed_all(world: &World, sched: &mut Scheduler, tick: u64) {
    sched.clear_world_tasks();
    let seed = world.seed;
    let n = world.size.tiles() as i32;
    for y in 0..n {
        for x in 0..n {
            let pos = TilePos { x, y };
            schedule_grass_spread(world, sched, seed, tick, pos);
            schedule_water_erosion(world, sched, seed, tick, pos);
        }
    }
    // Цепочки роста кустов — по чанкам: и кандидаты, и предел считаются на чанк.
    for cy in 0..world.chunks_per_side {
        for cx in 0..world.chunks_per_side {
            schedule_bush_growth(world, sched, seed, tick, ChunkPos { x: cx, y: cy });
        }
    }
}

/// После изменения тайла (редактор, игрок) перезапускает цепочки, которые
/// могли появиться или возобновиться вокруг `pos`.
///
/// Повторное планирование возможно для цепочки, уже живущей на соседнем
/// тайле (редактор красит несколько тайлов подряд): дубликат лишь временно
/// слегка ускоряет правило и гаснет вместе с основной цепочкой, когда
/// соседи перестают подходить.
pub fn after_tile_changed(world: &World, sched: &mut Scheduler, tick: u64, pos: TilePos) {
    let seed = world.seed;
    let kind = world.terrain_kind(pos);

    // Цепочки самого тайла: трава и вода действуют сами, чанк леса
    // растит кусты.
    schedule_grass_spread(world, sched, seed, tick, pos);
    schedule_water_erosion(world, sched, seed, tick, pos);
    if kind == TileType::ForestGrass {
        let (chunk, _) = split_tile(pos);
        schedule_bush_growth(world, sched, seed, tick, chunk);
    }
    // Новая земля рядом с травой: у соседних тайлов травы появилась цель.
    if kind == TileType::Dirt {
        for neighbor in neighbors(world, pos) {
            if world.terrain_kind(neighbor) == TileType::Grass {
                schedule_grass_spread(world, sched, seed, tick, neighbor);
            }
        }
    }
    // Новый эродируемый тайл рядом с водой: у соседних тайлов воды появилась цель.
    if erodible(kind) {
        for neighbor in neighbors(world, pos) {
            if world.terrain_kind(neighbor).is_water() {
                schedule_water_erosion(world, sched, seed, tick, neighbor);
            }
        }
    }
}
/// Обрабатывает исполненное событие растительности: применяет правило,
/// планирует продолжение цепочки и дописывает изменившиеся тайлы в
/// `dirty` с каналом причины (трава/эрозия меняют рельеф, кусты — декор).
/// Вызывается тиковой системой app-слоя.
pub fn apply_task(
    kind: &TaskKind,
    world: &mut World,
    sched: &mut Scheduler,
    tick: u64,
    dirty: &mut DirtyTiles,
) {
    match *kind {
        TaskKind::GrassSpread { chunk, local_index } => {
            grass_spread(world, sched, tick, chunk, local_index, dirty)
        }
        TaskKind::BushGrowth { chunk } => bush_growth(world, sched, tick, chunk, dirty),
        TaskKind::WaterErosion { chunk, local_index } => {
            water_erosion(world, sched, tick, chunk, local_index, dirty)
        }
        _ => {}
    }
}

/// Событие тайла травы: занимает одну случайную соседнюю землю. Тайл,
/// переставший быть травой, и тайл без соседней земли гасят цепочку.
fn grass_spread(
    world: &mut World,
    sched: &mut Scheduler,
    tick: u64,
    chunk: ChunkPos,
    local: u8,
    dirty: &mut DirtyTiles,
) {
    let seed = world.seed;
    let pos = tile_pos(chunk, local);
    if world.terrain_kind(pos) != TileType::Grass {
        return;
    }
    let dirt: Vec<TilePos> = neighbors(world, pos)
        .filter(|n| world.terrain_kind(*n) == TileType::Dirt)
        .collect();
    if dirt.is_empty() {
        return;
    }

    let mut rng = event_rng(seed, tick, TAG_GRASS, chunk, local);
    let target = dirt[rng.random_range(0..dirt.len())];
    world.set_terrain(target, TileType::Grass);
    dirty.terrain(target);
    // Новый тайл травы начинает собственную цепочку, источник продолжается,
    // пока рядом остаётся земля (проверки внутри планировщиков).
    schedule_grass_spread(world, sched, seed, tick, target);
    schedule_grass_spread(world, sched, seed, tick, pos);
}

/// Событие чанка леса: вырастить один куст, если предел ещё не достигнут.
fn bush_growth(
    world: &mut World,
    sched: &mut Scheduler,
    tick: u64,
    chunk: ChunkPos,
    dirty: &mut DirtyTiles,
) {
    let seed = world.seed;
    // Один обход слоя чанка: считаем кусты и собираем кандидатов — тайлы
    // лесной травы без объекта. 256 тайлов на событие (не на тик).
    let chunk_ref = world.chunk(chunk);
    let mut bushes = 0_u32;
    for obj in chunk_ref.objects.values() {
        if obj.kind == ObjectKind::Bush {
            bushes += 1;
        }
    }
    if bushes >= BUSH_CAP_PER_CHUNK {
        return;
    }
    let mut candidates: Vec<u8> = Vec::new();
    for idx in 0..CHUNK_TILES {
        if chunk_ref.terrain.get(idx) == TileType::ForestGrass
            && !chunk_ref.objects.contains_key(&(idx as u8))
        {
            candidates.push(idx as u8);
        }
    }
    if candidates.is_empty() {
        return;
    }

    let mut rng = event_rng(seed, tick, TAG_BUSH, chunk, 0);
    let idx = candidates[rng.random_range(0..candidates.len())];
    let pos = tile_pos(chunk, idx);
    world.set_object(pos, Some(WorldObject::bush()));
    dirty.decor(pos);
    // Продолжаем, пока есть кандидаты и предел не достигнут (проверка
    // внутри с учётом только что выросшего куста).
    schedule_bush_growth(world, sched, seed, tick, chunk);
}

/// Событие тайла воды: подтачивает одного случайного эродируемого соседа.
fn water_erosion(
    world: &mut World,
    sched: &mut Scheduler,
    tick: u64,
    chunk: ChunkPos,
    local: u8,
    dirty: &mut DirtyTiles,
) {
    let seed = world.seed;
    let pos = tile_pos(chunk, local);
    if !world.terrain_kind(pos).is_water() {
        return;
    }
    let targets: Vec<TilePos> = neighbors(world, pos)
        .filter(|n| erodible(world.terrain_kind(*n)))
        .collect();
    if targets.is_empty() {
        return;
    }

    let mut rng = event_rng(seed, tick, TAG_EROSION, chunk, local);
    let target = targets[rng.random_range(0..targets.len())];
    let kind = world.terrain_kind(target);
    world.set_terrain(target, eroded(kind));
    dirty.terrain(target);
    // Вода продолжает подтачивать, пока есть что эродировать.
    schedule_water_erosion(world, sched, seed, tick, pos);
}

/// Планирует следующее событие цепочки травы, если тайл — трава и рядом
/// есть земля. Иначе цепочка не возобновляется.
fn schedule_grass_spread(world: &World, sched: &mut Scheduler, seed: u64, tick: u64, pos: TilePos) {
    if world.terrain_kind(pos) != TileType::Grass {
        return;
    }
    if !neighbors(world, pos)
        .any(|n| world.terrain_kind(n) == TileType::Dirt)
    {
        return;
    }
    let (chunk, local) = split_tile(pos);
    let mut rng = event_rng(seed, tick, TAG_GRASS, chunk, local);
    let delta = interval_ticks(&mut rng, GRASS_SPREAD_P);
    sched.schedule_at(tick + delta, TaskKind::GrassSpread { chunk, local_index: local });
}

/// Планирует следующее событие цепочки воды, если тайл — вода и рядом
/// есть эродируемый сосед.
fn schedule_water_erosion(world: &World, sched: &mut Scheduler, seed: u64, tick: u64, pos: TilePos) {
    if !world.terrain_kind(pos).is_water() {
        return;
    }
    if !neighbors(world, pos)
        .any(|n| erodible(world.terrain_kind(n)))
    {
        return;
    }
    let (chunk, local) = split_tile(pos);
    let mut rng = event_rng(seed, tick, TAG_EROSION, chunk, local);
    let delta = interval_ticks(&mut rng, EROSION_P);
    sched.schedule_at(tick + delta, TaskKind::WaterErosion { chunk, local_index: local });
}

/// Планирует следующее событие роста кустов в чанке, если предел не
/// достигнут и есть кандидаты. Суммарная вероятность чанка = вероятность
/// одного тайла × число кандидатов (приближение Пуассона для малых p).
fn schedule_bush_growth(world: &World, sched: &mut Scheduler, seed: u64, tick: u64, chunk: ChunkPos) {
    let chunk_ref = world.chunk(chunk);
    let mut bushes = 0_u32;
    let mut candidates = 0_usize;
    for obj in chunk_ref.objects.values() {
        if obj.kind == ObjectKind::Bush {
            bushes += 1;
        }
    }
    if bushes >= BUSH_CAP_PER_CHUNK {
        return;
    }
    for idx in 0..CHUNK_TILES {
        if chunk_ref.terrain.get(idx) == TileType::ForestGrass
            && !chunk_ref.objects.contains_key(&(idx as u8))
        {
            candidates += 1;
        }
    }
    if candidates == 0 {
        return;
    }
    let mut rng = event_rng(seed, tick, TAG_BUSH, chunk, 0);
    let delta = interval_ticks(&mut rng, (candidates as f32 * BUSH_GROW_P).min(1.0));
    sched.schedule_at(tick + delta, TaskKind::BushGrowth { chunk });
}
