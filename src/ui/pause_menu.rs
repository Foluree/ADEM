//! ESC-меню (пауза) и настройки: категории «Геймплей», «Графика»,
//! «Звуки», «Управление», сброс и «Назад».
//!
//! Управление меню — один обработчик ESC: он же закрывает меню
//! технологий/институтов, ходит по категориям назад и открывает меню
//! заново. Единая цепочка вместо нескольких систем с согласованием
//! порядка: одно нажатие — ровно одно действие.
//!
//! Настройки применяются сразу (без черновика, как в Endless_Space:
//! в ADEM нет тяжёлых пересборок мира на смену настройки), файл
//! перезаписывается при закрытии меню и выходе из игры.

use bevy::input::{ButtonState, keyboard::KeyboardInput};
use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{Outline, Pressed};
use bevy::ui_widgets::{Activate, Button, TextInput};

use crate::app::audio::{SoundGate, UiSounds, hover_just_entered, play_menu_sound};
use crate::app::saves::{LoadGameRequest, SAVE_SLOTS, SaveGameRequest, slot_info};
use crate::app::settings::{ControlAction, Settings, SoundBus, keycode_name, settings_path};
use crate::app::state::{GameState, MenuKind};
use crate::simulation::save::SaveHeader;
use crate::simulation::GameClock;
use crate::ui::UiFont;
use crate::ui::construction::{ConstructionRoot, ConstructionUi};
use crate::ui::inventory::{InventoryRoot, InventoryUi};
use crate::ui::style;
use crate::ui::text_input_focused;
use crate::ui::BlocksWorldInput;

pub struct PauseMenuPlugin;

impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PauseMenuView>()
            .init_resource::<BindingCapture>()
            .add_systems(Startup, load_menu_fonts)
            // Цепочка ESC → захват бинда строго в этом порядке: иначе в
            // одном кадре ESC мог бы отменить захват и одновременно
            // уйти назад по меню (capture обнуляется до проверки).
            .add_systems(Update, (handle_escape, capture_keybind).chain())
            .add_systems(
                Update,
                (
                    update_menu_button_visuals,
                    menu_select_sound,
                    update_sliders,
                ),
            )
            .add_observer(on_menu_button_activate)
            .add_observer(menu_click_sound);
    }
}

// ---------- Состояние меню ----------

/// Какой экран пауз-меню открыт.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseMenuView {
    /// Корень: продолжить / настройки / сейвы / выход.
    #[default]
    Root,
    /// Список категорий настроек.
    Settings,
    /// Открытая категория настроек.
    Category(SettingsCategory),
    /// Выбор слота для записи.
    SaveSlots,
    /// Выбор слота для чтения.
    LoadSlots,
}

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

/// Действие кнопки меню.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Продолжить игру (закрыть меню, снять паузу).
    Continue,
    /// Открыть список категорий настроек.
    OpenSettings,
    /// Открыть выбор слота для записи.
    OpenSaveSlots,
    /// Открыть выбор слота для чтения.
    OpenLoadSlots,
    /// Записать игру в слот (сообщение уходит системе сохранения).
    SaveToSlot(u8),
    /// Загрузить игру из слота (сообщение уходит системе загрузки).
    LoadFromSlot(u8),
    /// Выйти из игры.
    Quit,
    /// Открыть категорию настроек.
    OpenCategory(SettingsCategory),
    /// «Назад» из категории — к списку категорий.
    BackToSettings,
    /// «Назад» из настроек — в корень меню.
    BackToRoot,
    /// Сбросить все настройки к дефолтам.
    ResetSettings,
    /// Тумблер «Пауза при потере фокуса».
    TogglePauseOnFocusLoss,
    /// Цикл «Ограничение FPS».
    CycleFpsLimit,
    /// Цикл «Режим окна».
    CycleWindowMode,
    /// Цикл «Сглаживание» (Выкл / FXAA / MSAA / SMAA).
    CycleAntialias,
}

/// Кнопка меню. `Hovered` обязателен: пикинг обновляет его только у
/// сущностей, у которых компонент уже есть.
#[derive(Component, Clone, Copy, Debug)]
#[require(Hovered)]
pub struct MenuButton(pub MenuAction);

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
pub struct SliderBar(pub VolumeSlider);

/// Заливка полосы слайдера (ширина — доля значения).
#[derive(Component, Clone, Copy, Debug)]
pub struct SliderFill(pub VolumeSlider);

/// Подпись слайдера: название + текущее значение.
#[derive(Component, Clone, Copy, Debug)]
pub struct SliderLabel(pub VolumeSlider);

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

/// Строка-статус на экранах слотов: результат сохранения/загрузки
/// или текст ошибки. Пересоздаётся вместе с меню.
#[derive(Component)]
pub struct SaveStatusText;

/// Подпись слота на кнопке экрана слотов — обновляется на месте после
/// записи, без пересборки экрана.
#[derive(Component)]
pub struct SlotLabel(pub u8);

/// Маркер корня пауз-меню: по нему меню сносится целиком.
#[derive(Component)]
pub struct PauseMenuRoot;

/// Шрифты меню: bold для заголовков, medium (общий UiFont) для строк.
#[derive(Resource)]
pub(crate) struct MenuFonts {
    bold: Handle<Font>,
    medium: Handle<Font>,
}

/// Загружает шрифты меню. Medium берётся из общего ресурса UI.
fn load_menu_fonts(mut commands: Commands, ui: Res<UiFont>, assets: Res<AssetServer>) {
    commands.insert_resource(MenuFonts {
        bold: assets.load("fonts/FiraSans-Bold.ttf"),
        medium: ui.0.clone(),
    });
}

// ---------- ESC: единая цепочка ----------

/// Обрабатывает ESC для всех модальных меню.
///
/// Порядок: захват бинда (ESC отменяет его в `capture_keybind`, здесь
/// только блокирует навигацию) → технологическое/социальное меню
/// (закрыть) → категория настроек (назад) → корень настроек (назад) →
/// корень пауз-меню (закрыть) → ничего не открыто (открыть меню).
/// Пока фокус в текстовом поле, ESC отдаётся полю ввода.
fn handle_escape(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    mut view: ResMut<PauseMenuView>,
    mut inventory: ResMut<InventoryUi>,
    mut station: ResMut<crate::ui::station::StationUi>,
    mut construction: ResMut<ConstructionUi>,
    capture: Res<BindingCapture>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
    fonts: Res<MenuFonts>,
    settings: Res<Settings>,
    // Корни всех модальных окон одним параметром-кортежем: у систем Bevy
    // предел в 16 параметров, а закрытие окон трогает каждый из них.
    (roots, modal_roots, inventory_roots, station_roots, construction_roots): (
        Query<Entity, With<PauseMenuRoot>>,
        Query<Entity, With<crate::ui::menus::MenuRoot>>,
        Query<Entity, With<InventoryRoot>>,
        Query<Entity, With<crate::ui::station::StationRoot>>,
        Query<Entity, With<ConstructionRoot>>,
    ),
) {
    if text_input_focused(&focus, &text_inputs) || capture.0.is_some() {
        return;
    }
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }

    // Технологическое/социальное/творческое меню: закрываются первым слоем.
    if state.active_menu == Some(MenuKind::Tech)
        || state.active_menu == Some(MenuKind::Social)
        || state.active_menu == Some(MenuKind::Creative)
    {
        for e in &modal_roots {
            commands.entity(e).despawn();
        }
        state.active_menu = None;
        return;
    }

    // Окно станции — второй слой: ESC закрывает его, а не открывает меню
    // (содержимое сетки вернёт close_station_window).
    if station.open {
        station.open = false;
        station.selected = None;
        for e in &station_roots {
            commands.entity(e).despawn();
        }
        return;
    }

    // Окно инвентаря — второй слой: ESC закрывает его, а не открывает меню.
    if inventory.open {
        inventory.open = false;
        for e in &inventory_roots {
            commands.entity(e).despawn();
        }
        return;
    }

    // Меню строительства — тот же слой не-модальных окон, что и инвентарь.
    // ESC сначала снимает выбор постройки (окно снова видно), повторный —
    // закрывает меню.
    if construction.open {
        if construction.selected.is_some() {
            construction.selected = None;
        } else {
            construction.open = false;
            for e in &construction_roots {
                commands.entity(e).despawn();
            }
        }
        return;
    }

    if state.active_menu == Some(MenuKind::Pause) {
        match *view {
            PauseMenuView::Category(_) => {
                *view = PauseMenuView::Settings;
                rebuild_menu(&mut commands, &roots, *view, &settings, &fonts);
            }
            PauseMenuView::Settings | PauseMenuView::SaveSlots | PauseMenuView::LoadSlots => {
                *view = PauseMenuView::Root;
                rebuild_menu(&mut commands, &roots, *view, &settings, &fonts);
            }
            PauseMenuView::Root => {
                close_menu(&mut commands, &roots, &mut state, &mut view, &settings);
            }
        }
        return;
    }

    // Ничего не открыто — ESC открывает меню и ставит игру на паузу.
    state.active_menu = Some(MenuKind::Pause);
    state.paused = true;
    *view = PauseMenuView::Root;
    spawn_pause_menu(&mut commands, *view, &settings, &fonts);
}

/// Закрывает пауз-меню: сносит узлы, снимает паузу, пишет настройки.
/// Используется и загрузкой игры (app/saves.rs) — единственная точка
/// закрытия сохраняет единые правила выхода из меню.
pub(crate) fn close_menu(
    commands: &mut Commands,
    roots: &Query<Entity, With<PauseMenuRoot>>,
    state: &mut GameState,
    view: &mut PauseMenuView,
    settings: &Settings,
) {
    for e in roots {
        commands.entity(e).despawn();
    }
    state.active_menu = None;
    state.paused = false;
    *view = PauseMenuView::Root;
    settings.save(&settings_path());
}

/// Сносит текущий корень меню и строит новый под `view`.
fn rebuild_menu(
    commands: &mut Commands,
    roots: &Query<Entity, With<PauseMenuRoot>>,
    view: PauseMenuView,
    settings: &Settings,
    fonts: &MenuFonts,
) {
    for e in roots {
        commands.entity(e).despawn();
    }
    spawn_pause_menu(commands, view, settings, fonts);
}

// ---------- Кнопки ----------

/// Клик по кнопке меню или бинда. Кнопки биндов начинают захват
/// клавиши, остальные выполняют действие. Пока идёт захват, клики
/// игнорируются, чтобы не открыть категорию той же рукой.
fn on_menu_button_activate(
    trigger: On<Activate>,
    mut commands: Commands,
    mut settings: ResMut<Settings>,
    mut state: ResMut<GameState>,
    mut view: ResMut<PauseMenuView>,
    mut capture: ResMut<BindingCapture>,
    mut exits: MessageWriter<AppExit>,
    mut saves: MessageWriter<SaveGameRequest>,
    mut loads: MessageWriter<LoadGameRequest>,
    fonts: Res<MenuFonts>,
    roots: Query<Entity, With<PauseMenuRoot>>,
    buttons: Query<(&MenuButton, &Children)>,
    binds: Query<&BindButton>,
    mut texts: Query<&mut Text>,
) {
    if capture.0.is_some() {
        return;
    }

    // Кнопка бинда: фронт нажатия начинает захват клавиши. Подсветку
    // переключает update_menu_button_visuals по ресурсу захвата.
    if let Ok(bind) = binds.get(trigger.entity) {
        capture.0 = Some(bind.0);
        return;
    }

    let Ok((button, children)) = buttons.get(trigger.entity) else {
        return;
    };

    match button.0 {
        MenuAction::Continue => close_menu(&mut commands, &roots, &mut state, &mut view, &settings),
        MenuAction::OpenSettings => {
            *view = PauseMenuView::Settings;
            rebuild_menu(&mut commands, &roots, *view, &settings, &fonts);
        }
        MenuAction::OpenSaveSlots => {
            *view = PauseMenuView::SaveSlots;
            rebuild_menu(&mut commands, &roots, *view, &settings, &fonts);
        }
        MenuAction::OpenLoadSlots => {
            *view = PauseMenuView::LoadSlots;
            rebuild_menu(&mut commands, &roots, *view, &settings, &fonts);
        }
        // Сами файловые операции делают системы app/saves.rs: у них свой
        // набор параметров (часы, планировщик, аккумулятор).
        MenuAction::SaveToSlot(slot) => {
            saves.write(SaveGameRequest(slot));
        }
        MenuAction::LoadFromSlot(slot) => {
            loads.write(LoadGameRequest(slot));
        }
        MenuAction::OpenCategory(category) => {
            *view = PauseMenuView::Category(category);
            rebuild_menu(&mut commands, &roots, *view, &settings, &fonts);
        }
        MenuAction::BackToSettings => {
            *view = PauseMenuView::Settings;
            rebuild_menu(&mut commands, &roots, *view, &settings, &fonts);
        }
        MenuAction::BackToRoot => {
            *view = PauseMenuView::Root;
            rebuild_menu(&mut commands, &roots, *view, &settings, &fonts);
        }
        MenuAction::Quit => {
            settings.save(&settings_path());
            exits.write(AppExit::Success);
        }
        MenuAction::ResetSettings => {
            *settings = Settings::default();
            rebuild_menu(&mut commands, &roots, *view, &settings, &fonts);
        }
        MenuAction::TogglePauseOnFocusLoss => {
            settings.pause_on_focus_loss = !settings.pause_on_focus_loss;
            update_first_button_text(children, &mut texts, focus_loss_label(&settings));
        }
        MenuAction::CycleFpsLimit => {
            settings.fps_limit = settings.fps_limit.next();
            update_first_button_text(children, &mut texts, fps_label(&settings));
        }
        MenuAction::CycleWindowMode => {
            settings.window_mode = settings.window_mode.next();
            update_first_button_text(children, &mut texts, window_mode_label(&settings));
        }
        MenuAction::CycleAntialias => {
            settings.antialias = settings.antialias.next();
            update_first_button_text(children, &mut texts, antialias_label(&settings));
        }
    }
}

/// Пишет текст в строку-статус экранов слотов (app/saves.rs).
pub(crate) fn set_status_text(
    status: &Query<Entity, With<SaveStatusText>>,
    texts: &mut Query<&mut Text, Without<SlotLabel>>,
    text: &str,
) {
    for e in status {
        if let Ok(mut t) = texts.get_mut(e) {
            **t = text.to_string();
        }
    }
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

// ---------- Подсветка и звуки ----------

/// Подсветка кнопок меню и биндов: ховер осветляет фон и включает
/// обводку; кнопка в захвате клавиши подсвечена постоянно. Цвет пишется
/// только при смене, чтобы не дёргать переразметку каждый кадр.
fn update_menu_button_visuals(
    capture: Res<BindingCapture>,
    mut menu: Query<
        (&mut BackgroundColor, &Hovered, &mut Outline),
        (With<MenuButton>, Without<BindButton>),
    >,
    mut binds: Query<(&mut BackgroundColor, &Hovered, &mut Outline, &BindButton)>,
) {
    for (mut bg, hovered, mut outline) in &mut menu {
        apply_highlight(&mut bg, &mut outline, style::surface(), hovered.get());
    }
    for (mut bg, hovered, mut outline, bind) in &mut binds {
        let base = if capture.0 == Some(bind.0) {
            style::surface_capture()
        } else {
            style::surface()
        };
        apply_highlight(&mut bg, &mut outline, base, hovered.get());
    }
}

/// Применяет подсветку: фон + обводка акцентом на ховере.
fn apply_highlight(
    bg: &mut BackgroundColor,
    outline: &mut Outline,
    base: Color,
    hovered: bool,
) {
    let want_bg = if hovered { style::surface_hover() } else { base };
    if bg.0 != want_bg {
        bg.0 = want_bg;
    }
    let want_outline = if hovered {
        style::surface_active()
    } else {
        Color::NONE
    };
    if outline.color != want_outline {
        outline.color = want_outline;
    }
}

/// Звук клика по кнопкам меню, биндов, слайдеров и нижней панели.
///
/// Отдельный обсервер на тот же `Activate`: у обработчика действий уже
/// много параметров, а звук нужен от любой интерактивной кнопки.
fn menu_click_sound(
    trigger: On<Activate>,
    mut commands: Commands,
    time: Res<Time>,
    sounds: Res<UiSounds>,
    mut gate: ResMut<SoundGate>,
    settings: Res<Settings>,
    menu: Query<(), With<MenuButton>>,
    binds: Query<(), With<BindButton>>,
    bars: Query<(), With<SliderBar>>,
    panel: Query<(), With<crate::ui::bottom_panel::PanelAction>>,
) {
    if menu.get(trigger.entity).is_err()
        && binds.get(trigger.entity).is_err()
        && bars.get(trigger.entity).is_err()
        && panel.get(trigger.entity).is_err()
    {
        return;
    }
    play_menu_sound(
        &mut commands,
        sounds.click.clone(),
        &mut gate,
        &time,
        &settings,
    );
}

/// Звук выделения: мышь встала на кнопку или слайдер.
///
/// Нюанс движка: `Hovered` — bool-обёртка, которую пикинг INSERT'ит
/// один раз и дальше обновляет на месте. Поэтому фронт `Added<Hovered>`
/// срабатывал бы ровно один раз за жизнь кнопки. Переход ловим сами:
/// множество сущностей под курсором в прошлом кадре лежит в `Local`,
/// новые — это разница. Очередь при быстром обходе режет гейт.
fn menu_select_sound(
    mut commands: Commands,
    time: Res<Time>,
    sounds: Res<UiSounds>,
    mut gate: ResMut<SoundGate>,
    settings: Res<Settings>,
    buttons: Query<
        (Entity, &Hovered),
        Or<(
            With<MenuButton>,
            With<BindButton>,
            With<SliderBar>,
            With<crate::ui::bottom_panel::PanelAction>,
        )>,
    >,
    mut prev_hovered: Local<std::collections::HashSet<Entity>>,
    mut hovered_now: Local<Vec<Entity>>,
) {
    hovered_now.clear();
    hovered_now.extend(buttons.iter().filter_map(|(e, h)| h.get().then_some(e)),
    );
    if !hover_just_entered(&hovered_now, &mut prev_hovered) {
        return;
    }
    play_menu_sound(
        &mut commands,
        sounds.select.clone(),
        &mut gate,
        &time,
        &settings,
    );
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
fn update_sliders(
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
fn capture_keybind(
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

/// Создаёт экран пауз-меню под текущий `view`.
fn spawn_pause_menu(
    commands: &mut Commands,
    view: PauseMenuView,
    settings: &Settings,
    fonts: &MenuFonts,
) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                bottom: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(14.0),
                ..default()
            },
            BackgroundColor(style::overlay_strong()),
            PauseMenuRoot,
            // Оверлей на весь экран: карта позади меню не реагирует на клики.
            BlocksWorldInput,
        ))
        .with_children(|overlay| {
            overlay.spawn((
                Text::new(title_of(view)),
                TextFont {
                    font: fonts.bold.clone().into(),
                    font_size: 26.0.into(),
                    ..default()
                },
                TextColor(style::text()),
            ));

            overlay
                .spawn((Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(8.0),
                    ..default()
                },))
                .with_children(|content| match view {
                    PauseMenuView::Root => build_root(content, fonts),
                    PauseMenuView::Settings => build_settings_root(content, fonts),
                    PauseMenuView::Category(category) => build_category(content, fonts, settings, category),
                    PauseMenuView::SaveSlots => build_slot_picker(content, fonts, true),
                    PauseMenuView::LoadSlots => build_slot_picker(content, fonts, false),
                });

            overlay.spawn((
                Text::new(hint_of(view)),
                TextFont {
                    font: fonts.medium.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
        });
}

/// Заголовок экрана.
fn title_of(view: PauseMenuView) -> &'static str {
    match view {
        PauseMenuView::Root => "Пауза",
        PauseMenuView::Settings => "Настройки",
        PauseMenuView::Category(c) => c.name(),
        PauseMenuView::SaveSlots => "Сохранить игру",
        PauseMenuView::LoadSlots => "Загрузить игру",
    }
}

/// Подсказка по ESC под меню.
fn hint_of(view: PauseMenuView) -> &'static str {
    match view {
        PauseMenuView::Root => "Esc — продолжить",
        _ => "Esc — назад",
    }
}

/// Подпись слота в списке: дата из заголовка файла или «пусто».
pub(crate) fn slot_label(slot: u8, info: Option<&SaveHeader>) -> String {
    match info {
        Some(info) => {
            let clock = GameClock { tick: info.tick };
            format!(
                "Слот {}: Год {}, {}, день {}, {:02}:{:02}",
                slot,
                clock.year() + 1,
                clock.season().label(),
                clock.day_of_month() + 1,
                clock.hour_of_day(),
                clock.minute_of_hour()
            )
        }
        None => format!("Слот {slot}: пусто"),
    }
}

/// Корень меню: игровые действия.
fn build_root(parent: &mut ChildSpawnerCommands, fonts: &MenuFonts) {
    action_button(parent, fonts, "Продолжить игру", MenuAction::Continue, style::text());
    action_button(parent, fonts, "Настройки", MenuAction::OpenSettings, style::text());
    action_button(parent, fonts, "Сохранить игру", MenuAction::OpenSaveSlots, style::text());
    action_button(parent, fonts, "Загрузить игру", MenuAction::OpenLoadSlots, style::text());
    action_button(parent, fonts, "Выйти из игры", MenuAction::Quit, style::danger());
}

/// Экран выбора слота: 4 кнопки с датами, статус и «Назад».
/// Один и тот же список для записи и чтения; действие отличает клик.
fn build_slot_picker(parent: &mut ChildSpawnerCommands, fonts: &MenuFonts, save_mode: bool) {
    for slot in 1..=SAVE_SLOTS {
        let label = slot_label(slot, slot_info(slot).as_ref());
        let action = if save_mode {
            MenuAction::SaveToSlot(slot)
        } else {
            MenuAction::LoadFromSlot(slot)
        };
        slot_button(parent, fonts, &label, action, slot);
    }

    parent.spawn((
        Text::new(" "),
        TextFont {
            font: fonts.medium.clone().into(),
            font_size: 13.0.into(),
            ..default()
        },
        TextColor(style::text_dim()),
        SaveStatusText,
    ));

    action_button(parent, fonts, "Назад", MenuAction::BackToRoot, style::text());
}

/// Кнопка слота с маркером на подписи: после записи подпись обновляется
/// на месте, без пересборки экрана.
fn slot_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &MenuFonts,
    text: &str,
    action: MenuAction,
    slot: u8,
) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(480.0),
                height: Val::Px(40.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::surface()),
            BorderColor::all(style::border()),
            Outline::new(Val::Px(2.0), Val::Px(0.0), Color::NONE),
            MenuButton(action),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(text.to_string()),
                TextFont {
                    font: fonts.medium.clone().into(),
                    font_size: 16.0.into(),
                    ..default()
                },
                TextColor(style::text()),
                SlotLabel(slot),
            ));
        });
}

/// Корень настроек: категории, сброс, назад.
fn build_settings_root(parent: &mut ChildSpawnerCommands, fonts: &MenuFonts) {
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
fn build_category(
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

/// Кнопка-строка на всю ширину меню.
fn action_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &MenuFonts,
    text: &str,
    action: MenuAction,
    color: Color,
) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(480.0),
                height: Val::Px(40.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(style::surface()),
            BorderColor::all(style::border()),
            // Обводка подсветки: висит всегда с NONE, цвет ставит
            // система подсветки (вне layout — соседей не раздвигает).
            Outline::new(Val::Px(2.0), Val::Px(0.0), Color::NONE),
            MenuButton(action),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(text.to_string()),
                TextFont {
                    font: fonts.medium.clone().into(),
                    font_size: 17.0.into(),
                    ..default()
                },
                TextColor(color),
            ));
        });
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
