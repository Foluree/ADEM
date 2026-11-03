//! Перетаскивание стеков между слотами инвентаря, хотбара и мира: зажать
//! ЛКМ на слоте и отпустить над целевым. Пока стек «в руке», за курсором
//! следует иконка-призрак; отпускание мимо слота возвращает стек на место
//! или бросает его в мир, ПКМ отменяет перетаскивание.

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;

use crate::app::input::cursor_world_tile;
use crate::app::state::{CommandQueueRes, GameMode, GameState, MenuKind, WindowFocusState};
use crate::simulation::commands::Command;
use crate::simulation::items::{SlotArea, SlotRef};
use crate::simulation::TilePos;
use crate::ui::icons::UiIcons;
use crate::ui::cursor_over_node;

use super::{
    DragGhost, DragState, GHOST_HALF, InventoryPanel, InventoryUi, InvSlot,
};

/// Перетаскивание стеков между слотами инвентаря и хотбара: зажать ЛКМ на
/// слоте и отпустить над целевым. Пока стек «в руке», за курсором следует
/// иконка-призрак; отпускание мимо слота возвращает стек на место, ПКМ
/// отменяет перетаскивание. В режиме игрока хотбар перетаскивается и без
/// открытого окна инвентаря. Клик без движения остаётся выбором источника
/// для сетки крафта (см. `on_slot_activated`).
pub(crate) fn inventory_drag(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    focus: Res<WindowFocusState>,
    mut state: ResMut<GameState>,
    mut inv: ResMut<InventoryUi>,
    mut queue: ResMut<CommandQueueRes>,
    icons: Res<UiIcons>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    slots: Query<(&InvSlot, &ComputedNode, &UiGlobalTransform)>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<InventoryPanel>>,
    creative_panels: Query<(&ComputedNode, &UiGlobalTransform), With<crate::ui::creative::CreativePanel>>,
    mut ghosts: Query<(Entity, &mut Node), With<DragGhost>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    // Окно потеряло фокус (Alt+Tab): отпускание кнопки могло не дойти до
    // игры, поэтому стек возвращается на место, а не висит «в руке».
    if !focus.focused {
        if inv.drag.is_some() {
            cancel_drag(&mut commands, &mut inv, &mut state);
        }
        return;
    }
    // Окно инвентаря открыто — его слоты и хотбар; иначе перетаскивание
    // доступно по видимому хотбару, в том числе при открытом творческом
    // меню (бросок стека на плитки удаляет его, как в Minecraft).
    let creative_open = state.active_menu == Some(MenuKind::Creative);
    let hotbar_only =
        !inv.open && state.mode == GameMode::Player && (state.active_menu.is_none() || creative_open);
    if !inv.open && !hotbar_only {
        // Режим сменился/открылось меню посреди перетаскивания — стек
        // возвращается в исходный слот, а не остаётся «в руке» навсегда.
        if inv.drag.is_some() {
            cancel_drag(&mut commands, &mut inv, &mut state);
        }
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        // Курсор ушёл за пределы окна: отпускание кнопки всё равно
        // нужно обработать, иначе стек останется «в руке».
        if inv.drag.is_some() {
            if mouse.just_released(MouseButton::Right) {
                cancel_drag(&mut commands, &mut inv, &mut state);
            } else if mouse.just_released(MouseButton::Left) {
                finish_drag(&mut commands, &mut inv, &mut state, None);
            }
        }
        return;
    };

    // Старт: ЛКМ нажата над непустым слотом инвентаря/хотбара — стек
    // снимается в «руку» и появляется призрак. Не выходим из системы:
    // если кнопку успели отпустить в том же кадре (быстрый клик между
    // кадрами), ниже сработает и завершение перетаскивания.
    if mouse.just_pressed(MouseButton::Left) && inv.drag.is_none() {
        let Some(hover) = hovered_slot(window, &slots, cursor) else {
            return;
        };
        let target = SlotRef { area: hover.area, index: hover.index };
        let Some(p) = state.controlled_mind_mut() else {
            return;
        };
        let Some(stack) = p.inventory.get(target) else {
            return;
        };
        p.inventory.take(target);

        // Призрак — самостоятельный UI-узел поверх всего: окно инвентаря
        // может закрыться прямо во время перетаскивания, а хотбар вообще
        // живёт без него. `GlobalZIndex` держит иконку над панелями.
        let ghost = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(cursor.x - GHOST_HALF),
                    top: Val::Px(cursor.y - GHOST_HALF),
                    width: Val::Px(GHOST_HALF * 2.0),
                    height: Val::Px(GHOST_HALF * 2.0),
                    ..default()
                },
                ImageNode::new(icons.item(stack.kind).clone()),
                // Призрак висит прямо под курсором: будучи обычным UI-узлом,
                // он перехватывал бы наведение и клики у слота под ним.
                Pickable::IGNORE,
                GlobalZIndex(i32::MAX),
                DragGhost,
            ))
            .id();
        inv.drag = Some(DragState { from: target, stack, ghost });
    }
    if inv.drag.is_none() {
        return;
    }
    // ПКМ во время перетаскивания: отмена, стек возвращается на место.
    if mouse.just_released(MouseButton::Right) {
        cancel_drag(&mut commands, &mut inv, &mut state);
        return;
    }
    // Призрак следует за курсором.
    for (_, mut node) in &mut ghosts {
        node.left = Val::Px(cursor.x - GHOST_HALF);
        node.top = Val::Px(cursor.y - GHOST_HALF);
    }
    // Отпускание ЛКМ: перенос в слот под курсором; над панелью творческого
    // меню — удаление стека; за пределами открытого окна — выброс в мир в
    // сторону курсора; иначе возврат на место.
    if mouse.just_released(MouseButton::Left) {
        let hovered = hovered_slot(window, &slots, cursor);
        let target = hovered.map(|s| SlotRef { area: s.area, index: s.index });
        let over_panel = panels
            .iter()
            .any(|(computed, transform)| cursor_over_node(window, computed, transform, cursor));
        let over_creative = creative_panels
            .iter()
            .any(|(computed, transform)| cursor_over_node(window, computed, transform, cursor));
        if let Some(t) = target {
            finish_drag(&mut commands, &mut inv, &mut state, Some(t));
        } else if creative_open && over_creative {
            delete_drag(&mut commands, &mut inv);
        } else if inv.open
            && !over_panel
            && let Some(tile) = cursor_world_tile(window, &cameras, state.world.as_ref())
        {
            throw_drag(&mut commands, &mut inv, &mut state, &mut queue, tile);
        } else {
            finish_drag(&mut commands, &mut inv, &mut state, None);
        }
    }
}

/// Удаляет перетащенный стек (отпускание над панелью творческого меню):
/// стек не возвращается в слот и исчезает, как в Minecraft.
fn delete_drag(commands: &mut Commands, inv: &mut InventoryUi) {
    let Some(drag) = inv.drag.take() else {
        return;
    };
    commands.entity(drag.ghost).despawn();
}

/// Бросает перетащенный стек в мир в сторону курсора: стек возвращается
/// в исходный слот, а команда `PlayerThrowStack` списывает его оттуда и
/// бросает (симуляция сама считает дальность и задержку подбора).
/// Бросить можно только предмет из misc; остальное возвращается на место.
fn throw_drag(
    commands: &mut Commands,
    inv: &mut InventoryUi,
    state: &mut GameState,
    queue: &mut CommandQueueRes,
    target: TilePos,
) {
    let Some(drag) = inv.drag.take() else {
        return;
    };
    commands.entity(drag.ghost).despawn();
    let throw_slot = (drag.from.area == SlotArea::Misc).then_some(drag.from.index);
    if let Some(p) = state.controlled_mind_mut() {
        p.inventory.put(drag.from, drag.stack);
    }
    if let Some(slot) = throw_slot {
        queue.0.push(Command::PlayerThrowStack {
            target,
            slot: slot as u8,
        });
    }
}

/// Слот инвентаря под курсором, если есть. Хит-тест общий с другими
/// проверками UI (`cursor_over_node`).
fn hovered_slot<'a>(
    window: &Window,
    slots: &'a Query<(&InvSlot, &ComputedNode, &UiGlobalTransform)>,
    cursor: Vec2,
) -> Option<&'a InvSlot> {
    slots
        .iter()
        .find(|(_, computed, transform)| cursor_over_node(window, computed, transform, cursor))
        .map(|(slot, _, _)| slot)
}

/// Завершает перетаскивание: стек возвращается в исходный слот, затем
/// переносится в целевой (если он указан и отличен от исходного) через
/// `Inventory::transfer`. Отпускание над исходным слотом — обычный клик:
/// выбор источника остаётся за наблюдателем активации.
fn finish_drag(
    commands: &mut Commands,
    inv: &mut InventoryUi,
    state: &mut GameState,
    target: Option<SlotRef>,
) {
    let Some(drag) = inv.drag.take() else {
        return;
    };
    commands.entity(drag.ghost).despawn();
    let Some(p) = state.controlled_mind_mut() else {
        return;
    };
    // Сначала возвращаем стек «в руку» в исходный слот: transfer берёт его оттуда. 
    // Если целевой слот несовместим — стек просто останется на месте.
    p.inventory.put(drag.from, drag.stack);
    if let Some(t) = target
        && t != drag.from
        && p.inventory.transfer(drag.from, t)
    {
        // Перенос завершён - выбор источника больше не нужен.
        inv.selected = None;
    }
}

/// Отменяет перетаскивание (ПКМ): стек возвращается в исходный слот, призрак удаляется.
fn cancel_drag(
    commands: &mut Commands,
    inv: &mut InventoryUi,
    state: &mut GameState,
) {
    let Some(drag) = inv.drag.take() else {
        return;
    };
    commands.entity(drag.ghost).despawn();
    if let Some(p) = state.controlled_mind_mut() {
        p.inventory.put(drag.from, drag.stack);
    }
}
