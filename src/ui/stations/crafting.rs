//! Сетка крафта станции 3×3 и кузнечный стол: временные UI-ячейки
//! (возвращаются в инвентарь при закрытии окна), подбор рецепта по
//! категориям станции, слот результата с массовым крафтом.

use bevy::input::mouse::MouseButton;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use crate::app::state::GameState;
use crate::simulation::items::{ItemKind, SlotArea, SlotRef, ItemStack};
use crate::simulation::objects::{BuildingKind, ObjectKind};
use crate::ui::bottom_panel::FitIcon;
use crate::ui::inventory::{ICON_BOX, SLOT_GAP, SLOT_SIZE};
use crate::ui::stations::shared::{
    spawn_addressed_slot, StationCraftSlot, StationResultSlot, StationSlotCount, StationToolSlot,
};
use crate::ui::style;
use crate::ui::{CraftBookRes, UiFont};

use super::{StationAddress, StationUi};

/// Категории рецептов станции (верстак наследует ручные).
pub fn station_categories(kind: ObjectKind) -> &'static [&'static str] {
    let Some(kind) = kind.building_kind() else {
        return &[];
    };
    match kind {
        BuildingKind::CraftingTable => &["manual", "crafting_table"],
        BuildingKind::SmithingTable => &["smithing_table"],
        _ => &[],
    }
}

/// Слоты инструментов кузнечного стола (ряд из четырёх).
pub(crate) fn spawn_smithing_tools(panel: &mut ChildSpawnerCommands, font: &UiFont) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|block| {
            block.spawn((
                Text::new("Инструменты"),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
            block
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(SLOT_GAP),
                    ..default()
                })
                .with_children(|row| {
                    for index in 0..4 {
                        spawn_addressed_slot::<StationToolSlot>(row, index, font);
                    }
                });
        });
}

/// Сетка крафта станции 3×3 + слот результата.
pub(crate) fn spawn_station_crafting(panel: &mut ChildSpawnerCommands, font: &UiFont) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|block| {
            block.spawn((
                Text::new("Крафт"),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
            block
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(12.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(SLOT_GAP),
                        ..default()
                    })
                    .with_children(|grid| {
                        let mut idx = 0usize;
                        for _ in 0..3 {
                            grid.spawn(Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(SLOT_GAP),
                                ..default()
                            })
                            .with_children(|r| {
                                for _ in 0..3 {
                                    let i = idx;
                                    idx += 1;
                                    spawn_addressed_slot::<StationCraftSlot>(r, i, font);
                                }
                            });
                        }
                    });
                    spawn_station_result(row, font);
                });
        });
}

/// Слот результата станции.
fn spawn_station_result(row: &mut ChildSpawnerCommands, font: &UiFont) {
    row.spawn((
        Button,
        Node {
            width: Val::Px(SLOT_SIZE),
            height: Val::Px(SLOT_SIZE),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::surface()),
        BorderColor::all(style::border()),
        Hovered::default(),
        StationResultSlot,
    ))
    .with_children(|slot| {
        slot.spawn((ImageNode::default(), Node::default()))
            .insert(FitIcon { box_px: ICON_BOX });
        slot.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(3.0),
                bottom: Val::Px(2.0),
                ..default()
            },
            Text::new(String::new()),
            TextFont {
                font: font.0.clone().into(),
                font_size: 12.0.into(),
                ..default()
            },
            TextColor(style::text()),
            StationSlotCount,
        ));
    });
}

/// Активация клетки сетки крафта станции: ЛКМ кладёт один предмет из
/// выбранного misc-слота, ПКМ забирает один, Shift+ЛКМ возвращает весь.
pub(crate) fn on_station_craft_slot_activated(
    activate: On<Activate>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut station: ResMut<StationUi>,
    mut state: ResMut<GameState>,
    slots: Query<(&StationCraftSlot, &crate::ui::stations::shared::SlotIndex)>,
) {
    let Ok((_, index)) = slots.get(activate.entity) else {
        return;
    };
    let target = index.0;
    if mouse.just_released(MouseButton::Middle) {
        return;
    }
    let Some(p) = state.controlled_mind_mut() else {
        return;
    };

    // ПКМ: забрать один предмет обратно в инвентарь.
    if mouse.just_released(MouseButton::Right) {
        if let Some(stack) = station.grid[target] && p.inventory.can_fit(stack.kind, 1) {
            p.inventory.add(stack.kind, 1);
            take_from_station_cell(&mut station.grid, target, 1);
        }
        return;
    }
    // Shift+ЛКМ: вернуть весь слот.
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        if let Some(stack) = station.grid[target] {
            let left = p.inventory.add(stack.kind, stack.count);
            take_from_station_cell(&mut station.grid, target, stack.count - left);
        }
        return;
    }
    // ЛКМ: перенос по одному из выбранного misc-слота (как в окне инвентаря).
    if let Some(StationAddress::Player(src)) = station.selected {
        let src_ref = SlotRef { area: SlotArea::Misc, index: src };
        let Some(stack) = p.inventory.get(src_ref) else {
            station.selected = None;
            return;
        };
        let kind = stack.kind;
        let can_place = match station.grid[target] {
            None => true,
            Some(existing) => existing.kind == kind && existing.count < kind.max_stack(),
        };
        if !can_place {
            return;
        }
        let mut emptied = false;
        if let Some(cell) = p.inventory.items.get_mut(src) && let Some(s) = cell {
            s.count -= 1;
            emptied = s.count == 0;
            if emptied {
                *cell = None;
            }
        }
        match &mut station.grid[target] {
            Some(existing) => existing.count += 1,
            None => station.grid[target] = Some(ItemStack { kind, count: 1 }),
        }
        if emptied {
            station.selected = None;
        }
        return;
    }
    // Выбор источника в сетке.
    if station.grid[target].is_some() {
        station.selected = Some(StationAddress::Grid(target));
    }
}

/// Списывает `count` предметов из клетки сетки станции.
fn take_from_station_cell(cells: &mut [Option<ItemStack>], index: usize, count: u32) {
    if let Some(cell) = cells.get_mut(index)
        && let Some(stack) = cell
    {
        stack.count = stack.count.saturating_sub(count);
        if stack.count == 0 {
            *cell = None;
        }
    }
}

/// Клик по слоту результата верстака/кузнечного стола: один крафт,
/// Shift+ЛКМ — максимум с добором ингредиентов из инвентаря.
pub(crate) fn on_station_result_activated(
    activate: On<Activate>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut station: ResMut<StationUi>,
    book: Res<CraftBookRes>,
    mut state: ResMut<GameState>,
    results: Query<(), With<StationResultSlot>>,
) {
    if !results.contains(activate.entity) {
        return;
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some(kind) = station.kind else {
        return;
    };
    let Some(p) = state.controlled_mind_mut() else {
        return;
    };
    let held_tools: Vec<ItemKind> = station.tools.iter().flatten().map(|s| s.kind).collect();
    let Some(recipe) = book
        .0
        .matching_recipe_for(station_categories(kind), &station.grid, &held_tools)
        .cloned()
    else {
        return;
    };
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        let max = recipe.max_crafts(&station.grid, &p.inventory);
        for _ in 0..max {
            if !recipe.try_craft_one(&mut station.grid, &mut p.inventory) {
                break;
            }
        }
        return;
    }
    recipe.try_craft_one(&mut station.grid, &mut p.inventory);
}
