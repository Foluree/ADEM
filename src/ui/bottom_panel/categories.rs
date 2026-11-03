//! Категории инструментов и содержимое подменю: наборы кнопок базового
//! вида и открытых подменю, их иконки и русские названия для тултипов.

use crate::app::state::{GameState, LensKind, SimSpeed, Tool, ToolCategory};
use crate::simulation::entities::EntityKind;
use crate::simulation::objects::{BuildingKind, ObjectKind};
use crate::simulation::tile::TileType;
use bevy::prelude::*;

use crate::ui::icons::UiIcons;

use super::{ButtonContent, PanelAction, PanelItem, PanelMenu};

/// Размещаемые тайлы в порядке кнопок (Empty не ставится).
const PLACEABLE_TILES: [TileType; 14] = [
    TileType::Water,
    TileType::DeepWater,
    TileType::Ocean,
    TileType::Sand,
    TileType::Dirt,
    TileType::Grass,
    TileType::ForestGrass,
    TileType::Stone,
    TileType::Mountain,
    TileType::Clay,
    TileType::Farmland,
    TileType::Moss,
    TileType::Mud,
    TileType::Podzol,
];

/// Кнопки инструментов категории, по порядку в сетке. Постройки —
/// только в категории Buildings, объекты — в Objects.
fn tools_of(category: ToolCategory) -> Vec<Tool> {
    match category {
        ToolCategory::Tiles => PLACEABLE_TILES.iter().map(|t| Tool::Tile(*t)).collect(),
        ToolCategory::Objects => ObjectKind::ALL.iter().copied().map(Tool::Object).collect(),
        ToolCategory::Buildings => {
            BuildingKind::ALL.iter().copied().map(Tool::Building).collect()
        }
        ToolCategory::Entities => EntityKind::ALL.iter().map(|k| Tool::Entity(*k)).collect(),
    }
}

/// Иконка кнопки категории в базовом виде панели.
fn category_icon(category: ToolCategory, icons: &UiIcons) -> Handle<Image> {
    match category {
        ToolCategory::Tiles => icons.tile(TileType::Grass),
        ToolCategory::Objects => icons.object(ObjectKind::Tree),
        ToolCategory::Buildings => icons.object(ObjectKind::Building(BuildingKind::Campfire)),
        ToolCategory::Entities => icons.entity(EntityKind::Wolf),
    }
    .clone()
}

// --- Названия для тултипов ---

/// Русское название тайла (тултипы панели редактора).
pub(crate) fn tile_name(kind: TileType) -> &'static str {
    match kind {
        TileType::Empty => "Пустота",
        TileType::Water => "Вода",
        TileType::Sand => "Песок",
        TileType::Dirt => "Земля",
        TileType::Grass => "Трава",
        TileType::ForestGrass => "Лесная трава",
        TileType::Stone => "Камень",
        TileType::Clay => "Глина",
        TileType::Farmland => "Пашня",
        TileType::DeepWater => "Глубокая вода",
        TileType::Ocean => "Океан",
        TileType::Mountain => "Гора",
        TileType::Moss => "Мох",
        TileType::Mud => "Грязь",
        TileType::Podzol => "Подзол",
    }
}

/// Русское название природного объекта или постройки.
pub(crate) fn object_kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Tree => "Дерево",
        ObjectKind::Bush => "Куст",
        ObjectKind::Rock => "Валун",
        ObjectKind::AppleTree => "Яблоня",
        ObjectKind::Building(kind) => crate::ui::construction::building_name(kind),
    }
}

/// Русское название вида существ.
pub(crate) fn entity_kind_name(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Wolf => "Волк",
        EntityKind::Human => "Человек",
    }
}

/// Название кнопки панели для тултипа. `None` — тултип не нужен: плитки
/// времени (пауза, скорость) подписаны сами.
pub(crate) fn panel_action_tooltip(action: &PanelAction) -> Option<&'static str> {
    match action {
        PanelAction::TogglePause | PanelAction::SetSpeed(_) => None,
        PanelAction::OpenMenu(PanelMenu::Speed) => None,
        PanelAction::OpenMenu(PanelMenu::Tools(category)) => Some(match category {
            ToolCategory::Tiles => "Тайлы",
            ToolCategory::Objects => "Объекты",
            ToolCategory::Buildings => "Постройки",
            ToolCategory::Entities => "Существа",
        }),
        PanelAction::OpenMenu(PanelMenu::Lenses) => Some("Линзы карты"),
        PanelAction::Back => Some("Назад"),
        PanelAction::SelectTool(tool) => Some(match tool {
            Tool::Tile(kind) => tile_name(*kind),
            Tool::Object(kind) => object_kind_name(*kind),
            Tool::Building(kind) => crate::ui::construction::building_name(*kind),
            Tool::Entity(kind) => entity_kind_name(*kind),
        }),
        PanelAction::ToggleLens(kind) => Some(match kind {
            LensKind::Sentients => "Разумные существа",
            LensKind::Buildings => "Постройки",
        }),
    }
}

// --- Содержимое кнопок ---

/// Базовые кнопки: пауза, скорость, тайлы, объекты, существа.
pub(crate) fn base_items(state: &GameState, icons: &UiIcons) -> Vec<PanelItem> {
    let pause_icon = if state.paused {
        icons.play().clone()
    } else {
        icons.pause().clone()
    };
    let mut items = vec![
        PanelItem {
            action: PanelAction::TogglePause,
            content: ButtonContent::Pause(pause_icon),
        },
        // Кнопка скорости показывает текущее значение, подменю — все.
        PanelItem {
            action: PanelAction::OpenMenu(PanelMenu::Speed),
            content: ButtonContent::Text(state.speed.label()),
        },
    ];
    for category in ToolCategory::ALL {
        items.push(PanelItem {
            action: PanelAction::OpenMenu(PanelMenu::Tools(category)),
            content: ButtonContent::Icon(category_icon(category, icons)),
        });
    }
    // Линзы карты: отдельное подменю переключателей (не инструменты —
    // клик по карте ничего не ставит).
    items.push(PanelItem {
        action: PanelAction::OpenMenu(PanelMenu::Lenses),
        content: ButtonContent::Icon(icons.entity(EntityKind::Human).clone()),
    });
    items
}

/// Элементы открытого подменю в порядке кнопок.
pub(crate) fn menu_items(menu: PanelMenu, icons: &UiIcons) -> Vec<PanelItem> {
    match menu {
        PanelMenu::Speed => SimSpeed::all()
            .iter()
            .map(|speed| PanelItem {
                action: PanelAction::SetSpeed(*speed),
                content: ButtonContent::Text(speed.label()),
            })
            .collect(),
        PanelMenu::Tools(category) => tools_of(category)
            .into_iter()
            .map(|tool| PanelItem {
                action: PanelAction::SelectTool(tool),
                content: ButtonContent::Icon(match tool {
                    Tool::Tile(kind) => icons.tile(kind),
                    Tool::Object(kind) => icons.object(kind),
                    Tool::Building(kind) => icons.object(ObjectKind::Building(kind)),
                    Tool::Entity(kind) => icons.entity(kind),
                }
                .clone()),
            })
            .collect(),
        PanelMenu::Lenses => [
            (LensKind::Sentients, icons.entity(EntityKind::Human).clone()),
            (
                LensKind::Buildings,
                icons.object(ObjectKind::Building(BuildingKind::Campfire)).clone(),
            ),
        ]
        .into_iter()
        .map(|(kind, icon)| PanelItem {
            action: PanelAction::ToggleLens(kind),
            content: ButtonContent::Icon(icon),
        })
        .collect(),
    }
}
