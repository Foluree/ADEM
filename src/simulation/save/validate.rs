//! Структурные проверки уровня формата при загрузке сохранения: они не
//! привязаны к разбору отдельной записи и работают по уже прочитанным
//! структурам. Проверки полей (HP, стеки, дискриминанты) живут в `read.rs`
//! рядом с чтением соответствующих байтов.

use std::collections::HashMap;

use crate::simulation::chunk::CHUNK_SIZE;
use crate::simulation::containers;
use crate::simulation::objects::WorldObject;
use crate::simulation::world::WorldSize;
use super::SaveError;

/// Размер мира обязан быть корректным — `WorldSize::new` иначе зажал бы его,
/// и сохранённые координаты разъехались бы с фактической картой.
pub(super) fn validate_world_size(side: u32) -> Result<WorldSize, SaveError> {
    if !(WorldSize::MIN..=WorldSize::MAX).contains(&side) || !side.is_multiple_of(CHUNK_SIZE as u32) {
        return Err(SaveError::BadData("недопустимый размер мира"));
    }
    Ok(WorldSize::new(side))
}

/// Контейнер существует только у тайла станции и по размеру совпадает
/// с ёмкостью её вида; разъехавшиеся слои — повреждение.
pub(super) fn validate_container_layer(
    objects: &HashMap<u8, WorldObject>,
    containers_map: &HashMap<u8, containers::Container>,
) -> Result<(), SaveError> {
    for (idx, container) in containers_map {
        let Some(obj) = objects.get(idx) else {
            return Err(SaveError::BadData("контейнер без станции"));
        };
        if containers::capacity(obj.kind) != Some(container.slots.len()) {
            return Err(SaveError::BadData("ёмкость контейнера"));
        }
    }
    Ok(())
}

/// Тики часов и планировщика обязаны совпадать: планировщик исполняет
/// задачи по мировому тику, рассинхрон значил бы потерю или дубль событий.
pub(super) fn validate_scheduler_tick(sched_tick: u64, clock_tick: u64) -> Result<(), SaveError> {
    if sched_tick != clock_tick {
        return Err(SaveError::BadData("тики часов и планировщика разошлись"));
    }
    Ok(())
}
