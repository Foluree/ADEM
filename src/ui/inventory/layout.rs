//! Построение окна инвентаря: обёртка, секции слотов (броня/одежда/
//! косметика столбцами слева, misc и сетка крафта справа), мусорка,
//! обновление иконок/счётчиков/рамок по текущему инвентарю.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use crate::simulation::items::{ARMOR_LAYOUT, SlotArea, SlotRef};
use crate::ui::UiFont;
use crate::ui::bottom_panel::{FitDone, FitIcon};
use crate::ui::icons::UiIcons;
use crate::ui::style;
use crate::app::state::GameState;
use crate::ui::BlocksWorldInput;

use super::{
    CraftBookRes, CraftResultSlot, CraftSlot, CraftingUi, ICON_BOX, InventoryPanel, InventoryRoot,
    InventoryUi, InventoryWindowSlot, InvSlot, SLOT_GAP, SLOT_SIZE, SLOTS_PER_ROW,
};

/// Маркер текста-счётчика внутри слота.
#[derive(Component)]
pub(crate) struct SlotCount;

impl SlotArea {
    /// Заголовок секции (мусорка спавнится без заголовка).
    fn title(self) -> &'static str {
        match self {
            SlotArea::Misc => "Предметы",
            SlotArea::Armor => "Броня",
            SlotArea::Clothes => "Одежда",
            SlotArea::Bauble => "Косметика",
            SlotArea::Trash => "",
        }
    }
}

/// Строит окно инвентаря: заголовок, слева столбцы брони и косметики,
/// справа основные слоты и под ними сетка крафта. Полноэкранный wrapper
/// нужен для центрирования; маркер `InventoryRoot` стоит именно на нём,
/// чтобы закрытие окна сносило весь overlay — иначе невидимый full-screen
/// слой остаётся и навсегда перехватывает клики по UI. Маркер блокировки
/// карты (`BlocksWorldInput`) — только на самой панели: карта за пределами
/// окна кликается.
pub(crate) fn spawn_inventory(commands: &mut Commands, font: &UiFont) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            InventoryRoot,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(style::background()),
                    BorderColor::from(style::border()),
                    BlocksWorldInput,
                    InventoryPanel,
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Инвентарь"),
                        TextFont {
                            font: font.0.clone().into(),
                            font_size: 20.0.into(),
                            ..default()
                        },
                        TextColor(style::text()),
                    ));
                    // Слева — броня, одежда и косметика (столбцы), справа —
                    // основные слоты и крафт; фон панели растягивается
                    // под содержимое, панель остаётся по центру экрана.
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(12.0),
                            align_items: AlignItems::FlexStart,
                            ..default()
                        })
                        .with_children(|content| {
                            spawn_section(content, SlotArea::Armor, font, SectionLayout::Column);
                            spawn_section(content, SlotArea::Clothes, font, SectionLayout::Column);
                            // Мусорка завершает колонку косметики снизу.
                            content
                                .spawn(Node {
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                })
                                .with_children(|column| {
                                    spawn_section(
                                        column,
                                        SlotArea::Bauble,
                                        font,
                                        SectionLayout::Column,
                                    );
                                    spawn_trash(column, font);
                                });
                            content
                                .spawn(Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(10.0),
                                    ..default()
                                })
                                .with_children(|right| {
                                    spawn_section(
                                        right,
                                        SlotArea::Misc,
                                        font,
                                        SectionLayout::Rows,
                                    );
                                    spawn_crafting(right, font);
                                });
                        });
                });
        });
}

/// Направление раскладки секции: рядами по `SLOTS_PER_ROW` или столбцом
/// (броня и косметика — вертикальные колонки слева от основных слотов).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SectionLayout {
    Rows,
    Column,
}

/// Секция: заголовок и слоты. Рядами — по SLOTS_PER_ROW в строку,
/// столбцом — по одному слоту в строке.
fn spawn_section(
    panel: &mut ChildSpawnerCommands,
    area: SlotArea,
    font: &UiFont,
    layout: SectionLayout,
) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|block| {
            block.spawn((
                Text::new(area.title()),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
            block
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(SLOT_GAP),
                    ..default()
                })
                .with_children(|rows| {
                    let count = area.slots_count();
                    let per_row = match layout {
                        SectionLayout::Rows => SLOTS_PER_ROW,
                        SectionLayout::Column => 1,
                    };
                    for row_start in (0..count).step_by(per_row) {
                        rows.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(SLOT_GAP),
                            ..default()
                        })
                        .with_children(|row| {
                            for index in row_start..(row_start + per_row).min(count) {
                                spawn_slot(row, area, index, font);
                            }
                        });
                    }
                });
        });
}

/// Мусорка: слот удаления предметов под столбцом косметики, с отступом
/// в пол-высоты слота. Один на всё окно: принимает что угодно, каждая
/// новая кладка затирает прежний стек (тот ещё можно забрать перетаскиванием).
/// Пустой показывает текстуру `trash_can.png`, занятый — иконку предмета.
fn spawn_trash(content: &mut ChildSpawnerCommands, font: &UiFont) {
    content.spawn((
        Button,
        Node {
            width: Val::Px(SLOT_SIZE),
            height: Val::Px(SLOT_SIZE),
            margin: UiRect::top(Val::Px(SLOT_SIZE * 0.5)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::surface()),
        BorderColor::all(style::border()),
        Hovered::default(),
        InvSlot { area: SlotArea::Trash, index: 0 },
        InventoryWindowSlot,
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
            SlotCount,
        ));
    });
}

/// Слот: рамка, пустая иконка (заполнит refresh_slots), счётчик.
/// Кликабелен: `Button`-виджет шлёт `Activate`, который ловит глобальный
/// наблюдатель переноса. В пустых слотах брони иконку-заглушку рисует
/// `refresh_slots` (текстуры `textures/ui/inventory/empty_armor_slot_*`).
fn spawn_slot(
    row: &mut ChildSpawnerCommands,
    area: SlotArea,
    index: usize,
    font: &UiFont,
) {
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
        InvSlot { area, index },
        InventoryWindowSlot,
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
            SlotCount,
        ));
    });
}

/// Обновляет иконки, счётчики и рамки слотов по текущему инвентарю игрока.
/// Пишет только реальные изменения, чтобы не дёргать переразметку.
pub(crate) fn refresh_slots(
    mut commands: Commands,
    inv: Res<InventoryUi>,
    state: Res<GameState>,
    icons: Res<UiIcons>,
    mut slots: Query<(Entity, &InvSlot, &mut BorderColor, &Children), With<InventoryWindowSlot>>,
    mut images: Query<&mut ImageNode>,
    mut texts: Query<&mut Text>,
) {
    if !inv.open {
        return;
    }
    let Some(p) = state.controlled_mind() else {
        return;
    };

    for (_, slot, mut border, children) in &mut slots {
        let stack = p
            .inventory
            .area(slot.area)
            .get(slot.index)
            .copied()
            .flatten();

        // Рамка: выбранный слот-перенос подсвечивается активным цветом.
        let want_border = if inv.selected == Some(SlotRef { area: slot.area, index: slot.index }) {
            style::surface_active()
        } else {
            style::border()
        };
        let want_border = BorderColor::all(want_border);
        if *border != want_border {
            *border = want_border;
        }

        for child in children.iter() {
            // Иконка предмета — дочерний узел слота (не сам слот):
            // обновляем по ссылке на ребёнка через отдельный запрос.
            // Пустой слот брони показывает текстуру-заглушку специализации.
            if let Ok(mut image) = images.get_mut(child) {
                let want_icon = match stack {
                    Some(s) => icons.item(s.kind).clone(),
                    None => match slot.area {
                        SlotArea::Armor => icons
                            .empty_armor_slot(ARMOR_LAYOUT[slot.index])
                            .cloned()
                            .unwrap_or_default(),
                        SlotArea::Trash => icons.trash_can().clone(),
                        _ => ImageNode::default().image,
                    },
                };
                if image.image != want_icon {
                    image.image = want_icon;
                    // Размер иконки мог измениться — вписываем заново.
                    commands.entity(child).remove::<FitDone>();
                }
            }
            // Текстовые дети: счётчик количества (правый нижний угол).
            if let Ok(mut t) = texts.get_mut(child) {
                let want_text = match stack {
                    Some(s) if s.count > 1 => s.count.to_string(),
                    _ => String::new(),
                };
                if t.as_str() != want_text {
                    **t = want_text;
                }
            }
        }
    }
}

/// Меню крафта 3x3 (как в Minecraft): сетка 3x3 и слот результата.
fn spawn_crafting(panel: &mut ChildSpawnerCommands, font: &UiFont) {
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
                    // Сетка 3x3.
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
                                    r.spawn((
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
                                        CraftSlot { index: i },
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
                                            SlotCount,
                                        ));
                                    });
                                }
                            });
                        }
                    });
                    // Слот результата: показывает подобранный рецепт, клик выдаёт предмет.
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
                        CraftResultSlot,
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
                        ));
                    });
                });
        });
}

pub(crate) fn refresh_crafting(
    mut commands: Commands,
    craft: Res<CraftingUi>,
    book: Res<CraftBookRes>,
    mut slots: Query<(&CraftSlot, &mut BorderColor, &Children), Without<CraftResultSlot>>,
    mut results: Query<(&mut BorderColor, &Children), With<CraftResultSlot>>,
    mut images: Query<&mut ImageNode>,
    mut texts: Query<&mut Text>,
    icons: Res<UiIcons>,
) {
    let result = book.0.matching_recipe(&craft.slots).map(|recipe| recipe.result);

    for (slot, mut border, children) in &mut slots {
        let stack = craft.slots[slot.index];
        let selected = craft.selected == Some(slot.index);
        let want_border = if selected {
            style::surface_active()
        } else {
            style::border()
        };
        *border = BorderColor::all(want_border);
        for child in children.iter() {
            if let Ok(mut image) = images.get_mut(child) {
                let want_icon = match stack {
                    Some(s) => icons.item(s.kind).clone(),
                    None => ImageNode::default().image,
                };
                if image.image != want_icon {
                    image.image = want_icon;
                    commands.entity(child).remove::<FitDone>();
                }
            }
            if let Ok(mut t) = texts.get_mut(child) {
                let want_text = stack
                    .map(|s| if s.count > 1 { s.count.to_string() } else { String::new() })
                    .unwrap_or_default();
                if t.as_str() != want_text {
                    **t = want_text;
                }
            }
        }
    }

    // Слот результата: подсвечивается, когда сетка образует рецепт.
    for (mut border, children) in &mut results {
        *border = BorderColor::all(if result.is_some() {
            style::surface_active()
        } else {
            style::border()
        });
        for child in children.iter() {
            if let Ok(mut image) = images.get_mut(child) {
                let want_icon = match result {
                    Some(s) => icons.item(s.kind).clone(),
                    None => ImageNode::default().image,
                };
                if image.image != want_icon {
                    image.image = want_icon;
                    commands.entity(child).remove::<FitDone>();
                }
            }
            if let Ok(mut t) = texts.get_mut(child) {
                let want_text = result
                    .map(|s| if s.count > 1 { s.count.to_string() } else { String::new() })
                    .unwrap_or_default();
                if t.as_str() != want_text {
                    **t = want_text;
                }
            }
        }
    }
}
