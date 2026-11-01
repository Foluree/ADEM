//! Хотбар (строка слотов MISC) в режиме игрока: показывается внизу экрана
//! по центру, как в Minecraft. Выбор слота — цифрами 1..8, Q/E циклически.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::picking::Pickable;
use bevy::ui_widgets::TextInput;

use crate::app::settings::{ControlAction, Settings};
use crate::app::state::{GameMode, GameState, PlayerRes};
use bevy::ui_widgets::Button;
use crate::simulation::items::{MISC_SLOTS, SlotArea};
use crate::ui::inventory::{InventoryUi, InvSlot};
use crate::ui::text_input_focused;
use crate::ui::UiFont;
use crate::ui::icons::UiIcons;
use crate::ui::style;
use crate::ui::BlocksWorldInput;

pub struct HotbarPlugin;

/// Размеры слотов хотбара, px. На ~10% крупнее исходных (44/32/4):
/// слот, иконка и зазор масштабируются вместе, счётчик стека — тоже.
const SLOT_SIZE: f32 = 48.4;
const ICON_SIZE: f32 = 35.2;
const SLOT_GAP: f32 = 4.4;
const COUNT_FONT_SIZE: f32 = 13.2;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hotbar)
            .add_systems(
                Update,
                (
                    toggle_hotbar_visibility,
                    handle_hotbar_selection,
                    refresh_hotbar,
                ),
            );
    }
}

#[derive(Component)]
pub struct HotbarRoot;

#[derive(Component)]
struct HotbarSlot {
    index: usize,
}

#[derive(Component)]
struct HotbarIcon;

#[derive(Component)]
struct HotbarCount;

fn spawn_hotbar(mut commands: Commands, font: Res<UiFont>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                // Отступ от нижнего края — 5% высоты экрана (не фиксированные
                // пиксели: положение держится при смене размера окна).
                bottom: Val::Percent(4.0),
                left: Val::Percent(0.0),
                right: Val::Percent(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            HotbarRoot,
            Pickable::default(),
        ))
        .with_children(|root| {
            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(SLOT_GAP),
                ..default()
            })
            .with_children(|row| {
                for index in 0..MISC_SLOTS {
                    row.spawn((
                        Button,
                        Node {
                            width: Val::Px(SLOT_SIZE),
                            height: Val::Px(SLOT_SIZE),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(style::surface()),
                        BorderColor::all(style::border()),
                        HotbarSlot { index },
                        InvSlot { area: SlotArea::Misc, index },
                        BlocksWorldInput,
                    ))
                    .with_children(|slot| {
                        slot.spawn((
                            ImageNode::default(),
                            Node {
                                width: Val::Px(ICON_SIZE),
                                height: Val::Px(ICON_SIZE),
                                ..default()
                            },
                            HotbarIcon,
                        ));
                        slot.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(4.4),
                                bottom: Val::Px(2.2),
                                ..default()
                            },
                            Text::new(String::new()),
                            TextFont {
                                font: font.0.clone().into(),
                                font_size: COUNT_FONT_SIZE.into(),
                                ..default()
                            },
                            TextColor(style::text()),
                            HotbarCount,
                        ));
                    });
                }
            });
        });
}

fn toggle_hotbar_visibility(
    state: Res<GameState>,
    mut roots: Query<(&mut Node, &mut Pickable), With<HotbarRoot>>,
) {
    for (mut node, mut pickable) in &mut roots {
        if state.mode == GameMode::Player {
            node.display = Display::Flex;
            *pickable = Pickable::default();
        } else {
            node.display = Display::None;
            pickable.should_block_lower = false;
            pickable.is_hoverable = false;
        }
    }
}

fn handle_hotbar_selection(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<Settings>,
    state: Res<GameState>,
    inv: Res<InventoryUi>,
    station: Res<crate::ui::station::StationUi>,
    mut player: ResMut<PlayerRes>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
    capture: Res<crate::ui::pause_menu::BindingCapture>,
) {
    if state.mode != GameMode::Player {
        return;
    }
    if capture.0.is_some() || text_input_focused(&focus, &text_inputs) {
        return;
    }
    // Пока открыто окно инвентаря или станции, цифры 1–8 принадлежат GUI
    // (перекладывание предметов по номеру слота), а не выбору слота
    // хотбара — иначе выбор прыгал бы под руками. Q/E работают всегда.
    let gui_open = inv.open
        || station.open
        || state.active_menu == Some(crate::app::state::MenuKind::Creative);
    let inventory_key = settings.controls.key(ControlAction::Inventory);
    let Some(p) = player.0.as_mut() else {
        return;
    };
    // Цифры 1..8.
    for digit in 0..MISC_SLOTS {
        let code = match digit {
            0 => KeyCode::Digit1,
            1 => KeyCode::Digit2,
            2 => KeyCode::Digit3,
            3 => KeyCode::Digit4,
            4 => KeyCode::Digit5,
            5 => KeyCode::Digit6,
            6 => KeyCode::Digit7,
            7 => KeyCode::Digit8,
            _ => continue,
        };
        // Клавиша открытия инвентаря может совпадать с горячей клавишей
        // хотбара (по умолчанию это не так, но бинд настраивается): в этом
        // кадре она принадлежит инвентарю.
        if keys.just_pressed(code) && code != inventory_key && !gui_open {
            p.selected_slot = digit;
        }
    }
    if keys.just_pressed(KeyCode::KeyQ) && inventory_key != KeyCode::KeyQ {
        if p.selected_slot == 0 {
            p.selected_slot = MISC_SLOTS - 1;
        } else {
            p.selected_slot -= 1;
        }
    }
    if keys.just_pressed(KeyCode::KeyE) && inventory_key != KeyCode::KeyE {
        if p.selected_slot + 1 >= MISC_SLOTS {
            p.selected_slot = 0;
        } else {
            p.selected_slot += 1;
        }
    }
}

fn refresh_hotbar(
    state: Res<GameState>,
    player: Res<PlayerRes>,
    icons: Res<UiIcons>,
    mut slots: Query<(&HotbarSlot, &mut BorderColor, &mut BackgroundColor, &Children)>,
    mut images: Query<&mut ImageNode>,
    mut texts: Query<(&mut Text, Option<&HotbarCount>)>,
) {
    if state.mode != GameMode::Player {
        return;
    }
    let Some(p) = player.0.as_ref() else {
        return;
    };
    for (slot, mut border, mut bg, children) in &mut slots {
        let stack = p.inventory.items.get(slot.index).copied().flatten();
        let selected = p.selected_slot == slot.index;
        let want_border = if selected {
            style::surface_active()
        } else {
            style::border()
        };
        border.set_if_neq(BorderColor::all(want_border));
        // Пустые слоты чуть темнее.
        bg.set_if_neq(BackgroundColor(if stack.is_none() {
            style::surface_disabled()
        } else {
            style::surface()
        }));
        for child in children.iter() {
            if let Ok(mut image) = images.get_mut(child) {
                match stack {
                    Some(s) => {
                        let want_icon = icons.item(s.kind).clone();
                        if image.image != want_icon {
                            image.image = want_icon;
                        }
                        image.color = Color::WHITE;
                    }
                    None => {
                        image.image = Handle::default();
                        image.color = Color::NONE;
                    }
                }
            }
            if let Ok((t, _)) = texts.get_mut(child) {
                let want_text = stack
                    .map(|s| if s.count > 1 { s.count.to_string() } else { String::new() })
                    .unwrap_or_default();
                crate::ui::set_text_if_neq(t, &want_text);
            }
        }
    }
}
