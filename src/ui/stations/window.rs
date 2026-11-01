//! Жизненный цикл окна станции: открытие/переключение, снос при
//! закрытии (с возвратом содержимого временной сетки крафта), автозакрытие
//! при отходе игрока.

use bevy::prelude::*;

use crate::app::state::{CommandQueueRes, GameState};
use crate::simulation::commands::Command;
use crate::simulation::items::ItemStack;
use crate::simulation::objects::{BuildingKind, ObjectKind};
use crate::simulation::TilePos;
use crate::ui::stations::shared::{spawn_player_misc, StationPanel, StationRoot};
use crate::ui::style;
use crate::ui::UiFont;

use super::StationUi;

/// Открывает/переключает окно станции (вызывается из ввода по ПКМ).
/// Повторный вызов с той же станцией закрывает окно.
pub fn toggle_station(
    commands: &mut Commands,
    station: &mut StationUi,
    roots: &Query<Entity, With<StationRoot>>,
    font: &UiFont,
    pos: TilePos,
    kind: ObjectKind,
) {
    if station.open && station.pos == Some(pos) {
        // Закрытие: корень снесёт close_station_window, сетку вернёт он же.
        station.open = false;
        station.selected = None;
        return;
    }
    // Переключение на другую станцию: старое окно сносим сразу, иначе
    // остались бы два корня (окно пересоздаётся только при закрытии).
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    station.open = true;
    station.pos = Some(pos);
    station.kind = Some(kind);
    station.selected = None;
    spawn_station_window(commands, station, font);
}

/// Сносит окно станции, когда флаг открытости снят, и возвращает
/// содержимое временной сетки крафта/инструментов в инвентарь.
pub(crate) fn close_station_window(
    mut commands: Commands,
    mut station: ResMut<StationUi>,
    mut state: ResMut<GameState>,
    mut queue: ResMut<CommandQueueRes>,
    roots: Query<Entity, With<StationRoot>>,
) {
    if station.open {
        return;
    }
    if !roots.is_empty() {
        for e in &roots {
            commands.entity(e).despawn();
        }
    }
    // Сетка/инструменты не заполнены — обычный случай, выходим сразу.
    if station.grid.iter().all(|c| c.is_none()) && station.tools.iter().all(|c| c.is_none()) {
        return;
    }
    // Центр сброса невлезшего — тайл существа (как в reset_crafting_on_close).
    let Some(actor) = state.controlled else {
        station.grid = Default::default();
        station.tools = Default::default();
        return;
    };
    let center = state
        .world
        .as_ref()
        .map(|world| {
            let size = world.size.tiles() as i32;
            let pos = world
                .entities
                .get(actor)
                .map(|e| e.pos)
                .unwrap_or_default();
            TilePos {
                x: (pos.x.floor() as i32).clamp(0, size - 1),
                y: (pos.y.floor() as i32).clamp(0, size - 1),
            }
        });
    let Some(center) = center else {
        return;
    };
    let Some(p) = state.controlled_mind_mut() else {
        station.grid = Default::default();
        station.tools = Default::default();
        return;
    };
    let mut cells: Vec<Option<ItemStack>> =
        station.grid.iter().chain(station.tools.iter()).copied().collect();
    for cell in cells.iter_mut() {
        let Some(stack) = *cell else {
            continue;
        };
        let left = p.inventory.add(stack.kind, stack.count);
        if left > 0 {
            queue.0.push(Command::PlayerDropStack {
                pos: center,
                stack: ItemStack { kind: stack.kind, count: left },
            });
        }
        *cell = None;
    }
}

/// Строит окно станции: заголовок, слоты станции, (для верстака/
/// кузнечного стола — инструменты и сетка крафта) и ряд misc игрока.
fn spawn_station_window(commands: &mut Commands, station: &StationUi, font: &UiFont) {
    let Some(kind) = station.kind else {
        return;
    };
    let title = station_title(kind);
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
            StationRoot,
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
                    crate::ui::BlocksWorldInput,
                    StationPanel,
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new(title),
                        TextFont {
                            font: font.0.clone().into(),
                            font_size: 20.0.into(),
                            ..default()
                        },
                        TextColor(style::text()),
                    ));
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(10.0),
                            ..default()
                        })
                        .with_children(|column| {
                            match kind {
                                ObjectKind::Building(BuildingKind::CraftingTable) => {
                                    super::crafting::spawn_station_crafting(column, font);
                                }
                                ObjectKind::Building(BuildingKind::SmithingTable) => {
                                    super::crafting::spawn_smithing_tools(column, font);
                                    super::crafting::spawn_station_crafting(column, font);
                                }
                                // Контейнерные станции: слоты по раскладке.
                                ObjectKind::Building(
                                    BuildingKind::Campfire
                                    | BuildingKind::Furnace
                                    | BuildingKind::BlastFurnace
                                    | BuildingKind::Barrel
                                    | BuildingKind::Storage,
                                ) => {
                                    super::furnace::spawn_container_slots(column, font, kind);
                                }
                                _ => {}
                            }
                            // MISC игрока как хотбар.
                            spawn_player_misc(column, font);
                        });
                });
        });
}

/// Заголовок окна по виду станции.
fn station_title(kind: ObjectKind) -> &'static str {
    let Some(kind) = kind.building_kind() else {
        return "Станция";
    };
    match kind {
        BuildingKind::Campfire => "Костёр",
        BuildingKind::CraftingTable => "Верстак",
        BuildingKind::Furnace => "Печь",
        BuildingKind::Barrel => "Бочка",
        BuildingKind::BlastFurnace => "Плавильная печь",
        BuildingKind::Storage => "Склад",
        BuildingKind::SmithingTable => "Кузнечный стол",
    }
}

/// Закрывает окно станции, когда игрок отошёл дальше радиуса
/// взаимодействия с тайлами (открыть издалека нельзя — и держать тоже).
pub(crate) fn station_reach_close(
    state: Res<GameState>,
    mut station: ResMut<StationUi>,
) {
    if !station.open {
        return;
    }
    let Some(pos) = station.pos else {
        return;
    };
    let reachable = match (state.controlled, state.world.as_ref()) {
        (Some(actor), Some(world)) => {
            crate::simulation::actions::reachable(world, actor, pos)
        }
        _ => false,
    };
    if !reachable {
        station.open = false;
        station.selected = None;
    }
}
