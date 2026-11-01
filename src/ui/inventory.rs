//! Окно инвентаря (клавиша E, бинд «Инвентарь»): секции слотов по типам
//! (misc / броня / косметика), иконки предметов, счётчики и работа с
//! сеткой крафта 3×3. Перемещение между слотами инвентаря — перетаскиванием
//! (зажать ЛКМ и отпустить над целевым слотом); клик по слоту выбирает
//! стек-источник для укладки в сетку крафта. В сетке: ЛКМ — доложить
//! один предмет, ПКМ — забрать один обратно, Shift+ЛКМ — вернуть весь
//! слот; удержание кнопки при переводе курсора на новую клетку кладёт
//! (+ЛКМ) или забирает (−ПКМ) по предмету на каждую посещённую клетку;
//! клик по слоту результата создаёт предмет, Shift+ЛКМ — перекрафчивает
//! всё доступное. Правила совместимости — в симуляции,
//! `Inventory::accepts`/`transfer`. Раскладка задаётся
//! константами ниже и правится в одном месте.

use bevy::input::mouse::MouseButton;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button, TextInput};

use crate::app::input::cursor_world_tile;
use crate::app::settings::{ControlAction, Settings};
use crate::app::state::{
    CommandQueueRes, GameMode, GameState, MenuKind, PlayerRes, WindowFocusState,
};
use crate::simulation::commands::Command;
use crate::simulation::crafts::{CraftBook, GRID_SLOTS};
use crate::simulation::items::{
    ARMOR_LAYOUT, SlotArea, SlotRef, ARMOR_SLOTS, BAUBLE_SLOTS, CLOTHES_SLOTS, MISC_SLOTS,
};
use crate::simulation::{ItemStack, TilePos};
use crate::ui::UiFont;
use crate::ui::bottom_panel::{FitDone, FitIcon};
use crate::ui::icons::UiIcons;
use crate::ui::pause_menu::BindingCapture;
use crate::ui::style;
use crate::ui::{BlocksWorldInput, cursor_over_node, text_input_focused};

pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InventoryUi>()
            .init_resource::<CraftingUi>()
            .init_resource::<CraftBookRes>()
            .init_resource::<CraftHoldFill>()
            .add_systems(Startup, load_crafts)
            .add_observer(on_slot_activated)
            .add_observer(on_craft_slot_activated)
            .add_observer(on_result_activated)
            .add_systems(
                Update,
                (
                    toggle_inventory,
                    // Сброс сетки крафта и перетаскивание видят состояние
                    // окна уже после переключения в этом же кадре.
                    reset_crafting_on_close,
                    inventory_drag,
                    craft_grid_hold_fill,
                    refresh_slots,
                    refresh_crafting,
                ).chain(),
            );
    }
}

// --- Раскладка инвентаря: правится здесь ---

/// Сторона слота.
pub(crate) const SLOT_SIZE: f32 = 44.0;
/// Зазор между слотами и рядами.
pub(crate) const SLOT_GAP: f32 = 6.0;
/// Квадрат иконки внутри слота (пропорции вписывает fit_icons панели).
pub(crate) const ICON_BOX: f32 = 30.0;
/// Слотов в одном ряду секции.
const SLOTS_PER_ROW: usize = 8;

impl SlotArea {
    /// Заголовок секции (мусорка спавнится без заголовка).
    fn title(self) -> &'static str {
        match self {
            SlotArea::Misc => "Предметы",
            SlotArea::Armor => "Броня",
            SlotArea::Clothes => "Одежда",
            SlotArea::Bauble => "Косметика",
            SlotArea::Trash => "",
        }
    }
}

/// Открыто ли окно инвентаря. Видно ESC-цепочке пауз-меню: ESC сначала
/// закрывает инвентарь, потом открывает меню.
#[derive(Resource, Default, Debug)]
pub struct InventoryUi {
    pub open: bool,
    /// Выбранный слот-источник: клик по слоту инвентаря выбирает стек,
    /// который затем укладывается в сетку крафта кликами по её клеткам.
    pub(crate) selected: Option<SlotRef>,
    /// Активное перетаскивание стека между слотами инвентаря. Виден
    /// тултипам: пока стек «в руке», под курсором не содержимое слота.
    pub(crate) drag: Option<DragState>,
}

/// Панель окна инвентаря (не фуллскрин-обёртка): хит-тест «курсор над
/// окном» для выброса перетаскиваемого стека за пределами GUI.
#[derive(Component)]
pub(crate) struct InventoryPanel;

/// Идущее перетаскивание: слот-источник, снятый с него стек и призрак-
/// иконка под курсором. При закрытии окна состояние очищается, призрак
/// удаляется явно (он живёт поверх UI, а не внутри корня окна). Виден
/// тултипам (`InventoryUi::drag` — «стек в руке» скрывает тултип).
#[derive(Debug)]
pub(crate) struct DragState {
    from: SlotRef,
    stack: ItemStack,
    ghost: Entity,
}

/// Полуразмер призрака-иконки, px (иконка вписана в квадрат `GHOST_HALF * 2`).
pub(crate) const GHOST_HALF: f32 = 16.0;

/// Маркер призрака-иконки перетаскиваемого стека (следует за курсором).
/// Общий с окном станций: одинаковая механика «стек под курсором».
#[derive(Component)]
pub(crate) struct DragGhost;

/// Сетка крафта 3x3: слоты и выбранный источник в ней.
#[derive(Resource, Default, Debug)]
pub struct CraftingUi {
    pub slots: [Option<ItemStack>; GRID_SLOTS],
    pub selected: Option<usize>,
}

/// Состояние заполнения сетки крафта зажатыми кнопками: последний слот,
/// посещённый каждой кнопкой, и слот, обработанный текущим зажатием.
/// `filled_*` нужен наблюдателю активации: отпускание кнопки над слотом,
/// который заполнился при перемещении, не должно срабатывать второй раз.
#[derive(Resource, Default, Debug)]
struct CraftHoldFill {
    last_lmb: Option<usize>,
    last_rmb: Option<usize>,
    filled_lmb: Option<usize>,
    filled_rmb: Option<usize>,
}

/// Рецепты ручного крафта, загруженные из `data/crafts/*.json`.
/// Newtype: модель рецептов живёт в симуляции без зависимости от Bevy.
#[derive(Resource, Default, Debug)]
pub struct CraftBookRes(pub CraftBook);

/// Читает рецепты один раз при старте. Ошибки отдельных файлов не роняют
/// игру — только пишутся в лог, остальные рецепты продолжают работать.
fn load_crafts(mut commands: Commands) {
    let (book, errors) = CraftBook::load_dir("data/crafts");
    for error in errors {
        warn!("Крафт: {error}");
    }
    debug!("Загружено рецептов крафта: {}", book.recipes.len());
    commands.insert_resource(CraftBookRes(book));
}

/// Маркер корня окна (сносится целиком при закрытии). Виден ESC-цепочке пауз-меню.
#[derive(Component)]
pub(crate) struct InventoryRoot;

/// Маркер слота: секция и индекс — по ним читается инвентарь.
#[derive(Component)]
pub(crate) struct InvSlot {
    pub(crate) area: SlotArea,
    pub(crate) index: usize,
}

/// Слот именно окна инвентаря. Хотбар тоже носит `InvSlot` (чтобы стеки
/// можно было перетаскивать между ним и окном), но перекрашивать его
/// рамку из окна нельзя: у хотбара своя подсветка выбранного слота.
/// Маркер виден тултипам: слот окна и слот хотбара по-разному решают,
/// видно ли их на экране.
#[derive(Component)]
pub(crate) struct InventoryWindowSlot;

/// Клетка сетки крафта: индекс клетки 0..GRID_SLOTS. Виден тултипам.
#[derive(Component)]
pub(crate) struct CraftSlot {
    pub(crate) index: usize,
}

/// Кликабельный слот результата: показывает подобранный рецепт и выдаёт
/// предмет по клику. Виден тултипам: наведение показывает название
/// результата, как и у плиток творческого меню.
#[derive(Component)]
pub(crate) struct CraftResultSlot;

/// Маркер текста-счётчика внутри слота.
#[derive(Component)]
struct SlotCount;

/// Тоггл окна по бинду инвентаря (по умолчанию E). Пока фокус в текстовом
/// поле, идёт захват бинда или открыто модальное меню — не срабатывает.
/// Окно доступно, только если существо игрока существует (и в режиме
/// игрока, и «припаркованное» в наблюдателе).
fn toggle_inventory(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut inv: ResMut<InventoryUi>,
    state: Res<GameState>,
    player: Res<PlayerRes>,
    settings: Res<Settings>,
    capture: Res<BindingCapture>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
    font: Res<UiFont>,
    roots: Query<Entity, With<InventoryRoot>>,
    mut station: ResMut<crate::ui::station::StationUi>,
    station_roots: Query<Entity, With<crate::ui::station::StationRoot>>,
) {
    if text_input_focused(&focus, &text_inputs) || capture.0.is_some() || state.active_menu.is_some() {
        return;
    }
    if !keys.just_pressed(settings.controls.key(ControlAction::Inventory)) || player.0.is_none() {
        return;
    }

    // Открытое окно станции перехватывает бинд инвентаря: Tab закрывает
    // его и НЕ открывает инвентарь (содержимое сетки вернёт
    // close_station_window). Повторный Tab при закрытой станции откроет
    // инвентарь как обычно.
    if station.open {
        station.open = false;
        station.selected = None;
        for e in &station_roots {
            commands.entity(e).despawn();
        }
        return;
    }

    inv.open = !inv.open;
    if inv.open {
        // Окно пересоздаётся с нуля: сбрасываем клик-выбор слота.
        inv.selected = None;
        spawn_inventory(&mut commands, &font);
    } else {
        // Выбор и перетаскивание разбирает reset_crafting_on_close,
        // который вернёт стек «в руки» в свой слот.
        inv.selected = None;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
}

/// Строит окно инвентаря: заголовок, слева столбцы брони и косметики,
/// справа основные слоты и под ними сетка крафта. Полноэкранный wrapper
/// нужен для центрирования; маркер `InventoryRoot` стоит именно на нём,
/// чтобы закрытие окна сносило весь overlay — иначе невидимый full-screen
/// слой остаётся и навсегда перехватывает клики по UI. Маркер блокировки
/// карты (`BlocksWorldInput`) — только на самой панели: карта за пределами
/// окна кликается.
fn spawn_inventory(commands: &mut Commands, font: &UiFont) {
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
            InventoryRoot,
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
                    InventoryPanel,
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Инвентарь"),
                        TextFont {
                            font: font.0.clone().into(),
                            font_size: 20.0.into(),
                            ..default()
                        },
                        TextColor(style::text()),
                    ));
                    // Слева — броня, одежда и косметика (столбцы), справа —
                    // основные слоты и крафт; фон панели растягивается
                    // под содержимое, панель остаётся по центру экрана.
                    panel
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(12.0),
                            align_items: AlignItems::FlexStart,
                            ..default()
                        })
                        .with_children(|content| {
                            spawn_section(content, SlotArea::Armor, font, SectionLayout::Column);
                            spawn_section(content, SlotArea::Clothes, font, SectionLayout::Column);
                            // Мусорка завершает колонку косметики снизу.
                            content
                                .spawn(Node {
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                })
                                .with_children(|column| {
                                    spawn_section(
                                        column,
                                        SlotArea::Bauble,
                                        font,
                                        SectionLayout::Column,
                                    );
                                    spawn_trash(column, font);
                                });
                            content
                                .spawn(Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(10.0),
                                    ..default()
                                })
                                .with_children(|right| {
                                    spawn_section(
                                        right,
                                        SlotArea::Misc,
                                        font,
                                        SectionLayout::Rows,
                                    );
                                    spawn_crafting(right, font);
                                });
                        });
                });
        });
}

/// Направление раскладки секции: рядами по `SLOTS_PER_ROW` или столбцом
/// (броня и косметика — вертикальные колонки слева от основных слотов).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SectionLayout {
    Rows,
    Column,
}

/// Секция: заголовок и слоты. Рядами — по SLOTS_PER_ROW в строку,
/// столбцом — по одному слоту в строке.
fn spawn_section(
    panel: &mut ChildSpawnerCommands,
    area: SlotArea,
    font: &UiFont,
    layout: SectionLayout,
) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|block| {
            block.spawn((
                Text::new(area.title()),
                TextFont {
                    font: font.0.clone().into(),
                    font_size: 13.0.into(),
                    ..default()
                },
                TextColor(style::text_dim()),
            ));
            block
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(SLOT_GAP),
                    ..default()
                })
                .with_children(|rows| {
                    let count = area.slots_count();
                    let per_row = match layout {
                        SectionLayout::Rows => SLOTS_PER_ROW,
                        SectionLayout::Column => 1,
                    };
                    for row_start in (0..count).step_by(per_row) {
                        rows.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(SLOT_GAP),
                            ..default()
                        })
                        .with_children(|row| {
                            for index in row_start..(row_start + per_row).min(count) {
                                spawn_slot(row, area, index, font);
                            }
                        });
                    }
                });
        });
}

/// Мусорка: слот удаления предметов под столбцом косметики, с отступом
/// в пол-высоты слота. Один на всё окно: принимает что угодно, каждая
/// новая кладка затирает прежний стек (тот ещё можно забрать перетаскиванием).
/// Пустой показывает текстуру `trash_can.png`, занятый — иконку предмета.
fn spawn_trash(content: &mut ChildSpawnerCommands, font: &UiFont) {
    content.spawn((
        Button,
        Node {
            width: Val::Px(SLOT_SIZE),
            height: Val::Px(SLOT_SIZE),
            margin: UiRect::top(Val::Px(SLOT_SIZE * 0.5)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(style::surface()),
        BorderColor::all(style::border()),
        Hovered::default(),
        InvSlot { area: SlotArea::Trash, index: 0 },
        InventoryWindowSlot,
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
            SlotCount,
        ));
    });
}

/// Слот: рамка, пустая иконка (заполнит refresh_slots), счётчик.
/// Кликабелен: `Button`-виджет шлёт `Activate`, который ловит глобальный
/// наблюдатель переноса. В пустых слотах брони иконку-заглушку рисует
/// `refresh_slots` (текстуры `textures/ui/inventory/empty_armor_slot_*`).
fn spawn_slot(
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
        InvSlot { area, index },
        InventoryWindowSlot,
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
            SlotCount,
        ));
    });
}

/// Активация слота инвентаря (Button-виджет). Клик по слоту только
/// выбирает стек-источник для укладки в сетку крафта (повторный клик
/// снимает выбор). Перенос между слотами инвентаря — перетаскиванием
/// (`inventory_drag`), поэтому здесь нет логики перемещения.
fn on_slot_activated(
    activate: On<Activate>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut inv: ResMut<InventoryUi>,
    player: Res<PlayerRes>,
    slots: Query<&InvSlot>,
) {
    // Активация приходит на клик любой кнопкой мыши; слоты инвентаря
    // реагируют только на ЛКМ.
    if mouse.just_released(MouseButton::Right) || mouse.just_released(MouseButton::Middle) {
        return;
    }
    // Слоты хотбара — тоже `InvSlot`, но выбор источника для крафта
    // осмыслен только при открытом окне.
    if !inv.open {
        return;
    }
    let Ok(clicked) = slots.get(activate.entity) else {
        return;
    };
    let target = SlotRef { area: clicked.area, index: clicked.index };
    let Some(p) = player.0.as_ref() else {
        return;
    };
    // Под зажатой ЛКМ стек уже снят из слота в «руку» (`inventory_drag`),
    // а `Activate` приходит раньше, чем `Update` вернёт его на место.
    // Поэтому слот считается непустым и в этом случае.
    let held_here = inv.drag.as_ref().is_some_and(|drag| drag.from == target);

    match inv.selected {
        // Повторный клик по выбранному слоту снимает выбор.
        Some(from) if from == target => inv.selected = None,
        // Новый источник: только непустой слот.
        _ if held_here || p.inventory.get(target).is_some() => inv.selected = Some(target),
        _ => {}
    }
}

/// Списывает `count` предметов из клетки сетки крафта. Опустевшая клетка
/// освобождается; если на неё указывал выбор в сетке — выбор снимается.
fn take_from_craft_cell(craft: &mut CraftingUi, index: usize, count: u32) {
    if let Some(cell) = craft.slots[index].as_mut() {
        cell.count = cell.count.saturating_sub(count);
        if cell.count == 0 {
            craft.slots[index] = None;
            if craft.selected == Some(index) {
                craft.selected = None;
            }
        }
    }
}

/// Кладёт один предмет из выбранного слота инвентаря в клетку сетки.
/// Общая логика клика и заполнения зажатием. `false` — класть нечем
/// или клетка не принимает этот предмет.
fn place_one_into_craft_cell(
    inv: &mut InventoryUi,
    craft: &mut CraftingUi,
    player: &mut PlayerRes,
    target: usize,
) -> bool {
    let Some(src) = inv.selected else {
        return false;
    };
    let Some(p) = player.0.as_mut() else {
        return false;
    };
    let Some(stack) = p.inventory.get(src) else {
        inv.selected = None;
        return false;
    };
    let kind = stack.kind;
    let can_place = match craft.slots[target] {
        None => true,
        Some(existing) => existing.kind == kind && existing.count < kind.max_stack(),
    };
    if !can_place {
        return false;
    }

    // Списываем одну единицу из слота инвентаря.
    let mut emptied = false;
    if let Some(cell) = p.inventory.area_mut(src.area).get_mut(src.index)
        && let Some(s) = cell
    {
        s.count -= 1;
        emptied = s.count == 0;
        if emptied {
            *cell = None;
        }
    }
    match &mut craft.slots[target] {
        Some(existing) => existing.count += 1,
        None => craft.slots[target] = Some(ItemStack { kind, count: 1 }),
    }
    if emptied {
        inv.selected = None;
    }
    true
}

/// Забирает один предмет из клетки сетки обратно в инвентарь.
/// `false` — клетка пуста или в инвентаре нет места.
fn take_one_from_craft_cell(craft: &mut CraftingUi, player: &mut PlayerRes, target: usize) -> bool {
    let Some(stack) = craft.slots[target] else {
        return false;
    };
    let Some(p) = player.0.as_mut() else {
        return false;
    };
    if !p.inventory.can_fit(stack.kind, 1) {
        return false;
    }
    p.inventory.add(stack.kind, 1);
    take_from_craft_cell(craft, target, 1);
    true
}

/// Активация клетки сетки крафта. ЛКМ — доложить один предмет из
/// выбранного слота инвентаря (или переложить один между клетками, или
/// выбрать клетку), ПКМ — забрать один предмет обратно в инвентарь,
/// Shift+ЛКМ — вернуть весь слот в инвентарь целиком. Удержание кнопки
/// при перемещении по клеткам обрабатывает `craft_grid_hold_fill`.
fn on_craft_slot_activated(
    activate: On<Activate>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut inv: ResMut<InventoryUi>,
    mut craft: ResMut<CraftingUi>,
    hold: Res<CraftHoldFill>,
    mut player: ResMut<PlayerRes>,
    slots: Query<&CraftSlot>,
) {
    let Ok(clicked) = slots.get(activate.entity) else {
        return;
    };
    let target = clicked.index;
    // Активация приходит на клик любой кнопкой мыши; средняя в сетке
    // крафта не используется.
    if mouse.just_released(MouseButton::Middle) {
        return;
    }
    // Слот, уже обработанный текущим зажатием (заполнение при перемещении
    // по сетке), не срабатывает второй раз на отпускание кнопки.
    if mouse.just_released(MouseButton::Left) && hold.filled_lmb == Some(target) {
        return;
    }
    if mouse.just_released(MouseButton::Right) && hold.filled_rmb == Some(target) {
        return;
    }
    let Some(p) = player.0.as_mut() else {
        return;
    };

    // ПКМ по клетке: забрать один предмет в инвентарь. Места нет — клетка
    // не тратится (предмет не должен исчезать «в никуда»).
    if mouse.just_released(MouseButton::Right) {
        take_one_from_craft_cell(&mut craft, &mut player, target);
        return;
    }

    // Shift+ЛКМ по клетке: вернуть весь слот в инвентарь. `add` доливает
    // в неполные стеки и занимает пустые слоты; не поместившейся остаток
    // остаётся в клетке сетки.
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        let Some(stack) = craft.slots[target] else {
            return;
        };
        let left = p.inventory.add(stack.kind, stack.count);
        take_from_craft_cell(&mut craft, target, stack.count - left);
        return;
    }

    // Если выбран источник в инвентаре — кладём в крафт-слот по одному
    // предмету за клик (клетка не принимает — клик просто ничего не делает).
    // Источник остаётся выбранным, пока стек не опустеет: так удобно
    // докладывать несколько единиц подряд.
    if inv.selected.is_some() {
        place_one_into_craft_cell(&mut inv, &mut craft, &mut player, target);
        return;
    }

    // Если выбран источник в крафте — переносим между ячейками по одному
    // предмету (кроме обмена разными видами: тут меняются стеки целиком).
    if let Some(src) = craft.selected {
        if src == target {
            craft.selected = None;
            return;
        }
        let source = craft.slots[src];
        let dest = craft.slots[target];
        match (source, dest) {
            (Some(s), None) => {
                craft.slots[target] = Some(ItemStack { kind: s.kind, count: 1 });
                if let Some(cell) = craft.slots[src].as_mut() {
                    cell.count -= 1;
                    if cell.count == 0 {
                        craft.slots[src] = None;
                        craft.selected = None;
                    }
                }
            }
            (Some(s), Some(d)) if s.kind == d.kind && d.count < d.kind.max_stack() => {
                if let Some(cell) = craft.slots[target].as_mut() {
                    cell.count += 1;
                }
                if let Some(cell) = craft.slots[src].as_mut() {
                    cell.count -= 1;
                    if cell.count == 0 {
                        craft.slots[src] = None;
                        craft.selected = None;
                    }
                }
            }
            (Some(s), Some(d)) if s.kind != d.kind => {
                // обмен разными предметами
                craft.slots[target] = Some(s);
                craft.slots[src] = Some(d);
                craft.selected = None;
            }
            _ => {}
        }
        return;
    }

    // Выбор источника в крафте.
    if craft.slots[target].is_some() {
        craft.selected = Some(target);
    }
}

/// Заполнение сетки крафта зажатыми кнопками: пока ЛКМ/ПКМ удерживается,
/// каждый новый слот, на который переведён курсор, получает один предмет
/// (ЛКМ — из выбранного слота инвентаря, ПКМ — отдаёт один предмет
/// обратно в инвентарь). Само нажатие ничего не кладёт: обычный клик
/// обрабатывает наблюдатель активации, иначе предмет задваивался бы.
fn craft_grid_hold_fill(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    mut inv: ResMut<InventoryUi>,
    mut craft: ResMut<CraftingUi>,
    mut hold: ResMut<CraftHoldFill>,
    mut player: ResMut<PlayerRes>,
    slots: Query<(&CraftSlot, &ComputedNode, &UiGlobalTransform)>,
) {
    // Кнопка отпущена: следующее нажатие начинается с чистого листа.
    if !mouse.pressed(MouseButton::Left) {
        hold.last_lmb = None;
    }
    if !mouse.pressed(MouseButton::Right) {
        hold.last_rmb = None;
    }
    if !inv.open || inv.drag.is_some() {
        return;
    }
    // Shift+ЛКМ по клетке — вернуть слот целиком (наблюдатель активации);
    // заполнение зажатием с Shift не смешиваем.
    if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let hovered = hovered_craft_slot(window, &slots, cursor);

    // Кадр нажатия только фиксирует стартовый слот: клик без движения
    // целиком остаётся за наблюдателем активации.
    if mouse.just_pressed(MouseButton::Left) {
        hold.last_lmb = hovered;
        hold.filled_lmb = None;
    } else if mouse.pressed(MouseButton::Left) && hovered != hold.last_lmb {
        hold.last_lmb = hovered;
        if let Some(target) = hovered {
            hold.filled_lmb = Some(target);
            place_one_into_craft_cell(&mut inv, &mut craft, &mut player, target);
        }
    }
    if mouse.just_pressed(MouseButton::Right) {
        hold.last_rmb = hovered;
        hold.filled_rmb = None;
    } else if mouse.pressed(MouseButton::Right) && hovered != hold.last_rmb {
        hold.last_rmb = hovered;
        if let Some(target) = hovered {
            hold.filled_rmb = Some(target);
            take_one_from_craft_cell(&mut craft, &mut player, target);
        }
    }
}

/// Клетка сетки крафта под курсором, если есть. Хит-тест общий с другими
/// проверками UI (`cursor_over_node`).
fn hovered_craft_slot(
    window: &Window,
    slots: &Query<(&CraftSlot, &ComputedNode, &UiGlobalTransform)>,
    cursor: Vec2,
) -> Option<usize> {
    slots
        .iter()
        .find(|(_, computed, transform)| cursor_over_node(window, computed, transform, cursor))
        .map(|(slot, _, _)| slot.index)
}

/// Клик по слоту результата: если сетка образует рецепт и результат
/// помещается в инвентарь, выдаёт предмет и списывает ингредиенты.
/// Shift+ЛКМ — скрафтить максимум доступного (с добором ингредиентов из
/// инвентаря), Shift+ПКМ — половину максимума с округлением вниз (но не
/// меньше одного крафта, если хоть один возможен).
fn on_result_activated(
    activate: On<Activate>,
    results: Query<(), With<CraftResultSlot>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    inv: Res<InventoryUi>,
    mut craft: ResMut<CraftingUi>,
    book: Res<CraftBookRes>,
    mut player: ResMut<PlayerRes>,
) {
    // `Activate` — глобальное событие UI: его шлёт любая кнопка (слоты
    // инвентаря и сетки, кнопки нижней панели). Крафт должен срабатывать
    // только от нажатия на слот результата при открытом окне, иначе
    // последний уложенный в сетку ингредиент «сам» создавал предмет.
    if !inv.open || !results.contains(activate.entity) {
        return;
    }
    let left_click = mouse.just_released(MouseButton::Left);
    let right_click = mouse.just_released(MouseButton::Right);
    if !left_click && !right_click {
        return;
    }
    let Some(p) = player.0.as_mut() else {
        return;
    };
    let Some(recipe) = book.0.matching_recipe(&craft.slots).cloned() else {
        return;
    };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if shift {
        let max = recipe.max_crafts(&craft.slots, &p.inventory);
        // SHIFT+ПКМ — половина максимума; при max = 1 всё равно один крафт,
        // иначе кнопка выглядела бы «сломанной» при единственном доступном.
        let crafts = if right_click {
            if max > 0 { (max / 2).max(1) } else { 0 }
        } else {
            max
        };
        for _ in 0..crafts {
            if !recipe.try_craft_one(&mut craft.slots, &mut p.inventory) {
                break;
            }
        }
        craft.selected = None;
        return;
    }

    // Обычный клик — ровно один крафт. ПКМ без Shift ничего не делает.
    if !left_click {
        return;
    }
    recipe.try_craft_one(&mut craft.slots, &mut p.inventory);
    craft.selected = None;
}

/// Сброс сетки крафта при закрытом окне инвентаря: компоненты возвращаются
/// в инвентарь (`add` доливает в неполные стеки и занимает пустые слоты),
/// а то, что не влезло, сбрасывается на землю рядом с игроком командой
/// симуляции — UI не пишет в мир напрямую. Перетаскивание стека сюда не
/// входит: хотбар работает и при закрытом окне, а бросок драга разбирает
/// `inventory_drag`.
fn reset_crafting_on_close(
    mut inv: ResMut<InventoryUi>,
    mut craft: ResMut<CraftingUi>,
    mut player: ResMut<PlayerRes>,
    mut queue: ResMut<CommandQueueRes>,
    state: Res<GameState>,
) {
    if inv.open {
        // Окно открыто: сетка живёт, перетаскивание активно.
        return;
    }
    // Окно закрыто: выбора больше нет.
    inv.selected = None;
    craft.selected = None;
    // Сетка пуста — делать нечего (обычный случай каждого кадра).
    if craft.slots.iter().all(|cell| cell.is_none()) {
        return;
    }

    let Some(p) = player.0.as_mut() else {
        // Игрока нет — сетку очищаем: заполнить её без игрока невозможно.
        craft.slots = Default::default();
        return;
    };
    let Some(world) = state.world.as_ref() else {
        return;
    };

    // Тайл игрока — центр сброса остатков; зажимаем в границы мира,
    // позиция существа могла остановиться ровно на краю.
    let size = world.size.tiles() as i32;
    let center = TilePos {
        x: (p.pos.x.floor() as i32).clamp(0, size - 1),
        y: (p.pos.y.floor() as i32).clamp(0, size - 1),
    };

    for cell in craft.slots.iter_mut() {
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

/// Перетаскивание стеков между слотами инвентаря и хотбара: зажать ЛКМ на
/// слоте и отпустить над целевым. Пока стек «в руке», за курсором следует
/// иконка-призрак; отпускание мимо слота возвращает стек на место, ПКМ
/// отменяет перетаскивание. В режиме игрока хотбар перетаскивается и без
/// открытого окна инвентаря. Клик без движения остаётся выбором источника
/// для сетки крафта (см. `on_slot_activated`).
fn inventory_drag(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    focus: Res<WindowFocusState>,
    state: Res<GameState>,
    mut inv: ResMut<InventoryUi>,
    mut player: ResMut<PlayerRes>,
    mut queue: ResMut<CommandQueueRes>,
    icons: Res<UiIcons>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    slots: Query<(&InvSlot, &ComputedNode, &UiGlobalTransform)>,
    panels: Query<(&ComputedNode, &UiGlobalTransform), With<InventoryPanel>>,
    creative_panels: Query<(&ComputedNode, &UiGlobalTransform), With<crate::ui::creative::CreativePanel>>,
    mut ghosts: Query<(Entity, &mut Node), With<DragGhost>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    // Окно потеряло фокус (Alt+Tab): отпускание кнопки могло не дойти до
    // игры, поэтому стек возвращается на место, а не висит «в руке».
    if !focus.focused {
        if inv.drag.is_some() {
            cancel_drag(&mut commands, &mut inv, &mut player);
        }
        return;
    }
    // Окно инвентаря открыто — его слоты и хотбар; иначе перетаскивание
    // доступно по видимому хотбару, в том числе при открытом творческом
    // меню (бросок стека на плитки удаляет его, как в Minecraft).
    let creative_open = state.active_menu == Some(MenuKind::Creative);
    let hotbar_only =
        !inv.open && state.mode == GameMode::Player && (state.active_menu.is_none() || creative_open);
    if !inv.open && !hotbar_only {
        // Режим сменился/открылось меню посреди перетаскивания — стек
        // возвращается в исходный слот, а не остаётся «в руке» навсегда.
        if inv.drag.is_some() {
            cancel_drag(&mut commands, &mut inv, &mut player);
        }
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        // Курсор ушёл за пределы окна: отпускание кнопки всё равно
        // нужно обработать, иначе стек останется «в руке».
        if inv.drag.is_some() {
            if mouse.just_released(MouseButton::Right) {
                cancel_drag(&mut commands, &mut inv, &mut player);
            } else if mouse.just_released(MouseButton::Left) {
                finish_drag(&mut commands, &mut inv, &mut player, None);
            }
        }
        return;
    };

    // Старт: ЛКМ нажата над непустым слотом инвентаря/хотбара — стек
    // снимается в «руку» и появляется призрак. Не выходим из системы:
    // если кнопку успели отпустить в том же кадре (быстрый клик между
    // кадрами), ниже сработает и завершение перетаскивания.
    if mouse.just_pressed(MouseButton::Left) && inv.drag.is_none() {
        let Some(hover) = hovered_slot(window, &slots, cursor) else {
            return;
        };
        let target = SlotRef { area: hover.area, index: hover.index };
        let Some(p) = player.0.as_mut() else {
            return;
        };
        let Some(stack) = p.inventory.get(target) else {
            return;
        };
        p.inventory.take(target);

        // Призрак — самостоятельный UI-узел поверх всего: окно инвентаря
        // может закрыться прямо во время перетаскивания, а хотбар вообще
        // живёт без него. `GlobalZIndex` держит иконку над панелями.
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
                // Призрак висит прямо под курсором: будучи обычным UI-узлом,
                // он перехватывал бы наведение и клики у слота под ним.
                Pickable::IGNORE,
                GlobalZIndex(i32::MAX),
                DragGhost,
            ))
            .id();
        inv.drag = Some(DragState { from: target, stack, ghost });
    }
    if inv.drag.is_none() {
        return;
    }
    // ПКМ во время перетаскивания: отмена, стек возвращается на место.
    if mouse.just_released(MouseButton::Right) {
        cancel_drag(&mut commands, &mut inv, &mut player);
        return;
    }
    // Призрак следует за курсором.
    for (_, mut node) in &mut ghosts {
        node.left = Val::Px(cursor.x - GHOST_HALF);
        node.top = Val::Px(cursor.y - GHOST_HALF);
    }
    // Отпускание ЛКМ: перенос в слот под курсором; над панелью творческого
    // меню — удаление стека; за пределами открытого окна — выброс в мир в
    // сторону курсора; иначе возврат на место.
    if mouse.just_released(MouseButton::Left) {
        let hovered = hovered_slot(window, &slots, cursor);
        let target = hovered.map(|s| SlotRef { area: s.area, index: s.index });
        let over_panel = panels
            .iter()
            .any(|(computed, transform)| cursor_over_node(window, computed, transform, cursor));
        let over_creative = creative_panels
            .iter()
            .any(|(computed, transform)| cursor_over_node(window, computed, transform, cursor));
        if let Some(t) = target {
            finish_drag(&mut commands, &mut inv, &mut player, Some(t));
        } else if creative_open && over_creative {
            delete_drag(&mut commands, &mut inv);
        } else if inv.open
            && !over_panel
            && let Some(tile) = cursor_world_tile(window, &cameras, state.world.as_ref())
        {
            throw_drag(&mut commands, &mut inv, &mut player, &mut queue, tile);
        } else {
            finish_drag(&mut commands, &mut inv, &mut player, None);
        }
    }
}

/// Удаляет перетащенный стек (отпускание над панелью творческого меню):
/// стек не возвращается в слот и исчезает, как в Minecraft.
fn delete_drag(commands: &mut Commands, inv: &mut InventoryUi) {
    let Some(drag) = inv.drag.take() else {
        return;
    };
    commands.entity(drag.ghost).despawn();
}

/// Бросает перетащенный стек в мир в сторону курсора: стек возвращается
/// в исходный слот, а команда `PlayerThrowStack` списывает его оттуда и
/// бросает (симуляция сама считает дальность и задержку подбора).
/// Бросить можно только предмет из misc; остальное возвращается на место.
fn throw_drag(
    commands: &mut Commands,
    inv: &mut InventoryUi,
    player: &mut PlayerRes,
    queue: &mut CommandQueueRes,
    target: TilePos,
) {
    let Some(drag) = inv.drag.take() else {
        return;
    };
    commands.entity(drag.ghost).despawn();
    let throw_slot = (drag.from.area == SlotArea::Misc).then_some(drag.from.index);
    if let Some(p) = player.0.as_mut() {
        p.inventory.put(drag.from, drag.stack);
    }
    if let Some(slot) = throw_slot {
        queue.0.push(Command::PlayerThrowStack {
            target,
            slot: slot as u8,
        });
    }
}

/// Слот инвентаря под курсором, если есть. Хит-тест общий с другими
/// проверками UI (`cursor_over_node`).
fn hovered_slot<'a>(
    window: &Window,
    slots: &'a Query<(&InvSlot, &ComputedNode, &UiGlobalTransform)>,
    cursor: Vec2,
) -> Option<&'a InvSlot> {
    slots
        .iter()
        .find(|(_, computed, transform)| cursor_over_node(window, computed, transform, cursor))
        .map(|(slot, _, _)| slot)
}

/// Завершает перетаскивание: стек возвращается в исходный слот, затем
/// переносится в целевой (если он указан и отличен от исходного) через
/// `Inventory::transfer`. Отпускание над исходным слотом — обычный клик:
/// выбор источника остаётся за наблюдателем активации.
fn finish_drag(
    commands: &mut Commands,
    inv: &mut InventoryUi,
    player: &mut PlayerRes,
    target: Option<SlotRef>,
) {
    let Some(drag) = inv.drag.take() else {
        return;
    };
    commands.entity(drag.ghost).despawn();
    let Some(p) = player.0.as_mut() else {
        return;
    };
    // Сначала возвращаем стек «в руку» в исходный слот: transfer берёт его оттуда. 
    // Если целевой слот несовместим — стек просто останется на месте.
    p.inventory.put(drag.from, drag.stack);
    if let Some(t) = target
        && t != drag.from
        && p.inventory.transfer(drag.from, t)
    {
        // Перенос завершён - выбор источника больше не нужен.
        inv.selected = None;
    }
}

/// Отменяет перетаскивание (ПКМ): стек возвращается в исходный слот, призрак удаляется.
fn cancel_drag(
    commands: &mut Commands,
    inv: &mut InventoryUi,
    player: &mut PlayerRes,
) {
    let Some(drag) = inv.drag.take() else {
        return;
    };
    commands.entity(drag.ghost).despawn();
    if let Some(p) = player.0.as_mut() {
        p.inventory.put(drag.from, drag.stack);
    }
}

/// Обновляет иконки, счётчики и рамки слотов по текущему инвентарю игрока.
/// Пишет только реальные изменения, чтобы не дёргать переразметку.
fn refresh_slots(
    mut commands: Commands,
    inv: Res<InventoryUi>,
    player: Res<PlayerRes>,
    icons: Res<UiIcons>,
    mut slots: Query<(Entity, &InvSlot, &mut BorderColor, &Children), With<InventoryWindowSlot>>,
    mut images: Query<&mut ImageNode>,
    mut texts: Query<&mut Text>,
) {
    if !inv.open {
        return;
    }
    let Some(p) = player.0.as_ref() else {
        return;
    };

    for (_, slot, mut border, children) in &mut slots {
        let stack = p
            .inventory
            .area(slot.area)
            .get(slot.index)
            .copied()
            .flatten();

        // Рамка: выбранный слот-перенос подсвечивается активным цветом.
        let want_border = if inv.selected == Some(SlotRef { area: slot.area, index: slot.index }) {
            style::surface_active()
        } else {
            style::border()
        };
        let want_border = BorderColor::all(want_border);
        if *border != want_border {
            *border = want_border;
        }

        for child in children.iter() {
            // Иконка предмета — дочерний узел слота (не сам слот):
            // обновляем по ссылке на ребёнка через отдельный запрос.
            // Пустой слот брони показывает текстуру-заглушку специализации.
            if let Ok(mut image) = images.get_mut(child) {
                let want_icon = match stack {
                    Some(s) => icons.item(s.kind).clone(),
                    None => match slot.area {
                        SlotArea::Armor => icons
                            .empty_armor_slot(ARMOR_LAYOUT[slot.index])
                            .cloned()
                            .unwrap_or_default(),
                        SlotArea::Trash => icons.trash_can().clone(),
                        _ => ImageNode::default().image,
                    },
                };
                if image.image != want_icon {
                    image.image = want_icon;
                    // Размер иконки мог измениться — вписываем заново.
                    commands.entity(child).remove::<FitDone>();
                }
            }
            // Текстовые дети: счётчик количества (правый нижний угол).
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
}

/// Меню крафта 3x3 (как в Minecraft): сетка 3x3 и слот результата.
fn spawn_crafting(panel: &mut ChildSpawnerCommands, font: &UiFont) {
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
                    // Сетка 3x3.
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
                                    r.spawn((
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
                                        CraftSlot { index: i },
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
                                            SlotCount,
                                        ));
                                    });
                                }
                            });
                        }
                    });
                    // Слот результата: показывает подобранный рецепт, клик выдаёт предмет.
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
                        CraftResultSlot,
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
                        ));
                    });
                });
        });
}

fn refresh_crafting(
    mut commands: Commands,
    craft: Res<CraftingUi>,
    book: Res<CraftBookRes>,
    mut slots: Query<(&CraftSlot, &mut BorderColor, &Children), Without<CraftResultSlot>>,
    mut results: Query<(&mut BorderColor, &Children), With<CraftResultSlot>>,
    mut images: Query<&mut ImageNode>,
    mut texts: Query<&mut Text>,
    icons: Res<UiIcons>,
) {
    let result = book.0.matching_recipe(&craft.slots).map(|recipe| recipe.result);

    for (slot, mut border, children) in &mut slots {
        let stack = craft.slots[slot.index];
        let selected = craft.selected == Some(slot.index);
        let want_border = if selected {
            style::surface_active()
        } else {
            style::border()
        };
        *border = BorderColor::all(want_border);
        for child in children.iter() {
            if let Ok(mut image) = images.get_mut(child) {
                let want_icon = match stack {
                    Some(s) => icons.item(s.kind).clone(),
                    None => ImageNode::default().image,
                };
                if image.image != want_icon {
                    image.image = want_icon;
                    commands.entity(child).remove::<FitDone>();
                }
            }
            if let Ok(mut t) = texts.get_mut(child) {
                let want_text = stack
                    .map(|s| if s.count > 1 { s.count.to_string() } else { String::new() })
                    .unwrap_or_default();
                if t.as_str() != want_text {
                    **t = want_text;
                }
            }
        }
    }

    // Слот результата: подсвечивается, когда сетка образует рецепт.
    for (mut border, children) in &mut results {
        *border = BorderColor::all(if result.is_some() {
            style::surface_active()
        } else {
            style::border()
        });
        for child in children.iter() {
            if let Ok(mut image) = images.get_mut(child) {
                let want_icon = match result {
                    Some(s) => icons.item(s.kind).clone(),
                    None => ImageNode::default().image,
                };
                if image.image != want_icon {
                    image.image = want_icon;
                    commands.entity(child).remove::<FitDone>();
                }
            }
            if let Ok(mut t) = texts.get_mut(child) {
                let want_text = result
                    .map(|s| if s.count > 1 { s.count.to_string() } else { String::new() })
                    .unwrap_or_default();
                if t.as_str() != want_text {
                    **t = want_text;
                }
            }
        }
    }
}

/// Число слотов секции (для разумного существа — GAME_DESIGN 3.4):
/// константы иллюстрируют раскладку, реальная проверка — на данных.
const _: () = {
    assert!(MISC_SLOTS == 8, "misc слотов должно быть 8");
    assert!(ARMOR_SLOTS == 4, "слотов брони должно быть 4");
    assert!(CLOTHES_SLOTS == 4, "слотов одежды должно быть 4");
    assert!(BAUBLE_SLOTS == 4, "слотов косметики должно быть 4");
};
