//! Экраны настроек пауз-меню: список категорий, «Геймплей», «Графика»,
//! «Звуки» (слайдеры громкости), «Управление» (перебиндивание клавиш),
//! сброс. Применяются сразу (без черновика: в ADEM нет тяжёлых пересборок
//! мира на смену настройки), файл перезаписывается при закрытии меню
//! (`pause_menu::close_menu`) и выходе из игры.
//!
//! Экраны встраиваются в общий каркас пауз-меню: навигация (ESC, кнопка
//! «Назад») и звуки остаются в `pause_menu.rs`, здесь — только содержимое
//! категорий, слайдеры и захват клавиш.

use bevy::input::{ButtonState, keyboard::KeyboardInput};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Outline;
use bevy::ui::Pressed;
use bevy::ui_widgets::Button;

use crate::app::audio::{SoundGate, UiSounds, play_menu_sound};
use crate::app::settings::{ControlAction, Settings, SoundBus, keycode_name};
use crate::ui::style;
use crate::ui::pause_menu::{MenuAction, MenuFonts, PauseMenuRoot, PauseMenuView, action_button, rebuild_menu};

/// Категория настроек.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    Gameplay,
    Graphics,
    Sounds,
    Controls,
}

impl SettingsCategory {
    /// Подпись и заголовок категории.
    pub fn name(self) -> &'static str {
        match self {
            SettingsCategory::Gameplay => "Геймплей",
            SettingsCategory::Graphics => "Графика",
            SettingsCategory::Sounds => "Звуки",
            SettingsCategory::Controls => "Управление",
        }
    }

    /// Все категории в порядке меню.
    pub const ALL: [SettingsCategory; 4] = [
        SettingsCategory::Gameplay,
        SettingsCategory::Graphics,
        SettingsCategory::Sounds,
        SettingsCategory::Controls,
    ];
}

/// Кнопка привязки клавиши в «Управлении».
#[derive(Component, Clone, Copy, Debug)]
#[require(Hovered)]
pub struct BindButton(pub ControlAction);

/// Второй `Text` кнопки бинда — текущая клавиша. Маркер надёжнее
/// счётчика детей: добавление элементов внутрь кнопки не сдвинет текст.
#[derive(Component)]
pub struct BindLabelValue;

/// Кнопка, ожидающая назначение клавиши.
#[derive(Resource, Default, Debug)]
pub struct BindingCapture(pub Option<ControlAction>);

/// Полоса слайдера громкости: клик и перетаскивание меняют значение.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct SliderBar(pub VolumeSlider);

/// Заливка полосы слайдера (ширина — доля значения).
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct SliderFill(pub VolumeSlider);

/// Подпись слайдера: название + текущее значение.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct SliderLabel(pub VolumeSlider);

/// Какой слайдер громкости крутится.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VolumeSlider {
    Master,
    Bus(SoundBus),
}

impl VolumeSlider {
    /// Все слайдеры в порядке меню «Звуки».
    pub const ALL: [VolumeSlider; 5] = [
        VolumeSlider::Master,
        VolumeSlider::Bus(SoundBus::Ambient),
        VolumeSlider::Bus(SoundBus::Effect),
        VolumeSlider::Bus(SoundBus::Creature),
        VolumeSlider::Bus(SoundBus::Ui),
    ];

    /// Название в меню.
    fn name(self) -> &'static str {
        match self {
            VolumeSlider::Master => "Общая громкость",
            VolumeSlider::Bus(bus) => bus.label(),
        }
    }

    /// Текущее значение, 0..1.
    fn get(self, s: &Settings) -> f32 {
        match self {
            VolumeSlider::Master => s.volume,
            VolumeSlider::Bus(SoundBus::Ambient) => s.volume_ambient,
            VolumeSlider::Bus(SoundBus::Effect) => s.volume_effect,
            VolumeSlider::Bus(SoundBus::Creature) => s.volume_creature,
            VolumeSlider::Bus(SoundBus::Ui) => s.volume_ui,
        }
    }

    /// Записывает значение (уже зажатое в 0..1).
    fn set(self, s: &mut Settings, value: f32) {
        match self {
            VolumeSlider::Master => s.volume = value,
            VolumeSlider::Bus(SoundBus::Ambient) => s.volume_ambient = value,
            VolumeSlider::Bus(SoundBus::Effect) => s.volume_effect = value,
            VolumeSlider::Bus(SoundBus::Creature) => s.volume_creature = value,
            VolumeSlider::Bus(SoundBus::Ui) => s.volume_ui = value,
        }
    }

    /// Подпись «Название: N%».
    fn label(self, s: &Settings) -> String {
        format!("{}: {:.0}%", self.name(), self.get(s) * 100.0)
    }
}

// ---------- Действия настроек ----------

/// Выполняет действие кнопки настроек (тумблеры, циклы, категории, сброс).
/// Возвращает `false`, если действие не относится к настройкам — вызыватель
/// (`pause_menu::on_menu_button_activate`) обрабатывает свои сам.
/// Тексты тумблеров обновляются на месте, навигация перестраивает экран.
pub(crate) fn apply_settings_action(
    action: MenuAction,
    commands: &mut Commands,
    settings: &mut Settings,
    view: &mut PauseMenuView,
    roots: &Query<Entity, With<PauseMenuRoot>>,
    fonts: &MenuFonts,
    children: &Children,
    texts: &mut Query<&mut Text>,
) -> bool {
    match action {
        MenuAction::OpenCategory(category) => {
            *view = PauseMenuView::Category(category);
            rebuild_menu(commands, roots, *view, settings, fonts);
        }
        MenuAction::BackToSettings => {
            *view = PauseMenuView::Settings;
            rebuild_menu(commands, roots, *view, settings, fonts);
        }
        MenuAction::ResetSettings => {
            *settings = Settings::default();
            rebuild_menu(commands, roots, *view, settings, fonts);
        }
        MenuAction::TogglePauseOnFocusLoss => {
            settings.pause_on_focus_loss = !settings.pause_on_focus_loss;
            update_first_button_text(children, texts, focus_loss_label(settings));
        }
        MenuAction::CycleFpsLimit => {
            settings.fps_limit = settings.fps_limit.next();
            update_first_button_text(children, texts, fps_label(settings));
        }
        MenuAction::CycleWindowMode => {
            settings.window_mode = settings.window_mode.next();
            update_first_button_text(children, texts, window_mode_label(settings));
        }
        MenuAction::CycleAntialias => {
            settings.antialias = settings.antialias.next();
            update_first_button_text(children, texts, antialias_label(settings));
        }
        MenuAction::ToggleLighting => {
            settings.lighting = !settings.lighting;
            update_first_button_text(children, texts, lighting_label(settings));
        }
        _ => return false,
    }
    true
}

/// Обновляет первый `Text`-потомок кнопки (подпись тумблеров и циклов).
fn update_first_button_text(children: &Children, texts: &mut Query<&mut Text>, text: String) {
    for child in children {
        if let Ok(mut t) = texts.get_mut(*child) {
            **t = text;
            break;
        }
    }
}

/// Подпись тумблера «Пауза при потере фокуса».
fn focus_loss_label(settings: &Settings) -> String {
    format!(
        "Пауза при потере фокуса: {}",
        if settings.pause_on_focus_loss { "Да" } else { "Нет" }
    )
}

/// Подпись кнопки «Ограничение FPS».
fn fps_label(settings: &Settings) -> String {
    format!("Ограничение FPS: {}", settings.fps_limit.name())
}

/// Подпись кнопки «Режим окна».
fn window_mode_label(settings: &Settings) -> String {
    format!("Режим окна: {}", settings.window_mode.name())
}

/// Подпись кнопки «Сглаживание».
fn antialias_label(settings: &Settings) -> String {
    format!("Сглаживание: {}", settings.antialias.name())
}

/// Подпись тумблера «Освещение».
fn lighting_label(settings: &Settings) -> String {
    format!("Освещение: {}", if settings.lighting { "Вкл" } else { "Выкл" })
}

// ---------- Слайдеры ----------

/// Слайдеры следуют за курсором: клик в точку полосы ставит значение
/// сразу, зажатая ЛКМ тянет дальше. Громкость пишется прямо в настройки:
/// `apply_volumes` слышит смену и двигает играющие источники.
///
/// Координата берётся абсолютной: `ComputedNode::normalize_point`
/// переводит курсор в доли полосы (−0.5 слева, 0.5 справа). Курсор окна —
/// в логических пикселях, а нода — в физических, отсюда множитель
/// `scale_factor` (как в штатном UI-пикинге bevy).
pub(crate) fn update_sliders(
    windows: Query<&Window>,
    mut commands: Commands,
    time: Res<Time>,
    sounds: Res<UiSounds>,
    mut gate: ResMut<SoundGate>,
    mut settings: ResMut<Settings>,
    bars: Query<(Entity, Has<Pressed>, &ComputedNode, &UiGlobalTransform, &SliderBar)>,
    mut texts: Query<&mut Text>,
    labels: Query<(Entity, &SliderLabel)>,
    fills: Query<(Entity, &SliderFill)>,
    mut nodes: Query<&mut Node>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let scale = window.scale_factor().max(f32::EPSILON);
    let cursor_phys = cursor * scale;

    for (_entity, pressed, computed, transform, bar) in &bars {
        // Тяга идёт только с зажатой ЛКМ на полосе.
        if !pressed {
            continue;
        }
        // Мимо полосы (нода без размера на первом кадре) — пропускаем.
        let Some(normalized) = computed.normalize_point(*transform, cursor_phys) else {
            continue;
        };
        let kind = bar.0;
        let value = (normalized.x + 0.5).clamp(0.0, 1.0);
        if refresh_slider_ui(kind, value, &mut settings, &mut texts, &labels, &fills, &mut nodes) {
            // Значение реально сдвинулось — короткий блип (гейт режет
            // очередь при быстрой тяге).
            play_menu_sound(
                &mut commands,
                sounds.slider.clone(),
                &mut gate,
                &time,
                &settings,
            );
        }
    }
}

/// Применяет значение слайдера и обновляет его подпись и заливку.
/// Пишет только при реальном сдвиге, иначе тяга дёргала бы переразметку
/// текста каждый кадр. Возвращает, сдвинулось ли значение (нужно звуку).
fn refresh_slider_ui(
    kind: VolumeSlider,
    value: f32,
    settings: &mut Settings,
    texts: &mut Query<&mut Text>,
    labels: &Query<(Entity, &SliderLabel)>,
    fills: &Query<(Entity, &SliderFill)>,
    nodes: &mut Query<&mut Node>,
) -> bool {
    let before = kind.get(settings);
    if (value - before).abs() <= f32::EPSILON {
        return false;
    }
    kind.set(settings, value);

    let text = kind.label(settings);
    for (label_entity, label) in labels {
        if label.0 == kind
            && let Ok(mut t) = texts.get_mut(label_entity)
            && t.as_str() != text
        {
            **t = text.clone();
        }
    }
    let width = Val::Percent(value * 100.0);
    for (fill_entity, fill) in fills {
        if fill.0 == kind
            && let Ok(mut node) = nodes.get_mut(fill_entity)
            && node.width != width
        {
            node.width = width;
        }
    }
    true
}

// ---------- Захват клавиш для биндов ----------

/// Захват клавиши для бинда: первое нажатие (не повтор) назначается
/// действию, ESC отменяет. Работает только пока захват активен.
///
/// События вычитываются всегда (даже без активного захвата): иначе
/// зажатая до клика клавиша назначилась бы сама в следующем кадре.
/// Клавиши вне перебиндиваемого набора (F-ряд и т.п.) игнорируются:
/// кнопка остаётся в захвате, запись не «пачкается» неизвестным именем.
pub(crate) fn capture_keybind(
    mut events: MessageReader<KeyboardInput>,
    mut capture: ResMut<BindingCapture>,
    mut settings: ResMut<Settings>,
    mut binds: Query<(&BindButton, &Children, &mut BackgroundColor)>,
    mut texts: Query<&mut Text, With<BindLabelValue>>,
) {
    let mut pressed: Option<KeyCode> = None;
    for event in events.read() {
        if event.state == ButtonState::Pressed && !event.repeat {
            pressed = Some(event.key_code);
            break;
        }
    }
    let Some(action) = capture.0 else { return };
    let Some(code) = pressed else { return };
    if code == KeyCode::Escape {
        // Отмена — не выход: handle_escape при захвате молчит.
        capture.0 = None;
        return;
    }
    let name = keycode_name(code);
    if name == "?" {
        return;
    }
    settings.controls.set(action, code);
    capture.0 = None;
    // Клавиша на кнопке — Text с маркером BindLabelValue.
    for (button, children, mut bg) in &mut binds {
        if button.0 != action {
            continue;
        }
        bg.0 = style::surface();
        for child in children {
            if let Ok(mut t) = texts.get_mut(*child)
                && t.as_str() != name
            {
                **t = name.to_string();
            }
        }
    }
}

// ---------- Отрисовка ----------

/// Корень настроек: категории, сброс, назад.
pub(crate) fn build_settings_root(parent: &mut ChildSpawnerCommands, fonts: &MenuFonts) {
    for category in SettingsCategory::ALL {
        action_button(
            parent,
            fonts,
            category.name(),
            MenuAction::OpenCategory(category),
            style::text(),
        );
    }
    action_button(parent, fonts, "Сбросить настройки", MenuAction::ResetSettings, style::danger());
    action_button(parent, fonts, "Назад", MenuAction::BackToRoot, style::text());
}

/// Содержимое категории настроек.
pub(crate) fn build_category(
    parent: &mut ChildSpawnerCommands,
    fonts: &MenuFonts,
    settings: &Settings,
    category: SettingsCategory,
) {
    match category {
        SettingsCategory::Gameplay => {
            action_button(
                parent,
                fonts,
                &focus_loss_label(settings),
                MenuAction::TogglePauseOnFocusLoss,
                style::text(),
            );
        }
        SettingsCategory::Graphics => {
            action_button(parent, fonts, &fps_label(settings), MenuAction::CycleFpsLimit, style::text());
            action_button(
                parent,
                fonts,
                &window_mode_label(settings),
                MenuAction::CycleWindowMode,
                style::text(),
            );
            action_button(
                parent,
                fonts,
                &antialias_label(settings),
                MenuAction::CycleAntialias,
                style::text(),
            );
            action_button(
                parent,
                fonts,
                &lighting_label(settings),
                MenuAction::ToggleLighting,
                style::text(),
            );
        }
        SettingsCategory::Sounds => {
            for kind in VolumeSlider::ALL {
                spawn_slider_row(parent, fonts, kind, settings);
            }
        }
        SettingsCategory::Controls => {
            for action in ControlAction::ALL {
                spawn_bind_button(parent, fonts, action, settings);
            }
        }
    }
    action_button(parent, fonts, "Назад", MenuAction::BackToSettings, style::text());
}

/// Строка слайдера: подпись слева, полоса справа с заливкой.
fn spawn_slider_row(
    parent: &mut ChildSpawnerCommands,
    fonts: &MenuFonts,
    kind: VolumeSlider,
    settings: &Settings,
) {
    parent
        .spawn((Node {
            width: Val::Px(480.0),
            height: Val::Px(30.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: Val::Px(12.0),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Text::new(kind.label(settings)),
                TextFont {
                    font: fonts.medium.clone().into(),
                    font_size: 15.0.into(),
                    ..default()
                },
                TextColor(style::text()),
                SliderLabel(kind),
            ));
            row.spawn((
                Button,
                Node {
                    width: Val::Px(220.0),
                    height: Val::Px(18.0),
                    border_radius: BorderRadius::all(Val::Px(5.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.08, 0.08, 0.08, 0.9)),
                BorderColor::all(style::border()),
                SliderBar(kind),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Node {
                        width: Val::Percent(kind.get(settings) * 100.0),
                        height: Val::Percent(100.0),
                        border_radius: BorderRadius::all(Val::Px(5.0)),
                        ..default()
                    },
                    BackgroundColor(style::surface_active()),
                    SliderFill(kind),
                ));
            });
        });
}

/// Кнопка бинда: действие + текущая клавиша.
fn spawn_bind_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &MenuFonts,
    action: ControlAction,
    settings: &Settings,
) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(480.0),
                height: Val::Px(40.0),
                padding: UiRect::horizontal(Val::Px(16.0)),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::surface()),
            BorderColor::all(style::border()),
            Outline::new(Val::Px(2.0), Val::Px(0.0), Color::NONE),
            BindButton(action),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(action.label()),
                TextFont {
                    font: fonts.medium.clone().into(),
                    font_size: 16.0.into(),
                    ..default()
                },
                TextColor(style::text()),
            ));
            b.spawn((
                Text::new(keycode_name(settings.controls.key(action)).to_string()),
                TextFont {
                    font: fonts.medium.clone().into(),
                    font_size: 16.0.into(),
                    ..default()
                },
                TextColor(style::surface_active()),
                BindLabelValue,
            ));
        });
}
