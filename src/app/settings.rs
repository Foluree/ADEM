//! Настройки игрока: громкости, графика, управление.
//!
//! Хранятся в `settings.json` рядом с exe (или в рабочем каталоге при
//! `cargo run`). Битый файл не мешает запуску: недостающие поля берутся
//! из serde-дефолтов, некорректные — исправляются в `clamp`.

use bevy::audio::Volume;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Имя файла настроек.
pub const SETTINGS_PATH: &str = "settings.json";

/// Шина звука. Громкость каждой настраивается отдельно в меню «Звуки».
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SoundBus {
    /// Фоновая атмосфера (assets/sounds/ambient).
    Ambient,
    /// Игровые события (появятся вместе с существами/эффектами).
    Effect,
    /// Звуки существ.
    Creature,
    /// Звуки интерфейса: наведение, клики, слайдеры.
    Ui,
}

impl SoundBus {
    /// Подпись в меню «Звуки».
    pub fn label(self) -> &'static str {
        match self {
            SoundBus::Ambient => "Окружение",
            SoundBus::Effect => "Эффекты",
            SoundBus::Creature => "Существа",
            SoundBus::Ui => "Интерфейс",
        }
    }
}

/// Ограничитель частоты кадров в категории «Графика».
///
/// `Vsync` — без жёсткого лимита (ограничивает монитор), остальные —
/// жёсткий кап через период обновления цикла winit.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FpsLimit {
    Fps15,
    Fps24,
    Fps30,
    Fps60,
    Fps120,
    #[default]
    Vsync,
}

impl FpsLimit {
    /// Подпись на кнопке.
    pub fn name(self) -> &'static str {
        match self {
            FpsLimit::Fps15 => "15",
            FpsLimit::Fps24 => "24",
            FpsLimit::Fps30 => "30",
            FpsLimit::Fps60 => "60",
            FpsLimit::Fps120 => "120",
            FpsLimit::Vsync => "VSync",
        }
    }

    /// Следующий вариант по клику.
    pub fn next(self) -> Self {
        match self {
            FpsLimit::Fps15 => FpsLimit::Fps24,
            FpsLimit::Fps24 => FpsLimit::Fps30,
            FpsLimit::Fps30 => FpsLimit::Fps60,
            FpsLimit::Fps60 => FpsLimit::Fps120,
            FpsLimit::Fps120 => FpsLimit::Vsync,
            FpsLimit::Vsync => FpsLimit::Fps15,
        }
    }

    /// Целевая частота кадров. `None` — без жёсткого лимита (VSync).
    pub fn target_fps(self) -> Option<u32> {
        match self {
            FpsLimit::Fps15 => Some(15),
            FpsLimit::Fps24 => Some(24),
            FpsLimit::Fps30 => Some(30),
            FpsLimit::Fps60 => Some(60),
            FpsLimit::Fps120 => Some(120),
            FpsLimit::Vsync => None,
        }
    }
}

/// Режим сглаживания в категории «Графика».
///
/// `Off` — без сглаживания (быстрее всего), `Fxaa` и `Smaa` — постпроцесс
/// поверх кадра, `Msaa` — аппаратное мультисэмплирование 4x.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AntiAlias {
    /// Сглаживание отключено.
    #[default]
    Off,
    /// FXAA: быстрый постпроцесс, слегка размывает картинку.
    Fxaa,
    /// MSAA 4x: аппаратное сглаживание геометрии.
    Msaa,
    /// SMAA: постпроцесс высокого качества, резче FXAA.
    Smaa,
}

impl AntiAlias {
    /// Подпись на кнопке.
    pub fn name(self) -> &'static str {
        match self {
            AntiAlias::Off => "Выкл",
            AntiAlias::Fxaa => "FXAA",
            AntiAlias::Msaa => "MSAA",
            AntiAlias::Smaa => "SMAA",
        }
    }

    /// Следующий режим по клику.
    pub fn next(self) -> Self {
        match self {
            AntiAlias::Off => AntiAlias::Fxaa,
            AntiAlias::Fxaa => AntiAlias::Msaa,
            AntiAlias::Msaa => AntiAlias::Smaa,
            AntiAlias::Smaa => AntiAlias::Off,
        }
    }
}

/// Режим окна в категории «Графика».
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowModeSetting {
    /// Оконный.
    #[default]
    Windowed,
    /// Полноэкранный безрамочный (текущий монитор).
    Borderless,
    /// Эксклюзивный полноэкранный (текущий монитор и видеорежим).
    Exclusive,
}

impl WindowModeSetting {
    /// Подпись на кнопке.
    pub fn name(self) -> &'static str {
        match self {
            WindowModeSetting::Windowed => "Оконный",
            WindowModeSetting::Borderless => "Безрамочный",
            WindowModeSetting::Exclusive => "Эксклюзивный",
        }
    }

    /// Следующий режим по клику.
    pub fn next(self) -> Self {
        match self {
            WindowModeSetting::Windowed => WindowModeSetting::Borderless,
            WindowModeSetting::Borderless => WindowModeSetting::Exclusive,
            WindowModeSetting::Exclusive => WindowModeSetting::Windowed,
        }
    }
}

/// Перебиндиваемое действие управления.
///
/// ESC и F11 — системные (меню/назад и режим окна), не перебиндиваются.
/// F1 (режим игрока) — тоже системная, по решению автора проекта.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControlAction {
    /// Пауза симуляции (по умолчанию Space).
    Pause,
    /// Меню технологий (по умолчанию T).
    TechMenu,
    /// Меню социальных институтов (по умолчанию Y).
    SocialMenu,
    /// Творческое меню (по умолчанию I): все предметы, клик — полный стак.
    CreativeMenu,
    /// Окно инвентаря (по умолчанию Tab).
    Inventory,
    /// «Назад» из категории редактора к базовым кнопкам панели
    /// (по умолчанию Q).
    EditorBack,
    /// Бросить один предмет из выбранного слота по направлению курсора
    /// (по умолчанию G).
    ThrowItem,
    /// Меню строительства (по умолчанию C).
    ConstructionMenu,
    /// Движение вверх (по умолчанию W): существо в игре, камера — в редакторе.
    MoveUp,
    /// Движение вниз (по умолчанию S).
    MoveDown,
    /// Движение влево (по умолчанию A).
    MoveLeft,
    /// Движение вправо (по умолчанию D).
    MoveRight,
    /// Положить предмет из руки на землю (по умолчанию V).
    DropItem,
}

impl ControlAction {
    /// Все действия в порядке меню «Управление».
    pub const ALL: [ControlAction; 13] = [
        ControlAction::Pause,
        ControlAction::TechMenu,
        ControlAction::SocialMenu,
        ControlAction::CreativeMenu,
        ControlAction::Inventory,
        ControlAction::EditorBack,
        ControlAction::ThrowItem,
        ControlAction::ConstructionMenu,
        ControlAction::MoveUp,
        ControlAction::MoveDown,
        ControlAction::MoveLeft,
        ControlAction::MoveRight,
        ControlAction::DropItem,
    ];

    /// Подпись действия в меню.
    pub fn label(self) -> &'static str {
        match self {
            ControlAction::Pause => "Пауза",
            ControlAction::TechMenu => "Меню технологий",
            ControlAction::SocialMenu => "Меню институтов",
            ControlAction::CreativeMenu => "Творческое меню",
            ControlAction::Inventory => "Инвентарь",
            ControlAction::EditorBack => "Назад (редактор)",
            ControlAction::ThrowItem => "Бросить предмет",
            ControlAction::ConstructionMenu => "Меню строительства",
            ControlAction::MoveUp => "Вверх",
            ControlAction::MoveDown => "Вниз",
            ControlAction::MoveLeft => "Влево",
            ControlAction::MoveRight => "Вправо",
            ControlAction::DropItem => "Положить предмет",
        }
    }

    /// Клавиша по умолчанию (откат при битом имени в файле).
    pub fn default_key(self) -> KeyCode {
        match self {
            ControlAction::Pause => KeyCode::Space,
            ControlAction::TechMenu => KeyCode::KeyT,
            ControlAction::SocialMenu => KeyCode::KeyY,
            ControlAction::CreativeMenu => KeyCode::KeyI,
            ControlAction::Inventory => KeyCode::Tab,
            ControlAction::EditorBack => KeyCode::KeyQ,
            ControlAction::ThrowItem => KeyCode::KeyG,
            ControlAction::ConstructionMenu => KeyCode::KeyC,
            ControlAction::MoveUp => KeyCode::KeyW,
            ControlAction::MoveDown => KeyCode::KeyS,
            ControlAction::MoveLeft => KeyCode::KeyA,
            ControlAction::MoveRight => KeyCode::KeyD,
            ControlAction::DropItem => KeyCode::KeyV,
        }
    }
}

/// Привязки клавиш. Хранятся строками: у `KeyCode` в нашей сборке bevy
/// нет serde, а строки в settings.json читаются глазами прямо в файле.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Controls {
    #[serde(default = "d_key_pause")]
    pub pause: String,
    #[serde(default = "d_key_tech")]
    pub tech_menu: String,
    #[serde(default = "d_key_social")]
    pub social_menu: String,
    #[serde(default = "d_key_creative")]
    pub creative_menu: String,
    #[serde(default = "d_key_inventory")]
    pub inventory: String,
    #[serde(default = "d_key_editor_back")]
    pub editor_back: String,
    #[serde(default = "d_key_construction")]
    pub construction: String,
    #[serde(default = "d_key_move_up")]
    pub move_up: String,
    #[serde(default = "d_key_move_down")]
    pub move_down: String,
    #[serde(default = "d_key_move_left")]
    pub move_left: String,
    #[serde(default = "d_key_move_right")]
    pub move_right: String,
    #[serde(default = "d_key_drop_item")]
    pub drop_item: String,
    #[serde(default = "d_key_throw_item")]
    pub throw_item: String,
}

fn d_key_pause() -> String {
    keycode_name(KeyCode::Space).to_string()
}
fn d_key_tech() -> String {
    keycode_name(KeyCode::KeyT).to_string()
}
fn d_key_social() -> String {
    keycode_name(KeyCode::KeyY).to_string()
}
fn d_key_creative() -> String {
    keycode_name(KeyCode::KeyI).to_string()
}
fn d_key_inventory() -> String {
    keycode_name(KeyCode::Tab).to_string()
}
fn d_key_editor_back() -> String {
    keycode_name(KeyCode::KeyQ).to_string()
}
fn d_key_construction() -> String {
    keycode_name(KeyCode::KeyC).to_string()
}
fn d_key_move_up() -> String {
    keycode_name(KeyCode::KeyW).to_string()
}
fn d_key_move_down() -> String {
    keycode_name(KeyCode::KeyS).to_string()
}
fn d_key_move_left() -> String {
    keycode_name(KeyCode::KeyA).to_string()
}
fn d_key_move_right() -> String {
    keycode_name(KeyCode::KeyD).to_string()
}
fn d_key_drop_item() -> String {
    keycode_name(KeyCode::KeyV).to_string()
}
fn d_key_throw_item() -> String {
    keycode_name(KeyCode::KeyG).to_string()
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            pause: d_key_pause(),
            tech_menu: d_key_tech(),
            social_menu: d_key_social(),
            creative_menu: d_key_creative(),
            inventory: d_key_inventory(),
            editor_back: d_key_editor_back(),
            construction: d_key_construction(),
            move_up: d_key_move_up(),
            move_down: d_key_move_down(),
            move_left: d_key_move_left(),
            move_right: d_key_move_right(),
            drop_item: d_key_drop_item(),
            throw_item: d_key_throw_item(),
        }
    }
}

impl Controls {
    /// Клавиша действия с откатом на дефолт при битом имени:
    /// повреждённый settings.json не должен отбирать управление.
    pub fn key(&self, action: ControlAction) -> KeyCode {
        let name = match action {
            ControlAction::Pause => &self.pause,
            ControlAction::TechMenu => &self.tech_menu,
            ControlAction::SocialMenu => &self.social_menu,
            ControlAction::CreativeMenu => &self.creative_menu,
            ControlAction::Inventory => &self.inventory,
            ControlAction::EditorBack => &self.editor_back,
            ControlAction::ConstructionMenu => &self.construction,
            ControlAction::MoveUp => &self.move_up,
            ControlAction::MoveDown => &self.move_down,
            ControlAction::MoveLeft => &self.move_left,
            ControlAction::MoveRight => &self.move_right,
            ControlAction::DropItem => &self.drop_item,
            ControlAction::ThrowItem => &self.throw_item,
        };
        parse_keycode(name).unwrap_or_else(|| action.default_key())
    }
    /// Назначает действию новую клавишу.
    pub fn set(&mut self, action: ControlAction, code: KeyCode) {
        let name = keycode_name(code).to_string();
        match action {
            ControlAction::Pause => self.pause = name,
            ControlAction::TechMenu => self.tech_menu = name,
            ControlAction::SocialMenu => self.social_menu = name,
            ControlAction::CreativeMenu => self.creative_menu = name,
            ControlAction::Inventory => self.inventory = name,
            ControlAction::EditorBack => self.editor_back = name,
            ControlAction::ConstructionMenu => self.construction = name,
            ControlAction::MoveUp => self.move_up = name,
            ControlAction::MoveDown => self.move_down = name,
            ControlAction::MoveLeft => self.move_left = name,
            ControlAction::MoveRight => self.move_right = name,
            ControlAction::DropItem => self.drop_item = name,
            ControlAction::ThrowItem => self.throw_item = name,
        }
    }
}

/// Клавиши, доступные для биндов: буквы, цифры, стрелки, пробел.
const BINDABLE_KEYS: [(KeyCode, &str); 90] = [
    (KeyCode::KeyA, "A"),
    (KeyCode::KeyB, "B"),
    (KeyCode::KeyC, "C"),
    (KeyCode::KeyD, "D"),
    (KeyCode::KeyE, "E"),
    (KeyCode::KeyF, "F"),
    (KeyCode::KeyG, "G"),
    (KeyCode::KeyH, "H"),
    (KeyCode::KeyI, "I"),
    (KeyCode::KeyJ, "J"),
    (KeyCode::KeyK, "K"),
    (KeyCode::KeyL, "L"),
    (KeyCode::KeyM, "M"),
    (KeyCode::KeyN, "N"),
    (KeyCode::KeyO, "O"),
    (KeyCode::KeyP, "P"),
    (KeyCode::KeyQ, "Q"),
    (KeyCode::KeyR, "R"),
    (KeyCode::KeyS, "S"),
    (KeyCode::KeyT, "T"),
    (KeyCode::KeyU, "U"),
    (KeyCode::KeyV, "V"),
    (KeyCode::KeyW, "W"),
    (KeyCode::KeyX, "X"),
    (KeyCode::KeyY, "Y"),
    (KeyCode::KeyZ, "Z"),
    (KeyCode::Digit0, "0"),
    (KeyCode::Digit1, "1"),
    (KeyCode::Digit2, "2"),
    (KeyCode::Digit3, "3"),
    (KeyCode::Digit4, "4"),
    (KeyCode::Digit5, "5"),
    (KeyCode::Digit6, "6"),
    (KeyCode::Digit7, "7"),
    (KeyCode::Digit8, "8"),
    (KeyCode::Digit9, "9"),
    (KeyCode::ArrowUp, "Up"),
    (KeyCode::ArrowDown, "Down"),
    (KeyCode::ArrowLeft, "Left"),
    (KeyCode::ArrowRight, "Right"),
    (KeyCode::Space, "Space"),
    (KeyCode::Tab, "Tab"),
    (KeyCode::Backspace, "Backspace"),
    (KeyCode::Delete, "Delete"),
    (KeyCode::End, "End"),
    (KeyCode::Help, "Help"),
    (KeyCode::Home, "Home"),
    (KeyCode::PageDown, "PageDown"),
    (KeyCode::PageUp, "PageUp"),
    (KeyCode::Backquote, "~"),
    (KeyCode::Backslash, "\\"),
    (KeyCode::BracketLeft, "["),
    (KeyCode::BracketRight, "]"),
    (KeyCode::Comma, ","),
    (KeyCode::Equal, "="),
    (KeyCode::Minus, "-"),
    (KeyCode::Period, "."),
    (KeyCode::Quote, "'"),
    (KeyCode::Semicolon, ";"),
    (KeyCode::Slash, "/"),

    (KeyCode::Numpad0, "Numpad 0"),
    (KeyCode::Numpad1, "Numpad 1"),
    (KeyCode::Numpad2, "Numpad 2"),
    (KeyCode::Numpad3, "Numpad 3"),
    (KeyCode::Numpad4, "Numpad 4"),
    (KeyCode::Numpad5, "Numpad 5"),
    (KeyCode::Numpad6, "Numpad 6"),
    (KeyCode::Numpad7, "Numpad 7"),
    (KeyCode::Numpad8, "Numpad 8"),
    (KeyCode::Numpad9, "Numpad 9"),
    (KeyCode::NumpadAdd, "Numpad Add"),
    (KeyCode::NumpadBackspace, "Numpad Backspace"),
    (KeyCode::NumpadClear, "Numpad Clear"),
    (KeyCode::NumpadClearEntry, "Numpad Clear Entry"),
    (KeyCode::NumpadComma, "Numpad Comma"),
    (KeyCode::NumpadDecimal, "Numpad Decimal"),
    (KeyCode::NumpadDivide, "Numpad Divide"),
    (KeyCode::NumpadEnter, "Numpad Enter"),
    (KeyCode::NumpadEqual, "Numpad Equal"),
    (KeyCode::NumpadHash, "Numpad Hash"),
    (KeyCode::NumpadMemoryAdd, "Numpad Memory Add"),
    (KeyCode::NumpadMemoryClear, "Numpad Memory Clear"),
    (KeyCode::NumpadMemoryRecall, "Numpad Memory Recall"),
    (KeyCode::NumpadMemoryStore, "Numpad Memory Store"),
    (KeyCode::NumpadMemorySubtract, "Numpad Memory Subtract"),
    (KeyCode::NumpadMultiply, "Numpad Multiply"),
    (KeyCode::NumpadParenLeft, "Numpad Left Parenthesis"),
    (KeyCode::NumpadParenRight, "Numpad Right Parenthesis"),
    (KeyCode::NumpadStar, "Numpad Star"),
    (KeyCode::NumpadSubtract, "Numpad Subtract"),
];

/// Короткое имя клавиши для кнопок меню и settings.json.
pub fn keycode_name(code: KeyCode) -> &'static str {
    BINDABLE_KEYS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
        .unwrap_or("?")
}

/// Обратный разбор имени. Понимает и полные имена стрелок из winit
/// (`ArrowUp`), чтобы не удивлять вставивших их руками в файл.
pub fn parse_keycode(name: &str) -> Option<KeyCode> {
    let name = name.trim();
    if let Some((code, _)) = BINDABLE_KEYS
        .iter()
        .find(|(_, known)| known.eq_ignore_ascii_case(name))
    {
        return Some(*code);
    }
    match name.to_ascii_lowercase().as_str() {
        "arrowup" => Some(KeyCode::ArrowUp),
        "arrowdown" => Some(KeyCode::ArrowDown),
        "arrowleft" => Some(KeyCode::ArrowLeft),
        "arrowright" => Some(KeyCode::ArrowRight),
        _ => None,
    }
}

fn default_volume() -> f32 {
    1.0
}
fn default_ambient() -> f32 {
    0.75
}
fn default_effect() -> f32 {
    0.9
}
fn default_creature() -> f32 {
    0.9
}
fn default_ui() -> f32 {
    0.9
}
fn default_true() -> bool {
    true
}

/// Настройки игрока из settings.json.
#[derive(Resource, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Settings {
    /// Общая громкость. Множитель для всех остальных шин.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Громкость фоновой атмосферы.
    #[serde(default = "default_ambient")]
    pub volume_ambient: f32,
    /// Громкость игровых эффектов.
    #[serde(default = "default_effect")]
    pub volume_effect: f32,
    /// Громкость звуков существ.
    #[serde(default = "default_creature")]
    pub volume_creature: f32,
    /// Громкость интерфейса.
    #[serde(default = "default_ui")]
    pub volume_ui: f32,

    /// Ограничитель частоты кадров (категория «Графика»).
    #[serde(default)]
    pub fps_limit: FpsLimit,
    /// Режим окна: оконный / безрамочный / эксклюзивный.
    #[serde(default)]
    pub window_mode: WindowModeSetting,
    /// Сглаживание: Выкл / FXAA / MSAA / SMAA (категория «Графика»).
    #[serde(default)]
    pub antialias: AntiAlias,

    /// Ставить ли игру на паузу при потере фокуса или сворачивании.
    #[serde(default = "default_true")]
    pub pause_on_focus_loss: bool,

    /// Перебиндиваемые клавиши.
    #[serde(default)]
    pub controls: Controls,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: default_volume(),
            volume_ambient: default_ambient(),
            volume_effect: default_effect(),
            volume_creature: default_creature(),
            volume_ui: default_ui(),
            fps_limit: FpsLimit::default(),
            window_mode: WindowModeSetting::default(),
            antialias: AntiAlias::default(),
            pause_on_focus_loss: true,
            controls: Controls::default(),
        }
    }
}

/// Путь к settings.json: сначала рабочий каталог (cargo run из корня
/// проекта), затем каталог exe (запуск собранной игры напрямую).
pub fn settings_path() -> PathBuf {
    if Path::new(SETTINGS_PATH).is_file() {
        return PathBuf::from(SETTINGS_PATH);
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    exe_dir.join(SETTINGS_PATH)
}

impl Settings {
    /// Загружает настройки, исправляя некорректные значения.
    pub fn load(path: &Path) -> Self {
        let mut settings: Settings = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        settings.clamp();
        settings
    }

    /// Пишет настройки на диск. Ошибка не фатальна: игра продолжит
    /// работать с настройками в памяти.
    pub fn save(&self, path: &Path) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    /// Значения из файла могут быть любыми (в том числе битыми),
    /// поэтому всё приводим к допустимому диапазону.
    pub fn clamp(&mut self) {
        self.volume = clamp01(self.volume, default_volume());
        self.volume_ambient = clamp01(self.volume_ambient, default_ambient());
        self.volume_effect = clamp01(self.volume_effect, default_effect());
        self.volume_creature = clamp01(self.volume_creature, default_creature());
        self.volume_ui = clamp01(self.volume_ui, default_ui());
        // Битые имена клавиш откатываются по одной, остальное живёт.
        for action in ControlAction::ALL {
            let key = self.controls.key(action);
            self.controls.set(action, key);
        }
    }

    /// Итоговая громкость шины: общая * частная.
    pub fn bus_volume(&self, bus: SoundBus) -> f32 {
        let specific = match bus {
            SoundBus::Ambient => self.volume_ambient,
            SoundBus::Effect => self.volume_effect,
            SoundBus::Creature => self.volume_creature,
            SoundBus::Ui => self.volume_ui,
        };
        self.volume * specific
    }

    /// Амплитуда для bevy. Bevy масштабирует амплитуду, а не
    /// воспринимаемую громкость, поэтому значение возводим в квадрат —
    /// иначе половина шкалы ползунка звучала бы почти как полная.
    pub fn amplitude_for(&self, bus: SoundBus) -> f32 {
        self.bus_volume(bus).powi(2)
    }
}

/// Громкость для bevy: с учётом возведения в квадрат (см. amplitude_for).
pub fn volume_for(settings: &Settings, bus: SoundBus) -> Volume {
    Volume::Linear(settings.amplitude_for(bus))
}

fn clamp01(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        fallback
    }
}

// Старый settings.json без новых полей должен грузиться: у всех добавленных полей есть serde-дефолт.
