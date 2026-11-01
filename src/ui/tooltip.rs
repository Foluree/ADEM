//! Тултипы предметов: всплывающее название при наведении на слот
//! инвентаря, клетку крафта или слот хотбара, в котором лежит предмет.
//!
//! Названия предметов — данные в `info/items.json` (каталог описаний по
//! GAME_DESIGN раздел 4.1): предметов будет много, правки не должны
//! требовать перекомпиляции. Отсутствующий или битый файл не роняет игру —
//! остаётся технический идентификатор предмета.

use std::collections::HashMap;

use bevy::picking::Pickable;
use bevy::prelude::*;

use crate::app::state::{GameMode, GameState, MenuKind, PlayerRes};
use crate::simulation::containers;
use crate::simulation::items::{ItemKind, SlotRef};
use crate::ui::cursor_over_node;
use crate::ui::UiFont;
use crate::ui::creative::CreativeTile;
use crate::ui::station::{SlotIndex, StationPlayerSlot, StationSlot, StationUi};
use crate::ui::inventory::{CraftBookRes, CraftResultSlot, CraftingUi, InventoryUi, InventoryWindowSlot};
use crate::ui::inventory::{CraftSlot, InvSlot};
use crate::ui::style;
use serde::Deserialize;

pub struct TooltipPlugin;

impl Plugin for TooltipPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemNames>()
            .add_systems(Startup, load_item_names)
            .add_systems(Update, update_tooltip);
    }
}

/// Названия предметов по строковым идентификаторам (`ItemKind::id`).
#[derive(Resource, Default)]
pub struct ItemNames {
    names: HashMap<String, String>,
}

impl ItemNames {
    /// Название предмета. Нет записи в файле — технический id, чтобы
    /// новый предмет не оставался безымянным до правки данных.
    pub fn name(&self, kind: ItemKind) -> String {
        self.names
            .get(kind.id())
            .cloned()
            .unwrap_or_else(|| kind.id().to_string())
    }
}

#[derive(Deserialize, Default)]
struct ItemsFile {
    #[serde(default)]
    items: Vec<ItemEntry>,
}

#[derive(Deserialize)]
struct ItemEntry {
    id: String,
    name: String,
}

/// Читает названия один раз при старте. Ошибка файла — предупреждение в
/// лог, игра работает с id-названиями.
fn load_item_names(mut commands: Commands) {
    let mut names = ItemNames::default();
    match std::fs::read_to_string("info/items.json")
        .map_err(|e| e.to_string())
        .and_then(|text| serde_json::from_str::<ItemsFile>(&text).map_err(|e| e.to_string()))
    {
        Ok(file) => {
            for entry in file.items {
                names.names.insert(entry.id, entry.name);
            }
            debug!("Загружено названий предметов: {}", names.names.len());
        }
        Err(error) => {
            warn!("Тултипы: info/items.json не прочитан ({error}); названия предметов = id");
        }
    }
    commands.insert_resource(names);
}

/// Маркер узла тултипа. Один на кадр: создаётся при наведении, удаляется,
/// когда курсор ушёл со слота или предмет кончился.
#[derive(Component)]
struct TooltipNode;

/// Текст названия внутри тултипа.
#[derive(Component)]
struct TooltipText;

/// Смещение тултипа от курсора, px (право-низ, как подсказка у курсора).
const TOOLTIP_OFFSET: f32 = 16.0;

/// Показывает тултип над слотом с предметом. Кадр без наведённого слота с
/// предметом — тултипа нет. Перетаскивание стека скрывает тултип: под
/// курсором «виртуальная рука», а не содержимое слота.
fn update_tooltip(
    mut commands: Commands,
    windows: Query<&Window>,
    state: Res<GameState>,
    inv: Res<InventoryUi>,
    craft: Res<CraftingUi>,
    book: Res<CraftBookRes>,
    player: Res<PlayerRes>,
    station: Res<StationUi>,
    names: Res<ItemNames>,
    font: Res<UiFont>,
    inv_slots: Query<(
        &InvSlot,
        Has<InventoryWindowSlot>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    craft_slots: Query<(&CraftSlot, &ComputedNode, &UiGlobalTransform)>,
    result_slots: Query<(&ComputedNode, &UiGlobalTransform), With<CraftResultSlot>>,
    // Слоты всех окон одним параметром-кортежем: у систем Bevy предел в
    // 16 параметров.
    (creative_tiles, station_slots, station_player_slots): (
        Query<(&CreativeTile, &ComputedNode, &UiGlobalTransform)>,
        Query<(&StationSlot, &ComputedNode, &UiGlobalTransform)>,
        Query<(&StationPlayerSlot, &SlotIndex, &ComputedNode, &UiGlobalTransform)>,
    ),
    mut tooltips: Query<(Entity, &mut Node, Option<&Children>), With<TooltipNode>>,
    mut texts: Query<&mut Text>,
) {
    // Модальные меню поверх всего (деревья, пауза): тултип слота под ними
    // вылезал бы поверх полноэкранного фона — спрайтов под курсором там
    // нет и показывать нечего. Творческое меню — исключение: у его плиток
    // есть тултипы.
    if state.active_menu.is_some() && state.active_menu != Some(MenuKind::Creative) {
        for (entity, _, _) in &mut tooltips {
            commands.entity(entity).despawn();
        }
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        for (entity, _, _) in &mut tooltips {
            commands.entity(entity).despawn();
        }
        return;
    };
    let name = hovered_item_name(
        window,
        cursor,
        &state,
        &inv,
        &craft,
        &book,
        &player,
        &station,
        &names,
        &inv_slots,
        &craft_slots,
        &result_slots,
        &creative_tiles,
        &station_slots,
        &station_player_slots,
    );

    let Some(name) = name else {
        for (entity, _, _) in &mut tooltips {
            commands.entity(entity).despawn();
        }
        return;
    };

    let left = cursor.x + TOOLTIP_OFFSET;
    let top = cursor.y + TOOLTIP_OFFSET;
    match tooltips.iter().next() {
        Some((_, _, _)) => {
            // Тултип уже на экране: двигаем узел и обновляем текст.
            for (_, mut node, _) in &mut tooltips {
                node.left = Val::Px(left);
                node.top = Val::Px(top);
            }
            for (_, _, children) in &mut tooltips {
                if let Some(children) = children
                    && let Some(child) = children.iter().next()
                    && let Ok(mut t) = texts.get_mut(child)
                    && t.as_str() != name
                {
                    **t = name.clone();
                }
            }
        }
        None => {
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(left),
                        top: Val::Px(top),
                        padding: UiRect::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(style::background()),
                    BorderColor::all(style::border()),
                    // Тултип следует за курсором и не должен перехватывать
                    // наведение/клики у слота под ним.
                    Pickable::IGNORE,
                    // Поверх панелей и модальных меню (творческое меню тоже
                    // показывает тултипы своих плиток).
                    GlobalZIndex(i32::MAX - 1),
                    TooltipNode,
                ))
                .with_children(|tip| {
                    tip.spawn((
                        Text::new(name),
                        TextFont {
                            font: font.0.clone().into(),
                            font_size: 14.0.into(),
                            ..default()
                        },
                        TextColor(style::text()),
                        TooltipText,
                    ));
                });
        }
    }
}

/// Название предмета в слоте под курсором, если такой есть.
#[allow(clippy::too_many_arguments)]
fn hovered_item_name(
    window: &Window,
    cursor: Vec2,
    state: &GameState,
    inv: &InventoryUi,
    craft: &CraftingUi,
    book: &CraftBookRes,
    player: &PlayerRes,
    station: &StationUi,
    names: &ItemNames,
    inv_slots: &Query<(
        &InvSlot,
        Has<InventoryWindowSlot>,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    craft_slots: &Query<(&CraftSlot, &ComputedNode, &UiGlobalTransform)>,
    result_slots: &Query<(&ComputedNode, &UiGlobalTransform), With<CraftResultSlot>>,
    creative_tiles: &Query<(&CreativeTile, &ComputedNode, &UiGlobalTransform)>,
    station_slots: &Query<(&StationSlot, &ComputedNode, &UiGlobalTransform)>,
    station_player_slots: &Query<(
        &StationPlayerSlot,
        &SlotIndex,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
) -> Option<String> {
    if inv.drag.is_some() || station.drag.is_some() {
        return None;
    }
    let under_cursor = |computed: &ComputedNode, transform: &UiGlobalTransform| -> bool {
        cursor_over_node(window, computed, transform, cursor)
    };

    // Творческое меню: плитка показывает название своего предмета.
    if state.active_menu == Some(MenuKind::Creative) {
        let (tile, _, _) = creative_tiles
            .iter()
            .find(|(_, computed, transform)| under_cursor(computed, transform))?;
        return Some(names.name(tile.kind));
    }
    if state.active_menu.is_some() {
        return None;
    }

    // Окно станции: пустой слот показывает роль («Топливо», «Только уголь»),
    // занятый — название предмета, как остальные слоты.
    if station.open {
        if let Some(kind) = station.kind
            && let Some((slot, ..)) = station_slots
                .iter()
                .find(|(_, computed, transform)| under_cursor(computed, transform))
        {
            let stack = state
                .world
                .as_ref()
                .and_then(|world| containers::container_at(world, station.pos?))
                .and_then(|container| container.get(slot.index));
            return Some(match stack {
                Some(stack) => names.name(stack.kind),
                None => containers::slot_hint(kind, slot.index)?.to_string(),
            });
        }
        if let Some((_, index, ..)) = station_player_slots
            .iter()
            .find(|(_, _, computed, transform)| under_cursor(computed, transform))
            && let Some(stack) = player
                .0
                .as_ref()
                .and_then(|p| p.inventory.items.get(index.0).copied().flatten())
        {
            return Some(names.name(stack.kind));
        }
        return None;
    }

    let p = player.0.as_ref()?;
    // Слот под курсором: общий хит-тест UI (физические пиксели, как в
    // пикинге bevy) — тот же, что у остальных проверок курсора.

    // Слот результата крафта: название подобранного рецепта.
    if inv.open
        && result_slots
            .iter()
            .any(|(computed, transform)| under_cursor(computed, transform))
        && let Some(recipe) = book.0.matching_recipe(&craft.slots)
    {
        return Some(names.name(recipe.result.kind));
    }

    // Клетки крафта (окно инвентаря открыто, иначе клеток не существует).
    if inv.open
        && let Some(stack) = craft_slots
            .iter()
            .find(|(_, computed, transform)| under_cursor(computed, transform))
            .and_then(|(slot, _, _)| craft.slots.get(slot.index).copied().flatten())
    {
        return Some(names.name(stack.kind));
    }

    // Слоты инвентаря и хотбара (оба носят InvSlot). Слоты окна живут,
    // только пока окно открыто; хотбар виден в режиме игрока — скрытый
    // (Display::None) хотбар наведение ловить не должен.
    for (slot, window_slot, computed, transform) in inv_slots {
        if !under_cursor(computed, transform) {
            continue;
        }
        let visible = if window_slot { inv.open } else { state.mode == GameMode::Player };
        if !visible {
            continue;
        }
        let stack = p.inventory.get(SlotRef {
            area: slot.area,
            index: slot.index,
        })?;
        return Some(names.name(stack.kind));
    }
    None
}
