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
//!                    state u8, facing u8, hp f32, water_ticks u16,
//!                    state_ticks u64, state_duration u64,
//!                    target_x f32, target_y f32, stuck_ticks u16,
//!                    land_search_failed_at u64]
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
//!   игрок:
//!     present          u8 (0 = игрока нет, 1 = запись ниже)
//!     x f32, y f32     (позиция; prev_pos при загрузке = позиция)
//!     selected_slot    u8 (< MISC_SLOTS)
//!     hp f32, hunger f32 (в допустимых диапазонах)
//!     слоты misc:      MISC_SLOTS слотов
//!     слоты брони:     ARMOR_SLOTS слотов
//!     слоты одежды:    CLOTHES_SLOTS слотов
//!     слоты косметики: BAUBLE_SLOTS слотов
//!     мусорка:         1 слот
//!       слот:          tag u8: 0 = пусто | 1 = kind u8 + count u32
//! ```
//!
//! Обход разреженных слоёв идёт по возрастанию индексов, списки
//! планировщика отсортированы — одинаковые состояния дают одинаковые байты.
//! Файл должен читаться обратно без потерь: счёт скорости, пауза и прочее
//! transient-состояние UI сюда не входят — это выбор игрока, а не мира.
//! Кулдауны игрока (атака, еда) при загрузке сбрасываются: тики мира
//! могли уйти вперёд.

use std::fmt;

use super::chunk::{TerrainTiles, CHUNK_SIZE, CHUNK_TILES};
use super::entities::{EntityKind, EntityState, EntityStore, Facing, SimEntity};
use super::hunger::{Hunger, HUNGER_MAX};
use super::items::{
    Inventory, ItemKind, ItemStack, SlotArea, ARMOR_SLOTS, BAUBLE_SLOTS, CLOTHES_SLOTS, MISC_SLOTS,
};
use super::needs::NeedKind;
use super::objects::{ObjectKind, ObjectStage, WorldObject};
use super::player::{Player, WorldPos, PLAYER_MAX_HP};
use super::scheduler::{Scheduler, SchedulerSnapshot, Task, TaskKind};
use super::tile::{ResourceKind, TileType};
use super::time::GameClock;
use super::world::{ChunkPos, World, WorldSize};

/// Версия формата сохранения. При несовместимых изменениях — увеличить
/// и решить, поддерживать ли чтение старых версий. Версия 4: слой
/// предметов на земле и поле `land_search_failed_at` существ. Версия 5:
/// снаряды в полёте и воткнутые копья (боевая система). Версия 6: игрок
/// (позиция, hp, голод, инвентарь со всеми секциями и мусоркой).
/// Версия 1: основа. Версия 2: слои культур и контейнеров станций,
/// item-сущности дропа, задачи CropGrowth/StationTick.
pub const SAVE_FORMAT_VERSION: u32 = 2;

/// Магия в начале файла.
const MAGIC: [u8; 4] = *b"ADEM";

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
    pub world: World,
    pub scheduler: Scheduler,
    /// Игрок из сохранения; `None` — на момент сохранения существа не было.
    pub player: Option<Player>,
}

/// Записывает состояние симуляции в байты.
///
/// `player` — данные игрока (живут вне мира); `game_version` — версия
/// игры (константа проекта), пишется в заголовок для диагностики; на
/// совместимость не влияет — за это отвечает `SAVE_FORMAT_VERSION`.
pub fn save_game(
    world: &World,
    clock: &GameClock,
    scheduler: &Scheduler,
    player: Option<&Player>,
    game_version: &str,
) -> Vec<u8> {
    let mut w = Vec::with_capacity(1024);
    w.extend_from_slice(&MAGIC);
    write_u32(&mut w, SAVE_FORMAT_VERSION);
    debug_assert!(game_version.len() <= u8::MAX as usize);
    w.push(game_version.len().min(u8::MAX as usize) as u8);
    w.extend_from_slice(game_version.as_bytes());
    write_u64(&mut w, world.seed);
    write_u64(&mut w, clock.tick);
    write_u32(&mut w, world.size.tiles());

    // Мир.
    write_f32(&mut w, world.ambient_temperature_c);
    write_u32(&mut w, world.chunks_per_side);
    for chunk in &world.chunks {
        match &chunk.terrain {
            TerrainTiles::Uniform(kind) => {
                w.push(0);
                w.push(*kind as u8);
            }
            TerrainTiles::Full(arr) => {
                w.push(1);
                for kind in arr.iter() {
                    w.push(*kind as u8);
                }
            }
        }
        write_sparse_u16(&mut w, &chunk.resources, |d| (d.kind as u8, d.amount));
        write_sparse_objects(&mut w, &chunk.objects);
        write_sparse_items(&mut w, &chunk.items);
        write_sparse_crops(&mut w, &chunk.crops);
        write_sparse_containers(&mut w, &chunk.containers);
    }

    // Существа.
    write_entities(&mut w, &world.entities);

    // Снаряды и воткнутые копья (порядок в Vec стабилен — детерминизм).
    write_u64(&mut w, world.next_projectile_id);
    write_u32(&mut w, world.projectiles.len() as u32);
    for p in &world.projectiles {
        write_u64(&mut w, p.id);
        w.push(p.kind as u8);
        write_f32(&mut w, p.origin.x);
        write_f32(&mut w, p.origin.y);
        write_f32(&mut w, p.target.x);
        write_f32(&mut w, p.target.y);
        write_f32(&mut w, p.t);
        write_u64(&mut w, p.duration_ticks);
        write_f32(&mut w, p.arc_height);
        write_f32(&mut w, p.damage);
    }
    write_u32(&mut w, world.stuck_spears.len() as u32);
    for spear in &world.stuck_spears {
        write_f32(&mut w, spear.pos.x);
        write_f32(&mut w, spear.pos.y);
        write_f32(&mut w, spear.heading);
        write_f32(&mut w, spear.pitch);
    }

    // Item-сущности дропа (по возрастанию id — детерминизм).
    write_u32(&mut w, world.item_entities.items.len() as u32);
    for entity in &world.item_entities.items {
        write_u64(&mut w, entity.id);
        write_f32(&mut w, entity.pos.x);
        write_f32(&mut w, entity.pos.y);
        w.push(entity.stack.kind as u8);
        write_u32(&mut w, entity.stack.count);
        write_u64(&mut w, entity.spawn_tick);
        write_u64(&mut w, entity.pickup_after_tick);
    }

    // Планировщик.
    let snap = scheduler.snapshot();
    write_u64(&mut w, snap.current_tick);
    write_u64(&mut w, snap.next_id);
    write_u64(&mut w, snap.cancelled.len() as u64);
    for id in &snap.cancelled {
        write_u64(&mut w, *id);
    }
    // Ближнее окно: число бакетов фиксировано форматом (см. NEAR_WINDOW).
    write_u64(&mut w, snap.near.len() as u64);
    for bucket in &snap.near {
        write_u64(&mut w, bucket.len() as u64);
        for task in bucket {
            write_u64(&mut w, task.id);
            write_task_kind(&mut w, &task.kind);
        }
    }
    write_u64(&mut w, snap.far.len() as u64);
    for (tick, id, task) in &snap.far {
        write_u64(&mut w, *tick);
        write_u64(&mut w, *id);
        write_task_kind(&mut w, &task.kind);
    }

    // Игрок (живёт вне мира; в старых форматах не сохранялся вовсе).
    write_player(&mut w, player);
    w
}

/// Читает только заголовок сохранения (для списка слотов).
///
/// `bytes` может быть префиксом файла: функция читает ровно столько,
/// сколько нужно, и не требует, чтобы данные кончались вместе с заголовком.
pub fn read_header(bytes: &[u8]) -> Result<SaveHeader, SaveError> {
    let mut r = Reader::new(bytes);
    let magic = r.read_bytes(4)?;
    if magic != MAGIC {
        return Err(SaveError::BadMagic);
    }
    let format_version = r.read_u32()?;
    let len = r.read_u8()? as usize;
    let game_version = String::from_utf8_lossy(r.read_bytes(len)?).into_owned();
    let seed = r.read_u64()?;
    let tick = r.read_u64()?;
    let world_side = r.read_u32()?;
    Ok(SaveHeader {
        format_version,
        game_version,
        seed,
        tick,
        world_side,
    })
}

/// Разбирает байты сохранения целиком. Строгая проверка: лишние данные
/// в конце файла считаются повреждением, а не расширением формата.
pub fn load_game(bytes: &[u8]) -> Result<SavedGame, SaveError> {
    let header = read_header(bytes)?;
    if header.format_version != SAVE_FORMAT_VERSION {
        return Err(SaveError::UnsupportedFormat {
            found: header.format_version,
            expected: SAVE_FORMAT_VERSION,
        });
    }

    let mut r = Reader::new(bytes);
    // Пропускаем заголовок: разбор повторяет read_header по позициям.
    r.skip(4 + 4 + 1 + header.game_version.len())?;
    let seed = r.read_u64()?;
    let tick = r.read_u64()?;
    let side = r.read_u32()?;

    // Мир: размер обязан быть корректным — WorldSize::new иначе зажал бы его.
    if !(WorldSize::MIN..=WorldSize::MAX).contains(&side) || side % CHUNK_SIZE as u32 != 0 {
        return Err(SaveError::BadData("недопустимый размер мира"));
    }
    let size = WorldSize::new(side);
    let chunks_per_side = World::chunks_per_side(size);
    let ambient = r.read_f32()?;
    let saved_cps = r.read_u32()?;
    if saved_cps != chunks_per_side {
        return Err(SaveError::BadData("число чанков не совпадает с размером"));
    }

    let mut chunks = Vec::with_capacity((chunks_per_side * chunks_per_side) as usize);
    for _ in 0..chunks_per_side * chunks_per_side {
        let tag = r.read_u8()?;
        let terrain = match tag {
            0 => TerrainTiles::Uniform(
                TileType::from_discriminant(r.read_u8()?).ok_or(SaveError::BadData("вид тайла"))?,
            ),
            1 => {
                let mut arr = Box::new([TileType::Empty; CHUNK_TILES]);
                for slot in arr.iter_mut() {
                    *slot = TileType::from_discriminant(r.read_u8()?)
                        .ok_or(SaveError::BadData("вид тайла"))?;
                }
                TerrainTiles::Full(arr)
            }
            _ => return Err(SaveError::BadData("тип слоя рельефа")),
        };
        let resources = read_sparse_u16(&mut r, ResourceKind::from_discriminant)?;
        let objects = read_sparse_objects(&mut r)?;
        let items = read_sparse_items(&mut r)?;
        let crops = read_sparse_crops(&mut r)?;
        let containers = read_sparse_containers(&mut r)?;
        // Контейнер существует только у тайла станции и по размеру совпадает
        // с ёмкостью её вида; разъехавшиеся слои — повреждение.
        for (idx, container) in &containers {
            let Some(obj) = objects.get(idx) else {
                return Err(SaveError::BadData("контейнер без станции"));
            };
            if super::containers::capacity(obj.kind) != Some(container.slots.len()) {
                return Err(SaveError::BadData("ёмкость контейнера"));
            }
        }
        chunks.push(super::chunk::Chunk { terrain, resources, objects, items, crops, containers });
    }

    // Существа.
    let entities = read_entities(&mut r)?;

    // Снаряды и воткнутые копья.
    let next_projectile_id = r.read_u64()?;
    let projectiles_len = r.read_u32()?;
    if projectiles_len > 1_000_000 {
        return Err(SaveError::BadData("слишком много снарядов"));
    }
    let mut projectiles = Vec::with_capacity(projectiles_len as usize);
    let mut previous_projectile = 0_u64;
    for _ in 0..projectiles_len {
        let projectile = read_projectile(&mut r)?;
        if projectile.id == 0 || projectile.id >= next_projectile_id || projectile.id <= previous_projectile {
            return Err(SaveError::BadData("id снаряда"));
        }
        previous_projectile = projectile.id;
        projectiles.push(projectile);
    }
    let spears_len = r.read_u32()?;
    if spears_len > 1_000_000 {
        return Err(SaveError::BadData("слишко много воткнутых копьёв"));
    }
    let mut stuck_spears = Vec::with_capacity(spears_len as usize);
    for _ in 0..spears_len {
        stuck_spears.push(read_stuck_spear(&mut r)?);
    }

    // Item-сущности дропа.
    let item_entities = read_item_entities(&mut r, side)?;

    // Планировщик.
    let sched_tick = r.read_u64()?;
    if sched_tick != tick {
        return Err(SaveError::BadData("тики часов и планировщика разошлись"));
    }
    let next_id = r.read_u64()?;
    let cancelled_len = r.read_u64()?;
    let mut cancelled = Vec::with_capacity(cancelled_len.min(1 << 20) as usize);
    for _ in 0..cancelled_len {
        cancelled.push(r.read_u64()?);
    }
    let buckets = r.read_u64()?;
    let mut near = Vec::with_capacity(buckets.min(4096) as usize);
    for _ in 0..buckets {
        let count = r.read_u64()?;
        let mut bucket = Vec::with_capacity(count.min(1 << 20) as usize);
        for _ in 0..count {
            let id = r.read_u64()?;
            let kind = read_task_kind(&mut r, chunks_per_side)?;
            bucket.push(Task { id, kind });
        }
        near.push(bucket);
    }
    let far_len = r.read_u64()?;
    let mut far = Vec::with_capacity(far_len.min(1 << 20) as usize);
    for _ in 0..far_len {
        let far_tick = r.read_u64()?;
        let id = r.read_u64()?;
        let kind = read_task_kind(&mut r, chunks_per_side)?;
        far.push((far_tick, id, Task { id, kind }));
    }

    // Игрок.
    let player = read_player(&mut r)?;
    if r.remaining() != 0 {
        return Err(SaveError::BadData("лишние данные в конце файла"));
    }

    let scheduler = Scheduler::from_snapshot(SchedulerSnapshot {
        current_tick: sched_tick,
        near,
        far,
        next_id,
        cancelled,
    });

    Ok(SavedGame {
        seed,
        tick,
        world: World {
            size,
            seed,
            chunks_per_side,
            chunks,
            entities,
            ambient_temperature_c: ambient,
            projectiles,
            stuck_spears,
            next_projectile_id,
            item_entities,
        },
        scheduler,
        player,
    })
}

// ---------- Разреженные слои ----------

/// Пишет разреженный слой `HashMap<u8, T>` по возрастанию индексов.
fn write_sparse_u16<T, F: Fn(&T) -> (u8, u16)>(
    w: &mut Vec<u8>,
    map: &std::collections::HashMap<u8, T>,
    fields: F,
) {
    let count = map.len();
    debug_assert!(count <= u16::MAX as usize);
    write_u16(w, count as u16);
    // Ключей ≤ 256: сортировка списка дешевле обхода всех 256 индексов.
    let mut indices: Vec<u8> = map.keys().copied().collect();
    indices.sort_unstable();
    for idx in indices {
        let value = &map[&idx];
        w.push(idx);
        let (kind, amount) = fields(value);
        w.push(kind);
        write_u16(w, amount);
    }
}

/// Читает разреженный слой вида (index u8, kind u8, amount u16).
fn read_sparse_u16(
    r: &mut Reader<'_>,
    kind: fn(u8) -> Option<ResourceKind>,
) -> Result<std::collections::HashMap<u8, super::chunk::ResourceDeposit>, SaveError> {
    let count = r.read_u16()? as usize;
    let mut map = std::collections::HashMap::with_capacity(count);
    for _ in 0..count {
        let idx = r.read_u8()?;
        let kind = kind(r.read_u8()?).ok_or(SaveError::BadData("вид ресурса"))?;
        let amount = r.read_u16()?;
        if map.insert(idx, super::chunk::ResourceDeposit { kind, amount }).is_some() {
            return Err(SaveError::BadData("дубликат индекса ресурса"));
        }
    }
    Ok(map)
}

/// Пишет разреженный слой объектов.
fn write_sparse_objects(
    w: &mut Vec<u8>,
    map: &std::collections::HashMap<u8, WorldObject>,
) {
    let count = map.len();
    debug_assert!(count <= u16::MAX as usize);
    write_u16(w, count as u16);
    let mut indices: Vec<u8> = map.keys().copied().collect();
    indices.sort_unstable();
    for idx in indices {
        let obj = &map[&idx];
        w.push(idx);
        w.push(obj.kind.discriminant());
        w.push(obj.stage as u8);
        write_u16(w, obj.charge);
    }
}

/// Читает разреженный слой объектов.
fn read_sparse_objects(
    r: &mut Reader<'_>,
) -> Result<std::collections::HashMap<u8, WorldObject>, SaveError> {
    let count = r.read_u16()? as usize;
    let mut map = std::collections::HashMap::with_capacity(count);
    for _ in 0..count {
        let idx = r.read_u8()?;
        let kind = ObjectKind::from_discriminant(r.read_u8()?)
            .ok_or(SaveError::BadData("вид объекта"))?;
        let stage = ObjectStage::from_discriminant(r.read_u8()?)
            .ok_or(SaveError::BadData("стадия объекта"))?;
        let charge = r.read_u16()?;
        if map.insert(idx, WorldObject { kind, stage, charge }).is_some() {
            return Err(SaveError::BadData("дубликат индекса объекта"));
        }
    }
    Ok(map)
}

/// Пишет разреженный слой предметов на земле.
fn write_sparse_items(w: &mut Vec<u8>, map: &std::collections::HashMap<u8, ItemStack>) {
    let count = map.len();
    debug_assert!(count <= u16::MAX as usize);
    write_u16(w, count as u16);
    let mut indices: Vec<u8> = map.keys().copied().collect();
    indices.sort_unstable();
    for idx in indices {
        let stack = &map[&idx];
        w.push(idx);
        w.push(stack.kind as u8);
        write_u32(w, stack.count);
    }
}

/// Читает разреженный слой предметов на земле.
fn read_sparse_items(
    r: &mut Reader<'_>,
) -> Result<std::collections::HashMap<u8, ItemStack>, SaveError> {
    let count = r.read_u16()? as usize;
    let mut map = std::collections::HashMap::with_capacity(count);
    for _ in 0..count {
        let idx = r.read_u8()?;
        let kind = ItemKind::from_discriminant(r.read_u8()?)
            .ok_or(SaveError::BadData("вид предмета"))?;
        let stack_count = r.read_u32()?;
        if stack_count == 0 {
            return Err(SaveError::BadData("пустой стек предметов"));
        }
        if map.insert(idx, ItemStack { kind, count: stack_count }).is_some() {
            return Err(SaveError::BadData("дубликат индекса предмета"));
        }
    }
    Ok(map)
}

/// Пишет существ мира: следующий id, количество и записи в порядке создания.
fn write_entities(w: &mut Vec<u8>, store: &EntityStore) {
    write_u64(w, store.next_id);
    write_u32(w, store.entities.len() as u32);
    for entity in &store.entities {
        write_u64(w, entity.id);
        w.push(entity.kind as u8);
        write_f32(w, entity.pos.x);
        write_f32(w, entity.pos.y);
        write_f32(w, entity.prev_pos.x);
        write_f32(w, entity.prev_pos.y);
        w.push(entity.state as u8);
        w.push(entity.facing as u8);
        write_f32(w, entity.hp);
        write_u16(w, entity.water_ticks);
        write_u64(w, entity.state_ticks);
        write_u64(w, entity.state_duration);
        write_f32(w, entity.target.x);
        write_f32(w, entity.target.y);
        write_u16(w, entity.stuck_ticks);
        write_u64(w, entity.land_search_failed_at);
    }
}

/// Читает существ мира. Проверяет стабильные виды, порядок id и HP:
/// повреждённый файл не должен создать бессмертного или отрицательного волка.
fn read_entities(r: &mut Reader<'_>) -> Result<EntityStore, SaveError> {
    let next_id = r.read_u64()?;
    let count = r.read_u32()?;
    if count > 1_000_000 {
        return Err(SaveError::BadData("слишком много существ"));
    }
    if count > 0 && next_id == 0 {
        return Err(SaveError::BadData("нет следующего id существ"));
    }

    let mut entities = Vec::with_capacity(count as usize);
    let mut previous_id = 0_u64;
    for _ in 0..count {
        let entity = read_entity(r)?;
        if entity.id == 0 || entity.id >= next_id || entity.id <= previous_id {
            return Err(SaveError::BadData("id существа"));
        }
        previous_id = entity.id;
        entities.push(entity);
    }
    Ok(EntityStore { entities, next_id })
}

/// Читает одно существо; порядок полей совпадает с `write_entities`.
fn read_entity(r: &mut Reader<'_>) -> Result<SimEntity, SaveError> {
    let id = r.read_u64()?;
    let kind = EntityKind::from_discriminant(r.read_u8()?)
        .ok_or(SaveError::BadData("вид существа"))?;
    let x = r.read_f32()?;
    let y = r.read_f32()?;
    let prev_x = r.read_f32()?;
    let prev_y = r.read_f32()?;
    let state = EntityState::from_discriminant(r.read_u8()?)
        .ok_or(SaveError::BadData("состояние существа"))?;
    let facing = Facing::from_discriminant(r.read_u8()?)
        .ok_or(SaveError::BadData("направление существа"))?;
    let hp = r.read_f32()?;
    if !hp.is_finite() || hp < 0.0 || hp > kind.max_hp() {
        return Err(SaveError::BadData("HP существа"));
    }
    let water_ticks = r.read_u16()?;
    let state_ticks = r.read_u64()?;
    let state_duration = r.read_u64()?;
    let target_x = r.read_f32()?;
    let target_y = r.read_f32()?;
    let stuck_ticks = r.read_u16()?;
    let land_search_failed_at = r.read_u64()?;

    let coords = [x, y, prev_x, prev_y, target_x, target_y];
    if !coords.iter().all(|value| value.is_finite()) {
        return Err(SaveError::BadData("координаты существа"));
    }

    Ok(SimEntity {
        id,
        kind,
        pos: WorldPos { x, y },
        prev_pos: WorldPos { x: prev_x, y: prev_y },
        facing,
        state,
        hp,
        water_ticks,
        state_ticks,
        state_duration,
        target: WorldPos { x: target_x, y: target_y },
        stuck_ticks,
        land_search_failed_at,
    })
}

// ---------- Снаряды и воткнутые копья ----------

/// Читает один снаряд; проверяет конечность координат и разумные диапазоны.
fn read_projectile(r: &mut Reader<'_>) -> Result<super::combat::Projectile, SaveError> {
    let id = r.read_u64()?;
    let kind = super::combat::ProjectileKind::from_discriminant(r.read_u8()?)
        .ok_or(SaveError::BadData("вид снаряда"))?;
    let ox = r.read_f32()?;
    let oy = r.read_f32()?;
    let tx = r.read_f32()?;
    let ty = r.read_f32()?;
    let t = r.read_f32()?;
    let duration_ticks = r.read_u64()?;
    let arc_height = r.read_f32()?;
    let damage = r.read_f32()?;

    let coords = [ox, oy, tx, ty, t, arc_height, damage];
    if !coords.iter().all(|value| value.is_finite()) {
        return Err(SaveError::BadData("координаты снаряда"));
    }
    if !(0.0..=1.0).contains(&t) || duration_ticks == 0 || damage < 0.0 || arc_height < 0.0 {
        return Err(SaveError::BadData("параметры снаряда"));
    }

    Ok(super::combat::Projectile {
        id,
        kind,
        origin: WorldPos { x: ox, y: oy },
        target: WorldPos { x: tx, y: ty },
        t,
        duration_ticks,
        arc_height,
        damage,
    })
}

/// Читает одно воткнутое копьё.
fn read_stuck_spear(r: &mut Reader<'_>) -> Result<super::combat::StuckSpear, SaveError> {
    let x = r.read_f32()?;
    let y = r.read_f32()?;
    let heading = r.read_f32()?;
    let pitch = r.read_f32()?;
    if ![x, y, heading, pitch].iter().all(|value| value.is_finite()) {
        return Err(SaveError::BadData("координаты воткнутого копья"));
    }
    Ok(super::combat::StuckSpear {
        pos: WorldPos { x, y },
        heading,
        pitch,
    })
}

// ---------- Игрок ----------

/// Пишет данные игрока: наличие, позиция, выбранный слот, hp, голод и все
/// секции инвентаря (misc, броня, одежда, косметика, мусорка). Кулдауны
/// (атака, еда) не пишутся — transient: тики мира при загрузке могли
/// уйти вперёд.
fn write_player(w: &mut Vec<u8>, player: Option<&Player>) {
    let Some(p) = player else {
        w.push(0);
        return;
    };
    w.push(1);
    write_f32(w, p.pos.x);
    write_f32(w, p.pos.y);
    debug_assert!(p.selected_slot < MISC_SLOTS);
    w.push(p.selected_slot as u8);
    write_f32(w, p.hp);
    write_f32(w, p.hunger.value());
    write_slots(w, &p.inventory.items);
    write_slots(w, &p.inventory.armor);
    write_slots(w, &p.inventory.clothes);
    write_slots(w, &p.inventory.baubles);
    write_slots(w, std::slice::from_ref(&p.inventory.trash));
}

/// Пишет массив слотов: на каждый — тег пустоты или стек (вид + количество).
fn write_slots(w: &mut Vec<u8>, slots: &[Option<ItemStack>]) {
    for slot in slots {
        match slot {
            None => w.push(0),
            Some(stack) => {
                w.push(1);
                w.push(stack.kind as u8);
                write_u32(w, stack.count);
            }
        }
    }
}

/// Читает данные игрока. Проверяет диапазоны hp/голода, адрес слота хотбара,
/// виды предметов и пределы стаков; слоты брони и одежды дополнительно —
/// соответствие части тела. Битый файл не должен воскресить игрока с
/// отрицательным здоровьем или надетым шлемом на ногах.
fn read_player(r: &mut Reader<'_>) -> Result<Option<Player>, SaveError> {
    if r.read_u8()? == 0 {
        return Ok(None);
    }
    let x = r.read_f32()?;
    let y = r.read_f32()?;
    if ![x, y].iter().all(|value| value.is_finite()) {
        return Err(SaveError::BadData("координаты игрока"));
    }
    let selected_slot = r.read_u8()? as usize;
    if selected_slot >= MISC_SLOTS {
        return Err(SaveError::BadData("выбранный слот хотбара"));
    }
    let hp = r.read_f32()?;
    if !hp.is_finite() || !(0.0..=PLAYER_MAX_HP).contains(&hp) {
        return Err(SaveError::BadData("HP игрока"));
    }
    let hunger = r.read_f32()?;
    if !hunger.is_finite() || !(0.0..=HUNGER_MAX).contains(&hunger) {
        return Err(SaveError::BadData("голод игрока"));
    }

    let mut player = Player::new(WorldPos { x, y });
    player.selected_slot = selected_slot;
    player.hp = hp;
    player.hunger = Hunger::with_value(hunger);
    read_slots(r, SlotArea::Misc, MISC_SLOTS, &mut player.inventory.items)?;
    read_slots(r, SlotArea::Armor, ARMOR_SLOTS, &mut player.inventory.armor)?;
    read_slots(r, SlotArea::Clothes, CLOTHES_SLOTS, &mut player.inventory.clothes)?;
    read_slots(r, SlotArea::Bauble, BAUBLE_SLOTS, &mut player.inventory.baubles)?;
    // Мусорка — один слот отдельной секции: читаем во временный массив.
    let mut trash = [None; 1];
    read_slots(r, SlotArea::Trash, 1, &mut trash)?;
    player.inventory.trash = trash[0];
    Ok(Some(player))
}

/// Читает `count` слотов секции `area` в приёмник `slots`. Стеки проверяются
/// на допустимый вид и предел стака; секция брони — ещё и на соответствие
/// специализации слота (`Inventory::accepts`).
fn read_slots<const N: usize>(
    r: &mut Reader<'_>,
    area: SlotArea,
    count: usize,
    slots: &mut [Option<ItemStack>; N],
) -> Result<(), SaveError> {
    if slots.len() != count {
        return Err(SaveError::BadData("размер секции инвентаря"));
    }
    for (index, slot) in slots.iter_mut().enumerate() {
        match r.read_u8()? {
            0 => *slot = None,
            1 => {
                let kind = ItemKind::from_discriminant(r.read_u8()?)
                    .ok_or(SaveError::BadData("вид предмета игрока"))?;
                let stack_count = r.read_u32()?;
                if stack_count == 0 || stack_count > kind.max_stack() {
                    return Err(SaveError::BadData("размер стака игрока"));
                }
                if !Inventory::accepts(kind, area, index) {
                    return Err(SaveError::BadData("предмет в чужом слоте"));
                }
                *slot = Some(ItemStack { kind, count: stack_count });
            }
            _ => return Err(SaveError::BadData("тег слота игрока")),
        }
    }
    Ok(())
}

// ---------- Культуры и контейнеры ----------

/// Пишет разреженный слой культур (по возрастанию индексов).
fn write_sparse_crops(w: &mut Vec<u8>, map: &std::collections::HashMap<u8, super::crops::Crop>) {
    let count = map.len();
    debug_assert!(count <= u16::MAX as usize);
    write_u16(w, count as u16);
    let mut indices: Vec<u8> = map.keys().copied().collect();
    indices.sort_unstable();
    for idx in indices {
        let crop = &map[&idx];
        w.push(idx);
        w.push(crop.kind as u8);
        w.push(crop.stage);
    }
}

/// Читает разреженный слой культур; стадия не может превышать последнюю.
fn read_sparse_crops(
    r: &mut Reader<'_>,
) -> Result<std::collections::HashMap<u8, super::crops::Crop>, SaveError> {
    let count = r.read_u16()? as usize;
    let mut map = std::collections::HashMap::with_capacity(count);
    for _ in 0..count {
        let idx = r.read_u8()?;
        let kind = super::crops::CropKind::from_discriminant(r.read_u8()?)
            .ok_or(SaveError::BadData("вид культуры"))?;
        let stage = r.read_u8()?;
        if stage as usize >= kind.stage_count() {
            return Err(SaveError::BadData("стадия культуры"));
        }
        if map.insert(idx, super::crops::Crop { kind, stage }).is_some() {
            return Err(SaveError::BadData("дубликат индекса культуры"));
        }
    }
    Ok(map)
}

/// Пишет разреженный слой контейнеров станций (по возрастанию индексов).
fn write_sparse_containers(
    w: &mut Vec<u8>,
    map: &std::collections::HashMap<u8, super::containers::Container>,
) {
    let count = map.len();
    debug_assert!(count <= u16::MAX as usize);
    write_u16(w, count as u16);
    let mut indices: Vec<u8> = map.keys().copied().collect();
    indices.sort_unstable();
    for idx in indices {
        let container = &map[&idx];
        w.push(idx);
        // Вид станции не пишем: он восстановим из слоя объектов, но для
        // независимой проверки валидности слотов пишем явно.
        w.push(0xFF); // резервный тег формата
        match container.cook_finish_tick {
            None => w.push(0),
            Some(tick) => {
                w.push(1);
                write_u64(w, tick);
            }
        }
        match container.burn_out_tick {
            None => w.push(0),
            Some(tick) => {
                w.push(1);
                write_u64(w, tick);
            }
        }
        match container.burning_fuel {
            None => w.push(0xFF),
            Some(kind) => w.push(kind as u8),
        }
        debug_assert!(container.slots.len() <= u16::MAX as usize);
        write_u16(w, container.slots.len() as u16);
        for slot in &container.slots {
            match slot {
                None => w.push(0),
                Some(stack) => {
                    w.push(1);
                    w.push(stack.kind as u8);
                    write_u32(w, stack.count);
                }
            }
        }
    }
}

/// Читает разреженный слой контейнеров станций: ёмкость — по виду станции,
/// стеки — в пределах стаков, сроки — конечные неотрицательные.
fn read_sparse_containers(
    r: &mut Reader<'_>,
) -> Result<std::collections::HashMap<u8, super::containers::Container>, SaveError> {
    let count = r.read_u16()? as usize;
    let mut map = std::collections::HashMap::with_capacity(count);
    for _ in 0..count {
        let idx = r.read_u8()?;
        let _reserved = r.read_u8()?;
        let cook_finish_tick = match r.read_u8()? {
            0 => None,
            1 => Some(r.read_u64()?),
            _ => return Err(SaveError::BadData("тег срока готовки")),
        };
        let burn_out_tick = match r.read_u8()? {
            0 => None,
            1 => Some(r.read_u64()?),
            _ => return Err(SaveError::BadData("тег срока топлива")),
        };
        let burning_fuel = match r.read_u8()? {
            0xFF => None,
            raw => Some(
                ItemKind::from_discriminant(raw)
                    .ok_or(SaveError::BadData("вид горящего топлива"))?,
            ),
        };
        let slots_len = r.read_u16()? as usize;
        if slots_len > 256 {
            return Err(SaveError::BadData("слишком много слотов контейнера"));
        }
        let mut slots: Vec<Option<ItemStack>> = Vec::with_capacity(slots_len);
        for _ in 0..slots_len {
            match r.read_u8()? {
                0 => slots.push(None),
                1 => {
                    let kind = ItemKind::from_discriminant(r.read_u8()?)
                        .ok_or(SaveError::BadData("вид предмета в контейнере"))?;
                    let stack_count = r.read_u32()?;
                    if stack_count == 0 || stack_count > kind.max_stack() {
                        return Err(SaveError::BadData("размер стака контейнера"));
                    }
                    slots.push(Some(ItemStack { kind, count: stack_count }));
                }
                _ => return Err(SaveError::BadData("тег слота контейнера")),
            }
        }
        if map
            .insert(
                idx,
                super::containers::Container {
                    slots,
                    cook_finish_tick,
                    burn_out_tick,
                    burning_fuel,
                },
            )
            .is_some()
        {
            return Err(SaveError::BadData("дубликат индекса контейнера"));
        }
    }
    Ok(map)
}

// ---------- Item-сущности дропа ----------

/// Читает item-сущности: id по возрастанию, координаты в мире, стеки
/// в пределах стаков, тики согласованы (подбор не раньше дропа).
fn read_item_entities(r: &mut Reader<'_>, side: u32) -> Result<super::item_entity::ItemEntities, SaveError> {
    let count = r.read_u32()?;
    if count > 1_000_000 {
        return Err(SaveError::BadData("слишком много item-сущностей"));
    }
    let mut store = super::item_entity::ItemEntities::default();
    let mut previous_id = 0_u64;
    for _ in 0..count {
        let id = r.read_u64()?;
        let x = r.read_f32()?;
        let y = r.read_f32()?;
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 || x > side as f32 || y > side as f32
        {
            return Err(SaveError::BadData("координаты item-сущности"));
        }
        let kind = ItemKind::from_discriminant(r.read_u8()?)
            .ok_or(SaveError::BadData("вид предмета item-сущности"))?;
        let stack_count = r.read_u32()?;
        if stack_count == 0 || stack_count > kind.max_stack() {
            return Err(SaveError::BadData("стак item-сущности"));
        }
        let spawn_tick = r.read_u64()?;
        let pickup_after_tick = r.read_u64()?;
        if pickup_after_tick < spawn_tick {
            return Err(SaveError::BadData("сроки подбора item-сущности"));
        }
        if id == 0 || id <= previous_id {
            return Err(SaveError::BadData("id item-сущности"));
        }
        previous_id = id;
        store.items.push(super::item_entity::ItemEntity {
            id,
            pos: WorldPos { x, y },
            stack: ItemStack { kind, count: stack_count },
            spawn_tick,
            pickup_after_tick,
            // Полёт — транзиентное состояние: предмет сохраняется лежащим.
            fly: None,
        });
    }
    store.restore_next_id(previous_id + 1);
    Ok(store)
}

// ---------- Виды задач ----------

/// Кодирует вид задачи. Дискриминанты формата — часть соглашения
/// сохранения (не совпадают с порядком enum, поэтому явный match).
fn write_task_kind(w: &mut Vec<u8>, kind: &TaskKind) {
    match kind {
        TaskKind::TimeOfDayChanged => w.push(0),
        TaskKind::SeasonChanged => w.push(1),
        TaskKind::ObjectGrowth { chunk, local_index } => {
            w.push(2);
            write_u32(w, chunk.x);
            write_u32(w, chunk.y);
            w.push(*local_index);
        }
        TaskKind::NeedThreshold { owner, need } => {
            w.push(3);
            write_u64(w, *owner);
            w.push(*need as u8);
        }
        TaskKind::Marker(value) => {
            w.push(4);
            write_u32(w, *value);
        }
        TaskKind::GrassSpread { chunk, local_index } => {
            w.push(5);
            write_u32(w, chunk.x);
            write_u32(w, chunk.y);
            w.push(*local_index);
        }
        TaskKind::BushGrowth { chunk } => {
            w.push(6);
            write_u32(w, chunk.x);
            write_u32(w, chunk.y);
        }
        TaskKind::WaterErosion { chunk, local_index } => {
            w.push(7);
            write_u32(w, chunk.x);
            write_u32(w, chunk.y);
            w.push(*local_index);
        }
        TaskKind::CropGrowth { chunk, local_index } => {
            w.push(8);
            write_u32(w, chunk.x);
            write_u32(w, chunk.y);
            w.push(*local_index);
        }
        TaskKind::StationTick { chunk, local_index } => {
            w.push(9);
            write_u32(w, chunk.x);
            write_u32(w, chunk.y);
            w.push(*local_index);
        }
        TaskKind::FarmlandRevert { chunk, local_index } => {
            w.push(10);
            write_u32(w, chunk.x);
            write_u32(w, chunk.y);
            w.push(*local_index);
        }
    }
}

/// Декодирует вид задачи. Координаты чанков проверяются против размера
/// мира: битый файл не должен довести симуляцию до выхода за границы
/// `chunks` при первом исполнении задачи.
fn read_task_kind(r: &mut Reader<'_>, chunks_per_side: u32) -> Result<TaskKind, SaveError> {
    let tag = r.read_u8()?;
    Ok(match tag {
        0 => TaskKind::TimeOfDayChanged,
        1 => TaskKind::SeasonChanged,
        2 => {
            let (chunk, local_index) = read_chunk_task(r, chunks_per_side)?;
            TaskKind::ObjectGrowth { chunk, local_index }
        }
        3 => {
            let owner = r.read_u64()?;
            let need = NeedKind::from_discriminant(r.read_u8()?)
                .ok_or(SaveError::BadData("вид потребности"))?;
            TaskKind::NeedThreshold { owner, need }
        }
        4 => TaskKind::Marker(r.read_u32()?),
        5 => {
            let (chunk, local_index) = read_chunk_task(r, chunks_per_side)?;
            TaskKind::GrassSpread { chunk, local_index }
        }
        6 => {
            let chunk = read_chunk_pos(r, chunks_per_side)?;
            TaskKind::BushGrowth { chunk }
        }
        7 => {
            let (chunk, local_index) = read_chunk_task(r, chunks_per_side)?;
            TaskKind::WaterErosion { chunk, local_index }
        }
        8 => {
            let (chunk, local_index) = read_chunk_task(r, chunks_per_side)?;
            TaskKind::CropGrowth { chunk, local_index }
        }
        9 => {
            let (chunk, local_index) = read_chunk_task(r, chunks_per_side)?;
            TaskKind::StationTick { chunk, local_index }
        }
        10 => {
            let (chunk, local_index) = read_chunk_task(r, chunks_per_side)?;
            TaskKind::FarmlandRevert { chunk, local_index }
        }
        _ => return Err(SaveError::BadData("вид задачи")),
    })
}

/// Читает пару (чанк, локальный индекс) для задач на тайл.
fn read_chunk_task(r: &mut Reader<'_>, chunks_per_side: u32) -> Result<(ChunkPos, u8), SaveError> {
    let chunk = read_chunk_pos(r, chunks_per_side)?;
    let local_index = r.read_u8()?;
    Ok((chunk, local_index))
}

/// Читает позицию чанка; вне мира — повреждённые данные.
fn read_chunk_pos(r: &mut Reader<'_>, chunks_per_side: u32) -> Result<ChunkPos, SaveError> {
    let x = r.read_u32()?;
    let y = r.read_u32()?;
    if x >= chunks_per_side || y >= chunks_per_side {
        return Err(SaveError::BadData("чанк задачи вне мира"));
    }
    Ok(ChunkPos { x, y })
}

// ---------- Примитивы ----------

fn write_u32(w: &mut Vec<u8>, value: u32) {
    w.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(w: &mut Vec<u8>, value: u64) {
    w.extend_from_slice(&value.to_le_bytes());
}

fn write_u16(w: &mut Vec<u8>, value: u16) {
    w.extend_from_slice(&value.to_le_bytes());
}

fn write_f32(w: &mut Vec<u8>, value: f32) {
    w.extend_from_slice(&value.to_bits().to_le_bytes());
}

/// Побайтовый читатель с курсором: отдаёт ошибку Truncated при нехватке.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn read_bytes(&mut self, len: usize) -> Result<&'a [u8], SaveError> {
        if self.remaining() < len {
            return Err(SaveError::Truncated);
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    fn skip(&mut self, len: usize) -> Result<(), SaveError> {
        self.read_bytes(len).map(|_| ())
    }

    fn read_u8(&mut self) -> Result<u8, SaveError> {
        Ok(self.read_bytes(1)?[0])
    }

    fn read_u16(&mut self) -> Result<u16, SaveError> {
        let b = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn read_u32(&mut self) -> Result<u32, SaveError> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn read_u64(&mut self) -> Result<u64, SaveError> {
        let b = self.read_bytes(8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }

    fn read_f32(&mut self) -> Result<f32, SaveError> {
        Ok(f32::from_bits(self.read_u32()?))
    }
}
