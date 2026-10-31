//! Меню строительства (бинд «Меню строительства», по умолчанию C).
//! Работает только в режиме игрока: выбор постройки из списка, после чего
//! ПКМ по карте ставит команду строительства — симуляция сама спишет
//! плату и проверит тайл. Пока постройка выбрана, рендер рисует на курсоре
//! голограмму (см. `rendering`), зелёную на допустимом тайле и красную на
//! недопустимом, а окно меню скрывается — место установки ищется без меню
//! посреди экрана. ESC или C снимают выбор (окно снова видно), повторные —
//! закрывают меню.

use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, TextInput};

use crate::app::input::cursor_world_tile;
use crate::app::settings::{ControlAction, Settings};
use crate::app::state::{CommandQueueRes, GameMode, GameState, PlayerRes};
use crate::simulation::commands::Command;
use crate::simulation::objects::{ObjectKind, build_cost};
use crate::ui::UiFont;
use crate::ui::bottom_panel::FitIcon;
use crate::ui::icons::UiIcons;
use crate::ui::pause_menu::BindingCapture;
use crate::ui::style;
use crate::ui::text_input_focused;
use crate::ui::tooltip::ItemNames;
use crate::ui::BlocksWorldInput;

pub struct ConstructionPlugin;

impl Plugin for ConstructionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConstructionUi>()
            .add_systems(
                Update,
                (
                    toggle_construction,
                    // Закрытие по смене режима — до ввода размещения,
                    // чтобы выбор не пережил выход из режима игрока.
                    close_on_mode_change,
                    update_panel_visibility,
                    construction_place_input,
                    refresh_entries,
                )
                    .chain(),
            )
            .add_observer(on_entry_activated);
    }
}

/// Состояние меню строительства: открыто ли окно и какая постройка
/// выбрана (голограмма и клик по карте работают только с выбором).
#[derive(Resource, Default, Debug)]
pub struct ConstructionUi {
    pub open: bool,
    pub selected: Option<ObjectKind>,
}

impl ConstructionUi {
    /// Выбрана ли постройка в открытом меню: ПКМ по карте в режиме игрока
    /// строит, а не подбирает предметы и яблоки. Читается вводом
    /// (`app/input.rs`).
    pub(crate) fn building_selected(&self) -> bool {
        self.open && self.selected.is_some()
    }
}

/// Маркер корня окна (сносится целиком при закрытии). Виден ESC-цепочке пауз-меню.
#[derive(Component)]
pub(crate) struct ConstructionRoot;

/// Маркер панели со списком построек. При выбранной постройке панель
/// скрывается (`Visibility::Hidden`): голограмма показывает место
/// установки без меню посреди экрана. Скрытая панель не перехватывает и
/// клики по карте — `cursor_over_ui` пропускает скрытые узлы.
#[derive(Component)]
struct ConstructionPanel;

/// Строка постройки в меню.
#[derive(Component)]
struct BuildEntry(ObjectKind);

/// Тогл окна по бинду. Как окно инвентаря: модальные меню (пауза,
/// технологии) перекрывают, захват бинда и текстовый ввод отключают тогл.
fn toggle_construction(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<Settings>,
    mut construction: ResMut<ConstructionUi>,
    state: Res<GameState>,
    player: Res<PlayerRes>,
    capture: Res<BindingCapture>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
    font: Res<UiFont>,
    icons: Res<UiIcons>,
    names: Res<ItemNames>,
    roots: Query<Entity, With<ConstructionRoot>>,
) {
    if text_input_focused(&focus, &text_inputs) || capture.0.is_some() || state.active_menu.is_some()
    {
        return;
    }
    if !keys.just_pressed(settings.controls.key(ControlAction::ConstructionMenu)) {
        return;
    }
    // Меню строительства — инструмент игрока: в наблюдателе не открывается.
    if state.mode != GameMode::Player || player.0.is_none() {
        return;
    }

    // Повторный бинд при выбранной постройке — отмена строительства:
    // выбор сбрасывается, окно снова видно (узлы не пересоздаются).
    if construction.open && construction.selected.is_some() {
        construction.selected = None;
        return;
    }

    construction.open = !construction.open;
    construction.selected = None;
    for e in &roots {
        commands.entity(e).despawn();
    }
    if construction.open {
        spawn_window(&mut commands, &font, &icons, &names);
    }
}

/// Уход из режима игрока (парковка существа) закрывает меню: без существа
/// строить нечем, а висящее окно вводило бы в заблуждение.
fn close_on_mode_change(
    mut commands: Commands,
    state: Res<GameState>,
    player: Res<PlayerRes>,
    mut construction: ResMut<ConstructionUi>,
    roots: Query<Entity, With<ConstructionRoot>>,
) {
    if !construction.open {
        return;
    }
    if state.mode == GameMode::Player && player.0.is_some() {
        return;
    }
    construction.open = false;
    construction.selected = None;
    for e in &roots {
        commands.entity(e).despawn();
    }
}

/// Скрывает панель меню при выбранной постройке и возвращает её, когда
/// выбор снят (ESC/C или повторный клик). Состояние `open` не меняется:
/// голограмма и ПКМ-строительство продолжают работать.
fn update_panel_visibility(
    construction: Res<ConstructionUi>,
    mut panels: Query<&mut Visibility, With<ConstructionPanel>>,
) {
    if !construction.open {
        return;
    }
    let want = if construction.selected.is_some() {
        Visibility::Hidden
    } else {
        Visibility::Visible
    };
    for mut visibility in &mut panels {
        if *visibility != want {
            *visibility = want;
        }
    }
}

/// Окно меню: заголовок, подсказка и список построек с ценами.
/// Полноэкранный wrapper — для центрирования; `BlocksWorldInput` стоит
/// только на панели, карта за пределами окна кликается.
fn spawn_window(
    commands: &mut Commands,
    font: &UiFont,
    icons: &UiIcons,
    names: &ItemNames,
) {
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
            ConstructionRoot,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(style::background()),
                    BorderColor::from(style::border()),
                    BlocksWorldInput,
                    ConstructionPanel,
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Строительство"),
                        TextFont {
                            font: font.0.clone().into(),
                            font_size: 20.0.into(),
                            ..default()
                        },
                        TextColor(style::text()),
                    ));
                    panel.spawn((
                        Text::new("ПКМ по карте — построить, ESC или C — отмена"),
                        TextFont {
                            font: font.0.clone().into(),
                            font_size: 11.0.into(),
                            ..default()
                        },
                        TextColor(style::text_dim()),
                    ));
                    for kind in ObjectKind::BUILDINGS {
                        spawn_entry(panel, kind, font, icons, names);
                    }
                });
        });
}

/// Строка постройки: иконка, название, цена. Клик выбирает/снимает выбор.
fn spawn_entry(
    panel: &mut ChildSpawnerCommands,
    kind: ObjectKind,
    font: &UiFont,
    icons: &UiIcons,
    names: &ItemNames,
) {
    let name = building_name(kind);
    let price = match build_cost(kind) {
        Some((item, count)) => format!("{} × {}", count, names.name(item)),
        None => String::new(),
    };
    panel
        .spawn((
            Button,
            Node {
                width: Val::Px(360.0),
                height: Val::Px(48.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(10.0),
                padding: UiRect::horizontal(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::surface()),
            BorderColor::all(style::border()),
            Hovered::default(),
            BuildEntry(kind),
        ))
        .with_children(|row| {
            row.spawn((ImageNode::new(icons.object(kind).clone()), Node::default()))
                .insert(FitIcon { box_px: 32.0 });
            row.spawn((
                Text::new(name),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 15.0.into(),
                    ..default()
                },
                TextColor(style::text()),
            ));
            row.spawn((
                Text::new(price),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
        });
}

/// Название постройки для меню (предметные названия — в info/items.json).
fn building_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Campfire => "Костёр",
        _ => "Постройка",
    }
}

/// Клик по строке: выбор постройки или снятие выбора повторным кликом.
fn on_entry_activated(
    activate: On<Activate>,
    entries: Query<&BuildEntry>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut construction: ResMut<ConstructionUi>,
) {
    let Ok(entry) = entries.get(activate.entity) else {
        return;
    };
    // Активация приходит на клик любой кнопкой; выбор — только ЛКМ.
    if mouse.just_released(MouseButton::Right) || mouse.just_released(MouseButton::Middle) {
        return;
    }
    construction.selected = if construction.selected == Some(entry.0) {
        None
    } else {
        Some(entry.0)
    };
}

/// ПКМ по карте с выбранной постройкой — команда строительства: по нажатию
/// и затем при смене тайла под курсором, пока ПКМ зажата (как инструменты
/// редактора). Всё остальное — плату, тайл, радиус — проверяет симуляция.
fn construction_place_input(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    state: Res<GameState>,
    construction: Res<ConstructionUi>,
    mut queue: ResMut<CommandQueueRes>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
    mut drag: Local<Option<crate::simulation::TilePos>>,
) {
    if !mouse.pressed(MouseButton::Right) {
        *drag = None;
        return;
    }
    let Some(kind) = construction.selected else {
        *drag = None;
        return;
    };
    if !construction.open || state.mode != GameMode::Player || state.paused {
        *drag = None;
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    if crate::ui::cursor_over_ui(window, &blockers) {
        *drag = None;
        return;
    }
    let Some(world) = state.world.as_ref() else {
        return;
    };
    let Some(tile) = cursor_world_tile(window, &cameras, Some(world)) else {
        return;
    };
    // Ставим по нажатию и при смене тайла под курсором.
    if *drag == Some(tile) {
        return;
    }
    queue.0.push(Command::PlayerBuild { pos: tile, kind });
    *drag = Some(tile);
}

/// Подсветка выбранной строки постройки. Строк пересчитываются за кадр
/// единицы, поэтому проверка идёт всегда, без фильтров по изменениям.
fn refresh_entries(
    construction: Res<ConstructionUi>,
    mut entries: Query<(&BuildEntry, &mut BorderColor, &mut BackgroundColor)>,
) {
    for (entry, mut border, mut bg) in &mut entries {
        let selected = construction.selected == Some(entry.0);
        let want_border = BorderColor::all(if selected {
            style::surface_active()
        } else {
            style::border()
        });
        if *border != want_border {
            *border = want_border;
        }
        let want_bg = if selected {
            style::surface_active()
        } else {
            style::surface()
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
    }
}
