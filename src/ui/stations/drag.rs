//! Перенос стеков в окне станции: клик-источник → клик-цель, перетаскивание
//! с призраком, быстрый перенос (Shift: долив или первый свободный слот,
//! без обмена), половина стека (ПКМ), перекладывание цифровыми клавишами,
//! бросок за пределами окна.

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::app::input::cursor_world_tile;
use crate::app::state::{CommandQueueRes, GameState, WindowFocusState};
use crate::simulation::commands::Command;
use crate::simulation::containers::{self, capacity, can_put_into, SLOT_AREA_PLAYER_MISC};
use crate::simulation::items::{self, SlotArea, SlotRef};
use crate::simulation::objects::ObjectKind;
use crate::simulation::{ItemKind, TilePos};
use crate::ui::icons::UiIcons;
use crate::ui::inventory::{DragGhost, GHOST_HALF};
use crate::ui::stations::shared::{
    encode_address, hovered_station_address, stack_at, stack_kind, StationOutputSlot, StationPanel,
    StationPlayerSlot, StationSlot,
};
use crate::ui::cursor_over_node;

use super::{StationAddress, StationDragState, StationUi};

/// Клавиши 1–8 для перекладывания предметов в GUI станции.
const DIGIT_KEYS: [KeyCode; 8] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
];

/// Активация слота станции (контейнер или misc игрока): выбор источника,
/// перенос командой, Shift — быстрый перенос, ПКМ — половина стека.
pub(crate) fn on_station_slot_activated(
    activate: On<Activate>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut station: ResMut<StationUi>,
    mut queue: ResMut<CommandQueueRes>,
    state: Res<GameState>,
    station_slots: Query<&StationSlot>,
    player_slots: Query<(&StationPlayerSlot, &crate::ui::stations::shared::SlotIndex)>,
) {
    // Адрес слота: контейнерный или misc игрока.
    let address = if let Ok(slot) = station_slots.get(activate.entity) {
        StationAddress::Container(slot.index)
    } else if let Ok((_, index)) = player_slots.get(activate.entity) {
        StationAddress::Player(index.0)
    } else {
        return;
    };
    let Some(kind) = station.kind else {
        return;
    };
    let Some(pos) = station.pos else {
        return;
    };

    // ПКМ — половина стека на другую сторону (станция ↔ misc).
    if mouse.just_released(MouseButton::Right) {
        move_half(&mut station, &mut queue, &state, pos, kind, address);
        return;
    }
    if mouse.just_released(MouseButton::Middle) {
        return;
    }
    let shift_held = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    // ЛКМ по выходному слоту печи — забрать весь стек в misc (интуитивный
    // клик; Shift+клик обрабатывается ниже как быстрый перенос).
    if !shift_held
        && let StationAddress::Container(index) = address
        && containers::is_output_slot(kind, index)
        && let Some(stack) = stack_at(&station, &state, address)
        && let Some(target) = opposite_target(&state, pos, kind, address, stack.kind, stack.count)
    {
        queue.0.push(Command::StationMoveItem {
            pos,
            from: encode_address(address),
            to: target,
            count: 0,
        });
        station.selected = None;
        return;
    }
    // Shift+клик — быстрый перенос целого стека на другую сторону.
    if shift_held {
        quick_move(&mut station, &mut queue, &state, pos, kind, address);
        return;
    }
    match station.selected {
        Some(from) if from == address => station.selected = None,
        Some(from) => {
            queue.0.push(Command::StationMoveItem {
                pos,
                from: encode_address(from),
                to: encode_address(address),
                count: 0,
            });
            station.selected = None;
        }
        None => {
            // Выбор источника: только непустой слот (контейнер читает мир).
            if stack_at(&station, &state, address).is_some() {
                station.selected = Some(address);
            }
        }
    }
}

/// Половина стека: из станции в misc захваченного существа или наоборот.
fn move_half(
    station: &mut StationUi,
    queue: &mut CommandQueueRes,
    state: &Res<GameState>,
    pos: TilePos,
    kind: ObjectKind,
    address: StationAddress,
) {
    let Some(stack) = stack_at(station, state, address) else {
        return;
    };
    let half = stack.count.div_ceil(2);
    let Some(target) = opposite_target(state, pos, kind, address, stack.kind, half) else {
        return;
    };
    queue.0.push(Command::StationMoveItem {
        pos,
        from: encode_address(address),
        to: target,
        count: half.min(u8::MAX as u32) as u8,
    });
    station.selected = None;
}

/// Быстрый перенос: слот станции → первый подходящий misc, misc → первый
/// допустимый слот станции.
fn quick_move(
    station: &mut StationUi,
    queue: &mut CommandQueueRes,
    state: &Res<GameState>,
    pos: TilePos,
    kind: ObjectKind,
    address: StationAddress,
) {
    let Some(stack) = stack_at(station, state, address) else {
        return;
    };
    let Some(target) = opposite_target(state, pos, kind, address, stack.kind, stack.count)
    else {
        return;
    };
    queue.0.push(Command::StationMoveItem {
        pos,
        from: encode_address(address),
        to: target,
        count: 0,
    });
    station.selected = None;
}

/// Ищет целевой адрес на противоположной стороне (станция ↔ misc игрока).
/// Сначала долив в частичный стек того же предмета, затем первый свободный
/// слот. Обмена при быстром переносе нет: нет ни долива, ни свободного
/// слота — перенос запрещён (обмен остаётся обычному клику).
fn opposite_target(
    state: &GameState,
    pos: TilePos,
    kind: ObjectKind,
    address: StationAddress,
    item: ItemKind,
    count: u32,
) -> Option<(u8, u8)> {
    let p = state.controlled_mind()?;
    match address {
        StationAddress::Container(_) => {
            // В misc игрока: сначала частичный стек того же предмета
            // с местом, затем первый свободный слот.
            (0..items::MISC_SLOTS)
                .find(|&i| match p.inventory.items.get(i).copied().flatten() {
                    None => true,
                    Some(s) => s.kind == item && s.count + count <= item.max_stack(),
                })
                .map(|i| (SLOT_AREA_PLAYER_MISC, i as u8))
        }
        StationAddress::Player(_) => {
            // В контейнер: правила слотов (`can_put_into`) + долив или
            // пустой слот. Занятый другим предметом слот не подходит —
            // Shift+клик ничего не меняет местами.
            let world = state.world.as_ref()?;
            let cap = capacity(kind)?;
            (0..cap)
                .filter(|&slot| can_put_into(kind, slot, item))
                .find(|&slot| {
                    match containers::container_at(world, pos).and_then(|c| c.get(slot)) {
                        None => true,
                        Some(s) => s.kind == item && s.count + count <= item.max_stack(),
                    }
                })
                .map(|slot| (slot as u8, 0))
        }
        _ => None,
    }
}

/// Перетаскивание стеков в окне станции: ЛКМ над непустым слотом (не
/// выходом) берёт стек «в руку» (призрак), отпускание над другим слотом
/// переносит (misc↔misc — напрямую инвентарём, с контейнером — командой
/// `StationMoveItem`), отпускание за пределами окна бросает стек в мир в
/// сторону курсора. Рамка слота-источника подсвечивается `refresh`'ем.
pub(crate) fn station_drag(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    focus: Res<WindowFocusState>,
    mut state: ResMut<GameState>,
    mut station: ResMut<StationUi>,
    mut queue: ResMut<CommandQueueRes>,
    icons: Res<UiIcons>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    station_slots: Query<(
        &StationSlot,
        Option<&StationOutputSlot>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    player_slots: Query<(&StationPlayerSlot, &crate::ui::stations::shared::SlotIndex, &ComputedNode, &UiGlobalTransform)>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<StationPanel>>,
    mut ghosts: Query<(Entity, &mut Node), With<DragGhost>>,
) {
    if !station.open || !focus.focused {
        // Окно закрылось (или потеряло фокус) посреди перетаскивания:
        // реального снятия стека не было — достаточно снять призрака.
        if let Some(drag) = station.drag.take() {
            commands.entity(drag.ghost).despawn();
        }
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let cursor = window.cursor_position();

    // Завершение/отмена — до старта (быстрый клик между кадрами).
    if mouse.just_released(MouseButton::Right) {
        if let Some(drag) = station.drag.take() {
            commands.entity(drag.ghost).despawn();
        }
        return;
    }
    if mouse.just_released(MouseButton::Left) {
        if let Some(drag) = station.drag.take() {
            commands.entity(drag.ghost).despawn();
            finish_station_drag(
                &mut station,
                &mut state,
                &mut queue,
                drag,
                cursor,
                window,
                &cameras,
                &station_slots,
                &player_slots,
                &panels,
            );
        }
        return;
    }
    // Призрак следует за курсором.
    if station.drag.is_some()
        && let Some(cursor) = cursor
    {
        for (_, mut node) in &mut ghosts {
            node.left = Val::Px(cursor.x - GHOST_HALF);
            node.top = Val::Px(cursor.y - GHOST_HALF);
        }
    }
    // Старт: ЛКМ над непустым слотом, не являющимся выходом.
    if station.drag.is_none()
        && mouse.just_pressed(MouseButton::Left)
        && let Some(cursor) = cursor
        && let Some((from, is_output)) =
            hovered_station_address(window, &station_slots, &player_slots, cursor)
        && !is_output
        && stack_at(&station, &state, from).is_some()
    {
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
                ImageNode::new(icons.item(stack_kind(&station, &state, from)).clone()),
                Pickable::IGNORE,
                GlobalZIndex(i32::MAX),
                DragGhost,
            ))
            .id();
        station.drag = Some(StationDragState { from, ghost });
    }
}

/// Завершает перетаскивание станции: перенос в слот под курсором либо
/// (за пределами окна) бросок стека в мир в сторону курсора.
fn finish_station_drag(
    station: &mut StationUi,
    state: &mut GameState,
    queue: &mut CommandQueueRes,
    drag: StationDragState,
    cursor: Option<Vec2>,
    window: &Window,
    cameras: &Query<(&Camera, &Transform), With<Camera2d>>,
    station_slots: &Query<(
        &StationSlot,
        Option<&StationOutputSlot>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    player_slots: &Query<(
        &StationPlayerSlot,
        &crate::ui::stations::shared::SlotIndex,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    panels: &Query<(&ComputedNode, &UiGlobalTransform), With<StationPanel>>,
) {
    let Some(pos) = station.pos else {
        return;
    };
    let target = cursor.and_then(|c| {
        hovered_station_address(window, station_slots, player_slots, c)
            .filter(|(addr, _)| *addr != drag.from)
            .map(|(addr, _)| addr)
    });
    if let Some(target) = target {
        match (drag.from, target) {
            // misc ↔ misc: правила инвентаря применяются напрямую, как в
            // окне инвентаря (оба слота — misc).
            (StationAddress::Player(a), StationAddress::Player(b)) => {
                if let Some(p) = state.controlled_mind_mut() {
                    p.inventory.transfer(
                        SlotRef { area: SlotArea::Misc, index: a },
                        SlotRef { area: SlotArea::Misc, index: b },
                    );
                }
            }
            // С контейнером решает симуляция: правила слотов, долив, обмен.
            _ => {
                queue.0.push(Command::StationMoveItem {
                    pos,
                    from: encode_address(drag.from),
                    to: encode_address(target),
                    count: 0,
                });
            }
        }
        station.selected = None;
        return;
    }
    // Слота под курсором нет. Внутри окна — просто отмена (стек на месте),
    // за пределами — бросок в сторону курсора.
    let Some(cursor) = cursor else {
        return;
    };
    let over_panel = panels
        .iter()
        .any(|(computed, transform)| cursor_over_node(window, computed, transform, cursor));
    if over_panel {
        return;
    }
    let Some(world) = state.world.as_ref() else {
        return;
    };
    let Some(tile) = cursor_world_tile(window, cameras, Some(world)) else {
        return;
    };
    throw_station_drag(queue, state, drag.from, tile, pos);
    station.selected = None;
}

/// Бросает стек из слота-источника drag в мир: из misc — сразу командой;
/// из контейнера — сначала перенос в первый свободный слот misc (команды
/// одного кадра применяются по порядку), затем бросок.
fn throw_station_drag(
    queue: &mut CommandQueueRes,
    state: &GameState,
    from: StationAddress,
    target: TilePos,
    pos: TilePos,
) {
    match from {
        StationAddress::Player(i) => {
            queue.0.push(Command::PlayerThrowStack {
                target,
                slot: i as u8,
            });
        }
        StationAddress::Container(index) => {
            let Some(p) = state.controlled_mind() else {
                return;
            };
            let Some(free) =
                (0..items::MISC_SLOTS).find(|&i| p.inventory.items.get(i).copied().flatten().is_none())
            else {
                return;
            };
            queue.0.push(Command::StationMoveItem {
                pos,
                from: (index as u8, 0),
                to: (containers::SLOT_AREA_PLAYER_MISC, free as u8),
                count: 0,
            });
            queue.0.push(Command::PlayerThrowStack {
                target,
                slot: free as u8,
            });
        }
        _ => {}
    }
}

/// Перекладывание цифровыми клавишами: наведение на слот + 1–8 переносит
/// весь стек между слотом misc игрока с этим номером и слотом под курсором
/// (если заняты оба — обмен; правила слотов проверяет симуляция).
pub(crate) fn station_digit_transfer(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<GameState>,
    station: Res<StationUi>,
    mut queue: ResMut<CommandQueueRes>,
    windows: Query<&Window>,
    station_slots: Query<(
        &StationSlot,
        Option<&StationOutputSlot>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    player_slots: Query<(&StationPlayerSlot, &crate::ui::stations::shared::SlotIndex, &ComputedNode, &UiGlobalTransform)>,
) {
    if !station.open {
        return;
    }
    let Some(digit) = DIGIT_KEYS.iter().position(|key| keys.just_pressed(*key)) else {
        return;
    };
    if digit >= items::MISC_SLOTS || state.controlled_mind().is_none() || state.world.is_none() {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Some((target, _)) = hovered_station_address(window, &station_slots, &player_slots, cursor)
    else {
        return;
    };
    let from = StationAddress::Player(digit);
    if target == from {
        return;
    }
    let Some(pos) = station.pos else {
        return;
    };
    // Направление по содержимому: в хотбаре под номером есть предмет —
    // кладём в слот под курсором (при занятых обоих обмен решит симуляция);
    // хотбар пуст, а в слоте под курсором предмет — забираем его в хотбар
    // (повторное нажатие после перекладывания возвращает предмет).
    let source = if stack_at(&station, &state, from).is_some() {
        from
    } else if stack_at(&station, &state, target).is_some() {
        target
    } else {
        return;
    };
    queue.0.push(Command::StationMoveItem {
        pos,
        from: encode_address(source),
        to: encode_address(if source == from { target } else { from }),
        count: 0,
    });
}
