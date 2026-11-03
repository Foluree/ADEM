//! Логика сетки крафта 3×3: клики по клеткам и слоту результата, заполнение
//! зажатыми кнопками, сброс сетки при закрытии окна. Раскладку клеток
//! строит `layout.rs`, состояние живёт в `CraftingUi` (`mod.rs`).

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::app::state::{CommandQueueRes, GameState};
use crate::simulation::commands::Command;
use crate::simulation::items::SlotRef;
use crate::simulation::ItemStack;
use crate::simulation::TilePos;
use crate::ui::cursor_over_node;

use super::{CraftBookRes, CraftResultSlot, CraftSlot, CraftingUi, InventoryUi, InvSlot};

/// Состояние заполнения сетки крафта зажатыми кнопками: последний слот,
/// посещённый каждой кнопкой, и слот, обработанный текущим зажатием.
/// `filled_*` нужен наблюдателю активации: отпускание кнопки над слотом,
/// который заполнился при перемещении, не должно срабатывать второй раз.
#[derive(Resource, Default, Debug)]
pub(crate) struct CraftHoldFill {
    last_lmb: Option<usize>,
    last_rmb: Option<usize>,
    filled_lmb: Option<usize>,
    filled_rmb: Option<usize>,
}

/// Активация слота инвентаря (Button-виджет). Клик по слоту только
/// выбирает стек-источник для укладки в сетку крафта (повторный клик
/// снимает выбор). Перенос между слотами инвентаря — перетаскиванием
/// (`inventory_drag`), поэтому здесь нет логики перемещения.
pub(crate) fn on_slot_activated(
    activate: On<Activate>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut inv: ResMut<InventoryUi>,
    state: Res<GameState>,
    slots: Query<&InvSlot>,
) {
    // Активация приходит на клик любой кнопкой мыши; слоты инвентаря
    // реагируют только на ЛКМ.
    if mouse.just_released(MouseButton::Right) || mouse.just_released(MouseButton::Middle) {
        return;
    }
    // Слоты хотбара — тоже `InvSlot`, но выбор источника для крафта
    // осмыслен только при открытом окне.
    if !inv.open {
        return;
    }
    let Ok(clicked) = slots.get(activate.entity) else {
        return;
    };
    let target = SlotRef { area: clicked.area, index: clicked.index };
    let Some(p) = state.controlled_mind() else {
        return;
    };
    // Под зажатой ЛКМ стек уже снят из слота в «руку» (`inventory_drag`),
    // а `Activate` приходит раньше, чем `Update` вернёт его на место.
    // Поэтому слот считается непустым и в этом случае.
    let held_here = inv.drag.as_ref().is_some_and(|drag| drag.from == target);

    match inv.selected {
        // Повторный клик по выбранному слоту снимает выбор.
        Some(from) if from == target => inv.selected = None,
        // Новый источник: только непустой слот.
        _ if held_here || p.inventory.get(target).is_some() => inv.selected = Some(target),
        _ => {}
    }
}

/// Списывает `count` предметов из клетки сетки крафта. Опустевшая клетка
/// освобождается; если на неё указывал выбор в сетке — выбор снимается.
fn take_from_craft_cell(craft: &mut CraftingUi, index: usize, count: u32) {
    if let Some(cell) = craft.slots[index].as_mut() {
        cell.count = cell.count.saturating_sub(count);
        if cell.count == 0 {
            craft.slots[index] = None;
            if craft.selected == Some(index) {
                craft.selected = None;
            }
        }
    }
}

/// Кладёт один предмет из выбранного слота инвентаря в клетку сетки.
/// Общая логика клика и заполнения зажатием. `false` — класть нечем
/// или клетка не принимает этот предмет.
fn place_one_into_craft_cell(
    inv: &mut InventoryUi,
    craft: &mut CraftingUi,
    state: &mut GameState,
    target: usize,
) -> bool {
    let Some(src) = inv.selected else {
        return false;
    };
    let Some(p) = state.controlled_mind_mut() else {
        return false;
    };
    let Some(stack) = p.inventory.get(src) else {
        inv.selected = None;
        return false;
    };
    let kind = stack.kind;
    let can_place = match craft.slots[target] {
        None => true,
        Some(existing) => existing.kind == kind && existing.count < kind.max_stack(),
    };
    if !can_place {
        return false;
    }

    // Списываем одну единицу из слота инвентаря.
    let mut emptied = false;
    if let Some(cell) = p.inventory.area_mut(src.area).get_mut(src.index)
        && let Some(s) = cell
    {
        s.count -= 1;
        emptied = s.count == 0;
        if emptied {
            *cell = None;
        }
    }
    match &mut craft.slots[target] {
        Some(existing) => existing.count += 1,
        None => craft.slots[target] = Some(ItemStack { kind, count: 1 }),
    }
    if emptied {
        inv.selected = None;
    }
    true
}

/// Забирает один предмет из клетки сетки обратно в инвентарь.
/// `false` — клетка пуста или в инвентаре нет места.
fn take_one_from_craft_cell(craft: &mut CraftingUi, state: &mut GameState, target: usize) -> bool {
    let Some(stack) = craft.slots[target] else {
        return false;
    };
    let Some(p) = state.controlled_mind_mut() else {
        return false;
    };
    if !p.inventory.can_fit(stack.kind, 1) {
        return false;
    }
    p.inventory.add(stack.kind, 1);
    take_from_craft_cell(craft, target, 1);
    true
}

/// Активация клетки сетки крафта. ЛКМ — доложить один предмет из
/// выбранного слота инвентаря (или переложить один между клетками, или
/// выбрать клетку), ПКМ — забрать один предмет обратно в инвентарь,
/// Shift+ЛКМ — вернуть весь слот в инвентарь целиком. Удержание кнопки
/// при перемещении по клеткам обрабатывает `craft_grid_hold_fill`.
pub(crate) fn on_craft_slot_activated(
    activate: On<Activate>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut inv: ResMut<InventoryUi>,
    mut craft: ResMut<CraftingUi>,
    hold: Res<CraftHoldFill>,
    mut state: ResMut<GameState>,
    slots: Query<&CraftSlot>,
) {
    let Ok(clicked) = slots.get(activate.entity) else {
        return;
    };
    let target = clicked.index;
    // Активация приходит на клик любой кнопкой мыши; средняя в сетке
    // крафта не используется.
    if mouse.just_released(MouseButton::Middle) {
        return;
    }
    // Слот, уже обработанный текущим зажатием (заполнение при перемещении
    // по сетке), не срабатывает второй раз на отпускание кнопки.
    if mouse.just_released(MouseButton::Left) && hold.filled_lmb == Some(target) {
        return;
    }
    if mouse.just_released(MouseButton::Right) && hold.filled_rmb == Some(target) {
        return;
    }
    let Some(p) = state.controlled_mind_mut() else {
        return;
    };

    // ПКМ по клетке: забрать один предмет в инвентарь. Места нет — клетка
    // не тратится (предмет не должен исчезать «в никуда»).
    if mouse.just_released(MouseButton::Right) {
        take_one_from_craft_cell(&mut craft, &mut state, target);
        return;
    }

    // Shift+ЛКМ по клетке: вернуть весь слот в инвентарь. `add` доливает
    // в неполные стеки и занимает пустые слоты; не поместившейся остаток
    // остаётся в клетке сетки.
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        let Some(stack) = craft.slots[target] else {
            return;
        };
        let left = p.inventory.add(stack.kind, stack.count);
        take_from_craft_cell(&mut craft, target, stack.count - left);
        return;
    }

    // Если выбран источник в инвентаре — кладём в крафт-слот по одному
    // предмету за клик (клетка не принимает — клик просто ничего не делает).
    // Источник остаётся выбранным, пока стек не опустеет: так удобно
    // докладывать несколько единиц подряд.
    if inv.selected.is_some() {
        place_one_into_craft_cell(&mut inv, &mut craft, &mut state, target);
        return;
    }

    // Если выбран источник в крафте — переносим между ячейками по одному
    // предмету (кроме обмена разными видами: тут меняются стеки целиком).
    if let Some(src) = craft.selected {
        if src == target {
            craft.selected = None;
            return;
        }
        let source = craft.slots[src];
        let dest = craft.slots[target];
        match (source, dest) {
            (Some(s), None) => {
                craft.slots[target] = Some(ItemStack { kind: s.kind, count: 1 });
                if let Some(cell) = craft.slots[src].as_mut() {
                    cell.count -= 1;
                    if cell.count == 0 {
                        craft.slots[src] = None;
                        craft.selected = None;
                    }
                }
            }
            (Some(s), Some(d)) if s.kind == d.kind && d.count < d.kind.max_stack() => {
                if let Some(cell) = craft.slots[target].as_mut() {
                    cell.count += 1;
                }
                if let Some(cell) = craft.slots[src].as_mut() {
                    cell.count -= 1;
                    if cell.count == 0 {
                        craft.slots[src] = None;
                        craft.selected = None;
                    }
                }
            }
            (Some(s), Some(d)) if s.kind != d.kind => {
                // обмен разными предметами
                craft.slots[target] = Some(s);
                craft.slots[src] = Some(d);
                craft.selected = None;
            }
            _ => {}
        }
        return;
    }

    // Выбор источника в крафте.
    if craft.slots[target].is_some() {
        craft.selected = Some(target);
    }
}

/// Заполнение сетки крафта зажатыми кнопками: пока ЛКМ/ПКМ удерживается,
/// каждый новый слот, на который переведён курсор, получает один предмет
/// (ЛКМ — из выбранного слота инвентаря, ПКМ — отдаёт один предмет
/// обратно в инвентарь). Само нажатие ничего не кладёт: обычный клик
/// обрабатывает наблюдатель активации, иначе предмет задваивался бы.
pub(crate) fn craft_grid_hold_fill(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    mut inv: ResMut<InventoryUi>,
    mut craft: ResMut<CraftingUi>,
    mut hold: ResMut<CraftHoldFill>,
    mut state: ResMut<GameState>,
    slots: Query<(&CraftSlot, &ComputedNode, &UiGlobalTransform)>,
) {
    // Кнопка отпущена: следующее нажатие начинается с чистого листа.
    if !mouse.pressed(MouseButton::Left) {
        hold.last_lmb = None;
    }
    if !mouse.pressed(MouseButton::Right) {
        hold.last_rmb = None;
    }
    if !inv.open || inv.drag.is_some() {
        return;
    }
    // Shift+ЛКМ по клетке — вернуть слот целиком (наблюдатель активации);
    // заполнение зажатием с Shift не смешиваем.
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let hovered = hovered_craft_slot(window, &slots, cursor);

    // Кадр нажатия только фиксирует стартовый слот: клик без движения
    // целиком остаётся за наблюдателем активации.
    if mouse.just_pressed(MouseButton::Left) {
        hold.last_lmb = hovered;
        hold.filled_lmb = None;
    } else if mouse.pressed(MouseButton::Left) && hovered != hold.last_lmb {
        hold.last_lmb = hovered;
        if let Some(target) = hovered {
            hold.filled_lmb = Some(target);
            place_one_into_craft_cell(&mut inv, &mut craft, &mut state, target);
        }
    }
    if mouse.just_pressed(MouseButton::Right) {
        hold.last_rmb = hovered;
        hold.filled_rmb = None;
    } else if mouse.pressed(MouseButton::Right) && hovered != hold.last_rmb {
        hold.last_rmb = hovered;
        if let Some(target) = hovered {
            hold.filled_rmb = Some(target);
            take_one_from_craft_cell(&mut craft, &mut state, target);
        }
    }
}

/// Клетка сетки крафта под курсором, если есть. Хит-тест общий с другими
/// проверками UI (`cursor_over_node`).
fn hovered_craft_slot(
    window: &Window,
    slots: &Query<(&CraftSlot, &ComputedNode, &UiGlobalTransform)>,
    cursor: Vec2,
) -> Option<usize> {
    slots
        .iter()
        .find(|(_, computed, transform)| cursor_over_node(window, computed, transform, cursor))
        .map(|(slot, _, _)| slot.index)
}

/// Клик по слоту результата: если сетка образует рецепт и результат
/// помещается в инвентарь, выдаёт предмет и списывает ингредиенты.
/// Shift+ЛКМ — скрафтить максимум доступного (с добором ингредиентов из
/// инвентаря), Shift+ПКМ — половину максимума с округлением вниз (но не
/// меньше одного крафта, если хоть один возможен).
pub(crate) fn on_result_activated(
    activate: On<Activate>,
    results: Query<(), With<CraftResultSlot>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    inv: Res<InventoryUi>,
    mut craft: ResMut<CraftingUi>,
    book: Res<CraftBookRes>,
    mut state: ResMut<GameState>,
) {
    // `Activate` — глобальное событие UI: его шлёт любая кнопка (слоты
    // инвентаря и сетки, кнопки нижней панели). Крафт должен срабатывать
    // только от нажатия на слот результата при открытом окне, иначе
    // последний уложенный в сетку ингредиент «сам» создавал предмет.
    if !inv.open || !results.contains(activate.entity) {
        return;
    }
    let left_click = mouse.just_released(MouseButton::Left);
    let right_click = mouse.just_released(MouseButton::Right);
    if !left_click && !right_click {
        return;
    }
    let Some(p) = state.controlled_mind_mut() else {
        return;
    };
    let Some(recipe) = book.0.matching_recipe(&craft.slots).cloned() else {
        return;
    };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if shift {
        let max = recipe.max_crafts(&craft.slots, &p.inventory);
        // SHIFT+ПКМ — половина максимума; при max = 1 всё равно один крафт,
        // иначе кнопка выглядела бы «сломанной» при единственном доступном.
        let crafts = if right_click {
            if max > 0 { (max / 2).max(1) } else { 0 }
        } else {
            max
        };
        for _ in 0..crafts {
            if !recipe.try_craft_one(&mut craft.slots, &mut p.inventory) {
                break;
            }
        }
        craft.selected = None;
        return;
    }

    // Обычный клик — ровно один крафт. ПКМ без Shift ничего не делает.
    if !left_click {
        return;
    }
    recipe.try_craft_one(&mut craft.slots, &mut p.inventory);
    craft.selected = None;
}

/// Сброс сетки крафта при закрытом окне инвентаря: компоненты возвращаются
/// в инвентарь (`add` доливает в неполные стеки и занимает пустые слоты),
/// а то, что не влезло, сбрасывается на землю рядом с игроком командой
/// симуляции — UI не пишет в мир напрямую. Перетаскивание стека сюда не
/// входит: хотбар работает и при закрытом окне, а бросок драга разбирает
/// `inventory_drag`.
pub(crate) fn reset_crafting_on_close(
    mut inv: ResMut<InventoryUi>,
    mut craft: ResMut<CraftingUi>,
    mut queue: ResMut<CommandQueueRes>,
    mut state: ResMut<GameState>,
) {
    if inv.open {
        // Окно открыто: сетка живёт, перетаскивание активно.
        return;
    }
    // Окно закрыто: выбора больше нет.
    inv.selected = None;
    craft.selected = None;
    // Сетка пуста — делать нечего (обычный случай каждого кадра).
    if craft.slots.iter().all(|cell| cell.is_none()) {
        return;
    }

    // Центр сброса остатков — тайл существа; зажимаем в границы мира.
    let Some(actor) = state.controlled else {
        // Захвата нет — сетку очищаем: вернуть содержимое некуда.
        craft.slots = Default::default();
        return;
    };
    let center = state.world.as_ref().map(|world| {
        let size = world.size.tiles() as i32;
        let pos = world
            .entities
            .get(actor)
            .map(|e| e.pos)
            .unwrap_or_default();
        TilePos {
            x: (pos.x.floor() as i32).clamp(0, size - 1),
            y: (pos.y.floor() as i32).clamp(0, size - 1),
        }
    });
    let Some(center) = center else {
        return;
    };
    let Some(p) = state.controlled_mind_mut() else {
        craft.slots = Default::default();
        return;
    };

    for cell in craft.slots.iter_mut() {
        let Some(stack) = *cell else {
            continue;
        };
        let left = p.inventory.add(stack.kind, stack.count);
        if left > 0 {
            queue.0.push(Command::PlayerDropStack {
                pos: center,
                stack: ItemStack { kind: stack.kind, count: left },
            });
        }
        *cell = None;
    }
}
