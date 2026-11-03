//! Ввод режима игрока и редактора: переключение режимов, движение WASD,
//! клики ЛКМ по карте (инструменты «бога» в наблюдателе, добыча — в игре).
//!
//! UI не меняет мир напрямую: все действия уходят командами в очередь
//! симуляции (`simulation/commands.rs`).
//!
//! Состав модуля:
//! - `player` — режим игрока: захват существа (F1), движение, ЛКМ/ПКМ,
//!   выброс и бросок предметов;
//! - `editor` — режим наблюдателя: размещение/удаление инструментов,
//!   перетаскивание существ СКМ;
//! - `helpers` — общие лучи курсора (`cursor_world_pos`/`cursor_world_tile`)
//!   и пикинг существ (`entity_at`).

use bevy::prelude::*;

use crate::app::settings::{ControlAction, Settings};

mod editor;
mod helpers;
mod player;

pub(crate) use editor::{editor_delete_input, editor_entity_drag};
pub(crate) use helpers::cursor_world_tile;
pub(crate) use player::{
    world_click_input, player_drop_input, player_move_input, player_pickup_input,
    player_throw_input, toggle_player_mode,
};

/// Вид визуального эффекта оружия. Бой и кулдауны считаются в симуляции,
/// сообщение — только сигнал о визуальном событии для рендера.
#[derive(Clone, Copy, PartialEq, Eq, Message)]
pub enum WeaponFxKind {
    /// Дуга взмаха лезвия (нож, в будущем меч).
    Blade,
    /// Выпад копья: текстура от центра игрока вперёд и назад.
    Thrust,
    /// Выстрел из лука: текстура лука в центре игрока, повёрнутая к выстрелу.
    Bow,
}

/// Событие «игрок применил оружие» для рендера эффектов.
#[derive(Message)]
pub struct WeaponFx {
    pub kind: WeaponFxKind,
    /// Точка, откуда эффект (позиция игрока в мировых пикселях).
    pub origin: Vec2,
    /// Точка, куда направлено (прицел в мировых пикселях).
    pub aim: Vec2,
}

/// Направление движения игрока в координатах сетки мира.
/// Ноль — стоит на месте; нормализация и скорость — в симуляции.
/// Читается тиковой системой движения, поэтому направление — состояние,
/// а не поток команд.
#[derive(Resource, Default, Clone, Copy)]
pub struct PlayerInput {
    pub dir: (f32, f32),
}

/// Читает нажатия биндов движения (по умолчанию WASD). Возвращает
/// направление (−1/0/1 по осям) без нормализации. Общая для движения
/// игрока (input.rs) и камеры редактора (camera.rs).
pub(crate) fn move_dir(keys: &ButtonInput<KeyCode>, settings: &Settings) -> (f32, f32) {
    let mut x = 0.0;
    let mut y = 0.0;
    if keys.pressed(settings.controls.key(ControlAction::MoveUp)) {
        y += 1.0;
    }
    if keys.pressed(settings.controls.key(ControlAction::MoveDown)) {
        y -= 1.0;
    }
    if keys.pressed(settings.controls.key(ControlAction::MoveRight)) {
        x += 1.0;
    }
    if keys.pressed(settings.controls.key(ControlAction::MoveLeft)) {
        x -= 1.0;
    }
    (x, y)
}

/// Преобразует экранное направление в диагональные оси изометрической сетки:
/// экранные вверх/вправо соответствуют (x+y, y-x) в координатах мира.
fn player_grid_dir(screen_dir: (f32, f32)) -> (f32, f32) {
    let (screen_x, screen_y) = screen_dir;
    (screen_x + screen_y, screen_y - screen_x)
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerInput>()
            .add_message::<WeaponFx>()
            .add_systems(
                Update,
                (
                    toggle_player_mode,
                    player_move_input,
                    world_click_input,
                    player_drop_input,
                    player_pickup_input,
                    editor_delete_input,
                    editor_entity_drag,
                    player_throw_input,
                )
                    .after(crate::app::camera::update_camera_transforms),
            );
    }
}

#[cfg(test)]
#[path = "../../tests/input.rs"]
mod tests;
