//! Рецепты крафта из `data/crafts/**/*.json` и сопоставление с сеткой 3×3.
//!
//! У каждого рецепта есть `parent` — вид (или виды) крафтера, которому
//! рецепт доступен. Значения: `manual`, `crafting_table`, `smithing_table`,
//! `campfire`, `furnace`, `blast_furnace`. Схема рецепта зависит от вида
//! крафтера:
//! - сеточные (`manual`, `crafting_table`, `smithing_table`) — `shaped`
//!   (по умолчанию) с картой символов `ingredients` и сеткой `grid` или
//!   `shapeless` с массивом `ingredients`;
//! - кулинарные (`campfire`, `furnace`, `blast_furnace`) — один `input`,
//!   без `shapeless`/`grid`: поля, которые для готовки бессмысленны, не
//!   пишутся в данных.
//!
//! Категория рецепта больше не выводится из места файла: папка остаётся
//! организацией, а видимость определяет `parent`. Загрузчик проверяет
//! раскладку (папка должна входить в `parent`), но при расхождении пишет
//! предупреждение в debug-сборке и **не** блокирует загрузку.
//!
//! Рецепты хранятся по группам крафтеров: матчинг смотрит только свою
//! группу, а у готовки есть индекс `входной предмет → рецепт` (O(1)).
//! Верстак наследует группу `manual`; кузнечный стол — только свою.
//!
//! Формат `grid` использует ссылки (как в Minecraft): в `ingredients`
//! перечислены символы, в клетках сетки — символы или `null`. При загрузке
//! это один раз разворачивается в компактный `ShapedPattern` с `ItemKind`.
//!
//! Рецепт может требовать инструменты (кузнечный стол): поле `tools`
//! перечисляет предметы, которые должны лежать в слотах инструментов.
//! Инструменты не расходуются — это требование «чем работаешь».

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::items::{ITEM_KIND_COUNT, Inventory, ItemKind, ItemStack};
use super::objects::BuildingKind;

/// Имя корневой группы (и корневой папки данных): ручной крафт в инвентаре.
pub const MANUAL_CATEGORY: &str = "manual";

/// Число ячеек сетки крафта.
pub const GRID_SLOTS: usize = 9;

/// Предохранитель для массового крафта: рецепт, который не уменьшает
/// суммарное количество предметов (например, 1 предмет → 1 предмет),
/// иначе крутил бы цикл подсчёта бесконечно. На реальных данных недостижим.
const MAX_BULK_CRAFTS: u32 = 10_000;

/// Вид крафтера — группа рецептов и источник правил их схемы.
///
/// Порядок вариантов — порядок групп в `CraftBook` (не сохраняется в
/// сейвах, поэтому стабильность дискриминантов не требуется).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum CrafterKind {
    /// Сетка 3×3 в окне инвентаря.
    Manual,
    /// Верстак: сетка 3×3, наследует `manual`.
    CraftingTable,
    /// Кузнечный стол: сетка 3×3 + обязательные инструменты.
    SmithingTable,
    /// Костёр: готовая еда, топливо не нужно.
    Campfire,
    /// Печь: готовка на топливе.
    Furnace,
    /// Плавильня: руда → слитки, топливо — только уголь.
    BlastFurnace,
}

/// Схема рецептов крафтера.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecipeShape {
    /// Сетка 3×3: shaped или shapeless.
    Grid,
    /// Один вход → результат, без сетки.
    Cooking,
}

const MANUAL_CHAIN: &[CrafterKind] = &[CrafterKind::Manual];
const CRAFTING_TABLE_CHAIN: &[CrafterKind] =
    &[CrafterKind::Manual, CrafterKind::CraftingTable];
const SMITHING_TABLE_CHAIN: &[CrafterKind] = &[CrafterKind::SmithingTable];
const CAMPFIRE_CHAIN: &[CrafterKind] = &[CrafterKind::Campfire];
const FURNACE_CHAIN: &[CrafterKind] = &[CrafterKind::Furnace];
const BLAST_FURNACE_CHAIN: &[CrafterKind] = &[CrafterKind::BlastFurnace];

impl CrafterKind {
    /// Все виды крафтеров в порядке групп.
    pub const ALL: [CrafterKind; 6] = [
        Self::Manual,
        Self::CraftingTable,
        Self::SmithingTable,
        Self::Campfire,
        Self::Furnace,
        Self::BlastFurnace,
    ];

    /// Число видов (размер массива групп).
    pub const COUNT: usize = Self::ALL.len();

    /// Стабильный строковый id из JSON.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::CraftingTable => "crafting_table",
            Self::SmithingTable => "smithing_table",
            Self::Campfire => "campfire",
            Self::Furnace => "furnace",
            Self::BlastFurnace => "blast_furnace",
        }
    }

    /// Вид крафтера по строковому id из JSON.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }

    /// Схема рецептов, которую принимает крафтер.
    pub const fn recipe_shape(self) -> RecipeShape {
        match self {
            Self::Manual | Self::CraftingTable | Self::SmithingTable => RecipeShape::Grid,
            Self::Campfire | Self::Furnace | Self::BlastFurnace => RecipeShape::Cooking,
        }
    }

    /// Готовит ли крафтер по рецептам с одним входом.
    pub const fn cooks(self) -> bool {
        matches!(self, Self::Campfire | Self::Furnace | Self::BlastFurnace)
    }

    /// Цепочка групп для матчинга: верстак видит свои рецепты и `manual`,
    /// остальные — только свои.
    pub const fn recipe_chain(self) -> &'static [CrafterKind] {
        match self {
            Self::Manual => MANUAL_CHAIN,
            Self::CraftingTable => CRAFTING_TABLE_CHAIN,
            Self::SmithingTable => SMITHING_TABLE_CHAIN,
            Self::Campfire => CAMPFIRE_CHAIN,
            Self::Furnace => FURNACE_CHAIN,
            Self::BlastFurnace => BLAST_FURNACE_CHAIN,
        }
    }

    /// Крафтер, соответствующий постройке. `None` — постройка без рецептов.
    pub fn from_building(kind: BuildingKind) -> Option<Self> {
        match kind {
            BuildingKind::Campfire => Some(Self::Campfire),
            BuildingKind::CraftingTable => Some(Self::CraftingTable),
            BuildingKind::Furnace => Some(Self::Furnace),
            BuildingKind::BlastFurnace => Some(Self::BlastFurnace),
            BuildingKind::SmithingTable => Some(Self::SmithingTable),
            BuildingKind::Barrel | BuildingKind::Storage => None,
        }
    }
}

/// Тело рецепта в уже разрешённом (оптимизированном) виде.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecipeBody {
    /// Форма сетки: обрезана по занятым клеткам.
    Shaped(ShapedPattern),
    /// Набор ингредиентов: виды + количества, слитые и отсортированные.
    Shapeless(Vec<(ItemKind, u32)>),
    /// Кулинарный вход: вид + количество.
    Cooking { input: (ItemKind, u32) },
}

/// Разобранный рецепт с уже разрешёнными видами предметов.
#[derive(Clone, Debug)]
pub struct CraftRecipe {
    /// Стабильный идентификатор рецепта (имя файла без расширения).
    pub id: String,
    /// Крафтеры, которым доступен рецепт (порядок из JSON, без дублей).
    pub parents: Vec<CrafterKind>,
    /// Тело рецепта: форма, набор ингредиентов или кулинарный вход.
    pub body: RecipeBody,
    /// Что получается.
    pub result: ItemStack,
    /// Требуемые инструменты (кузнечный стол): должны лежать в слотах
    /// инструментов, не расходуются. Пусто — инструменты не нужны.
    pub tools: Vec<ItemKind>,
    /// Стоимость одного крафта (слитая и отсортированная). Для готовки —
    /// вход. Считается один раз при загрузке: массовый крафт не аллоцирует.
    cost: Vec<(ItemKind, u32)>,
}

/// Обрезанная по занятым клеткам форма рецепта.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapedPattern {
    pub width: usize,
    pub height: usize,
    /// Клетки row-major, длина `width * height`. `None` — пустая клетка
    /// внутри формы (дырка, которая должна остаться пустой).
    pub cells: Vec<Option<ItemKind>>,
}

/// Набор рецептов с индексами по крафтерам. Загружается один раз при старте.
#[derive(Clone, Debug)]
pub struct CraftBook {
    /// Арена рецептов в порядке загрузки (детерминированный матчинг).
    pub recipes: Vec<CraftRecipe>,
    /// Индексы рецептов по группам крафтеров (в порядке загрузки).
    groups: [Vec<u32>; CrafterKind::COUNT],
    /// Индекс готовки: `cooking[крафтер][входной предмет] → индекс рецепта`.
    /// `u32::MAX` — рецепта нет. Первый загруженный рецепт побеждает.
    cooking: [[u32; ITEM_KIND_COUNT]; CrafterKind::COUNT],
}

/// Результат загрузки: книга, фатальные ошибки файлов и предупреждения.
#[derive(Debug)]
pub struct CraftBookLoad {
    pub book: CraftBook,
    /// Файлы, которые не загрузились (игра продолжает работать).
    pub errors: Vec<String>,
    /// Некритичные расхождения данных (папка vs `parent`). Логируются
    /// только в debug-сборке.
    pub warnings: Vec<String>,
}

impl Default for CraftBook {
    fn default() -> Self {
        Self {
            recipes: Vec::new(),
            groups: std::array::from_fn(|_| Vec::new()),
            cooking: [[u32::MAX; ITEM_KIND_COUNT]; CrafterKind::COUNT],
        }
    }
}

impl CraftBook {
    /// Читает все `*.json` из каталога и его подпапок. Видимость рецепта
    /// определяет `parent`; папка — только организация (и проверка
    /// раскладки с предупреждением). Файлы и подпапки обходятся в
    /// лексикографическом порядке, поэтому набор рецептов детерминирован.
    /// Ошибки отдельных файлов не роняют игру: они возвращаются списком.
    pub fn load_dir(dir: &str) -> CraftBookLoad {
        let mut load = CraftBookLoad {
            book: CraftBook::default(),
            errors: Vec::new(),
            warnings: Vec::new(),
        };
        let mut seen: HashSet<(CrafterKind, String)> = HashSet::new();
        load_recipes_recursive(
            Path::new(dir),
            Some(MANUAL_CATEGORY),
            &mut load,
            &mut seen,
        );
        load
    }

    /// Рецепт ручной сетки, подходящий под содержимое. Первый по порядку
    /// загрузки — стабильное поведение при совпадении форм.
    pub fn matching_recipe(
        &self,
        grid: &[Option<ItemStack>; GRID_SLOTS],
    ) -> Option<&CraftRecipe> {
        self.matching_recipe_for(CrafterKind::Manual.recipe_chain(), grid, &[])
    }

    /// Рецепт первой подходящей группы из `crafters` (порядок важен:
    /// наследование — `manual` раньше `crafting_table`). `held_tools` —
    /// виды предметов в слотах инструментов станции (кузнечный стол).
    pub fn matching_recipe_for(
        &self,
        crafters: &[CrafterKind],
        grid: &[Option<ItemStack>; GRID_SLOTS],
        held_tools: &[ItemKind],
    ) -> Option<&CraftRecipe> {
        // Пустая сетка не подходит ни одному сеточному рецепту. Проверка
        // до перебора групп: этот вызов идёт каждый кадр из UI.
        if grid.iter().all(Option::is_none) {
            return None;
        }
        for &crafter in crafters {
            for &index in &self.groups[crafter as usize] {
                let recipe = &self.recipes[index as usize];
                if recipe.is_grid()
                    && recipe.tools_satisfied(held_tools)
                    && recipe.matches(grid)
                {
                    return Some(recipe);
                }
            }
        }
        None
    }

    /// Кулинарный рецепт для входа — O(1) по индексу предмета.
    pub fn cooking_recipe(
        &self,
        crafter: CrafterKind,
        input: ItemKind,
    ) -> Option<&CraftRecipe> {
        let index = self.cooking[crafter as usize][input as usize];
        (index != u32::MAX).then(|| &self.recipes[index as usize])
    }

    /// Рецепты группы в порядке загрузки (отладка, тесты, будущий справочник).
    #[allow(dead_code)] // потребитель — тесты и будущий справочник рецептов
    pub fn recipes_for(
        &self,
        crafter: CrafterKind,
    ) -> impl Iterator<Item = &CraftRecipe> + '_ {
        self.groups[crafter as usize]
            .iter()
            .map(move |&index| &self.recipes[index as usize])
    }
}

/// Рекурсивный обход каталога рецептов. `folder` — имя текущей папки
/// (для корня — `manual`); используется только для предупреждения о
/// несовпадении с `parent`.
fn load_recipes_recursive(
    dir: &Path,
    folder: Option<&str>,
    load: &mut CraftBookLoad,
    seen: &mut HashSet<(CrafterKind, String)>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            load.errors.push(format!("{}: {error}", dir.display()));
            return;
        }
    };
    let mut files: Vec<PathBuf> = Vec::new();
    let mut subdirs: Vec<PathBuf> = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            files.push(path);
        }
    }
    files.sort();
    subdirs.sort();

    for path in files {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                load.errors.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        match parse_recipe(&text) {
            Ok(recipe) => {
                warn_folder_mismatch(&path, folder, &recipe, &mut load.warnings);
                push_recipe(&mut load.book, seen, recipe, &path, &mut load.errors);
            }
            Err(error) => load.errors.push(format!("{}: {error}", path.display())),
        }
    }
    for subdir in subdirs {
        // Имя подпапки — подсказка раскладки, а не категория рецепта.
        let Some(name) = subdir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        load_recipes_recursive(&subdir, Some(name), load, seen);
    }
}

/// Предупреждение, если файл лежит не в папке одного из своих крафтеров.
/// Загрузку не блокирует: удобнее отлаживать данные, не теряя рецепт.
fn warn_folder_mismatch(
    path: &Path,
    folder: Option<&str>,
    recipe: &CraftRecipe,
    warnings: &mut Vec<String>,
) {
    let Some(folder) = folder else { return };
    match CrafterKind::from_id(folder) {
        Some(folder_kind) if !recipe.parents.contains(&folder_kind) => {
            warnings.push(format!(
                "{}: папка '{folder}' не входит в parent ({})",
                path.display(),
                parent_list(&recipe.parents),
            ));
        }
        Some(_) => {}
        None => warnings.push(format!(
            "{}: папка '{folder}' не известна как крафтер",
            path.display(),
        )),
    }
}

fn parent_list(parents: &[CrafterKind]) -> String {
    parents
        .iter()
        .map(|parent| parent.id())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Кладёт рецепт в арену и во все его группы. Дубликат `(крафтер, id)`
/// отклоняется: иначе «первый подходящий» зависел бы от порядка файлов.
fn push_recipe(
    book: &mut CraftBook,
    seen: &mut HashSet<(CrafterKind, String)>,
    recipe: CraftRecipe,
    path: &Path,
    errors: &mut Vec<String>,
) {
    if let Some(&parent) = recipe
        .parents
        .iter()
        .find(|&&parent| seen.contains(&(parent, recipe.id.clone())))
    {
        errors.push(format!(
            "{}: дубликат id '{}' для крафтера '{}'",
            path.display(),
            recipe.id,
            parent.id(),
        ));
        return;
    }

    let index = book.recipes.len() as u32;
    for &parent in &recipe.parents {
        seen.insert((parent, recipe.id.clone()));
        book.groups[parent as usize].push(index);
        if let RecipeBody::Cooking { input } = &recipe.body {
            let slot = &mut book.cooking[parent as usize][input.0 as usize];
            if *slot == u32::MAX {
                *slot = index;
            }
        }
    }
    book.recipes.push(recipe);
}

impl CraftRecipe {
    /// Сеточный ли рецепт (shaped/shapeless). У кулинарного матчинга нет.
    pub fn is_grid(&self) -> bool {
        !matches!(self.body, RecipeBody::Cooking { .. })
    }

    /// Бесформенный ли рецепт.
    #[allow(dead_code)] // потребитель — тесты
    pub fn is_shapeless(&self) -> bool {
        matches!(self.body, RecipeBody::Shapeless(_))
    }

    /// Форма рецепта (только для shaped).
    pub fn pattern(&self) -> Option<&ShapedPattern> {
        match &self.body {
            RecipeBody::Shaped(pattern) => Some(pattern),
            _ => None,
        }
    }

    /// Вход кулинарного рецепта (вид + количество).
    pub fn cooking_input(&self) -> Option<(ItemKind, u32)> {
        match &self.body {
            RecipeBody::Cooking { input } => Some(*input),
            _ => None,
        }
    }

    /// Подходит ли рецепт под сетку игрока.
    pub fn matches(&self, grid: &[Option<ItemStack>; GRID_SLOTS]) -> bool {
        match &self.body {
            RecipeBody::Shaped(_) => self.matches_shaped(grid),
            RecipeBody::Shapeless(_) => self.matches_shapeless(grid),
            RecipeBody::Cooking { .. } => false,
        }
    }

    /// Лежат ли все требуемые инструменты в слотах инструментов (набор,
    /// порядок не важен). Инструменты не расходуются.
    pub fn tools_satisfied(&self, held_tools: &[ItemKind]) -> bool {
        self.tools
            .iter()
            .all(|tool| held_tools.contains(tool))
    }

    /// Снимает ингредиенты с сетки после успешного крафта. Для формы —
    /// по одному предмету из каждой занятой клетки; для бесформенного —
    /// ровно столько, сколько указано в рецепте.
    pub fn consume(&self, grid: &mut [Option<ItemStack>; GRID_SLOTS]) {
        match &self.body {
            RecipeBody::Shapeless(ingredients) => {
                for &(kind, count) in ingredients {
                    let mut left = count;
                    for cell in grid.iter_mut() {
                        if left == 0 {
                            break;
                        }
                        let Some(stack) = cell else { continue };
                        if stack.kind != kind {
                            continue;
                        }
                        let take = stack.count.min(left);
                        stack.count -= take;
                        left -= take;
                        if stack.count == 0 {
                            *cell = None;
                        }
                    }
                }
            }
            RecipeBody::Shaped(_) => {
                for cell in grid.iter_mut() {
                    let Some(stack) = cell else { continue };
                    stack.count -= 1;
                    if stack.count == 0 {
                        *cell = None;
                    }
                }
            }
            RecipeBody::Cooking { .. } => {}
        }
    }

    /// Стоимость одного крафта по видам предметов. Для формы — по одному
    /// предмету с каждой занятой клетки, для бесформенного — количества из
    /// рецепта. Одинаковые виды сложены при загрузке.
    fn ingredient_cost(&self) -> &[(ItemKind, u32)] {
        &self.cost
    }

    /// Сколько крафтов рецепта позволяет запас ингредиентов (сетка +
    /// инвентарь), без учёта места под результат.
    fn max_crafts_by_ingredients(
        &self,
        grid: &[Option<ItemStack>; GRID_SLOTS],
        inventory: &Inventory,
    ) -> u32 {
        let mut max = u32::MAX;
        for &(kind, need) in self.ingredient_cost() {
            if need == 0 {
                continue;
            }
            let in_grid: u32 = grid
                .iter()
                .flatten()
                .filter(|stack| stack.kind == kind)
                .map(|stack| stack.count)
                .sum();
            let available = in_grid + inventory.count_of(kind);
            max = max.min(available / need);
        }
        // Пустой рецепт не должен давать бесконечность.
        if max == u32::MAX {
            0
        } else {
            max.min(MAX_BULK_CRAFTS)
        }
    }

    /// Максимум крафтов из текущей сетки с добором ингредиентов из
    /// инвентаря и с учётом места под результат. Считается прогоном на
    /// копиях сетки и инвентаря — так же, как крафт идёт по одному:
    /// ингредиенты из сетки, недостающее из инвентаря, результат в
    /// инвентарь. Ограничение по ингредиентам не даёт циклу стать
    /// бесконечным даже у рецепта, который не уменьшает число предметов.
    pub fn max_crafts(
        &self,
        grid: &[Option<ItemStack>; GRID_SLOTS],
        inventory: &Inventory,
    ) -> u32 {
        if !self.is_grid() {
            return 0;
        }
        let limit = self.max_crafts_by_ingredients(grid, inventory);
        let mut grid = *grid;
        let mut inventory = *inventory;
        let mut crafts = 0;
        while crafts < limit {
            if !self.try_craft_one(&mut grid, &mut inventory) {
                break;
            }
            crafts += 1;
        }
        crafts
    }

    /// Один крафт: списывает ингредиенты из сетки, добирает недостающее из
    /// инвентаря и кладёт результат в инвентарь. Сетка после крафта
    /// восстанавливается до прежней формы из инвентаря — иначе следующий
    /// крафт «не узнал» бы рецепт. `false` — ингредиентов или места нет;
    /// в этом случае ничего не меняется.
    pub fn try_craft_one(
        &self,
        grid: &mut [Option<ItemStack>; GRID_SLOTS],
        inventory: &mut Inventory,
    ) -> bool {
        if !self.is_grid() {
            return false;
        }
        let enough_ingredients = self.ingredient_cost().iter().all(|&(kind, need)| {
            let in_grid: u32 = grid
                .iter()
                .flatten()
                .filter(|stack| stack.kind == kind)
                .map(|stack| stack.count)
                .sum();
            in_grid + inventory.count_of(kind) >= need
        });
        if !enough_ingredients {
            return false;
        }

        let template = *grid;
        self.consume(grid);
        // Результат проверяем после списания: ингредиенты могли освободить
        // слот. Не влез — возвращаем сетку в исходное состояние.
        if !inventory.can_fit(self.result.kind, self.result.count) {
            *grid = template;
            return false;
        }
        let left = inventory.add(self.result.kind, self.result.count);
        debug_assert_eq!(left, 0, "проверка can_fit должна гарантировать место");
        self.refill_grid(grid, inventory, &template);
        true
    }

    /// Добирает пустые/неполные клетки сетки до исходного шаблона из
    /// инвентаря. Шаблон — состояние сетки до крафта: форма и количества
    /// остаются прежними, расходуется ровно стоимость одного крафта.
    fn refill_grid(
        &self,
        grid: &mut [Option<ItemStack>; GRID_SLOTS],
        inventory: &mut Inventory,
        template: &[Option<ItemStack>; GRID_SLOTS],
    ) {
        for (cell, template_cell) in grid.iter_mut().zip(template.iter()) {
            let Some(template_stack) = *template_cell else {
                continue;
            };
            let current = cell.map_or(0, |stack| stack.count);
            let missing = template_stack.count.saturating_sub(current);
            if missing == 0 || !inventory.take_items(template_stack.kind, missing) {
                continue;
            }
            *cell = Some(ItemStack {
                kind: template_stack.kind,
                count: current + missing,
            });
        }
    }

    /// Бесформенное совпадение: одинаковый набор видов и количеств.
    /// Списки ингредиентов слиты и отсортированы при загрузке, поэтому
    /// проверка не аллоцирует (идёт каждый кадр из UI).
    fn matches_shapeless(&self, grid: &[Option<ItemStack>; GRID_SLOTS]) -> bool {
        let RecipeBody::Shapeless(ingredients) = &self.body else {
            return false;
        };
        if ingredients.is_empty() || ingredients.len() > GRID_SLOTS {
            return false;
        }
        let mut actual = [0u32; GRID_SLOTS];
        for stack in grid.iter().flatten() {
            let Some(index) = ingredients
                .iter()
                .position(|(kind, _)| *kind == stack.kind)
            else {
                return false;
            };
            actual[index] += stack.count;
        }
        ingredients
            .iter()
            .enumerate()
            .all(|(index, &(_, need))| actual[index] == need)
    }

    /// Совпадение по форме: та же обрезанная ширина/высота и те же виды
    /// в клетках. Количество в клетке не важно — тратится по одному.
    fn matches_shaped(&self, grid: &[Option<ItemStack>; GRID_SLOTS]) -> bool {
        let Some(pattern) = self.pattern() else {
            return false;
        };
        let Some((left, top, width, height)) = occupied_bounds(grid) else {
            return false;
        };
        if width != pattern.width || height != pattern.height {
            return false;
        }
        for row in 0..height {
            for col in 0..width {
                let index = (top + row) * 3 + left + col;
                let cell_kind = grid[index].map(|stack| stack.kind);
                let want = pattern.cells[row * width + col];
                if cell_kind != want {
                    return false;
                }
            }
        }
        true
    }
}

/// Складывает стоимость одного ингредиента в список «вид → количество».
fn add_cost(cost: &mut Vec<(ItemKind, u32)>, kind: ItemKind, count: u32) {
    match cost.iter_mut().find(|(existing, _)| *existing == kind) {
        Some((_, total)) => *total += count,
        None => cost.push((kind, count)),
    }
}

/// Стоимость одного крафта по телу рецепта (считается при загрузке).
fn build_cost(body: &RecipeBody) -> Vec<(ItemKind, u32)> {
    let mut cost = Vec::new();
    match body {
        RecipeBody::Shaped(pattern) => {
            for kind in pattern.cells.iter().flatten() {
                add_cost(&mut cost, *kind, 1);
            }
        }
        RecipeBody::Shapeless(ingredients) => {
            for &(kind, count) in ingredients {
                add_cost(&mut cost, kind, count);
            }
        }
        RecipeBody::Cooking { input } => add_cost(&mut cost, input.0, input.1),
    }
    cost.sort_by_key(|(kind, _)| *kind as u8);
    cost
}

/// Границы занятых клеток сетки: `(left, top, width, height)`.
fn occupied_bounds(grid: &[Option<ItemStack>; GRID_SLOTS]) -> Option<(usize, usize, usize, usize)> {
    let mut left = 3;
    let mut top = 3;
    let mut right = 0;
    let mut bottom = 0;
    let mut any = false;
    for (index, cell) in grid.iter().enumerate() {
        if cell.is_none() {
            continue;
        }
        any = true;
        let (x, y) = (index % 3, index / 3);
        left = left.min(x);
        top = top.min(y);
        right = right.max(x);
        bottom = bottom.max(y);
    }
    // Пустая сетка: границы не определены. Проверка обязана быть до вычисления `right - left` — при пустой сетке right < left.
    if !any {
        return None;
    }
    Some((left, top, right - left + 1, bottom - top + 1))
}

/// Сырой JSON-рецепт: строки предметов разрешаются в `ItemKind`.
#[derive(Deserialize)]
struct RawRecipe {
    id: String,
    parent: OneOrMany<String>,
    #[serde(default)]
    shapeless: bool,
    #[serde(default)]
    grid: Vec<Vec<Option<String>>>,
    #[serde(default)]
    ingredients: Option<RawIngredients>,
    #[serde(default)]
    input: Option<RawStack>,
    #[serde(default)]
    tools: Vec<String>,
    result: RawStack,
}

/// `ingredients` — объект «символ → предмет» у shaped и массив входов
/// у shapeless. Форма JSON выбирается по виду рецепта.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawIngredients {
    Keyed(BTreeMap<String, String>),
    List(Vec<RawIngredient>),
}

#[derive(Deserialize)]
struct RawIngredient {
    item: String,
    #[serde(default = "one")]
    count: u32,
}

#[derive(Deserialize)]
struct RawStack {
    item: String,
    #[serde(default = "one")]
    count: u32,
}

/// `parent` в JSON: одна строка или массив строк.
#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T> OneOrMany<T> {
    fn into_vec(self) -> Vec<T> {
        match self {
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

fn one() -> u32 {
    1
}

/// Разбирает один JSON-файл рецепта. Папка файла здесь не проверяется —
/// это делает загрузчик (несовпадение = предупреждение, не ошибка).
pub fn parse_recipe(json: &str) -> Result<CraftRecipe, String> {
    let mut raw: RawRecipe = serde_json::from_str(json).map_err(|error| error.to_string())?;
    if raw.id.is_empty() {
        return Err("пустой id рецепта".to_string());
    }

    // Крафтеры: известные, без дублей, одной схемы.
    let parent_ids =
        std::mem::replace(&mut raw.parent, OneOrMany::Many(Vec::new())).into_vec();
    if parent_ids.is_empty() {
        return Err("пустой parent".to_string());
    }
    let mut parents: Vec<CrafterKind> = Vec::with_capacity(parent_ids.len());
    for id in parent_ids {
        let kind = CrafterKind::from_id(&id)
            .ok_or_else(|| format!("неизвестный крафтер: {id}"))?;
        if !parents.contains(&kind) {
            parents.push(kind);
        }
    }
    let shape = parents[0].recipe_shape();
    if parents.iter().any(|parent| parent.recipe_shape() != shape) {
        return Err("parent смешивает сеточных и кулинарных крафтеров".to_string());
    }

    let result_kind = ItemKind::from_id(&raw.result.item)
        .ok_or_else(|| format!("неизвестный предмет результата: {}", raw.result.item))?;
    if raw.result.count == 0 {
        return Err("количество результата должно быть больше нуля".to_string());
    }

    // Требуемые инструменты: известные предметы, не больше слотов стола.
    if raw.tools.len() > 4 {
        return Err("больше четырёх требуемых инструментов".to_string());
    }
    let mut tools = Vec::with_capacity(raw.tools.len());
    for tool in &raw.tools {
        let kind = ItemKind::from_id(tool)
            .ok_or_else(|| format!("неизвестный инструмент: {tool}"))?;
        tools.push(kind);
    }
    if shape == RecipeShape::Cooking && !tools.is_empty() {
        return Err("кулинарный рецепт не поддерживает tools".to_string());
    }

    let body = match shape {
        RecipeShape::Cooking => parse_cooking_body(&raw)?,
        RecipeShape::Grid => parse_grid_body(&raw)?,
    };
    let cost = build_cost(&body);

    Ok(CraftRecipe {
        id: raw.id,
        parents,
        body,
        result: ItemStack { kind: result_kind, count: raw.result.count },
        tools,
        cost,
    })
}

/// Тело кулинарного рецепта: ровно один `input`, без сетки и `shapeless`.
fn parse_cooking_body(raw: &RawRecipe) -> Result<RecipeBody, String> {
    if raw.shapeless {
        return Err("у кулинарного рецепта не должно быть shapeless".to_string());
    }
    if !raw.grid.is_empty() {
        return Err("у кулинарного рецепта не должно быть grid".to_string());
    }
    if raw.ingredients.is_some() {
        return Err("у кулинарного рецепта не должно быть ingredients".to_string());
    }
    let input = raw
        .input
        .as_ref()
        .ok_or_else(|| "у кулинарного рецепта нет input".to_string())?;
    let kind = ItemKind::from_id(&input.item)
        .ok_or_else(|| format!("неизвестный входной предмет: {}", input.item))?;
    if input.count == 0 {
        return Err(format!("нулевое количество входа: {}", input.item));
    }
    Ok(RecipeBody::Cooking { input: (kind, input.count) })
}

/// Тело сеточного рецепта: shaped (карта символов + grid) или shapeless.
fn parse_grid_body(raw: &RawRecipe) -> Result<RecipeBody, String> {
    if raw.input.is_some() {
        return Err("у сеточного рецепта не должно быть input".to_string());
    }
    if raw.shapeless {
        if !raw.grid.is_empty() {
            return Err("у shapeless-рецепта не должно быть grid".to_string());
        }
        let Some(RawIngredients::List(list)) = raw.ingredients.as_ref() else {
            return Err(
                "у shapeless-рецепта ingredients должен быть массивом входов".to_string(),
            );
        };
        if list.is_empty() {
            return Err("у бесформенного рецепта нет ингредиентов".to_string());
        }
        let mut ingredients = Vec::with_capacity(list.len());
        for ingredient in list {
            let kind = ItemKind::from_id(&ingredient.item)
                .ok_or_else(|| format!("неизвестный ингредиент: {}", ingredient.item))?;
            if ingredient.count == 0 {
                return Err(format!("нулевое количество ингредиента: {}", ingredient.item));
            }
            add_cost(&mut ingredients, kind, ingredient.count);
        }
        if ingredients.len() > GRID_SLOTS {
            return Err("у бесформенного рецепта больше девяти разных ингредиентов".to_string());
        }
        ingredients.sort_by_key(|(kind, _)| *kind as u8);
        return Ok(RecipeBody::Shapeless(ingredients));
    }

    let Some(RawIngredients::Keyed(keys)) = raw.ingredients.as_ref() else {
        return Err(
            "у shaped-рецепта ingredients должен быть объектом «символ → предмет»".to_string(),
        );
    };
    if keys.is_empty() {
        return Err("у shaped-рецепта пустая карта ingredients".to_string());
    }
    let mut resolved: BTreeMap<&str, ItemKind> = BTreeMap::new();
    for (symbol, item) in keys {
        if symbol.is_empty() {
            return Err("пустой символ в ingredients".to_string());
        }
        let kind = ItemKind::from_id(item)
            .ok_or_else(|| format!("неизвестный ингредиент формы: {item}"))?;
        resolved.insert(symbol.as_str(), kind);
    }
    let pattern = parse_pattern(&raw.grid, &resolved)?;
    Ok(RecipeBody::Shaped(pattern))
}

/// Проверяет сетку 3×3, разворачивает символы в `ItemKind` и обрезает
/// форму по занятым клеткам.
fn parse_pattern(
    grid: &[Vec<Option<String>>],
    keys: &BTreeMap<&str, ItemKind>,
) -> Result<ShapedPattern, String> {
    if grid.len() != 3 || grid.iter().any(|row| row.len() != 3) {
        return Err("форма рецепта должна быть сеткой 3×3".to_string());
    }

    let mut cells: [Option<ItemKind>; GRID_SLOTS] = [None; GRID_SLOTS];
    let mut left = 3;
    let mut top = 3;
    let mut right = 0;
    let mut bottom = 0;
    let mut any = false;

    for (y, row) in grid.iter().enumerate() {
        for (x, cell) in row.iter().enumerate() {
            let Some(symbol) = cell else { continue };
            let Some(&kind) = keys.get(symbol.as_str()) else {
                return Err(format!("символ '{symbol}' не описан в ingredients"));
            };
            cells[y * 3 + x] = Some(kind);
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
            any = true;
        }
    }
    if !any {
        return Err("пустая форма рецепта".to_string());
    }

    let width = right - left + 1;
    let height = bottom - top + 1;
    let mut trimmed = Vec::with_capacity(width * height);
    for y in top..=bottom {
        for x in left..=right {
            trimmed.push(cells[y * 3 + x]);
        }
    }

    Ok(ShapedPattern { width, height, cells: trimmed })
}
