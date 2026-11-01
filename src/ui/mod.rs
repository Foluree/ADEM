//! UI: нижняя панель, FPS-счётчик, модальные меню, инвентарь, станции.

pub mod bottom_panel;
pub mod construction;
pub mod creative;
pub mod fps;
pub mod health;
pub mod hotbar;
pub mod icons;
pub mod inspector;
pub mod inventory;
pub mod menu_bg;
pub mod menus;
pub mod pause_menu;
pub mod stations;
pub mod style;
pub mod tooltip;

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::TextInput;

use icons::UiIcons;

pub use inventory::CraftBookRes;

/// Ресурс с хэндлом шрифта, чтобы все тексты использовали FiraSans-Medium.
#[derive(Resource)]
pub struct UiFont(pub Handle<Font>);

/// Проверяет, находится ли фокус в текстовом поле ввода.
/// Глобальные хоткеи (пауза, меню) во время набора не должны срабатывать.
pub fn text_input_focused(focus: &InputFocus, inputs: &Query<(), With<TextInput>>) -> bool {
    matches!(focus.get(), Some(entity) if inputs.contains(entity))
}

/// Маркер корневого узла UI, перекрывающего ввод по карте: клики ЛКМ
/// поверх таких панелей не должны ставить тайлы и добывать ресурсы.
#[derive(Component)]
pub struct BlocksWorldInput;

/// Перезаписывает текст только при изменении. Лишний `format!` + запись
/// в `Text` заставляет bevy заново размечать глифы каждый кадр — подписи,
/// обновляющиеся «на всякий случай», пишут через этот хелпер.
/// (Для прочих компонентов есть штатный `Mut::set_if_neq`.)
pub fn set_text_if_neq(mut text: Mut<'_, Text>, want: &str) {
    if text.as_str() != want {
        **text = want.to_string();
    }
}

/// Накрывает ли курсор (в логических пикселях окна) узел UI. Общее ядро
/// всех хит-тестов UI (`cursor_over_ui`, тултипы, слоты инвентаря):
/// прямоугольники берутся из вычисленной разметки в физических пикселях,
/// позиция курсора переводится в физические — так же, как в пикинге bevy.
pub fn cursor_over_node(
    window: &Window,
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
    cursor: Vec2,
) -> bool {
    let scale = window.scale_factor().max(f32::EPSILON);
    let cursor_phys = cursor * scale;
    computed
        .normalize_point(*transform, cursor_phys)
        .is_some_and(|p| p.x.abs() <= 0.5 && p.y.abs() <= 0.5)
}

/// Есть ли курсор над узлом с маркером `BlocksWorldInput`.
/// Скрытые узлы (`Visibility::Hidden`, например панель строительства
/// при выбранной постройке) ввод не перехватывают, как и в штатном
/// пикинге bevy.
pub fn cursor_over_ui(
    window: &Window,
    blockers: &Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
) -> bool {
    let Some(cursor) = window.cursor_position() else {
        return false;
    };
    for (computed, transform, visibility) in blockers {
        if *visibility == Visibility::Hidden {
            continue;
        }
        if cursor_over_node(window, computed, transform, cursor) {
            return true;
        }
    }
    false
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // PreStartup: `UiFont` и `UiIcons` должны существовать до систем
        // Startup, которые их читают.
        app.add_systems(PreStartup, (load_font, load_icons))
            .add_plugins(fps::FpsPlugin)
            .add_plugins(bottom_panel::BottomPanelPlugin)
            .add_plugins(menus::MenusPlugin)
            .add_plugins(pause_menu::PauseMenuPlugin)
            .add_plugins(creative::CreativePlugin)
            .add_plugins(inventory::InventoryPlugin)
            .add_plugins(health::HealthPlugin)
            .add_plugins(hotbar::HotbarPlugin)
            .add_plugins(construction::ConstructionPlugin)
            .add_plugins(stations::StationPlugin)
            .add_plugins(inspector::InspectorPlugin)
            .add_plugins(tooltip::TooltipPlugin);
    }
}

/// Загружает шрифт из `assets/fonts/FiraSans-Medium.ttf`.
fn load_font(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(UiFont(assets.load("fonts/FiraSans-Medium.ttf")));
}

/// Загружает иконки тайлов, объектов и предметов для UI.
fn load_icons(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(UiIcons::load(&assets));
}