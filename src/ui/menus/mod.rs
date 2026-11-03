//! Модальные меню технологий (T) и соц. институтов (Y): полноэкранные
//! деревья слева-направо из `data/science/tech_*.json` и
//! `data/culture/social_*.json`. Фон меню — текстура `textures/ui/bg.png`,
//! см. `ui/menu_bg.rs`.
//!
//! Состав модуля:
//! - `tech` — данные технологий и их загрузка;
//! - `social` — данные институтов и их загрузка;
//! - `tree_view` — общий движок дерева: раскладка, отрисовка, обзор
//!   (панорама/зум), валидация данных.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::TextInput;

use crate::app::settings::{ControlAction, Settings};
use crate::app::state::{GameState, MenuKind};
use crate::ui::menu_bg::{self, MenuBgCache};
use crate::ui::pause_menu::BindingCapture;
use crate::ui::{text_input_focused, UiFont};

mod social;
mod tech;
mod tree_view;

use social::{SocialData, load_social_dir};
use tech::{TechData, load_tech_dir};

// Движок дерева. Часть имен видна соседям (creative.rs, menu_bg.rs,
// pause_menu.rs используют MenuRoot через crate::ui::menus::MenuRoot).
pub(crate) use tree_view::{MenuRoot, cleanup_tree_view, pan_zoom_tree, spawn_tree, validate_tree};

pub struct MenusPlugin;

impl Plugin for MenusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TechData>()
            .init_resource::<SocialData>()
            .init_resource::<MenuBgCache>()
            .add_systems(Startup, (load_data, menu_bg::load_bg_source))
            .add_systems(
                Update,
                (toggle_menus, cleanup_tree_view, pan_zoom_tree, menu_bg::update_menu_background).chain(),
            );
    }
}

/// Читает каталоги данных эпох: технологии — `data/science/tech_*.json`,
/// институты — `data/culture/social_*.json`. Файлы эпох сливаются в
/// порядке лексикографии имён (детерминизм, как у рецептов крафта);
/// ошибки отдельных файлов не роняют игру — только лог. После загрузки
/// данные проверяются на типовые ошибки разметки (см. `validate_tree`).
fn load_data(mut tech: ResMut<TechData>, mut social: ResMut<SocialData>) {
    *tech = load_tech_dir("data/science");
    *social = load_social_dir("data/culture");
    validate_tree("технологии", &tech.technologies);
    validate_tree("институты", &social.institutions);
}

/// Обрабатывает горячие клавиши меню (бинды из настроек). Пока фокус
/// в текстовом поле, идёт захват бинда или открыт пауз-меню,
/// хоткеи не срабатывают.
fn toggle_menus(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    settings: Res<Settings>,
    capture: Res<BindingCapture>,
    tech: Res<TechData>,
    social: Res<SocialData>,
    bg: Res<MenuBgCache>,
    font: Res<UiFont>,
    windows: Query<&Window>,
    existing: Query<Entity, With<MenuRoot>>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
) {
    if text_input_focused(&focus, &text_inputs) || capture.0.is_some() {
        return;
    }
    if state.active_menu == Some(MenuKind::Pause) {
        return;
    }
    let want_tech = keys.just_pressed(settings.controls.key(ControlAction::TechMenu));
    let want_social = keys.just_pressed(settings.controls.key(ControlAction::SocialMenu));
    if !want_tech && !want_social {
        return;
    }
    let target = if want_tech { MenuKind::Tech } else { MenuKind::Social };

    let mut already_open = false;
    for e in &existing {
        commands.entity(e).despawn();
        already_open = true;
    }
    if already_open && state.active_menu == Some(target) {
        state.active_menu = None;
        return;
    }

    state.active_menu = Some(target);
    spawn_tree(&mut commands, target, &tech, &social, &bg, &font, &windows);
}

#[cfg(test)]
#[path = "../../tests/menus.rs"]
mod tests;
