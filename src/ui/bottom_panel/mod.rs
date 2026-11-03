//! Нижняя панель редактора в духе WorldBox.
//!
//! Панель — строго два ряда квадратных кнопок 64×64. Базовый вид: пауза,
//! скорость, тайлы, объекты, существа. Кнопка меню открывает подменю на
//! месте базовых кнопок: первый столбец занимает «назад» на всю высоту
//! панели, остальные элементы идут по столбцам сверху вниз
//! (1.1, 2.1, 1.2, 2.2, ...), поэтому панель никогда не растёт в высоту.
//! Поля размера и сида мира живут справа и в подменю не меняются.
//!
//! Состав модуля:
//! - `categories` — наборы кнопок базового вида и подменю (инструменты,
//!   линзы), их иконки и русские названия для тултипов;
//! - `world_fields` — поля размера мира и сида справа на панели.

mod categories;
mod world_fields;

pub(crate) use categories::panel_action_tooltip;
#[allow(unused_imports)]
pub(crate) use world_fields::reset_editable;

use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, ActivateOnPress, Button, TextInput};

use crate::app::settings::{ControlAction, Settings};
use crate::app::state::{GameMode, GameState, LensKind, SimSpeed, Tool, ToolCategory};


use crate::ui::UiFont;
use crate::ui::icons::UiIcons;
use crate::ui::pause_menu::BindingCapture;
use crate::ui::style;
use crate::ui::text_input_focused;
use crate::ui::BlocksWorldInput;

use categories::{base_items, menu_items};
use world_fields::spawn_world_fields;

pub struct BottomPanelPlugin;

impl Plugin for BottomPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PanelMenuState>()
            .add_systems(Startup, spawn_bottom_panel)
            .add_systems(
                Update,
                (
                    // Подменю сбрасывается при смене режима, затем кнопки
                    // перестраиваются под новый вид.
                    (reset_menu_on_mode_change, editor_back_input, rebuild_panel_buttons).chain(),
                    update_button_styles,
                    update_pause_icon,
                    fit_icons,
                    world_fields::handle_world_size_input,
                    world_fields::handle_world_seed_input,
                    toggle_bottom_panel_visibility,
                    clear_panel_pressed,
                ),
            );
        app.add_observer(on_panel_button_activated);
    }
}

// --- Раскладка панели: правится здесь ---

/// Сторона квадратной кнопки — все кнопки панели одного размера.
const BUTTON_SIZE: f32 = 64.0;
/// Зазор между кнопками (и столбцами).
pub(crate) const BUTTON_GAP: f32 = 6.0;
/// Вертикальный отступ панели сверху и снизу.
const PANEL_PADDING: f32 = 8.0;
/// Кнопок в столбце: панель строго в два ряда.
const ROWS: usize = 2;
/// Высота панели: два ряда кнопок плюс зазор и отступы.
const PANEL_HEIGHT: f32 = ROWS as f32 * BUTTON_SIZE + BUTTON_GAP + PANEL_PADDING * 2.0;
/// Горизонтальный отступ панели (кнопки не прилипают к краю экрана).
const PANEL_PADDING_X: f32 = 12.0;
/// Квадрат, в который вписывается иконка кнопки с сохранением пропорций.
const ICON_BOX: f32 = 46.0;

// --- Состояние вида панели ---

/// Открытое подменю панели.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelMenu {
    /// Скорости симуляции (x0.5…x5).
    Speed,
    /// Инструменты категории.
    Tools(ToolCategory),
    /// Линзы карты: подсветка чанков (обновление раз в секунду).
    Lenses,
}

/// Текущий вид панели: `None` — базовые кнопки, иначе открытое подменю.
#[derive(Resource, Default)]
pub struct PanelMenuState {
    pub open: Option<PanelMenu>,
}

/// Действие кнопки нижней панели.
///
/// `Hovered` обязателен: пикинг обновляет его только у сущностей,
/// у которых компонент уже есть, — от него зависят звуки наведения.
#[derive(Clone, Copy, PartialEq, Eq, Component)]
#[require(Hovered)]
pub enum PanelAction {
    TogglePause,
    /// Открыть подменю.
    OpenMenu(PanelMenu),
    /// Вернуться к базовым кнопкам.
    Back,
    SetSpeed(SimSpeed),
    /// Выбрать инструмент; повторный клик снимает выбор.
    SelectTool(Tool),
    /// Включить/выключить линзу карты.
    ToggleLens(LensKind),
}

// --- Компоненты ---

/// Корень панели: скрывается в режиме игрока.
#[derive(Component)]
pub struct BottomPanelRoot;

/// Контейнер кнопок: при смене вида его содержимое сносится и строится
/// заново (поля мира справа переживают перестройку).
#[derive(Component)]
struct PanelButtonsHost;

/// Иконка кнопки паузы: play/pause меняется по состоянию мира.
#[derive(Component)]
struct PauseIcon;

/// Маркер иконки, которую надо вписать в квадрат с сохранением пропорций.
/// Используется и окном инвентаря: система `fit_icons` глобальная.
#[derive(Component)]
pub(crate) struct FitIcon {
    pub(crate) box_px: f32,
}

/// Маркер «пропорции уже вписаны» — система `fit_icons` больше не трогает.
/// Виден инвентарю: смена иконки снимает его, чтобы вписать заново.
#[derive(Component)]
pub(crate) struct FitDone;

// --- Описание кнопки ---

/// Кнопка панели: действие и то, что рисуется внутри.
struct PanelItem {
    action: PanelAction,
    content: ButtonContent,
}

/// Содержимое кнопки. Подписи короткие: в подменю не больше двух-трёх
/// символов (скорость, «назад»), остальное — иконки.
enum ButtonContent {
    /// Иконка тайла, объекта или существа.
    Icon(Handle<Image>),
    /// Иконка паузы: play/pause по состоянию мира.
    Pause(Handle<Image>),
    /// Короткая подпись (скорость, «назад»).
    Text(&'static str),
}

// --- Построение ---

/// Спавнит панель: контейнер кнопок (наполняется системой перестройки)
/// и поля размера/сида мира справа.
fn spawn_bottom_panel(mut commands: Commands, state: Res<GameState>, font: Res<UiFont>) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                height: Val::Px(PANEL_HEIGHT),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect {
                    left: Val::Px(PANEL_PADDING_X),
                    right: Val::Px(PANEL_PADDING_X),
                    top: Val::Px(PANEL_PADDING),
                    bottom: Val::Px(PANEL_PADDING),
                },
                column_gap: Val::Px(16.0),
                ..default()
            },
            BackgroundColor(style::background()),
            BlocksWorldInput,
            BottomPanelRoot,
            Pickable::default(),
        ))
        .with_children(|panel| {
            panel.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(BUTTON_GAP),
                    ..default()
                },
                PanelButtonsHost,
            ));

            // Размер мира и сид: подписи + поля ввода.
            spawn_world_fields(panel, &state, &font);
        });
}

/// Строка-кластер внутри панели.
pub(crate) fn row_node() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        column_gap: Val::Px(BUTTON_GAP),
        align_items: AlignItems::Center,
        ..default()
    }
}

/// Столбец-кластер внутри панели.
pub(crate) fn column_node() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(BUTTON_GAP),
        align_items: AlignItems::Center,
        ..default()
    }
}

/// Перестраивает кнопки при смене вида панели.
///
/// Содержимое хоста сносится целиком: так подменю не требует ручной
/// синхронизации, а заодно сбрасываются залипшие `Pressed`/`Hovered`.
fn rebuild_panel_buttons(
    mut commands: Commands,
    menu: Res<PanelMenuState>,
    state: Res<GameState>,
    icons: Res<UiIcons>,
    font: Res<UiFont>,
    host: Query<Entity, With<PanelButtonsHost>>,
    mut built: Local<Option<Option<PanelMenu>>>,
) {
    if *built == Some(menu.open) {
        return;
    }
    let Ok(host) = host.single() else {
        return;
    };
    *built = Some(menu.open);
    commands.entity(host).despawn_children();
    commands.entity(host).with_children(|parent| {
        match menu.open {
            None => spawn_item_columns(parent, &base_items(&state, &icons), &font),
            Some(menu) => {
                // «Назад» — отдельный столбец на всю высоту панели.
                parent.spawn(column_node()).with_children(|col| {
                    let item = PanelItem {
                        action: PanelAction::Back,
                        content: ButtonContent::Text("<"),
                    };
                    spawn_item(col, &item, &font);
                });
                spawn_item_columns(parent, &menu_items(menu, &icons), &font);
            }
        }
    });
}

/// Раскладывает кнопки по столбцам сверху вниз: 1.1, 2.1, 1.2, 2.2, …
fn spawn_item_columns(parent: &mut ChildSpawnerCommands, items: &[PanelItem], font: &UiFont) {
    for column in items.chunks(ROWS) {
        parent
            .spawn(column_node())
            .with_children(|col| {
                for item in column {
                    spawn_item(col, item, font);
                }
            });
    }
}

/// Одна квадратная кнопка. «Назад» занимает оба ряда по высоте.
fn spawn_item(col: &mut ChildSpawnerCommands, item: &PanelItem, font: &UiFont) {
    let tall = item.action == PanelAction::Back;
    let height = if tall {
        BUTTON_SIZE * ROWS as f32 + BUTTON_GAP
    } else {
        BUTTON_SIZE
    };

    col.spawn((
        Button,
        Node {
            width: Val::Px(BUTTON_SIZE),
            height: Val::Px(height),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::surface()),
        BorderColor::all(style::border()),
        item.action,
        ActivateOnPress,
    ))
    .with_children(|b| match &item.content {
        ButtonContent::Icon(handle) => {
            b.spawn(icon_node(handle)).insert(FitIcon { box_px: ICON_BOX });
        }
        ButtonContent::Pause(handle) => {
            b.spawn(icon_node(handle))
                .insert(PauseIcon)
                .insert(FitIcon { box_px: ICON_BOX });
        }
        ButtonContent::Text(text) => {
            b.spawn((
                Text::new(*text),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: if tall { 30.0 } else { 18.0 }.into(),
                    ..default()
                },
                TextColor(style::text()),
            ));
        }
    });
}

/// Иконка-картинка. Размер ставит `fit_icons` после загрузки текстуры
/// (Stretch в уже вычисленный узел — пропорции держим сами).
fn icon_node(handle: &Handle<Image>) -> (ImageNode, Node) {
    (
        ImageNode {
            image: handle.clone(),
            image_mode: NodeImageMode::Stretch,
            ..default()
        },
        Node::default(),
    )
}

/// Сбрасывает подменю при смене режима: из игры в редактор панель
/// возвращается в базовом виде.
fn reset_menu_on_mode_change(
    state: Res<GameState>,
    mut menu: ResMut<PanelMenuState>,
    mut last: Local<Option<GameMode>>,
) {
    if *last == Some(state.mode) {
        return;
    }
    *last = Some(state.mode);
    if menu.open.is_some() {
        menu.open = None;
    }
}

/// Клавиша «Назад (редактор)» (по умолчанию Q) при открытом подменю
/// панели: закрывает его и сбрасывает выбранный инструмент — то же, что
/// клик по кнопке «<».
fn editor_back_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<Settings>,
    mut state: ResMut<GameState>,
    mut menu: ResMut<PanelMenuState>,
    capture: Res<BindingCapture>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
) {
    if capture.0.is_some() || text_input_focused(&focus, &text_inputs) {
        return;
    }
    // Модальное меню (пауза, технологии) перекрывает редактор.
    if state.active_menu.is_some() {
        return;
    }
    if menu.open.is_none() {
        return;
    }
    if !keys.just_pressed(settings.controls.key(ControlAction::EditorBack)) {
        return;
    }
    menu.open = None;
    state.selected_tool = None;
}

/// Обрабатывает активацию кнопок (клик мышью или Enter/Space).
fn on_panel_button_activated(
    activate: On<Activate>,
    actions: Query<&PanelAction>,
    mut state: ResMut<GameState>,
    mut menu: ResMut<PanelMenuState>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };

    match action {
        PanelAction::TogglePause => {
            state.paused = !state.paused;
        }
        PanelAction::OpenMenu(open) => {
            menu.open = Some(*open);
            // Инструмент другой категории в открытом меню не виден —
            // снимаем выбор, чтобы клик по карте не ставил «невидимое».
            if let PanelMenu::Tools(category) = open
                && state
                    .selected_tool
                    .is_some_and(|tool| tool.category() != *category)
            {
                state.selected_tool = None;
            }
        }
        PanelAction::Back => {
            menu.open = None;
            // Выход из категории — выбор инструмента сбрасывается: клик по
            // карте с «невидимым» инструментом чужой категории нежелателен.
            state.selected_tool = None;
        }
        PanelAction::SetSpeed(speed) => {
            state.speed = *speed;
        }
        // Инструменты редактора действуют только в режиме наблюдателя.
        PanelAction::SelectTool(tool) => {
            if state.mode == GameMode::Observer {
                // Повторный клик по выбранному инструменту снимает выбор.
                state.selected_tool =
                    if state.selected_tool == Some(*tool) { None } else { Some(*tool) };
            }
        }
        // Линзы — независимые переключатели, клик по карте не мешают.
        PanelAction::ToggleLens(kind) => match kind {
            LensKind::Sentients => state.lenses.sentients = !state.lenses.sentients,
            LensKind::Buildings => state.lenses.buildings = !state.lenses.buildings,
        },
    }
}

/// Фон и рамка кнопок: выбор — акцентом, нажатие — рамкой.
fn update_button_styles(
    state: Res<GameState>,
    mut buttons: Query<(
        &PanelAction,
        Has<Pressed>,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (action, pressed, mut background, mut border) in &mut buttons {
        border.set_if_neq(BorderColor::all(if pressed {
            style::surface_active()
        } else {
            style::border()
        }));

        let selected = match action {
            PanelAction::SetSpeed(speed) => state.speed == *speed,
            PanelAction::SelectTool(tool) => state.selected_tool == Some(*tool),
            PanelAction::ToggleLens(kind) => match kind {
                LensKind::Sentients => state.lenses.sentients,
                LensKind::Buildings => state.lenses.buildings,
            },
            _ => false,
        };
        let want = if selected {
            style::surface_active()
        } else {
            style::surface()
        };
        background.set_if_neq(BackgroundColor(want));
    }
}

/// Меняет иконку кнопки паузы: пока мир идёт — «пауза», на паузе — «play».
fn update_pause_icon(
    state: Res<GameState>,
    icons: Res<UiIcons>,
    mut pause_icons: Query<&mut ImageNode, With<PauseIcon>>,
) {
    let want = if state.paused { icons.play() } else { icons.pause() };
    for mut image in &mut pause_icons {
        if image.image != *want {
            image.image = want.clone();
        }
    }
}

/// Вписывает иконки в квадрат с сохранением пропорций, как только
/// загрузится текстура. Auto-режим ImageNode тянет непропорционально,
/// поэтому пропорции считаем сами и рисуем Stretch'ем в готовый узел.
fn fit_icons(
    mut commands: Commands,
    images: Res<Assets<Image>>,
    icons: Query<(Entity, &ImageNode, &FitIcon, &mut Node), Without<FitDone>>,
) {
    for (entity, image, fit, mut node) in icons {
        let Some(img) = images.get(&image.image) else {
            continue;
        };
        let (w, h) = (img.width() as f32, img.height() as f32);
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let scale = fit.box_px / w.max(h);
        node.width = Val::Px(w * scale);
        node.height = Val::Px(h * scale);
        commands.entity(entity).insert(FitDone);
    }
}

/// Скрывает нижнюю панель редактора в режиме игрока.
fn toggle_bottom_panel_visibility(
    state: Res<GameState>,
    mut panels: Query<(&mut Node, &mut Pickable), With<BottomPanelRoot>>,
) {
    for (mut node, mut pickable) in &mut panels {
        if state.mode == GameMode::Player {
            node.display = Display::None;
            pickable.should_block_lower = false;
            pickable.is_hoverable = false;
        } else {
            node.display = Display::Flex;
            *pickable = Pickable::default();
        }
    }
}

/// Снимает залипшее состояние нажатия кнопок панели после переключения
/// режимов (страховка: перестройка кнопок и так создаёт их заново).
fn clear_panel_pressed(
    state: Res<GameState>,
    mut commands: Commands,
    pressed: Query<Entity, (With<PanelAction>, With<Pressed>)>,
) {
    if !state.is_changed() {
        return;
    }
    if state.mode == GameMode::Observer {
        for entity in &pressed {
            commands.entity(entity).remove::<Pressed>();
        }
    }
}
