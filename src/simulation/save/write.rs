//! Запись сохранения: сериализация состояния симуляции в байты.
//! Разделы формата — в doc-комментарии `save/mod.rs`; порядок записи
//! должен совпадать с порядком чтения `read.rs` до байта.

use crate::simulation::chunk::TerrainTiles;
use crate::simulation::entities::{EntityStore, HumanMind};
use crate::simulation::items::{ItemStack, MISC_SLOTS};
use crate::simulation::objects::WorldObject;
use crate::simulation::scheduler::{Scheduler, TaskKind};
use crate::simulation::time::GameClock;
use crate::simulation::world::World;
use super::{MAGIC, SAVE_FORMAT_VERSION};

/// Записывает состояние симуляции в байты.
///
/// `game_version` — версия игры (константа проекта), пишется в заголовок
/// для диагностики; на совместимость не влияет — за это отвечает
/// `SAVE_FORMAT_VERSION`.
pub fn save_game(
    world: &World,
    clock: &GameClock,
    scheduler: &Scheduler,
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
        write_u64(&mut w, p.shooter);
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

    w
}
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
        write_f32(w, entity.hunger.value());
        write_u16(w, entity.water_ticks);
        write_u64(w, entity.state_ticks);
        write_u64(w, entity.state_duration);
        write_f32(w, entity.target.x);
        write_f32(w, entity.target.y);
        write_u16(w, entity.stuck_ticks);
        write_u64(w, entity.land_search_failed_at);
        write_mind(w, entity.mind.as_ref());
    }
}

/// Пишет «ум» существа (инвентарь, слот хотбара). Кулдауны атаки/еды,
/// цель ИИ и память (костёр/еда) не пишутся — transient: тики мира при
/// загрузке могли уйти вперёд, человек сам выберет цель заново.
/// `None` — неразумное существо (волк).
fn write_mind(w: &mut Vec<u8>, mind: Option<&HumanMind>) {
    let Some(m) = mind else {
        w.push(0);
        return;
    };
    w.push(1);
    debug_assert!(m.selected_slot < MISC_SLOTS);
    w.push(m.selected_slot as u8);
    write_slots(w, &m.inventory.items);
    write_slots(w, &m.inventory.armor);
    write_slots(w, &m.inventory.clothes);
    write_slots(w, &m.inventory.baubles);
    write_slots(w, std::slice::from_ref(&m.inventory.trash));
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
/// Пишет разреженный слой культур (по возрастанию индексов).
fn write_sparse_crops(w: &mut Vec<u8>, map: &std::collections::HashMap<u8, crate::simulation::crops::Crop>) {
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
/// Пишет разреженный слой контейнеров станций (по возрастанию индексов).
fn write_sparse_containers(
    w: &mut Vec<u8>,
    map: &std::collections::HashMap<u8, crate::simulation::containers::Container>,
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
