//! Окно-инспектор существа в редакторе: открывается СКМ-кликом по
//! существу без перетаскивания (обрабатывает ввод, см.
//! `editor_entity_drag` в app/input.rs). Показывает HP/голод/рейтинг брони
//! и весь инвентарь существа (misc/броня/одежда/косметика/мусорка) с
//! перетаскиванием предметов, как в обычном инвентаре.
//!
//! Перенос — командой `Command::EntityMoveItem` (симуляция валидирует
//! правила слотов через `Inventory::transfer`), выброс за пределами окна —
//! `Command::EntityDropItem` на землю под существом. Мир мутрует только
//! применение команд: окно лишь читает состояние и ставит команды.
//!
//! Неразумное существо (волк) — только полоса HP; секций инвентаря нет.
//! Окно живёт в режиме наблюдателя; при захвате существа по F1 оно
//! закрывается (у захваченного есть обычный UI игрока).

use bevy::input::mouse::MouseButton;
use bevy::picking::hover::Hovered;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

use crate::app::state::{CommandQueueRes, EditorSelection, GameMode, GameState};
use crate::simulation::armor::{ARMOR_RATING_MAX, rating as armor_rating};
use crate::simulation::commands::Command;
use crate::simulation::entities::SimEntity;
use crate::simulation::hunger::HUNGER_MAX;
use crate::simulation::items::{ItemStack, SlotArea, SlotRef, MISC_SLOTS};
use crate::ui::bottom_panel::{FitDone, FitIcon};
use crate::ui::icons::UiIcons;
use crate::ui::inventory::{DragGhost, GHOST_HALF, ICON_BOX, SLOT_GAP, SLOT_SIZE};
use crate::ui::style;
use crate::ui::{BlocksWorldInput, UiFont, cursor_over_node, set_text_if_neq};

/// Плагин окна-инспектора: жизненный цикл, перетаскивание, перерисовка.
pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectorUi>().add_systems(
            Update,
            (
                sync_inspector_window,
                inspector_drag,
                refresh_inspector,
            )
                .chain(),
        );
    }
}

/// Состояние окна-инспектора. `open` — существо, чьё окно должно быть
/// видно (дублирует `EditorSelection::selected` с фильтрами режима и
/// существования); `shown` — существо, для которого окно построено.
#[derive(Resource, Default)]
pub struct InspectorUi {
    /// id существа с открытым окном (для тултипов и ESC-цепочки).
    pub open: Option<u64>,
    shown: Option<u64>,
    /// Идущее перетаскивание стека (призрак под курсором). Реального
    /// снятия стека нет: перенос/выброс исполняются командой на отпускании.
    drag: Option<InspectorDrag>,
}

/// Перетаскивание в окне инспектора: слот-источник и призрак-иконка.
struct InspectorDrag {
    from: SlotRef,
    ghost: Entity,
}

/// Маркер корня окна инспектора (сносится при закрытии; виден ESC-цепочке).
#[derive(Component)]
pub(crate) struct InspectorRoot;

/// Слот инвентаря существа в окне инспектора: адрес в `Inventory`.
#[derive(Component)]
pub(crate) struct InspectorSlot {
    pub area: SlotArea,
    pub index: usize,
}

/// Панель окна (не фуллскрин-обёртка): отпускание перетаскивания за её
/// пределами бросает предмет на землю.
#[derive(Component)]
struct InspectorPanel;

/// Маркер текста-счётчика внутри слота.
#[derive(Component)]
struct InspectorSlotCount;

/// Заливка полосы значения (обновляется в `refresh_inspector`).
#[derive(Component)]
struct InspectorBarFill {
    kind: BarValue,
}

/// Какое значение показывает полоса (перерисовка из живой сущности).
#[derive(Clone, Copy, PartialEq, Eq)]
enum BarValue {
    Hp,
    Hunger,
    Armor,
}

/// Синхронизирует окно с выделением ввода: открытие/переключение сущности
/// пересобирает окно, закрытие (ESC, повторный клик, смена режима,
/// удаление существа) сносит корень.
fn sync_inspector_window(
    mut commands: Commands,
    selection: Res<EditorSelection>,
    state: Res<GameState>,
    mut inspector: ResMut<InspectorUi>,
    font: Res<UiFont>,
    roots: Query<Entity, With<InspectorRoot>>,
) {
    let want = if state.mode == GameMode::Observer {
        selection.selected
    } else {
        None
    };
    // Существо удалено (или было убрано редактором) — окно закрывается.
    let want = want.filter(|id| {
        state
            .world
            .as_ref()
            .is_some_and(|world| world.entities.get(*id).is_some())
    });
    inspector.open = want;

    if inspector.shown == want && want.is_some() {
        return;
    }
    // Пересборка: старый корень сносится, призрак снимается (стек не
    // снимался — перенос исполняется только на отпускании).
    for root in &roots {
        commands.entity(root).despawn();
    }
    if let Some(drag) = inspector.drag.take() {
        commands.entity(drag.ghost).despawn();
    }
    inspector.shown = want;
    let Some(id) = want else {
        return;
    };
    let Some(entity) = state
        .world
        .as_ref()
        .and_then(|world| world.entities.get(id))
    else {
        return;
    };
    spawn_inspector_window(&mut commands, entity, &font);
}

/// Стек в слоте инвентаря существа (только чтение).
fn stack_at(state: &GameState, id: u64, slot: SlotRef) -> Option<ItemStack> {
    state
        .world
        .as_ref()?
        .entities
        .get(id)?
        .mind
        .as_ref()?
        .inventory
        .get(slot)
}

/// Строит окно инспектора: полосы HP/голода/брони (над заголовком) и
/// секции инвентаря (у разумного) либо только HP (у волка).
fn spawn_inspector_window(commands: &mut Commands, entity: &SimEntity, font: &UiFont) {
    let title = match entity.kind {
        crate::simulation::EntityKind::Human => format!("Человек #{}", entity.id),
        crate::simulation::EntityKind::Wolf => format!("Волк #{}", entity.id),
    };
    let hp = entity.hp;
    let hunger = entity.hunger.value();
    let armor = entity.mind.as_ref().map(|m| armor_rating(&m.inventory));

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
            InspectorRoot,
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
                    InspectorPanel,
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
                    // Полосы: HP всегда, голод — у всех (волк тоже голодает),
                    // броня — только у разумных.
                    spawn_bar(
                        panel, font, "Здоровье", hp, entity.kind.max_hp(),
                        Color::srgb(0.78, 0.16, 0.14), BarValue::Hp,
                    );
                    spawn_bar(
                        panel, font, "Голод", hunger, HUNGER_MAX,
                        Color::srgb_u8(0xFF, 0x88, 0x44), BarValue::Hunger,
                    );
                    if entity.mind.is_some() {
                        spawn_bar(
                            panel, font, "Броня", armor.unwrap_or(0.0), ARMOR_RATING_MAX,
                            Color::srgb_u8(0xCC, 0xCC, 0xCC), BarValue::Armor,
                        );
                    }
                    if entity.mind.is_some() {
                        // Инвентарь целиком: misc, броня, одежда, косметика,
                        // мусорка — правила слотов решает симуляция.
                        let sections: [(SlotArea, &str, usize); 5] = [
                            (SlotArea::Misc, "Предметы", MISC_SLOTS),
                            (SlotArea::Armor, "Броня", 4),
                            (SlotArea::Clothes, "Одежда", 4),
                            (SlotArea::Bauble, "Косметика", 4),
                            (SlotArea::Trash, "Мусорка", 1),
                        ];
                        for (area, label, count) in sections {
                            panel
                                .spawn(Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(4.0),
                                    ..default()
                                })
                                .with_children(|block| {
                                    block.spawn((
                                        Text::new(label),
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
                                            for index in 0..count {
                                                spawn_inspector_slot(row, area, index, font);
                                            }
                                        });
                                });
                        }
                    }
                });
        });
}

/// Слот инвентаря существа: рамка, иконка и счётчик.
fn spawn_inspector_slot(
    row: &mut ChildSpawnerCommands,
    area: SlotArea,
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
        InspectorSlot { area, index },
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
            InspectorSlotCount,
        ));
    });
}

/// Одна полоса: фон с рамкой и заливка, подпись справа от бара (как в
/// шкалах игрока). Начальное значение — на момент открытия, дальше
/// обновляет `refresh_inspector`.
fn spawn_bar(
    panel: &mut ChildSpawnerCommands,
    font: &UiFont,
    label: &str,
    value: f32,
    max: f32,
    color: Color,
    kind: BarValue,
) {
    let ratio = if max > 0.0 { (value / max).clamp(0.0, 1.0) } else { 0.0 };
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(160.0),
                    height: Val::Px(12.0),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(style::surface()),
                BorderColor::all(style::border()),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Node {
                        width: Val::Percent(ratio * 100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(color),
                    InspectorBarFill { kind },
                ));
            });
            row.spawn((
                Text::new(label),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 12.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
        });
}

/// Перетаскивание стеков: ЛКМ над непустым слотом берёт стек «в руку»
/// (призрак), отпускание над другим слотом шлёт `EntityMoveItem`, за
/// пределами окна — `EntityDropItem` (предмет падает под существом).
/// ПКМ отменяет перетаскивание.
fn inspector_drag(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    state: Res<GameState>,
    mut inspector: ResMut<InspectorUi>,
    mut queue: ResMut<CommandQueueRes>,
    icons: Res<UiIcons>,
    windows: Query<&Window>,
    slots: Query<(&InspectorSlot, &ComputedNode, &UiGlobalTransform)>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<InspectorPanel>>,
    mut ghosts: Query<(Entity, &mut Node), With<DragGhost>>,
) {
    let Some(id) = inspector.open else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let cursor = window.cursor_position();

    // Отмена/завершение — до старта (быстрый клик между кадрами).
    if mouse.just_released(MouseButton::Right) {
        if let Some(drag) = inspector.drag.take() {
            commands.entity(drag.ghost).despawn();
        }
        return;
    }
    if mouse.just_released(MouseButton::Left) {
        if let Some(drag) = inspector.drag.take() {
            commands.entity(drag.ghost).despawn();
            finish_inspector_drag(&state, &mut queue, id, drag, cursor, window, &slots, &panels);
        }
        return;
    }
    // Призрак следует за курсором.
    if inspector.drag.is_some()
        && let Some(cursor) = cursor
    {
        for (_, mut node) in &mut ghosts {
            node.left = Val::Px(cursor.x - GHOST_HALF);
            node.top = Val::Px(cursor.y - GHOST_HALF);
        }
    }
    // Старт: ЛКМ над непустым слотом.
    if inspector.drag.is_none()
        && mouse.just_pressed(MouseButton::Left)
        && let Some(cursor) = cursor
        && let Some(slot) = slots
            .iter()
            .find(|(_, computed, transform)| {
                cursor_over_node(window, computed, transform, cursor)
            })
            .map(|(slot, _, _)| SlotRef { area: slot.area, index: slot.index })
        && let Some(stack) = stack_at(&state, id, slot)
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
                ImageNode::new(icons.item(stack.kind).clone()),
                Pickable::IGNORE,
                GlobalZIndex(i32::MAX),
                DragGhost,
            ))
            .id();
        inspector.drag = Some(InspectorDrag { from: slot, ghost });
    }
}

/// Завершает перетаскивание: над слотом — перенос командой; вне окна —
/// выброс стека на землю (`EntityDropItem`); внутри окна мимо слота —
/// отмена (стек остаётся на месте).
fn finish_inspector_drag(
    state: &GameState,
    queue: &mut CommandQueueRes,
    id: u64,
    drag: InspectorDrag,
    cursor: Option<Vec2>,
    window: &Window,
    slots: &Query<(&InspectorSlot, &ComputedNode, &UiGlobalTransform)>,
    panels: &Query<(&ComputedNode, &UiGlobalTransform), With<InspectorPanel>>,
) {
    let Some(cursor) = cursor else {
        return;
    };
    // Существование слота-источника проверяет симуляция: стек могли
    // забрать пока его тащили (редактор живёт на паузе, но всё же).
    if stack_at(state, id, drag.from).is_none() {
        return;
    }
    let target = slots
        .iter()
        .find(|(_, computed, transform)| cursor_over_node(window, computed, transform, cursor))
        .map(|(slot, _, _)| SlotRef { area: slot.area, index: slot.index })
        .filter(|to| *to != drag.from);
    if let Some(to) = target {
        queue.0.push(Command::EntityMoveItem { id, from: drag.from, to });
        return;
    }
    // Внутри окна — просто отмена; снаружи — выброс на землю.
    let over_panel = panels
        .iter()
        .any(|(computed, transform)| cursor_over_node(window, computed, transform, cursor));
    if !over_panel {
        queue.0.push(Command::EntityDropItem { id, from: drag.from });
    }
}

/// Перерисовка полос и слотов из живой сущности. Пишет только реальные
/// изменения (как `refresh_slots` инвентаря).
fn refresh_inspector(
    mut commands: Commands,
    state: Res<GameState>,
    inspector: Res<InspectorUi>,
    icons: Res<UiIcons>,
    mut fills: Query<(&mut Node, &InspectorBarFill)>,
    mut slots: Query<(Entity, &InspectorSlot, &mut BorderColor, &Children)>,
    mut images: Query<&mut ImageNode>,
    mut texts: Query<&mut Text>,
) {
    let Some(id) = inspector.open else {
        return;
    };
    let Some(entity) = state
        .world
        .as_ref()
        .and_then(|world| world.entities.get(id))
    else {
        return;
    };
    let hp = entity.hp;
    let hunger = entity.hunger.value();
    let armor = entity
        .mind
        .as_ref()
        .map(|m| armor_rating(&m.inventory));
    for (mut node, fill) in &mut fills {
        let (value, max) = match fill.kind {
            BarValue::Hp => (hp, entity.kind.max_hp()),
            BarValue::Hunger => (hunger, HUNGER_MAX),
            BarValue::Armor => (armor.unwrap_or(0.0), ARMOR_RATING_MAX),
        };
        let ratio = if max > 0.0 { (value / max).clamp(0.0, 1.0) } else { 0.0 };
        let want = Val::Percent(ratio * 100.0);
        if node.width != want {
            node.width = want;
        }
    }
    for (_slot_entity, slot, mut border, children) in &mut slots {
        let stack = entity
            .mind
            .as_ref()
            .and_then(|m| m.inventory.get(SlotRef { area: slot.area, index: slot.index }));
        let want_border = BorderColor::all(style::border());
        if *border != want_border {
            *border = want_border;
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
            if let Ok(t) = texts.get_mut(child) {
                let want_text = match stack {
                    Some(s) if s.count > 1 => s.count.to_string(),
                    _ => String::new(),
                };
                set_text_if_neq(t, &want_text);
            }
        }
    }
}
