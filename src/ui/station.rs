//! GUI рабочих станций: печь, плавильная печь, костёр, бочка, склад,
//! верстак, кузнечный стол. Открывается ПКМ по станции в режиме игрока.
//! В окне доступны слоты станции (контейнер из `simulation/containers`)
//! и MISC-инвентарь игрока как хотбар; броня, одежда и косметика недоступны.
//!
//! Перенос предметов между станцией и игроком идёт командой
//! `Command::StationMoveItem` (симуляция валидирует правила слотов):
//! клик по слоту выбирает источник, клик по другому слоту — перенос
//! целого стека; Shift+клик — быстрый перенос на другую сторону;
//! ПКМ — половина стека. Верстак и кузнечный стол контейнера не имеют:
//! их сетка 3×3 (и инструменты кузнечного стола) временные, живут в UI
//! и возвращаются в инвентарь при закрытии окна. Верстак наследует
//! ручные рецепты (`manual`) + свои (`crafting_table`), кузнечный стол —
//! `smithing_table` с требованием инструментов (не расходуются).

use bevy::input::mouse::MouseButton;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use crate::app::input::cursor_world_tile;
use crate::app::state::{ClockRes, CommandQueueRes, GameState, PlayerRes, WindowFocusState};
use crate::simulation::commands::Command;
use crate::simulation::containers::{
    self, capacity, can_put_into, container_at, cook_time_ticks, SLOT_AREA_PLAYER_MISC,
};
use crate::simulation::crafts::GRID_SLOTS;
use crate::simulation::items::{self, SlotArea, SlotRef};
use crate::simulation::objects::{BuildingKind, ObjectKind};
use crate::simulation::{ItemKind, ItemStack, TilePos};
use crate::ui::CraftBookRes;
use crate::ui::bottom_panel::{FitDone, FitIcon};
use crate::ui::icons::UiIcons;
use crate::ui::inventory::{DragGhost, GHOST_HALF};
use crate::ui::style;
use crate::ui::{BlocksWorldInput, UiFont, cursor_over_node};

// Единый визуальный язык с окном инвентаря.
use crate::ui::inventory::{ICON_BOX, SLOT_GAP, SLOT_SIZE};

pub struct StationPlugin;

impl Plugin for StationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<StationUi>()
            .add_observer(on_station_slot_activated)
            .add_observer(on_station_craft_slot_activated)
            .add_observer(on_station_result_activated)
            .add_systems(
                Update,
                (
                    close_station_window,
                    // Дистанция проверяется до drag: закрытая станция
                    // снимает свой призрак в station_drag этого же кадра.
                    station_reach_close,
                    station_drag,
                    station_digit_transfer,
                    refresh_station_slots,
                )
                    .chain(),
            );
    }
}

/// Открытая станция и временное состояние её окна.
#[derive(Resource, Default)]
pub struct StationUi {
    pub open: bool,
    /// Тайл станции, с которой открыто окно.
    pub pos: Option<TilePos>,
    /// Вид станции на момент открытия.
    pub kind: Option<ObjectKind>,
    /// Сетка крафта верстака/кузнечного стола (временная, живёт в UI).
    pub(crate) grid: [Option<ItemStack>; GRID_SLOTS],
    /// Инструменты кузнечного стола (временные, не расходуются).
    pub(crate) tools: [Option<ItemStack>; 4],
    /// Выбранный адрес-источник для переноса.
    pub(crate) selected: Option<StationAddress>,
    /// Идущее перетаскивание стека (призрак под курсором). Реального
    /// снятия стека нет: перенос/выброс исполняются командой на отпускании.
    pub(crate) drag: Option<StationDragState>,
}

/// Идущее перетаскивание в окне станции: слот-источник и призрак-иконка.
pub(crate) struct StationDragState {
    pub(crate) from: StationAddress,
    ghost: Entity,
}

/// Адрес слота в окне станции: контейнерный слот станции, misc игрока,
/// клетка сетки крафта или слот инструмента кузнечного стола.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StationAddress {
    /// Слот контейнера станции (индекс в `Container::slots`).
    Container(usize),
    /// Слот misc инвентаря игрока (индекс).
    Player(usize),
    /// Клетка сетки крафта (UI-локальная).
    Grid(usize),
    /// Слот инструмента кузнечного стола (UI-локальный).
    Tool(usize),
}

/// Маркер корня окна станции (сносится при закрытии; виден ESC-цепочке).
#[derive(Component)]
pub(crate) struct StationRoot;

/// Контейнерный слот станции: индекс в `Container::slots`.
#[derive(Component)]
pub(crate) struct StationSlot {
    pub(crate) index: usize,
}

/// Слот misc игрока в окне станции: перенос идёт командой симуляции,
/// а не напрямую (мир мутрует только `apply_all`).
#[derive(Component, Default)]
pub(crate) struct StationPlayerSlot;

/// Клетка сетки крафта станции (верстак/кузнечный стол).
#[derive(Component, Default)]
pub(crate) struct StationCraftSlot;

/// Слот инструмента кузнечного стола.
#[derive(Component, Default)]
pub(crate) struct StationToolSlot;

/// Слот результата верстака/кузнечного стола.
#[derive(Component)]
pub(crate) struct StationResultSlot;

/// Маркер текста-счётчика внутри слота станции.
#[derive(Component)]
struct StationSlotCount;

/// Панель окна станции (не фуллскрин-обёртка): хит-тест «курсор над
/// окном» — отпускание перетаскивания за её пределами бросает предмет.
#[derive(Component)]
pub(crate) struct StationPanel;

/// Слот результата печи («кнопка»): клик забирает стек, drag выключен.
#[derive(Component)]
pub(crate) struct StationOutputSlot;

/// Индикатор пламени печи: подсвечен, пока горит топливо (обновляется
/// в `refresh_station_slots` по `Container::is_lit`).
#[derive(Component)]
struct FurnaceFlame;

/// Заливка стрелки прогресса готовки: ширина = доля готовки.
#[derive(Component)]
struct FurnaceArrowFill;

/// Категории рецептов станции (верстак наследует ручные).
fn station_categories(kind: ObjectKind) -> &'static [&'static str] {
    let Some(kind) = kind.building_kind() else {
        return &[];
    };
    match kind {
        BuildingKind::CraftingTable => &["manual", "crafting_table"],
        BuildingKind::SmithingTable => &["smithing_table"],
        _ => &[],
    }
}

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
fn close_station_window(
    mut commands: Commands,
    mut station: ResMut<StationUi>,
    mut player: ResMut<PlayerRes>,
    mut queue: ResMut<CommandQueueRes>,
    state: Res<GameState>,
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
    let Some(p) = player.0.as_mut() else {
        station.grid = Default::default();
        station.tools = Default::default();
        return;
    };
    let Some(world) = state.world.as_ref() else {
        return;
    };
    // Тайл игрока — центр сброса невлезшего (как в reset_crafting_on_close).
    let size = world.size.tiles() as i32;
    let center = TilePos {
        x: (p.pos.x.floor() as i32).clamp(0, size - 1),
        y: (p.pos.y.floor() as i32).clamp(0, size - 1),
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
                    BlocksWorldInput,
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
                                    spawn_station_crafting(column, font);
                                }
                                ObjectKind::Building(BuildingKind::SmithingTable) => {
                                    spawn_smithing_tools(column, font);
                                    spawn_station_crafting(column, font);
                                }
                                // Контейнерные станции: слоты по раскладке.
                                ObjectKind::Building(
                                    BuildingKind::Campfire
                                    | BuildingKind::Furnace
                                    | BuildingKind::BlastFurnace
                                    | BuildingKind::Barrel
                                    | BuildingKind::Storage,
                                ) => {
                                    spawn_container_slots(column, font, kind);
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

/// Сторона большого слота результата (доля от обычного слота).
const OUTPUT_SLOT_SCALE: f32 = 1.3;
/// Размер индикатора пламени (топливо горит).
const FLAME_SIZE: f32 = 18.0;
/// Размер стрелки прогресса готовки.
const ARROW_WIDTH: f32 = 44.0;
const ARROW_HEIGHT: f32 = 14.0;

/// Слоты контейнера станции по раскладке вида. Печи — в стиле Minecraft:
/// слева вход, под ним индикатор пламени и топливо, в середине стрелка
/// прогресса готовки, справа увеличенный слот результата; зола — отдельным
/// рядом ниже. Бочка и склад — сетка хранения. Костёр — как печь без
/// топлива (горит вечно).
fn spawn_container_slots(panel: &mut ChildSpawnerCommands, font: &UiFont, kind: ObjectKind) {
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

/// Слот контейнера станции с заданным масштабом размера.
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

/// Слоты инструментов кузнечного стола (ряд из четырёх).
fn spawn_smithing_tools(panel: &mut ChildSpawnerCommands, font: &UiFont) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|block| {
            block.spawn((
                Text::new("Инструменты"),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
            block
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(SLOT_GAP),
                    ..default()
                })
                .with_children(|row| {
                    for index in 0..4 {
                        spawn_addressed_slot::<StationToolSlot>(row, index, font);
                    }
                });
        });
}

/// Сетка крафта станции 3×3 + слот результата.
fn spawn_station_crafting(panel: &mut ChildSpawnerCommands, font: &UiFont) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|block| {
            block.spawn((
                Text::new("Крафт"),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
            block
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(12.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(SLOT_GAP),
                        ..default()
                    })
                    .with_children(|grid| {
                        let mut idx = 0usize;
                        for _ in 0..3 {
                            grid.spawn(Node {
                                flex_direction: FlexDirection::Row,
                                column_gap: Val::Px(SLOT_GAP),
                                ..default()
                            })
                            .with_children(|r| {
                                for _ in 0..3 {
                                    let i = idx;
                                    idx += 1;
                                    spawn_addressed_slot::<StationCraftSlot>(r, i, font);
                                }
                            });
                        }
                    });
                    spawn_station_result(row, font);
                });
        });
}

/// Слот результата станции.
fn spawn_station_result(row: &mut ChildSpawnerCommands, font: &UiFont) {
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
        StationResultSlot,
    ))
    .with_children(|slot| {
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

/// Ряд misc-слотов игрока (хотбар в окне станции). Перенос — командой
/// симуляции, поэтому слоты носят `StationPlayerSlot`, а не `InvSlot`.
fn spawn_player_misc(panel: &mut ChildSpawnerCommands, font: &UiFont) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|block| {
            block.spawn((
                Text::new("Предметы"),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
            block
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(SLOT_GAP),
                    ..default()
                })
                .with_children(|row| {
                    for index in 0..items::MISC_SLOTS {
                        spawn_addressed_slot::<StationPlayerSlot>(row, index, font);
                    }
                });
        });
}

/// Обобщённый слот станции с маркером-адресом `M` (сетка, инструменты,
/// misc игрока) — единый вид со слотами контейнера.
fn spawn_addressed_slot<M: Component + Default>(
    row: &mut ChildSpawnerCommands,
    index: usize,
    font: &UiFont,
) {
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
        M::default(),
        SlotIndex(index),
    ))
    .with_children(|slot| {
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

/// Индекс слота для обобщённых маркеров (у всех адресованных слотов
/// одинаковая структура — отдельные компоненты без полей).
#[derive(Component, Default)]
pub(crate) struct SlotIndex(pub(crate) usize);

/// Активация слота станции (контейнер или misc игрока): выбор источника,
/// перенос командой, Shift — быстрый перенос, ПКМ — половина стека.
fn on_station_slot_activated(
    activate: On<Activate>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut station: ResMut<StationUi>,
    mut queue: ResMut<CommandQueueRes>,
    player: Res<PlayerRes>,
    state: Res<GameState>,
    station_slots: Query<&StationSlot>,
    player_slots: Query<(&StationPlayerSlot, &SlotIndex)>,
) {
    // Адрес слота: контейнерный или misc игрока.
    let address = if let Ok(slot) = station_slots.get(activate.entity) {
        StationAddress::Container(slot.index)
    } else if let Ok((_, index)) = player_slots.get(activate.entity) {
        StationAddress::Player(index.0)
    } else {
        return;
    };
    let Some(kind) = station.kind else {
        return;
    };
    let Some(pos) = station.pos else {
        return;
    };

    // ПКМ — половина стека на другую сторону (станция ↔ misc).
    if mouse.just_released(MouseButton::Right) {
        move_half(&mut station, &mut queue, &player, &state, pos, kind, address);
        return;
    }
    if mouse.just_released(MouseButton::Middle) {
        return;
    }
    let shift_held = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    // ЛКМ по выходному слоту печи — забрать весь стек в misc (интуитивный
    // клик; Shift+клик обрабатывается ниже как быстрый перенос).
    if !shift_held
        && let StationAddress::Container(index) = address
        && containers::is_output_slot(kind, index)
        && let Some(stack) = stack_at(&station, &player, &state, address)
        && let Some(target) = opposite_target(&station, &player, kind, address, stack.kind, stack.count)
    {
        queue.0.push(Command::StationMoveItem {
            pos,
            from: encode_address(address),
            to: target,
            count: 0,
        });
        station.selected = None;
        return;
    }
    // Shift+клик — быстрый перенос целого стека на другую сторону.
    if shift_held {
        quick_move(&mut station, &mut queue, &player, &state, pos, kind, address);
        return;
    }
    match station.selected {
        Some(from) if from == address => station.selected = None,
        Some(from) => {
            queue.0.push(Command::StationMoveItem {
                pos,
                from: encode_address(from),
                to: encode_address(address),
                count: 0,
            });
            station.selected = None;
        }
        None => {
            // Выбор источника: только непустой слот (контейнер читает мир).
            if stack_at(&station, &player, &state, address).is_some() {
                station.selected = Some(address);
            }
        }
    }
}

/// Кодирует адрес в пару (area, index) команды переноса: (0xFF, i) —
/// misc игрока, (слот, 0) — слот контейнера.
fn encode_address(address: StationAddress) -> (u8, u8) {
    match address {
        StationAddress::Container(i) => (i as u8, 0),
        StationAddress::Player(i) => (SLOT_AREA_PLAYER_MISC, i as u8),
        // Сетка и инструменты — UI-локальные, в команду не попадают.
        StationAddress::Grid(i) | StationAddress::Tool(i) => (i as u8, 0),
    }
}

/// Половина стека: из станции в misc игрока или наоборот.
fn move_half(
    station: &mut StationUi,
    queue: &mut CommandQueueRes,
    player: &Res<PlayerRes>,
    state: &Res<GameState>,
    pos: TilePos,
    kind: ObjectKind,
    address: StationAddress,
) {
    let Some(stack) = stack_at(station, player, state, address) else {
        return;
    };
    let half = stack.count.div_ceil(2);
    let Some(target) = opposite_target(station, player, kind, address, stack.kind, half)
    else {
        return;
    };
    queue.0.push(Command::StationMoveItem {
        pos,
        from: encode_address(address),
        to: target,
        count: half.min(u8::MAX as u32) as u8,
    });
    station.selected = None;
}

/// Быстрый перенос: слот станции → первый подходящий misc, misc → первый
/// допустимый слот станции.
fn quick_move(
    station: &mut StationUi,
    queue: &mut CommandQueueRes,
    player: &Res<PlayerRes>,
    state: &Res<GameState>,
    pos: TilePos,
    kind: ObjectKind,
    address: StationAddress,
) {
    let Some(stack) = stack_at(station, player, state, address) else {
        return;
    };
    let Some(target) = opposite_target(station, player, kind, address, stack.kind, stack.count)
    else {
        return;
    };
    queue.0.push(Command::StationMoveItem {
        pos,
        from: encode_address(address),
        to: target,
        count: 0,
    });
    station.selected = None;
}

/// Стек по адресу (контейнер — из мира, игрок/сетка — из ресурсов).
fn stack_at(
    station: &StationUi,
    player: &PlayerRes,
    state: &GameState,
    address: StationAddress,
) -> Option<ItemStack> {
    match address {
        StationAddress::Container(index) => {
            let world = state.world.as_ref()?;
            let pos = station.pos?;
            container_at(world, pos)?.get(index)
        }
        StationAddress::Player(i) => player
            .0
            .as_ref()?
            .inventory
            .items
            .get(i)
            .copied()
            .flatten(),
        StationAddress::Grid(i) => station.grid.get(i).copied().flatten(),
        StationAddress::Tool(i) => station.tools.get(i).copied().flatten(),
    }
}

/// Ищет целевой адрес на противоположной стороне (станция ↔ misc игрока).
fn opposite_target(
    _station: &StationUi,
    player: &PlayerRes,
    kind: ObjectKind,
    address: StationAddress,
    item: ItemKind,
    count: u32,
) -> Option<(u8, u8)> {
    let p = player.0.as_ref()?;
    match address {
        StationAddress::Container(_) => {
            // В misc игрока: первый слот, куда влезает хотя бы часть.
            (0..items::MISC_SLOTS)
                .find(|&i| match p.inventory.items.get(i).copied().flatten() {
                    None => true,
                    Some(s) => s.kind == item && s.count + count <= item.max_stack(),
                })
                .map(|i| (SLOT_AREA_PLAYER_MISC, i as u8))
        }
        StationAddress::Player(_) => {
            // Первый допустимый слот контейнера (правила `can_put_into`).
            let cap = capacity(kind)?;
            (0..cap)
                .find(|&slot| can_put_into(kind, slot, item))
                .map(|slot| (slot as u8, 0))
        }
        _ => None,
    }
}

/// Активация клетки сетки крафта станции: ЛКМ кладёт один предмет из
/// выбранного misc-слота, ПКМ забирает один, Shift+ЛКМ возвращает весь.
fn on_station_craft_slot_activated(
    activate: On<Activate>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut station: ResMut<StationUi>,
    mut player: ResMut<PlayerRes>,
    slots: Query<(&StationCraftSlot, &SlotIndex)>,
) {
    let Ok((_, index)) = slots.get(activate.entity) else {
        return;
    };
    let target = index.0;
    if mouse.just_released(MouseButton::Middle) {
        return;
    }
    let Some(p) = player.0.as_mut() else {
        return;
    };

    // ПКМ: забрать один предмет обратно в инвентарь.
    if mouse.just_released(MouseButton::Right) {
        if let Some(stack) = station.grid[target] && p.inventory.can_fit(stack.kind, 1) {
            p.inventory.add(stack.kind, 1);
            take_from_station_cell(&mut station.grid, target, 1);
        }
        return;
    }
    // Shift+ЛКМ: вернуть весь слот.
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        if let Some(stack) = station.grid[target] {
            let left = p.inventory.add(stack.kind, stack.count);
            take_from_station_cell(&mut station.grid, target, stack.count - left);
        }
        return;
    }
    // ЛКМ: перенос по одному из выбранного misc-слота (как в окне инвентаря).
    if let Some(StationAddress::Player(src)) = station.selected {
        let src_ref = SlotRef { area: SlotArea::Misc, index: src };
        let Some(stack) = p.inventory.get(src_ref) else {
            station.selected = None;
            return;
        };
        let kind = stack.kind;
        let can_place = match station.grid[target] {
            None => true,
            Some(existing) => existing.kind == kind && existing.count < kind.max_stack(),
        };
        if !can_place {
            return;
        }
        let mut emptied = false;
        if let Some(cell) = p.inventory.items.get_mut(src) && let Some(s) = cell {
            s.count -= 1;
            emptied = s.count == 0;
            if emptied {
                *cell = None;
            }
        }
        match &mut station.grid[target] {
            Some(existing) => existing.count += 1,
            None => station.grid[target] = Some(ItemStack { kind, count: 1 }),
        }
        if emptied {
            station.selected = None;
        }
        return;
    }
    // Выбор источника в сетке.
    if station.grid[target].is_some() {
        station.selected = Some(StationAddress::Grid(target));
    }
}

/// Списывает `count` предметов из клетки сетки станции.
fn take_from_station_cell(cells: &mut [Option<ItemStack>], index: usize, count: u32) {
    if let Some(cell) = cells.get_mut(index)
        && let Some(stack) = cell
    {
        stack.count = stack.count.saturating_sub(count);
        if stack.count == 0 {
            *cell = None;
        }
    }
}

/// Клик по слоту результата верстака/кузнечного стола: один крафт,
/// Shift+ЛКМ — максимум с добором ингредиентов из инвентаря.
fn on_station_result_activated(
    activate: On<Activate>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut station: ResMut<StationUi>,
    book: Res<CraftBookRes>,
    mut player: ResMut<PlayerRes>,
    results: Query<(), With<StationResultSlot>>,
) {
    if !results.contains(activate.entity) {
        return;
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let Some(kind) = station.kind else {
        return;
    };
    let Some(p) = player.0.as_mut() else {
        return;
    };
    let held_tools: Vec<ItemKind> = station.tools.iter().flatten().map(|s| s.kind).collect();
    let Some(recipe) = book
        .0
        .matching_recipe_for(station_categories(kind), &station.grid, &held_tools)
        .cloned()
    else {
        return;
    };
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        let max = recipe.max_crafts(&station.grid, &p.inventory);
        for _ in 0..max {
            if !recipe.try_craft_one(&mut station.grid, &mut p.inventory) {
                break;
            }
        }
        return;
    }
    recipe.try_craft_one(&mut station.grid, &mut p.inventory);
}

/// Клавиши 1–8 для перекладывания предметов в GUI станции.
const DIGIT_KEYS: [KeyCode; 8] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
];

/// Закрывает окно станции, когда игрок отошёл дальше радиуса
/// взаимодействия с тайлами (открыть издалека нельзя — и держать тоже).
fn station_reach_close(
    state: Res<GameState>,
    player: Res<PlayerRes>,
    mut station: ResMut<StationUi>,
) {
    if !station.open {
        return;
    }
    let Some(pos) = station.pos else {
        return;
    };
    let reachable = match (player.0.as_ref(), state.world.as_ref()) {
        (Some(p), Some(world)) => p.reachable(pos, world),
        _ => false,
    };
    if !reachable {
        station.open = false;
        station.selected = None;
    }
}

/// Адрес слота окна станции под курсором: (адрес, является ли выходом).
fn hovered_station_address(
    window: &Window,
    station_slots: &Query<(
        &StationSlot,
        Option<&StationOutputSlot>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    player_slots: &Query<(&StationPlayerSlot, &SlotIndex, &ComputedNode, &UiGlobalTransform)>,
    cursor: Vec2,
) -> Option<(StationAddress, bool)> {
    if let Some((slot, output, ..)) = station_slots
        .iter()
        .find(|(_, _, computed, transform)| cursor_over_node(window, computed, transform, cursor))
    {
        return Some((StationAddress::Container(slot.index), output.is_some()));
    }
    player_slots
        .iter()
        .find(|(_, _, computed, transform)| cursor_over_node(window, computed, transform, cursor))
        .map(|(_, index, _, _)| (StationAddress::Player(index.0), false))
}

/// Перетаскивание стеков в окне станции: ЛКМ над непустым слотом (не
/// выходом) берёт стек «в руку» (призрак), отпускание над другим слотом
/// переносит (misc↔misc — напрямую инвентарём, с контейнером — командой
/// `StationMoveItem`), отпускание за пределами окна бросает стек в мир в
/// сторону курсора. Рамка слота-источника подсвечивается `refresh`'ем.
fn station_drag(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    focus: Res<WindowFocusState>,
    state: Res<GameState>,
    mut station: ResMut<StationUi>,
    mut player: ResMut<PlayerRes>,
    mut queue: ResMut<CommandQueueRes>,
    icons: Res<UiIcons>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    station_slots: Query<(
        &StationSlot,
        Option<&StationOutputSlot>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    player_slots: Query<(&StationPlayerSlot, &SlotIndex, &ComputedNode, &UiGlobalTransform)>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<StationPanel>>,
    mut ghosts: Query<(Entity, &mut Node), With<DragGhost>>,
) {
    if !station.open || !focus.focused {
        // Окно закрылось (или потеряло фокус) посреди перетаскивания:
        // реального снятия стека не было — достаточно снять призрака.
        if let Some(drag) = station.drag.take() {
            commands.entity(drag.ghost).despawn();
        }
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let cursor = window.cursor_position();

    // Завершение/отмена — до старта (быстрый клик между кадрами).
    if mouse.just_released(MouseButton::Right) {
        if let Some(drag) = station.drag.take() {
            commands.entity(drag.ghost).despawn();
        }
        return;
    }
    if mouse.just_released(MouseButton::Left) {
        if let Some(drag) = station.drag.take() {
            commands.entity(drag.ghost).despawn();
            finish_station_drag(
                &mut station,
                &mut player,
                &mut queue,
                &state,
                drag,
                cursor,
                window,
                &cameras,
                &station_slots,
                &player_slots,
                &panels,
            );
        }
        return;
    }
    // Призрак следует за курсором.
    if station.drag.is_some()
        && let Some(cursor) = cursor
    {
        for (_, mut node) in &mut ghosts {
            node.left = Val::Px(cursor.x - GHOST_HALF);
            node.top = Val::Px(cursor.y - GHOST_HALF);
        }
    }
    // Старт: ЛКМ над непустым слотом, не являющимся выходом.
    if station.drag.is_none()
        && mouse.just_pressed(MouseButton::Left)
        && let Some(cursor) = cursor
        && let Some((from, is_output)) =
            hovered_station_address(window, &station_slots, &player_slots, cursor)
        && !is_output
        && stack_at(&station, &player, &state, from).is_some()
    {
        let ghost = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(cursor.x - GHOST_HALF),
                    top: Val::Px(cursor.y - GHOST_HALF),
                    width: Val::Px(GHOST_HALF * 2.0),
                    height: Val::Px(GHOST_HALF * 2.0),
                    ..default()
                },
                ImageNode::new(icons.item(stack_kind(&station, &player, &state, from)).clone()),
                Pickable::IGNORE,
                GlobalZIndex(i32::MAX),
                DragGhost,
            ))
            .id();
        station.drag = Some(StationDragState { from, ghost });
    }
}

/// Вид предмета в слоте-источнике drag (без него иконку призрака не взять).
fn stack_kind(
    station: &StationUi,
    player: &PlayerRes,
    state: &GameState,
    address: StationAddress,
) -> crate::simulation::ItemKind {
    stack_at(station, player, state, address)
        .map(|s| s.kind)
        .unwrap_or(crate::simulation::ItemKind::Rock)
}

/// Завершает перетаскивание станции: перенос в слот под курсором либо
/// (за пределами окна) бросок стека в мир в сторону курсора.
fn finish_station_drag(
    station: &mut StationUi,
    player: &mut PlayerRes,
    queue: &mut CommandQueueRes,
    state: &GameState,
    drag: StationDragState,
    cursor: Option<Vec2>,
    window: &Window,
    cameras: &Query<(&Camera, &Transform), With<Camera2d>>,
    station_slots: &Query<(
        &StationSlot,
        Option<&StationOutputSlot>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    player_slots: &Query<(&StationPlayerSlot, &SlotIndex, &ComputedNode, &UiGlobalTransform)>,
    panels: &Query<(&ComputedNode, &UiGlobalTransform), With<StationPanel>>,
) {
    let Some(pos) = station.pos else {
        return;
    };
    let target = cursor.and_then(|c| {
        hovered_station_address(window, station_slots, player_slots, c)
            .filter(|(addr, _)| *addr != drag.from)
            .map(|(addr, _)| addr)
    });
    if let Some(target) = target {
        match (drag.from, target) {
            // misc ↔ misc: правила инвентаря применяются напрямую, как в
            // окне инвентаря (оба слота — misc).
            (StationAddress::Player(a), StationAddress::Player(b)) => {
                if let Some(p) = player.0.as_mut() {
                    p.inventory.transfer(
                        SlotRef { area: SlotArea::Misc, index: a },
                        SlotRef { area: SlotArea::Misc, index: b },
                    );
                }
            }
            // С контейнером решает симуляция: правила слотов, долив, обмен.
            _ => {
                queue.0.push(Command::StationMoveItem {
                    pos,
                    from: encode_address(drag.from),
                    to: encode_address(target),
                    count: 0,
                });
            }
        }
        station.selected = None;
        return;
    }
    // Слота под курсором нет. Внутри окна — просто отмена (стек на месте),
    // за пределами — бросок в сторону курсора.
    let Some(cursor) = cursor else {
        return;
    };
    let over_panel = panels
        .iter()
        .any(|(computed, transform)| cursor_over_node(window, computed, transform, cursor));
    if over_panel {
        return;
    }
    let Some(world) = state.world.as_ref() else {
        return;
    };
    let Some(tile) = cursor_world_tile(window, cameras, Some(world)) else {
        return;
    };
    throw_station_drag(queue, player, drag.from, tile, pos);
    station.selected = None;
}

/// Бросает стек из слота-источника drag в мир: из misc — сразу командой;
/// из контейнера — сначала перенос в первый свободный слот misc (команды
/// одного кадра применяются по порядку), затем бросок.
fn throw_station_drag(
    queue: &mut CommandQueueRes,
    player: &PlayerRes,
    from: StationAddress,
    target: crate::simulation::TilePos,
    pos: crate::simulation::TilePos,
) {
    match from {
        StationAddress::Player(i) => {
            queue.0.push(Command::PlayerThrowStack {
                target,
                slot: i as u8,
            });
        }
        StationAddress::Container(index) => {
            let Some(p) = player.0.as_ref() else {
                return;
            };
            let Some(free) =
                (0..items::MISC_SLOTS).find(|&i| p.inventory.items.get(i).copied().flatten().is_none())
            else {
                return;
            };
            queue.0.push(Command::StationMoveItem {
                pos,
                from: (index as u8, 0),
                to: (containers::SLOT_AREA_PLAYER_MISC, free as u8),
                count: 0,
            });
            queue.0.push(Command::PlayerThrowStack {
                target,
                slot: free as u8,
            });
        }
        _ => {}
    }
}

/// Перекладывание цифровыми клавишами: наведение на слот + 1–8 переносит
/// весь стек между слотом misc игрока с этим номером и слотом под курсором
/// (если заняты оба — обмен; правила слотов проверяет симуляция).
fn station_digit_transfer(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<GameState>,
    station: Res<StationUi>,
    player: Res<PlayerRes>,
    mut queue: ResMut<CommandQueueRes>,
    windows: Query<&Window>,
    station_slots: Query<(
        &StationSlot,
        Option<&StationOutputSlot>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    player_slots: Query<(&StationPlayerSlot, &SlotIndex, &ComputedNode, &UiGlobalTransform)>,
) {
    if !station.open {
        return;
    }
    let Some(digit) = DIGIT_KEYS.iter().position(|key| keys.just_pressed(*key)) else {
        return;
    };
    if digit >= items::MISC_SLOTS || player.0.is_none() || state.world.is_none() {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Some((target, _)) = hovered_station_address(window, &station_slots, &player_slots, cursor)
    else {
        return;
    };
    let from = StationAddress::Player(digit);
    if target == from {
        return;
    }
    let Some(pos) = station.pos else {
        return;
    };
    // Направление по содержимому: в хотбаре под номером есть предмет —
    // кладём в слот под курсором (при занятых обоих обмен решит симуляция);
    // хотбар пуст, а в слоте под курсором предмет — забираем его в хотбар
    // (повторное нажатие после перекладывания возвращает предмет).
    let source = if stack_at(&station, &player, &state, from).is_some() {
        from
    } else if stack_at(&station, &player, &state, target).is_some() {
        target
    } else {
        return;
    };
    queue.0.push(Command::StationMoveItem {
        pos,
        from: encode_address(source),
        to: encode_address(if source == from { target } else { from }),
        count: 0,
    });
}

/// Обновляет иконки слотов окна станции по контейнеру мира, инвентарю
/// игрока, сетке крафта и инструментам. Маркеры слотов взаимоисключающие,
/// но Bevy это не знает — конфликты `&mut BorderColor` разруливаем
/// явными `Without` (иначе B0001 при инициализации системы).
fn refresh_station_slots(
    mut commands: Commands,
    station: Res<StationUi>,
    player: Res<PlayerRes>,
    state: Res<GameState>,
    clock: Res<ClockRes>,
    icons: Res<UiIcons>,
    mut container_slots: Query<(&StationSlot, &mut BorderColor, &Children)>,
    mut player_slots: Query<
        (&StationPlayerSlot, &SlotIndex, &mut BorderColor, &Children),
        Without<StationSlot>,
    >,
    mut craft_slots: Query<
        (&StationCraftSlot, &SlotIndex, &mut BorderColor, &Children),
        (Without<StationSlot>, Without<StationPlayerSlot>),
    >,
    mut tool_slots: Query<
        (&StationToolSlot, &SlotIndex, &mut BorderColor, &Children),
        (
            Without<StationSlot>,
            Without<StationPlayerSlot>,
            Without<StationCraftSlot>,
        ),
    >,
    mut flames: Query<(&mut BackgroundColor, &FurnaceFlame)>,
    mut arrow_fills: Query<(&mut Node, &FurnaceArrowFill)>,
    mut images: Query<&mut ImageNode>,
    mut texts: Query<&mut Text>,
) {
    if !station.open {
        return;
    }
    let world_stack = |index: usize| -> Option<ItemStack> {
        let pos = station.pos?;
        let container = container_at(state.world.as_ref()?, pos)?;
        container.get(index)
    };
    let selected_of = |address: StationAddress| station.selected == Some(address);

    for (slot, mut border, children) in &mut container_slots {
        paint_slot(
            &mut commands,
            &mut border,
            children,
            &mut images,
            &mut texts,
            &icons,
            world_stack(slot.index),
            selected_of(StationAddress::Container(slot.index)),
        );
    }
    for (_slot, index, mut border, children) in &mut player_slots {
        let stack = player
            .0
            .as_ref()
            .and_then(|p| p.inventory.items.get(index.0).copied().flatten());
        paint_slot(
            &mut commands,
            &mut border,
            children,
            &mut images,
            &mut texts,
            &icons,
            stack,
            selected_of(StationAddress::Player(index.0)),
        );
    }
    for (_slot, index, mut border, children) in &mut craft_slots {
        paint_slot(
            &mut commands,
            &mut border,
            children,
            &mut images,
            &mut texts,
            &icons,
            station.grid.get(index.0).copied().flatten(),
            selected_of(StationAddress::Grid(index.0)),
        );
    }
    for (_slot, index, mut border, children) in &mut tool_slots {
        paint_slot(
            &mut commands,
            &mut border,
            children,
            &mut images,
            &mut texts,
            &icons,
            station.tools.get(index.0).copied().flatten(),
            selected_of(StationAddress::Tool(index.0)),
        );
    }

    // Индикаторы печей: пламя (горит ли топливо) и ход готовки.
    if let (Some(kind), Some(pos), Some(world)) = (station.kind, station.pos, state.world.as_ref())
    {
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
        if let Some(container) = container_at(world, pos) {
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
    }
}

/// Пишет иконку, счётчик и рамку слота только при реальных изменениях.
fn paint_slot(
    commands: &mut Commands,
    border: &mut Mut<BorderColor>,
    children: &Children,
    images: &mut Query<&mut ImageNode>,
    texts: &mut Query<&mut Text>,
    icons: &UiIcons,
    stack: Option<ItemStack>,
    selected: bool,
) {
    let want_border = BorderColor::all(if selected {
        style::surface_active()
    } else {
        style::border()
    });
    if **border != want_border {
        **border = want_border;
    }
    for child in children.iter() {
        if let Ok(mut image) = images.get_mut(child) {
            let want_icon = stack
                .map(|s| icons.item(s.kind).clone())
                .unwrap_or_else(|| ImageNode::default().image);
            if image.image != want_icon {
                image.image = want_icon;
                commands.entity(child).remove::<FitDone>();
            }
        }
        if let Ok(mut t) = texts.get_mut(child) {
            let want_text = match stack {
                Some(s) if s.count > 1 => s.count.to_string(),
                _ => String::new(),
            };
            if t.as_str() != want_text {
                **t = want_text;
            }
        }
    }
}
