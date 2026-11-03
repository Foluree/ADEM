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
//!
//! Состав модуля:
//! - `layout` — построение окна (секции, мусорка, сетка крафта) и обновление слотов;
//! - `craft_grid` — логика сетки крафта: клики, заполнение зажатием, крафт;
//! - `drag` — перетаскивание стеков между слотами и бросок в мир.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::TextInput;

use crate::app::settings::{ControlAction, Settings};
use crate::app::state::GameState;
use crate::simulation::crafts::{CraftBook, GRID_SLOTS};
use crate::simulation::items::{SlotArea, SlotRef, ARMOR_SLOTS, BAUBLE_SLOTS, CLOTHES_SLOTS, MISC_SLOTS};
use crate::simulation::ItemStack;
use crate::ui::UiFont;
use crate::ui::pause_menu::BindingCapture;
use crate::ui::text_input_focused;

mod craft_grid;
mod drag;
mod layout;

use craft_grid::{CraftHoldFill, on_craft_slot_activated, on_result_activated, on_slot_activated, reset_crafting_on_close, craft_grid_hold_fill};
use drag::inventory_drag;
use layout::{refresh_crafting, refresh_slots, spawn_inventory};

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
    pub(crate) from: SlotRef,
    pub(crate) stack: ItemStack,
    pub(crate) ghost: Entity,
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

/// Тоггл окна по бинду инвентаря (по умолчанию E). Пока фокус в текстовом
/// поле, идёт захват бинда или открыто модальное меню — не срабатывает.
/// Окно доступно, только если существо игрока существует (и в режиме
/// игрока, и «припаркованное» в наблюдателе).
fn toggle_inventory(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    mut inv: ResMut<InventoryUi>,
    state: Res<GameState>,
    settings: Res<Settings>,
    capture: Res<BindingCapture>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
    font: Res<UiFont>,
    roots: Query<Entity, With<InventoryRoot>>,
    mut station: ResMut<crate::ui::stations::StationUi>,
    station_roots: Query<Entity, With<crate::ui::stations::StationRoot>>,
) {
    if text_input_focused(&focus, &text_inputs) || capture.0.is_some() || state.active_menu.is_some() {
        return;
    }
    if !keys.just_pressed(settings.controls.key(ControlAction::Inventory))
        || state.controlled_mind().is_none()
    {
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

/// Число слотов секции (для разумного существа — GAME_DESIGN 3.4):
/// константы иллюстрируют раскладку, реальная проверка — на данных.
const _: () = {
    assert!(MISC_SLOTS == 8, "misc слотов должно быть 8");
    assert!(ARMOR_SLOTS == 4, "слотов брони должно быть 4");
    assert!(CLOTHES_SLOTS == 4, "слотов одежды должно быть 4");
    assert!(BAUBLE_SLOTS == 4, "слотов косметики должно быть 4");
};
