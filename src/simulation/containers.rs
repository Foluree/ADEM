//! Контейнеры рабочих станций и складов: слоты предметов и рабочий цикл печей.
//!
//! Контейнер существует у тайла станции (объект `ObjectKind::is_station`)
//! и хранится в разреженном слое чанка `Chunk.containers`. Укладка слотов
//! у каждой станции своя — индексы фиксированы константами, чтобы UI,
//! симуляция и сохранения читали одни и те же адреса.
//!
//! Рабочий цикл печей (печь, плавильня, костёр) — событийный, без опроса
//! каждый тик: контейнер хранит **абсолютные** тики готовки и прогорания
//! топлива (`cook_finish_tick`, `burn_out_tick`), в планировщике стоит
//! одна задача `TaskKind::StationTick` на ближайшее из событий. Задача
//! перевыставляется после каждого события, пока цикл продолжается.
//! Стоимость — O(1) на событие; выключенные печи задач не имеют.

use super::crafts::{CraftBook, CraftRecipe, CrafterKind};
use super::dirty::DirtyTiles;
use super::items::{ItemKind, ItemStack, MAX_STACK};
use super::objects::{BuildingKind, ObjectKind};
use super::scheduler::{Scheduler, TaskKind};
use super::world::{ChunkPos, TilePos, World, split_tile, tile_pos};

/// Слот костра: что готовится.
pub const CAMPFIRE_INPUT: usize = 0;
/// Слот костра: результат.
pub const CAMPFIRE_OUTPUT: usize = 1;

/// Слот печи: что готовится.
pub const FURNACE_INPUT: usize = 0;
/// Слот печи: топливо.
pub const FURNACE_FUEL: usize = 1;
/// Слот печи: результат.
pub const FURNACE_OUTPUT: usize = 2;
/// Слоты печи: отходы (зола от топлива).
pub const FURNACE_WASTE: [usize; 2] = [3, 4];

/// Слоты плавильной печи: два входа, два топлива.
pub const BLAST_INPUT: [usize; 2] = [0, 1];
pub const BLAST_FUEL: [usize; 2] = [2, 3];
/// Слот плавильной печи: результат.
pub const BLAST_OUTPUT: usize = 4;
/// Слоты плавильной печи: отходы.
pub const BLAST_WASTE: [usize; 2] = [5, 6];

/// Число слотов хранения у бочки.
pub const BARREL_SLOTS: usize = 24;
/// Число слотов хранения у склада.
pub const STORAGE_SLOTS: usize = 16;

/// Время готовки в печи, тиков (решение автора).
pub const FURNACE_COOK_TICKS: u64 = 100;
/// Время плавки в плавильной печи, тиков.
pub const BLAST_COOK_TICKS: u64 = 200;
/// Время готовки на костре, тиков: примитивный способ — дольше печи.
pub const CAMPFIRE_COOK_TICKS: u64 = 200;

/// Адрес слота «misc игрока» в командах переноса (см. `Command::StationMoveItem`).
pub const SLOT_AREA_PLAYER_MISC: u8 = 0xFF;

/// Шанс, что прогоревшее топливо оставит золу в отходах.
const ASH_CHANCE: f32 = 0.4;
/// Шанс золы у угля в плавильной печи: горит чище.
const COAL_ASH_CHANCE: f32 = 0.25;

/// Раскладка слотов станции по видам (статические срезы — без аллокаций).
struct StationLayout {
    inputs: &'static [usize],
    fuels: &'static [usize],
    output: usize,
    waste: &'static [usize],
}

/// Раскладка слотов по виду станции.
fn layout(kind: ObjectKind) -> Option<StationLayout> {
    let ObjectKind::Building(kind) = kind else {
        return None;
    };
    match kind {
        BuildingKind::Campfire => Some(StationLayout {
            inputs: &[CAMPFIRE_INPUT],
            fuels: &[],
            output: CAMPFIRE_OUTPUT,
            waste: &[],
        }),
        BuildingKind::Furnace => Some(StationLayout {
            inputs: &[FURNACE_INPUT],
            fuels: &[FURNACE_FUEL],
            output: FURNACE_OUTPUT,
            waste: &FURNACE_WASTE,
        }),
        BuildingKind::BlastFurnace => Some(StationLayout {
            inputs: &BLAST_INPUT,
            fuels: &BLAST_FUEL,
            output: BLAST_OUTPUT,
            waste: &BLAST_WASTE,
        }),
        _ => None,
    }
}

/// Контейнер станции: слоты предметов + абсолютные сроки рабочего цикла
/// (`None` — цикл не идёт). У склада и бочки сроки не используются.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Container {
    /// Слоты по раскладке вида станции (см. константы выше).
    pub slots: Vec<Option<ItemStack>>,
    /// Тик завершения текущей готовки (пока вход готовится).
    pub cook_finish_tick: Option<u64>,
    /// Тик прогорания текущей порции топлива (`None` — не горит; у костра
    /// всегда `None`: горит вечно).
    pub burn_out_tick: Option<u64>,
    /// Вид топлива, горящего сейчас (для шанса золы при прогорании).
    pub burning_fuel: Option<ItemKind>,
}

impl Container {
    /// Пустой контейнер станции заданного вида.
    pub fn new(kind: ObjectKind) -> Self {
        Self {
            slots: vec![None; capacity(kind).unwrap_or(0)],
            cook_finish_tick: None,
            burn_out_tick: None,
            burning_fuel: None,
        }
    }
    /// Стек слота.
    pub fn get(&self, slot: usize) -> Option<ItemStack> {
        self.slots.get(slot).copied().flatten()
    }
    /// Мутабельный слот (внутренние операции).
    fn slot_mut(&mut self, slot: usize) -> Option<&mut Option<ItemStack>> {
        self.slots.get_mut(slot)
    }
    /// Снимает весь стак со слота.
    pub fn take(&mut self, slot: usize) -> Option<ItemStack> {
        self.slot_mut(slot).and_then(Option::take)
    }
    /// Кладёт стек в пустой слот (правило размещения проверяет `can_put_into`).
    /// Возвращает `false`, если слот занят — кладки не было.
    pub fn put(&mut self, slot: usize, stack: ItemStack) -> bool {
        match self.slot_mut(slot) {
            Some(cell @ None) => {
                *cell = Some(stack);
                true
            }
            _ => false,
        }
    }
    /// Заменяет содержимое слота (долив стека при переносе).
    pub fn replace(&mut self, slot: usize, stack: ItemStack) -> bool {
        match self.slot_mut(slot) {
            Some(cell) => {
                *cell = Some(stack);
                true
            }
            None => false,
        }
    }
    /// Горит ли станция сейчас (для текстуры front_on в рендере).
    pub fn is_lit(&self) -> bool {
        self.burn_out_tick.is_some()
    }
}

/// Вместимость контейнера по виду станции. `None` — виду контейнер не
/// положен (верстак и кузнечный стол работают с временной UI-сеткой).
pub fn capacity(kind: ObjectKind) -> Option<usize> {
    let ObjectKind::Building(kind) = kind else {
        return None;
    };
    match kind {
        BuildingKind::Campfire => Some(2),
        BuildingKind::Furnace => Some(5),
        BuildingKind::BlastFurnace => Some(7),
        BuildingKind::Barrel => Some(BARREL_SLOTS),
        BuildingKind::Storage => Some(STORAGE_SLOTS),
        // Верстак и кузнечный стол контейнера не имеют: их сетка
        // временная и живёт в UI.
        BuildingKind::CraftingTable | BuildingKind::SmithingTable => None,
    }
}

/// Снимок стеков входных слотов станции (для отслеживания их изменения).
pub fn input_snapshot(kind: ObjectKind, container: &Container) -> Vec<Option<ItemStack>> {
    match layout(kind) {
        Some(l) => l.inputs.iter().map(|&i| container.get(i)).collect(),
        None => Vec::new(),
    }
}

/// Сбрасывает готовку, если входные слоты изменились с момента снимка
/// (предмет забрали или заменили): прогресс жарки без исходного предмета
/// не имеет смысла. Возвращает `true`, если готовка была сброшена.
pub fn reset_cooking_if_inputs_changed(
    kind: ObjectKind,
    container: &mut Container,
    before: &[Option<ItemStack>],
) -> bool {
    if container.cook_finish_tick.is_none() {
        return false;
    }
    if input_snapshot(kind, container) == before {
        return false;
    }
    container.cook_finish_tick = None;
    true
}

/// Слот выхода станции (результат готовки), если станция с готовкой.
/// Выход — «кнопка»: кладётся только забирать.
pub fn output_slot(kind: ObjectKind) -> Option<usize> {
    layout(kind).map(|l| l.output)
}

/// Подсказка роли пустого слота станции для тултипа. `None` — подсказки
/// нет (слоты misc игрока и хранилищ подсказки не требуют).
pub fn slot_hint(kind: ObjectKind, slot: usize) -> Option<&'static str> {
    let ObjectKind::Building(kind) = kind else {
        return None;
    };
    match kind {
        BuildingKind::Furnace => match slot {
            FURNACE_INPUT => Some("Вход: что готовим"),
            FURNACE_FUEL => Some("Топливо"),
            FURNACE_OUTPUT => Some("Результат — забирать"),
            s if FURNACE_WASTE.contains(&s) => Some("Зола"),
            _ => None,
        },
        BuildingKind::BlastFurnace => match slot {
            s if BLAST_INPUT.contains(&s) => Some("Вход: руда"),
            s if BLAST_FUEL.contains(&s) => Some("Топливо: только уголь"),
            BLAST_OUTPUT => Some("Результат — забирать"),
            s if BLAST_WASTE.contains(&s) => Some("Зола"),
            _ => None,
        },
        BuildingKind::Campfire => match slot {
            CAMPFIRE_INPUT => Some("Что готовим"),
            CAMPFIRE_OUTPUT => Some("Результат — забирать"),
            _ => None,
        },
        _ => None,
    }
}

/// Является ли слот выходом станции (для UI: клик забирает, drag выключен).
pub fn is_output_slot(kind: ObjectKind, slot: usize) -> bool {
    output_slot(kind) == Some(slot)
}

/// Кулинарный крафтер станции в `CraftBook`. `None` — станция без готовки.
pub fn cooking_crafter(kind: ObjectKind) -> Option<CrafterKind> {
    let ObjectKind::Building(kind) = kind else {
        return None;
    };
    CrafterKind::from_building(kind).filter(|crafter| crafter.cooks())
}

/// Время готовки станции, тиков. Публично: UI читает для шкалы прогресса.
pub fn cook_time_ticks(kind: ObjectKind) -> u64 {
    let ObjectKind::Building(kind) = kind else {
        return 0;
    };
    match kind {
        BuildingKind::Campfire => CAMPFIRE_COOK_TICKS,
        BuildingKind::Furnace => FURNACE_COOK_TICKS,
        BuildingKind::BlastFurnace => BLAST_COOK_TICKS,
        _ => 0,
    }
}

/// Можно ли положить предмет в слот контейнера станции (правила размещения
/// для UI и команд переноса). Выход и отходы — только забирать; топливо —
/// только горючее (плавильня — только уголь); вход — что угодно.
pub fn can_put_into(kind: ObjectKind, slot: usize, item: ItemKind) -> bool {
    let ObjectKind::Building(kind) = kind else {
        return false;
    };
    match kind {
        BuildingKind::Campfire => match slot {
            CAMPFIRE_OUTPUT => false,
            CAMPFIRE_INPUT => true,
            _ => false,
        },
        BuildingKind::Furnace => match slot {
            s if s == FURNACE_OUTPUT || FURNACE_WASTE.contains(&s) => false,
            FURNACE_FUEL => item.burn_time() > 0,
            FURNACE_INPUT => true,
            _ => false,
        },
        BuildingKind::BlastFurnace => match slot {
            s if s == BLAST_OUTPUT || BLAST_WASTE.contains(&s) => false,
            i if BLAST_FUEL.contains(&i) => item == ItemKind::Coal,
            i if BLAST_INPUT.contains(&i) => true,
            _ => false,
        },
        // Хранилища: любые предметы в любые слоты.
        BuildingKind::Barrel | BuildingKind::Storage => {
            slot < capacity(ObjectKind::Building(kind)).unwrap_or(0)
        }
        _ => false,
    }
}

/// Влезет ли результат рецепта в слот выхода контейнера.
fn result_fits(container: &Container, output: usize, recipe: &CraftRecipe) -> bool {
    match container.get(output) {
        None => true,
        Some(out) => {
            out.kind == recipe.result.kind && out.count + recipe.result.count <= MAX_STACK
        }
    }
}

/// Ставит задачу станции на ближайшее событие (готовка или прогорание),
/// если контейнер активен. Единая точка перевыставления задачи.
fn schedule_next(sched: &mut Scheduler, pos: TilePos, container: &Container) {
    let next = match (container.cook_finish_tick, container.burn_out_tick) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    if let Some(at) = next {
        let (chunk, local) = split_tile(pos);
        sched.schedule_at(at, TaskKind::StationTick { chunk, local_index: local });
    }
}

/// Поджигает станцию, если есть чем: подходящий вход в слоте входа, место
/// под результат и топливо (у костра топливо не нужно). Списывает одну
/// единицу топлива и выставляет сроки. Возвращает `true` при поджиге.
fn try_ignite(world: &mut World, book: &CraftBook, pos: TilePos, kind: ObjectKind, tick: u64) -> bool {
    let Some(crafter) = cooking_crafter(kind) else {
        return false;
    };
    let Some(l) = layout(kind) else {
        return false;
    };
    let cook_time = cook_time_ticks(kind);

    let Some(container) = container_at_mut(world, pos) else {
        return false;
    };
    // Вход: первый слот входа с достаточным запасом ингредиента рецепта
    // и местом под результат.
    let mut input_kind = None;
    for &input_slot in l.inputs {
        if let Some(stack) = container.get(input_slot)
            && let Some(recipe) = book.cooking_recipe(crafter, stack.kind)
            && let Some((_, need)) = recipe.cooking_input()
            && need <= stack.count
            && result_fits(container, l.output, recipe)
        {
            input_kind = Some(stack.kind);
            break;
        }
    }
    let Some(_input_kind) = input_kind else {
        return false;
    };

    // Топливо: у костра его нет (горит вечно), у печей — первое доступное
    // горючее; правила размещения те же, что у `can_put_into` (плавильня
    // жжёт только уголь даже при чужом топливе, попавшем в слот мимо UI).
    let mut burn_out = None;
    let mut burning_fuel = None;
    if kind != ObjectKind::Building(BuildingKind::Campfire) {
        let mut found = None;
        for &slot in l.fuels {
            if let Some(stack) = container.get(slot) && can_put_into(kind, slot, stack.kind) {
                found = Some((slot, stack));
                break;
            }
        }
        let Some((fuel_slot, stack)) = found else {
            return false;
        };
        let burn = stack.kind.burn_time() as u64;
        let left = stack.count - 1;
        let fuel_stack = (left > 0).then_some(ItemStack { kind: stack.kind, count: left });
        *container.slot_mut(fuel_slot).unwrap() = fuel_stack;
        burn_out = Some(tick + burn);
        burning_fuel = Some(stack.kind);
    }

    container.cook_finish_tick = Some(tick + cook_time);
    container.burn_out_tick = burn_out;
    container.burning_fuel = burning_fuel;
    true
}

/// Детерминированный бросок золы от (seed, тик, координаты): старшие биты
/// хэша-смеси (как `event_rng` в vegetation, но без создания RNG).
fn ash_roll(seed: u64, tick: u64, chunk: ChunkPos, local: u8) -> f32 {
    let mut h = seed
        ^ tick.wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ 0x41C6_CE57_88B1_4C01
        ^ (chunk.x as u64).wrapping_mul(0x94D0_49BB_1331_11EB)
        ^ (chunk.y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (local as u64).wrapping_mul(0x2545_F491_4F6C_DD1D);
    h = h.wrapping_mul(0xD6E8_FEB8_6659_7B93);
    h ^= h >> 32;
    (h >> 40) as f32 / (1u32 << 24) as f32
}

/// Событие станции: завершить готовку или погасить топливо, затем
/// перевыставить задачу, пока цикл продолжается. Вызывается из app-слоя
/// по исполненной задаче `TaskKind::StationTick`.
pub fn apply_station_tick(
    world: &mut World,
    sched: &mut Scheduler,
    tick: u64,
    dirty: &mut DirtyTiles,
    book: &CraftBook,
    chunk: ChunkPos,
    local: u8,
) {
    let pos = tile_pos(chunk, local);
    let Some(kind) = world.object_at(pos).map(|o| o.kind) else {
        // Станцию снесли — контейнер обязана была удалить `set_object`.
        return;
    };
    if cooking_crafter(kind).is_none() {
        return;
    }

    // Готовка завершается раньше/вместе с топливом: выкладываем результат.
    if container_at(world, pos).is_some_and(|c| c.cook_finish_tick == Some(tick)) {
        let l = layout(kind).unwrap();
        let crafter = cooking_crafter(kind).unwrap();
        let container = container_at_mut(world, pos).unwrap();
        container.cook_finish_tick = None;
        if let Some(&input_slot) = l.inputs.first()
            && let Some(stack) = container.get(input_slot)
            && let Some(recipe) = book.cooking_recipe(crafter, stack.kind)
            && let Some((_, need)) = recipe.cooking_input() {
                // Место под результат и запас ингредиента проверялись
                // поджигом; повторная проверка — на случай ручного
                // вмешательства через UI.
                if result_fits(container, l.output, recipe) && stack.count >= need {
                    let left = stack.count - need;
                    let next = (left > 0).then_some(ItemStack { kind: stack.kind, count: left });
                    *container.slot_mut(input_slot).unwrap() = next;
                    let out_stack = match container.get(l.output) {
                        None => recipe.result,
                        Some(mut out) => {
                            out.count += recipe.result.count;
                            out
                        }
                    };
                    *container.slot_mut(l.output).unwrap() = Some(out_stack);
                }
            }
        dirty.decor(pos);
    }

    // Топливо прогорело: зола и очистка состояния горения.
    if container_at(world, pos).is_some_and(|c| c.burn_out_tick == Some(tick)) {
        let container = container_at_mut(world, pos).unwrap();
        container.burn_out_tick = None;
        let fuel = container.burning_fuel.take();
        dirty.decor(pos);

        let chance = match fuel {
            Some(ItemKind::Coal) => COAL_ASH_CHANCE,
            _ => ASH_CHANCE,
        };
        if ash_roll(world.seed, tick, chunk, local) < chance {
            let l = layout(kind).unwrap();
            let container = container_at_mut(world, pos).unwrap();
            for &slot in l.waste {
                if container.get(slot).is_none() {
                    container.put(slot, ItemStack { kind: ItemKind::DustPitAsh, count: 1 });
                    dirty.decor(pos);
                    break;
                }
            }
        }
    }

    // Перезапуск цикла: готовка кончилась — пробуем поджечь следующую порцию.
    if container_at(world, pos).is_some_and(|c| c.cook_finish_tick.is_none()) {
        try_ignite(world, book, pos, kind, tick);
    }
    let Some(container) = container_at(world, pos) else {
        return;
    };
    if container.cook_finish_tick.is_some() || container.burn_out_tick.is_some() {
        let snapshot = container.clone();
        schedule_next(sched, pos, &snapshot);
        dirty.decor(pos);
    }
}

/// Контейнер станции, если есть.
pub fn container_at(world: &World, pos: TilePos) -> Option<&Container> {
    let (chunk, idx) = split_tile(pos);
    world.chunk(chunk).containers.get(&idx)
}

/// Мутабельный контейнер станции, если есть.
pub fn container_at_mut(world: &mut World, pos: TilePos) -> Option<&mut Container> {
    let (chunk, idx) = split_tile(pos);
    world.chunk_mut(chunk).containers.get_mut(&idx)
}

/// Включает рабочий цикл станции: поджиг после ручного изменения слотов
/// (UI положил вход/топливо, команда переноса). Вызывается после любой
/// операции со слотами активируемой станции.
pub fn wake_station(
    world: &mut World,
    sched: &mut Scheduler,
    tick: u64,
    book: &CraftBook,
    pos: TilePos,
) {
    let Some(kind) = world.object_at(pos).map(|o| o.kind) else {
        return;
    };
    if cooking_crafter(kind).is_none() {
        return;
    }
    if container_at(world, pos).is_some_and(|c| c.cook_finish_tick.is_some()) {
        return; // цикл уже идёт, задача перевыставится сама
    }
    if try_ignite(world, book, pos, kind, tick) {
        let snapshot = container_at(world, pos).unwrap().clone();
        schedule_next(sched, pos, &snapshot);
    }
}
