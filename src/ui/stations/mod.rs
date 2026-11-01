//! GUI рабочих станций: печь, плавильная печь, костёр, бочка, склад,
//! верстак, кузнечный стол. Открывается ПКМ по станции в режиме игрока.
//! В окне доступны слоты станции (контейнер из `simulation/containers`)
//! и MISC-инвентарь игрока как хотбар; броня, одежда и косметика недоступны.
//!
//! Перенос предметов между станцией и игроком идёт командой
//! `Command::StationMoveItem` (симуляция валидирует правила слотов):
//! клик по слоту выбирает источник, клик по другому слоту — перенос
//! целого стека; Shift+клик — быстрый перенос на другую сторону
//! (долив в частичный стек того же предмета или первый свободный слот,
//! без обмена); ПКМ — половина стека. Верстак и кузнечный стол контейнера не имеют:
//! их сетка 3×3 (и инструменты кузнечного стола) временные, живут в UI
//! и возвращаются в инвентарь при закрытии окна. Верстак наследует
//! ручные рецепты (`manual`) + свои (`crafting_table`), кузнечный стол —
//! `smithing_table` с требованием инструментов (не расходуются).
//!
//! Разбивка по файлам: `shared` — маркеры и общие утилиты слотов,
//! `window` — жизненный цикл окна, `furnace` — контейнерные станции
//! (печи/костёр/хранилища), `crafting` — сетка 3×3 и кузнечный стол,
//! `drag` — перетаскивание и перенос стеков.

mod crafting;
mod drag;
mod furnace;
mod shared;
mod window;

use bevy::prelude::*;

use crate::simulation::crafts::GRID_SLOTS;
use crate::simulation::items::ItemStack;
use crate::simulation::objects::ObjectKind;
use crate::simulation::TilePos;

pub(crate) use crafting::{on_station_craft_slot_activated, on_station_result_activated};
pub(crate) use drag::on_station_slot_activated;
// Снаружи (тултипы) работают с маркерами слотов окна станции.
pub(crate) use shared::{SlotIndex, StationPlayerSlot, StationRoot, StationSlot};
pub use window::toggle_station;

/// Плагин окна станции: обсерверы активации слотов и цепочка систем
/// обновления (закрытие → дистанция → drag → цифры → перерисовка слотов
/// → индикаторы печей).
pub struct StationPlugin;

impl Plugin for StationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StationUi>()
            .add_observer(on_station_slot_activated)
            .add_observer(on_station_craft_slot_activated)
            .add_observer(on_station_result_activated)
            .add_systems(
                Update,
                (
                    window::close_station_window,
                    // Дистанция проверяется до drag: закрытая станция
                    // снимает свой призрак в station_drag этого же кадра.
                    window::station_reach_close,
                    drag::station_drag,
                    drag::station_digit_transfer,
                    shared::refresh_station_slots,
                    furnace::refresh_furnace_indicators,
                )
                    .chain(),
            );
    }
}

/// Открытая станция и временное состояние её окна.
#[derive(Resource, Default)]
pub struct StationUi {
    pub open: bool,
    /// Тайл станции, с которой открыто окно.
    pub pos: Option<TilePos>,
    /// Вид станции на момент открытия.
    pub kind: Option<ObjectKind>,
    /// Сетка крафта верстака/кузнечного стола (временная, живёт в UI).
    pub(crate) grid: [Option<ItemStack>; GRID_SLOTS],
    /// Инструменты кузнечного стола (временные, не расходуются).
    pub(crate) tools: [Option<ItemStack>; 4],
    /// Выбранный адрес-источник для переноса.
    pub(crate) selected: Option<StationAddress>,
    /// Идущее перетаскивание стека (призрак под курсором). Реального
    /// снятия стека нет: перенос/выброс исполняются командой на отпускании.
    pub(crate) drag: Option<StationDragState>,
}

/// Идущее перетаскивание в окне станции: слот-источник и призрак-иконка.
pub(crate) struct StationDragState {
    pub(crate) from: StationAddress,
    ghost: Entity,
}

/// Адрес слота в окне станции: контейнерный слот станции, misc игрока,
/// клетка сетки крафта или слот инструмента кузнечного стола.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StationAddress {
    /// Слот контейнера станции (индекс в `Container::slots`).
    Container(usize),
    /// Слот misc инвентаря игрока (индекс).
    Player(usize),
    /// Клетка сетки крафта (UI-локальная).
    Grid(usize),
    /// Слот инструмента кузнечного стола (UI-локальный).
    Tool(usize),
}
