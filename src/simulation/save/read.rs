//! Чтение сохранения: разбор байтов в состояние симуляции со строгой
//! валидацией (битый или подменённый файл даёт `SaveError`, а не панику).
//! Структурные проверки уровня формата — в `validate.rs`, поля уровня
//! записи проверяются здесь. Разделы формата — в doc-комментарии
//! `save/mod.rs`.

use crate::simulation::chunk::{TerrainTiles, CHUNK_TILES};
use crate::simulation::entities::{EntityKind, EntityState, EntityStore, Facing, HumanMind, SimEntity};
use crate::simulation::items::{
    Inventory, ItemKind, ItemStack, SlotArea, ARMOR_SLOTS, BAUBLE_SLOTS, CLOTHES_SLOTS, MISC_SLOTS,
};
use crate::simulation::needs::NeedKind;
use crate::simulation::objects::{ObjectKind, ObjectStage, WorldObject};
use crate::simulation::scheduler::{Scheduler, SchedulerSnapshot, Task, TaskKind};
use crate::simulation::tile::{ResourceKind, TileType};
use crate::simulation::world::{ChunkPos, World, WorldPos};
use super::validate::{validate_container_layer, validate_scheduler_tick, validate_world_size};
use super::{SaveError, SaveHeader, SavedGame, MAGIC, SAVE_FORMAT_VERSION};

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
    let size = validate_world_size(side)?;
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
        validate_container_layer(&objects, &containers)?;
        chunks.push(crate::simulation::chunk::Chunk { terrain, resources, objects, items, crops, containers });
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
    validate_scheduler_tick(sched_tick, tick)?;
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

    // Игрок больше не секция формата: он — существо мира (ум в записях
    // существ). Лишние данные в конце файла считаются повреждением.
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
            // Прогресс ломания — транзиент: после загрузки зажатие начинать заново.
            building_breaks: std::collections::HashMap::new(),
        },
        scheduler,
    })
}
/// Читает разреженный слой вида (index u8, kind u8, amount u16).
fn read_sparse_u16(
    r: &mut Reader<'_>,
    kind: fn(u8) -> Option<ResourceKind>,
) -> Result<std::collections::HashMap<u8, crate::simulation::chunk::ResourceDeposit>, SaveError> {
    let count = r.read_u16()? as usize;
    let mut map = std::collections::HashMap::with_capacity(count);
    for _ in 0..count {
        let idx = r.read_u8()?;
        let kind = kind(r.read_u8()?).ok_or(SaveError::BadData("вид ресурса"))?;
        let amount = r.read_u16()?;
        if map.insert(idx, crate::simulation::chunk::ResourceDeposit { kind, amount }).is_some() {
            return Err(SaveError::BadData("дубликат индекса ресурса"));
        }
    }
    Ok(map)
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
    // Голод — свойство существа (v4): пишется сразу после HP.
    let hunger = r.read_f32()?;
    if !hunger.is_finite() || !(0.0..=crate::simulation::hunger::HUNGER_MAX).contains(&hunger) {
        return Err(SaveError::BadData("голод существа"));
    }
    let water_ticks = r.read_u16()?;
    let state_ticks = r.read_u64()?;
    let state_duration = r.read_u64()?;
    let target_x = r.read_f32()?;
    let target_y = r.read_f32()?;
    let stuck_ticks = r.read_u16()?;
    let land_search_failed_at = r.read_u64()?;
    let mind = read_mind(r)?;

    let coords = [x, y, prev_x, prev_y, target_x, target_y];
    if !coords.iter().all(|value| value.is_finite()) {
        return Err(SaveError::BadData("координаты существа"));
    }
    // Неразумное существо с «умом» — рассинхрон вида и данных.
    if mind.is_some() != kind.is_sentient() {
        return Err(SaveError::BadData("ум не соответствует виду существа"));
    }

    Ok(SimEntity {
        id,
        kind,
        pos: WorldPos { x, y },
        prev_pos: WorldPos { x: prev_x, y: prev_y },
        facing,
        state,
        hp,
        hunger: crate::simulation::hunger::Hunger::with_value(hunger),
        water_ticks,
        state_ticks,
        state_duration,
        target: WorldPos { x: target_x, y: target_y },
        stuck_ticks,
        land_search_failed_at,
        mind,
        // Кулдаун укуса и обида — transient: мир после загрузки жил дальше.
        next_bite_tick: 0,
        provoked_by: None,
        provoked_until_tick: 0,
        // Кулдаун выпаса — transient: после загрузки кролик может есть.
        next_graze_tick: 0,
    })
}

/// Читает «ум» существа. Проверяет адрес слота хотбара, виды предметов
/// и пределы стаков; секции брони/одежды — ещё и соответствие части тела
/// (`Inventory::accepts`).
fn read_mind(r: &mut Reader<'_>) -> Result<Option<HumanMind>, SaveError> {
    if r.read_u8()? == 0 {
        return Ok(None);
    }
    let selected_slot = r.read_u8()? as usize;
    if selected_slot >= MISC_SLOTS {
        return Err(SaveError::BadData("выбранный слот хотбара"));
    }

    let mut mind = HumanMind::new();
    mind.selected_slot = selected_slot;
    read_slots(r, SlotArea::Misc, MISC_SLOTS, &mut mind.inventory.items)?;
    read_slots(r, SlotArea::Armor, ARMOR_SLOTS, &mut mind.inventory.armor)?;
    read_slots(r, SlotArea::Clothes, CLOTHES_SLOTS, &mut mind.inventory.clothes)?;
    read_slots(r, SlotArea::Bauble, BAUBLE_SLOTS, &mut mind.inventory.baubles)?;
    // Мусорка — один слот отдельной секции: читаем во временный массив.
    let mut trash = [None; 1];
    read_slots(r, SlotArea::Trash, 1, &mut trash)?;
    mind.inventory.trash = trash[0];
    Ok(Some(mind))
}

// ---------- Снаряды и воткнутые копья ----------

/// Читает один снаряд; проверяет конечность координат и разумные диапазоны.
fn read_projectile(r: &mut Reader<'_>) -> Result<crate::simulation::combat::Projectile, SaveError> {
    let id = r.read_u64()?;
    let kind = crate::simulation::combat::ProjectileKind::from_discriminant(r.read_u8()?)
        .ok_or(SaveError::BadData("вид снаряда"))?;
    let ox = r.read_f32()?;
    let oy = r.read_f32()?;
    let tx = r.read_f32()?;
    let ty = r.read_f32()?;
    let t = r.read_f32()?;
    let duration_ticks = r.read_u64()?;
    let arc_height = r.read_f32()?;
    let damage = r.read_f32()?;
    let shooter = r.read_u64()?;

    let coords = [ox, oy, tx, ty, t, arc_height, damage];
    if !coords.iter().all(|value| value.is_finite()) {
        return Err(SaveError::BadData("координаты снаряда"));
    }
    if !(0.0..=1.0).contains(&t) || duration_ticks == 0 || damage < 0.0 || arc_height < 0.0 {
        return Err(SaveError::BadData("параметры снаряда"));
    }

    Ok(crate::simulation::combat::Projectile {
        id,
        kind,
        origin: WorldPos { x: ox, y: oy },
        target: WorldPos { x: tx, y: ty },
        t,
        duration_ticks,
        arc_height,
        damage,
        shooter,
    })
}

/// Читает одно воткнутое копьё.
fn read_stuck_spear(r: &mut Reader<'_>) -> Result<crate::simulation::combat::StuckSpear, SaveError> {
    let x = r.read_f32()?;
    let y = r.read_f32()?;
    let heading = r.read_f32()?;
    let pitch = r.read_f32()?;
    if ![x, y, heading, pitch].iter().all(|value| value.is_finite()) {
        return Err(SaveError::BadData("координаты воткнутого копья"));
    }
    Ok(crate::simulation::combat::StuckSpear {
        pos: WorldPos { x, y },
        heading,
        pitch,
    })
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
                    .ok_or(SaveError::BadData("вид предмета ума"))?;
                let stack_count = r.read_u32()?;
                if stack_count == 0 || stack_count > kind.max_stack() {
                    return Err(SaveError::BadData("размер стака ума"));
                }
                if !Inventory::accepts(kind, area, index) {
                    return Err(SaveError::BadData("предмет в чужом слоте"));
                }
                *slot = Some(ItemStack { kind, count: stack_count });
            }
            _ => return Err(SaveError::BadData("тег слота ума")),
        }
    }
    Ok(())
}
/// Читает разреженный слой культур; стадия не может превышать последнюю.
fn read_sparse_crops(
    r: &mut Reader<'_>,
) -> Result<std::collections::HashMap<u8, crate::simulation::crops::Crop>, SaveError> {
    let count = r.read_u16()? as usize;
    let mut map = std::collections::HashMap::with_capacity(count);
    for _ in 0..count {
        let idx = r.read_u8()?;
        let kind = crate::simulation::crops::CropKind::from_discriminant(r.read_u8()?)
            .ok_or(SaveError::BadData("вид культуры"))?;
        let stage = r.read_u8()?;
        if stage as usize >= kind.stage_count() {
            return Err(SaveError::BadData("стадия культуры"));
        }
        if map.insert(idx, crate::simulation::crops::Crop { kind, stage }).is_some() {
            return Err(SaveError::BadData("дубликат индекса культуры"));
        }
    }
    Ok(map)
}
/// Читает разреженный слой контейнеров станций: ёмкость — по виду станции,
/// стеки — в пределах стаков, сроки — конечные неотрицательные.
fn read_sparse_containers(
    r: &mut Reader<'_>,
) -> Result<std::collections::HashMap<u8, crate::simulation::containers::Container>, SaveError> {
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
                crate::simulation::containers::Container {
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
fn read_item_entities(r: &mut Reader<'_>, side: u32) -> Result<crate::simulation::item_entity::ItemEntities, SaveError> {
    let count = r.read_u32()?;
    if count > 1_000_000 {
        return Err(SaveError::BadData("слишком много item-сущностей"));
    }
    let mut store = crate::simulation::item_entity::ItemEntities::default();
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
        store.items.push(crate::simulation::item_entity::ItemEntity {
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
