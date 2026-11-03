//! Ввод режима наблюдателя (редактор «бога»): размещение объектов
//! обрабатывает `player.rs` (общая система ЛКМ), здесь — удаление ПКМ
//! по категории открытого подменю и перетаскивание существ СКМ.

use bevy::camera::Camera2d;
use bevy::input::mouse::{MouseButton, MouseMotion};
use bevy::prelude::*;

use crate::app::state::{CommandQueueRes, EditorSelection, GameMode, GameState, ToolCategory, WindowFocusState};
use crate::rendering::{EntitySpriteVisuals, TileTextures};
use crate::simulation::commands::Command;
use crate::simulation::objects::{ObjectKind, WorldObject};
use crate::simulation::world::WorldPos;
use crate::simulation::TILE_SIZE;
use crate::ui::bottom_panel::{PanelMenu, PanelMenuState};
use crate::ui::{BlocksWorldInput, cursor_over_ui};

use super::helpers::{cursor_world_pos, cursor_world_tile, entity_at};

/// Объект, ставимый редактором по виду. У валуна — средний запас из
/// рабочего диапазона генерации (5–15 единиц, GAME_DESIGN приложение A).
pub(crate) fn placed_object(kind: ObjectKind) -> WorldObject {
    match kind {
        ObjectKind::Tree => WorldObject::tree(),
        ObjectKind::Bush => WorldObject::bush(),
        ObjectKind::Rock => WorldObject::rock(10),
        ObjectKind::AppleTree => WorldObject::apple_tree(),
        ObjectKind::Building(kind) => WorldObject::building(kind),
    }
}

/// Что удаляет ПКМ в редакторе. Класс цели задаётся открытым подменю
/// нижней панели: пока подменю категории не открыто, цель не выбрана.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DeleteClass {
    Objects,
    Entities,
}

/// ПКМ в режиме наблюдателя — удалить объект из открытой категории.
///
/// Удаление срабатывает по отпусканию без движения (порог в несколько
/// пикселей): зажатая ПКМ с движением — это панорамирование камеры, и
/// удалять то, что протащили под курсором, нельзя.
pub(crate) fn editor_delete_input(
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    windows: Query<&Window>,
    state: Res<GameState>,
    menu: Res<PanelMenuState>,
    mut queue: ResMut<CommandQueueRes>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
    textures: Res<TileTextures>,
    visuals: Res<EntitySpriteVisuals>,
    mut drag_distance: Local<Option<f32>>,
) {
    if state.mode != GameMode::Observer {
        *drag_distance = None;
        return;
    }

    // Класс цели — только из открытого подменю инструментов. Базовые кнопки,
    // скорость и категория тайлов цель для удаления не задают: ПКМ по карте
    // в этих состояниях ничего не трогает.
    let target_class = match menu.open {
        Some(PanelMenu::Tools(ToolCategory::Objects)) => Some(DeleteClass::Objects),
        Some(PanelMenu::Tools(ToolCategory::Buildings)) => Some(DeleteClass::Objects),
        Some(PanelMenu::Tools(ToolCategory::Entities)) => Some(DeleteClass::Entities),
        _ => None,
    };

    // Дрожание руки при клике не должно отменять удаление: порог в
    // несколько пикселей отделяет клик от осознанного панорамирования.
    const CLICK_DRAG_THRESHOLD: f32 = 4.0;
    let motion_delta: f32 = motion.read().map(|event| event.delta.length()).sum();
    if mouse.just_pressed(MouseButton::Right) {
        *drag_distance = Some(motion_delta);
    } else if let Some(distance) = drag_distance.as_mut() {
        *distance += motion_delta;
    }

    // Отпускание — единственный момент удаления: так быстрый клик
    // (нажатие и отпускание в одном кадре) тоже обрабатывается.
    if !mouse.just_released(MouseButton::Right) {
        return;
    }
    let Some(distance) = drag_distance.take() else {
        return;
    };
    if distance > CLICK_DRAG_THRESHOLD {
        return;
    }
    let Some(target_class) = target_class else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    if cursor_over_ui(window, &blockers) {
        return;
    }
    let Some(world) = state.world.as_ref() else {
        return;
    };
    let Some(tile) = cursor_world_tile(window, &cameras, Some(world)) else {
        return;
    };
    match target_class {
        DeleteClass::Objects => {
            if world.object_at(tile).is_some() {
                queue.0.push(Command::SetObject { pos: tile, object: None });
            }
        }
        DeleteClass::Entities => {
            if let Some(id) = entity_at(world, window, &cameras, &textures, &visuals) {
                queue.0.push(Command::RemoveEntity { id });
            }
        }
    }
}

/// СКМ перетаскивает существо в редакторе: нажатие на него
/// «берёт» существо, пока кнопка зажата — оно следует за курсором,
/// отпускание освобождает. Каждый кадр уходит команда `MoveEntity`.
pub(crate) fn editor_entity_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    windows: Query<&Window>,
    focus: Res<WindowFocusState>,
    state: Res<GameState>,
    mut selection: ResMut<EditorSelection>,
    mut queue: ResMut<CommandQueueRes>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
    textures: Res<TileTextures>,
    visuals: Res<EntitySpriteVisuals>,
    mut dragged: Local<Option<u64>>,
    // Накопленное движение мыши с момента нажатия СКМ: клик без
    // перетаскивания (порог в пикселях) открывает окно-инспектор.
    mut drag_distance: Local<f32>,
) {
    const CLICK_DRAG_THRESHOLD: f32 = 4.0;

    // Потеря фокуса окна: отпускание кнопки могло не дойти до игры
    // (например, кнопку отпустили в другом окне), поэтому перетаскивание
    // завершаем сразу, не дожидаясь следующего нажатия.
    if state.mode != GameMode::Observer || !focus.focused {
        *dragged = None;
        selection.hovered = None;
        return;
    }

    let Ok(window) = windows.single() else {
        *dragged = None;
        return;
    };

    // Наведение: существо под курсором (не над UI) подсвечивается белым
    // контуром. Пересчёт только в наблюдателе — в игре выделения нет.
    selection.hovered = if cursor_over_ui(window, &blockers) {
        None
    } else {
        state
            .world
            .as_ref()
            .and_then(|world| entity_at(world, window, &cameras, &textures, &visuals))
    };

    // Нажатие на существо начинает перетаскивание. Клик мимо — ничего.
    if mouse.just_pressed(MouseButton::Middle) {
        *dragged = None;
        *drag_distance = 0.0;
        if cursor_over_ui(window, &blockers) {
            return;
        }
        let Some(world) = state.world.as_ref() else {
            return;
        };
        if cursor_world_pos(window, &cameras, world).is_none() {
            return;
        }
        *dragged = entity_at(world, window, &cameras, &textures, &visuals);
    }
    // Дрожание руки при клике не должно открывать инспектор вместо
    // перетаскивания: порог тот же, что у удаления ПКМ.
    let motion_delta: f32 = motion.read().map(|event| event.delta.length()).sum();
    if mouse.pressed(MouseButton::Middle) {
        *drag_distance += motion_delta;
    }

    let Some(id) = *dragged else {
        return;
    };
    let releasing = mouse.just_released(MouseButton::Middle);
    if !mouse.pressed(MouseButton::Middle) && !releasing {
        *dragged = None;
        return;
    }

    // Отпускание без движения — это клик по существу: открыть/закрыть
    // окно-инспектор вместо переноса.
    if releasing && *drag_distance <= CLICK_DRAG_THRESHOLD {
        selection.selected = if selection.selected == Some(id) {
            None
        } else {
            Some(id)
        };
        *dragged = None;
        return;
    }

    // Перенос начинается только когда зажатие переросло клик: раньше
    // команда уходила уже в кадр нажатия, и СКМ-клик по сущности
    // телепортировала её к курсору (радиус выбора — до 2 тайлов) до того,
    // как releasing успевал превратить зажатие в инспектор.
    if *drag_distance > CLICK_DRAG_THRESHOLD
        && mouse.pressed(MouseButton::Middle)
        && !cursor_over_ui(window, &blockers)
        && let Some(world) = state.world.as_ref()
        && let Some(point) = cursor_world_pos(window, &cameras, world)
    {
        // Курсор в пикселях мира, позиция существа — в тайлах. Край мира
        // зажимаем чуть внутрь, чтобы существо не вышло за границу.
        let edge = (world.size.tiles() as f32 - 0.001).max(0.0);
        let pos = WorldPos {
            x: (point.x / TILE_SIZE).clamp(0.0, edge),
            y: (point.y / TILE_SIZE).clamp(0.0, edge),
        };
        queue.0.push(Command::MoveEntity { id, pos });
    }

    if releasing {
        *dragged = None;
    }
}
