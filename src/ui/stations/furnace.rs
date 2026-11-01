//! Контейнерные станции: печь, плавильная печь, костёр (раскладка
//! «вход → стрелка → выход»), бочка и склад (сетка хранения). Здесь же —
//! индикаторы пламени и прогресса готовки.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use crate::app::state::{ClockRes, GameState};
use crate::simulation::containers::{self, capacity, container_at, cook_time_ticks};
use crate::simulation::objects::{BuildingKind, ObjectKind};
use crate::ui::bottom_panel::FitIcon;
use crate::ui::inventory::{ICON_BOX, SLOT_GAP, SLOT_SIZE};
use crate::ui::style;
use crate::ui::UiFont;

use super::shared::{StationOutputSlot, StationSlot, StationSlotCount};
use super::StationUi;

/// Сторона большого слота результата (доля от обычного слота).
const OUTPUT_SLOT_SCALE: f32 = 1.3;
/// Размер индикатора пламени (топливо горит).
const FLAME_SIZE: f32 = 18.0;
/// Размер стрелки прогресса готовки.
const ARROW_WIDTH: f32 = 44.0;
const ARROW_HEIGHT: f32 = 14.0;

/// Индикатор пламени печи: подсвечен, пока горит топливо (обновляется
/// в `refresh_furnace_indicators` по `Container::is_lit`).
#[derive(Component)]
pub(crate) struct FurnaceFlame;

/// Заливка стрелки прогресса готовки: ширина = доля готовки.
#[derive(Component)]
pub(crate) struct FurnaceArrowFill;

/// Слоты контейнера станции по раскладке вида. Печи — в стиле Minecraft:
/// слева вход, под ним индикатор пламени и топливо, в середине стрелка
/// прогресса готовки, справа увеличенный слот результата; зола — отдельным
/// рядом ниже. Бочка и склад — сетка хранения. Костёр — как печь без
/// топлива (горит вечно).
pub(crate) fn spawn_container_slots(
    panel: &mut ChildSpawnerCommands,
    font: &UiFont,
    kind: ObjectKind,
) {
    let count = capacity(kind).unwrap_or(0);
    if matches!(
        kind,
        ObjectKind::Building(BuildingKind::Barrel | BuildingKind::Storage)
    ) {
        // Хранилище: сетка слотов по 8 в ряд.
        panel
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(SLOT_GAP),
                ..default()
            })
            .with_children(|grid| {
                let per_row = 8;
                for row_start in (0..count).step_by(per_row) {
                    grid.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(SLOT_GAP),
                        ..default()
                    })
                    .with_children(|row| {
                        for index in row_start..(row_start + per_row).min(count) {
                            spawn_station_slot(row, index, font, "");
                        }
                    });
                }
            });
        return;
    }

    // Печи и костёр: раскладка «вход → стрелка → выход».
    let (inputs, fuels, output, waste, input_label, fuel_label) = match kind {
        ObjectKind::Building(BuildingKind::Furnace) => (
            &[containers::FURNACE_INPUT][..],
            &[containers::FURNACE_FUEL][..],
            containers::FURNACE_OUTPUT,
            &containers::FURNACE_WASTE[..],
            "Вход",
            "Топливо",
        ),
        ObjectKind::Building(BuildingKind::BlastFurnace) => (
            &containers::BLAST_INPUT[..],
            &containers::BLAST_FUEL[..],
            containers::BLAST_OUTPUT,
            &containers::BLAST_WASTE[..],
            "Руда",
            "Уголь",
        ),
        ObjectKind::Building(BuildingKind::Campfire) => (
            &[containers::CAMPFIRE_INPUT][..],
            &[][..],
            containers::CAMPFIRE_OUTPUT,
            &[][..],
            "Готовить",
            "",
        ),
        _ => return,
    };

    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|block| {
            // Основной ряд: колонка входа+топлива, стрелка, результат.
            block
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(14.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    // Колонка: входы сверху, пламя посередине, топливо снизу.
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|column| {
                        for &index in inputs {
                            spawn_station_slot(column, index, font, input_label);
                        }
                        if !fuels.is_empty() {
                            column.spawn((
                                Node {
                                    width: Val::Px(FLAME_SIZE),
                                    height: Val::Px(FLAME_SIZE),
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                                FurnaceFlame,
                            ));
                            for &index in fuels {
                                spawn_station_slot(column, index, font, fuel_label);
                            }
                        }
                    });
                    spawn_furnace_arrow(row);
                    // Результат: увеличенный слот (кнопка — только забирать).
                    spawn_station_slot_sized(row, output, font, "Выход", OUTPUT_SLOT_SCALE, true);
                });
            // Зола от топлива: отдельный ряд маленьких слотов.
            if !waste.is_empty() {
                block
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(SLOT_GAP),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new("Зола"),
                            TextFont {
                                font: font.0.clone().into(),
                                font_size: 12.0.into(),
                                ..default()
                            },
                            TextColor(style::text_dim()),
                        ));
                        for &index in waste {
                            spawn_station_slot(row, index, font, "");
                        }
                    });
            }
        });
}

/// Стрелка прогресса готовки: рамка-контур и внутренняя заливка,
/// ширина которой обновляется по ходу готовки (`FurnaceArrowFill`).
fn spawn_furnace_arrow(row: &mut ChildSpawnerCommands) {
    row.spawn((
        Node {
            width: Val::Px(ARROW_WIDTH),
            height: Val::Px(ARROW_HEIGHT),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(style::border()),
        BackgroundColor(style::surface()),
    ))
    .with_children(|arrow| {
        arrow.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                height: Val::Percent(100.0),
                width: Val::Percent(0.0),
                ..default()
            },
            BackgroundColor(style::text_dim()),
            FurnaceArrowFill,
        ));
    });
}

/// Слот контейнера станции с заданным масштабом размера (у результата
/// он больше, как в Minecraft); `output` помечает слот выхода — его можно
/// только забирать (`StationOutputSlot`).
fn spawn_station_slot_sized(
    row: &mut ChildSpawnerCommands,
    index: usize,
    font: &UiFont,
    label: &str,
    scale: f32,
    output: bool,
) {
    let size = SLOT_SIZE * scale;
    let mut slot_cmd = row.spawn((
        Button,
        Node {
            width: Val::Px(size),
            height: Val::Px(size),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::surface()),
        BorderColor::all(style::border()),
        Hovered::default(),
        StationSlot { index },
    ));
    if output {
        slot_cmd.insert(StationOutputSlot);
    }
    slot_cmd.with_children(|slot| {
        if !label.is_empty() {
            slot.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(3.0),
                    top: Val::Px(1.0),
                    ..default()
                },
                Text::new(label),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 10.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
        }
        slot.spawn((ImageNode::default(), Node::default()))
            .insert(FitIcon { box_px: ICON_BOX * scale });
        slot.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(3.0),
                bottom: Val::Px(2.0),
                ..default()
            },
            Text::new(String::new()),
            TextFont {
                font: font.0.clone().into(),
                font_size: 12.0.into(),
                ..default()
            },
            TextColor(style::text()),
            StationSlotCount,
        ));
    });
}

/// Слот контейнера станции: рамка, подпись роли, иконка и счётчик.
fn spawn_station_slot(row: &mut ChildSpawnerCommands, index: usize, font: &UiFont, label: &str) {
    row.spawn((
        Button,
        Node {
            width: Val::Px(SLOT_SIZE),
            height: Val::Px(SLOT_SIZE),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::surface()),
        BorderColor::all(style::border()),
        Hovered::default(),
        StationSlot { index },
    ))
    .with_children(|slot| {
        if !label.is_empty() {
            slot.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(3.0),
                    top: Val::Px(1.0),
                    ..default()
                },
                Text::new(label),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 10.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
        }
        slot.spawn((ImageNode::default(), Node::default()))
            .insert(FitIcon { box_px: ICON_BOX });
        slot.spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(3.0),
                bottom: Val::Px(2.0),
                ..default()
            },
            Text::new(String::new()),
            TextFont {
                font: font.0.clone().into(),
                font_size: 12.0.into(),
                ..default()
            },
            TextColor(style::text()),
            StationSlotCount,
        ));
    });
}

/// Индикаторы печей: пламя (горит ли топливо) и заливка стрелки прогресса
/// готовки — по контейнеру мира на тайле открытой станции.
pub(crate) fn refresh_furnace_indicators(
    station: Res<StationUi>,
    state: Res<GameState>,
    clock: Res<ClockRes>,
    mut flames: Query<(&mut BackgroundColor, &FurnaceFlame)>,
    mut arrow_fills: Query<(&mut Node, &FurnaceArrowFill)>,
) {
    if !station.open {
        return;
    }
    let (Some(kind), Some(pos), Some(world)) =
        (station.kind, station.pos, state.world.as_ref())
    else {
        return;
    };
    let now = clock.0.tick;
    let flame_color = |lit: bool, fraction: f32| {
        if lit {
            // От насыщенного оранжевого к тусклому по мере прогорания.
            Color::srgb(1.0, 0.35 + 0.35 * fraction, 0.1)
        } else {
            Color::NONE
        }
    };
    let arrow_fraction = |container: &containers::Container| -> f32 {
        match container.cook_finish_tick {
            Some(finish) if finish > now => {
                let total = cook_time_ticks(kind) as f32;
                if total > 0.0 {
                    (1.0 - (finish - now) as f32 / total).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            }
            // Готовка завершится этим тиком или уже позади — полная.
            Some(_) => 1.0,
            None => 0.0,
        }
    };
    let Some(container) = container_at(world, pos) else {
        return;
    };
    let lit = container.is_lit();
    let fuel_fraction = match (container.burn_out_tick, container.burning_fuel) {
        (Some(burn_out), Some(fuel)) => {
            let total = fuel.burn_time() as u64;
            if total > 0 {
                (burn_out.saturating_sub(now)) as f32 / total as f32
            } else {
                0.0
            }
        }
        _ => 0.0,
    };
    let want_flame = flame_color(lit, fuel_fraction.clamp(0.0, 1.0));
    for (mut bg, _) in &mut flames {
        if **bg != want_flame {
            **bg = want_flame;
        }
    }
    let want_fill = arrow_fraction(container);
    for (mut node, _) in &mut arrow_fills {
        let want = Val::Percent(want_fill * 100.0);
        if node.width != want {
            node.width = want;
        }
    }
}
