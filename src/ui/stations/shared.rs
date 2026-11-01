//! Общие части окна станции: маркеры слотов, утилиты чтения стека по
//! адресу, универсальный строитель слота, перерисовка слотов.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use crate::app::state::GameState;
use crate::simulation::containers::{container_at, SLOT_AREA_PLAYER_MISC};
use crate::simulation::items::{self, ItemStack};
use crate::simulation::ItemKind;
use crate::ui::bottom_panel::{FitDone, FitIcon};
use crate::ui::icons::UiIcons;
use crate::ui::inventory::{ICON_BOX, SLOT_GAP, SLOT_SIZE};
use crate::ui::style;
use crate::ui::{cursor_over_node, UiFont};

use super::{StationAddress, StationUi};

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
pub(crate) struct StationSlotCount;

/// Панель окна станции (не фуллскрин-обёртка): хит-тест «курсор над
/// окном» — отпускание перетаскивания за её пределами бросает предмет.
#[derive(Component)]
pub(crate) struct StationPanel;

/// Слот результата печи («кнопка»): клик забирает стек, drag выключен.
#[derive(Component)]
pub(crate) struct StationOutputSlot;

/// Обобщённый слот станции с маркером-адресом `M` (сетка, инструменты,
/// misc игрока) — единый вид со слотами контейнера.
pub(crate) fn spawn_addressed_slot<M: Component + Default>(
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

/// Ряд misc-слотов игрока (хотбар в окне станции). Перенос — командой
/// симуляции, поэтому слоты носят `StationPlayerSlot`, а не `InvSlot`.
pub(crate) fn spawn_player_misc(panel: &mut ChildSpawnerCommands, font: &UiFont) {
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

/// Стек по адресу (контейнер — из мира, инвентарь захваченного существа
/// и UI-сетка — из ресурсов).
pub(crate) fn stack_at(
    station: &StationUi,
    state: &GameState,
    address: StationAddress,
) -> Option<ItemStack> {
    match address {
        StationAddress::Container(index) => {
            let world = state.world.as_ref()?;
            let pos = station.pos?;
            container_at(world, pos)?.get(index)
        }
        StationAddress::Player(i) => state
            .controlled_mind()?
            .inventory
            .items
            .get(i)
            .copied()
            .flatten(),
        StationAddress::Grid(i) => station.grid.get(i).copied().flatten(),
        StationAddress::Tool(i) => station.tools.get(i).copied().flatten(),
    }
}

/// Вид предмета в слоте-источнике drag (без него иконку призрака не взять).
pub(crate) fn stack_kind(
    station: &StationUi,
    state: &GameState,
    address: StationAddress,
) -> ItemKind {
    stack_at(station, state, address)
        .map(|s| s.kind)
        .unwrap_or(ItemKind::Rock)
}

/// Кодирует адрес в пару (area, index) команды переноса: (0xFF, i) —
/// misc игрока, (слот, 0) — слот контейнера.
pub(crate) fn encode_address(address: StationAddress) -> (u8, u8) {
    match address {
        StationAddress::Container(i) => (i as u8, 0),
        StationAddress::Player(i) => (SLOT_AREA_PLAYER_MISC, i as u8),
        // Сетка и инструменты — UI-локальные, в команду не попадают.
        StationAddress::Grid(i) | StationAddress::Tool(i) => (i as u8, 0),
    }
}

/// Адрес слота окна станции под курсором: (адрес, является ли выходом).
pub(crate) fn hovered_station_address(
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

/// Обновляет иконки слотов окна станции по контейнеру мира, инвентарю
/// игрока, сетке крафта и инструментам. Маркеры слотов взаимоисключающие,
/// но Bevy это не знает — конфликты `&mut BorderColor` разруливаем
/// явными `Without` (иначе B0001 при инициализации системы).
pub(crate) fn refresh_station_slots(
    mut commands: Commands,
    station: Res<StationUi>,
    state: Res<GameState>,
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
        let stack = state
            .controlled_mind()
            .and_then(|m| m.inventory.items.get(index.0).copied().flatten());
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
}

/// Пишет иконку, счётчик и рамку слота только при реальных изменениях.
pub(crate) fn paint_slot(
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
