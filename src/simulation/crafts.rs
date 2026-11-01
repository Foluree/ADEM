//! Рецепты крафта из `data/crafts/**/*.json` и сопоставление с сеткой 3×3.
//!
//! Рецепт бывает двух видов (гибрид по решению автора):
//! - `shaped` (по умолчанию) — важны форма и взаимное расположение
//!   ингредиентов; сетка рецепта обрезается по занятым клеткам и ищется
//!   в любом месте сетки игрока;
//! - `shapeless` — важны только виды и количества, порядок не важен.
//!
//! JSON держится в данных, а не в коде: строки предметов — стабильные
//! идентификаторы `ItemKind::id()`. Категория рецепта определяется местом
//! файла: корень каталога — ручной крафт (`manual`), подпапка — станция
//! с именем папки (`furnace/`, `crafting_table/`, …). Так верстак наследует
//! ручные рецепты без копирования (тот же файл), а свои добавляет в своей
//! папке.
//!
//! Рецепт может требовать инструменты (кузнечный стол): поле `tools`
//! перечисляет предметы, которые должны лежать в слотах инструментов.
//! Инструменты не расходуются — это требование «чем работаешь».

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use super::items::{Inventory, ItemKind, ItemStack};

/// Категория ручного крафта в сетке 3×3 (и верстака — сетка наследует
/// ручные рецепты).
pub const MANUAL_CATEGORY: &str = "manual";

/// Число ячеек сетки крафта.
pub const GRID_SLOTS: usize = 9;

/// Предохранитель для массового крафта: рецепт, который не уменьшает
/// суммарное количество предметов (например, 1 предмет → 1 предмет),
/// иначе крутил бы цикл подсчёта бесконечно. На реальных данных недостижим.
const MAX_BULK_CRAFTS: u32 = 10_000;

/// Разобранный рецепт с уже разрешёнными видами предметов.
#[derive(Clone, Debug)]
pub struct CraftRecipe {
    /// Стабильный идентификатор рецепта (имя файла без расширения).
    #[allow(dead_code)] // читается тестами и будущими станциями/отладкой
    pub id: String,
    /// Категория станции; для ручной сетки — `manual`.
    pub category: String,
    /// true — рецепт без формы (порядок ингредиентов не важен).
    pub shapeless: bool,
    /// Форма рецепта (только для shaped).
    pub pattern: Option<ShapedPattern>,
    /// Ингредиенты без формы: вид + количество.
    pub ingredients: Vec<(ItemKind, u32)>,
    /// Что получается.
    pub result: ItemStack,
    /// Требуемые инструменты (кузнечный стол): должны лежать в слотах
    /// инструментов, не расходуются. Пусто — инструменты не нужны.
    pub tools: Vec<ItemKind>,
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

/// Набор рецептов. Загружается один раз при старте.
#[derive(Clone, Debug, Default)]
pub struct CraftBook {
    pub recipes: Vec<CraftRecipe>,
}

impl CraftBook {
    /// Читает все `*.json` из каталога и его подпапок. Категория рецепта —
    /// по месту файла: корень — `manual`, подпапка X — `X` (станция).
    /// Файлы и подпапки обходятся в лексикографическом порядке, поэтому
    /// набор рецептов детерминирован. Ошибки отдельных файлов не роняют
    /// игру: они возвращаются списком для лога.
    pub fn load_dir(dir: &str) -> (Self, Vec<String>) {
        let mut book = Self::default();
        let mut errors = Vec::new();
        load_recipes_recursive(std::path::Path::new(dir), MANUAL_CATEGORY, &mut book, &mut errors);
        (book, errors)
    }

    /// Рецепт ручной сетки, подходящий под содержимое. Первый по порядку
    /// загрузки — стабильное поведение при совпадении форм.
    pub fn matching_recipe(
        &self,
        grid: &[Option<ItemStack>; GRID_SLOTS],
    ) -> Option<&CraftRecipe> {
        self.matching_recipe_for(&[MANUAL_CATEGORY], grid, &[])
    }

    /// Рецепт первой подходящей категории из `categories` (порядок важен:
    /// наследование — manual раньше crafting_table). `held_tools` — виды
    /// предметов в слотах инструментов станции (кузнечный стол).
    pub fn matching_recipe_for(
        &self,
        categories: &[&str],
        grid: &[Option<ItemStack>; GRID_SLOTS],
        held_tools: &[ItemKind],
    ) -> Option<&CraftRecipe> {
        self.recipes.iter().find(|recipe| {
            categories.contains(&recipe.category.as_str())
                && recipe.tools_satisfied(held_tools)
                && recipe.matches(grid)
        })
    }
}

/// Рекурсивный обход каталога рецептов: файлы текущей папки — с категорией
/// `category`, подпапки — со своей категорией по имени. Порядок стабильный.
fn load_recipes_recursive(
    dir: &std::path::Path,
    category: &str,
    book: &mut CraftBook,
    errors: &mut Vec<String>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push(format!("{}: {error}", dir.display()));
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
                errors.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        match parse_recipe(&text) {
            Ok(mut recipe) => {
                recipe.category = category.to_string();
                book.recipes.push(recipe);
            }
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
    }
    for subdir in subdirs {
        // Имя подпапки = категория станции (furnace/, crafting_table/…).
        let Some(name) = subdir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        load_recipes_recursive(&subdir, name, book, errors);
    }
}

impl CraftRecipe {
    /// Подходит ли рецепт под сетку игрока.
    pub fn matches(&self, grid: &[Option<ItemStack>; GRID_SLOTS]) -> bool {
        if self.shapeless {
            self.matches_shapeless(grid)
        } else {
            self.matches_shaped(grid)
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
        if self.shapeless {
            for &(kind, count) in &self.ingredients {
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
        } else {
            for cell in grid.iter_mut() {
                let Some(stack) = cell else { continue };
                stack.count -= 1;
                if stack.count == 0 {
                    *cell = None;
                }
            }
        }
    }

    /// Стоимость одного крафта по видам предметов. Для формы — по одному
    /// предмету с каждой занятой клетки, для бесформенного — количества из
    /// рецепта. Одинаковые виды складываются: две клетки палок в форме
    /// требуют две палки за крафт.
    fn ingredient_cost(&self) -> Vec<(ItemKind, u32)> {
        let mut cost = Vec::new();
        if self.shapeless {
            for &(kind, count) in &self.ingredients {
                add_cost(&mut cost, kind, count);
            }
        } else if let Some(pattern) = &self.pattern {
            for kind in pattern.cells.iter().flatten() {
                add_cost(&mut cost, *kind, 1);
            }
        }
        cost
    }

    /// Сколько крафтов рецепта позволяет запас ингредиентов (сетка +
    /// инвентарь), без учёта места под результат.
    fn max_crafts_by_ingredients(
        &self,
        grid: &[Option<ItemStack>; GRID_SLOTS],
        inventory: &Inventory,
    ) -> u32 {
        let mut max = u32::MAX;
        for (kind, need) in self.ingredient_cost() {
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
        let enough_ingredients = self.ingredient_cost().into_iter().all(|(kind, need)| {
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
    fn matches_shapeless(&self, grid: &[Option<ItemStack>; GRID_SLOTS]) -> bool {
        let mut counts: Vec<(ItemKind, u32)> = Vec::new();
        for stack in grid.iter().flatten() {
            match counts.iter_mut().find(|(kind, _)| *kind == stack.kind) {
                Some((_, count)) => *count += stack.count,
                None => counts.push((stack.kind, stack.count)),
            }
        }
        counts.sort_by_key(|(kind, _)| *kind as u8);
        let mut wanted = self.ingredients.clone();
        wanted.sort_by_key(|(kind, _)| *kind as u8);
        counts == wanted
    }

    /// Совпадение по форме: та же обрезанная ширина/высота и те же виды
    /// в клетках. Количество в клетке не важно — тратится по одному.
    fn matches_shaped(&self, grid: &[Option<ItemStack>; GRID_SLOTS]) -> bool {
        let Some(pattern) = &self.pattern else {
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
    #[serde(default = "manual_category")]
    category: String,
    #[serde(default)]
    shapeless: bool,
    #[serde(default)]
    grid: Vec<Vec<Option<String>>>,
    #[serde(default)]
    ingredients: Vec<RawIngredient>,
    #[serde(default)]
    tools: Vec<String>,
    result: RawResult,
}

#[derive(Deserialize)]
struct RawIngredient {
    item: String,
    #[serde(default = "one")]
    count: u32,
}

#[derive(Deserialize)]
struct RawResult {
    item: String,
    #[serde(default = "one")]
    count: u32,
}

fn manual_category() -> String {
    MANUAL_CATEGORY.to_string()
}
fn one() -> u32 {
    1
}

/// Разбирает один JSON-файл рецепта.
pub fn parse_recipe(json: &str) -> Result<CraftRecipe, String> {
    let raw: RawRecipe = serde_json::from_str(json).map_err(|error| error.to_string())?;
    if raw.id.is_empty() {
        return Err("пустой id рецепта".to_string());
    }
    let result_kind = ItemKind::from_id(&raw.result.item).ok_or_else(|| format!("неизвестный предмет результата: {}", raw.result.item))?;
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

    if raw.shapeless {
        if raw.ingredients.is_empty() {
            return Err("у бесформенного рецепта нет ингредиентов".to_string());
        }
        let mut ingredients = Vec::with_capacity(raw.ingredients.len());
        for ingredient in &raw.ingredients {
            let kind = ItemKind::from_id(&ingredient.item)
                .ok_or_else(|| format!("неизвестный ингредиент: {}", ingredient.item))?;
            if ingredient.count == 0 {
                return Err(format!("нулевое количество ингредиента: {}", ingredient.item));
            }
            ingredients.push((kind, ingredient.count));
        }
        return Ok(CraftRecipe {
            id: raw.id,
            category: raw.category,
            shapeless: true,
            pattern: None,
            ingredients,
            result: ItemStack { kind: result_kind, count: raw.result.count },
            tools,
        });
    }

    let pattern = parse_pattern(&raw.grid)?;
    Ok(CraftRecipe {
        id: raw.id,
        category: raw.category,
        shapeless: false,
        pattern: Some(pattern),
        ingredients: Vec::new(),
        result: ItemStack { kind: result_kind, count: raw.result.count },
        tools,
    })
}

/// Проверяет сетку 3×3 и обрезает её по занятым клеткам.
fn parse_pattern(grid: &[Vec<Option<String>>]) -> Result<ShapedPattern, String> {
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
            let Some(id) = cell else { continue };
            let kind = ItemKind::from_id(id)
                .ok_or_else(|| format!("неизвестный ингредиент формы: {id}"))?;
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
