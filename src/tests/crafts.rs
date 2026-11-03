//! Тесты крафта: разбор и сопоставление рецептов (shaped/shapeless),
//! списывание ингредиентов, загрузка JSON из `data/crafts`.

/// Разбор формы: рецепты обрезаются по занятым клеткам и совпадают
/// в любом месте сетки, а количество в клетке не важно.
#[test]
fn shaped_recipe_matches_trimmed_pattern_anywhere() {
    use crate::simulation::crafts::{parse_recipe, GRID_SLOTS};
    use crate::simulation::items::ItemKind;
    use crate::simulation::ItemStack;

    let recipe = parse_recipe(
        r#"{
            "id": "pair_of_sticks",
            "parent": "manual",
            "ingredients": { "S": "stick" },
            "grid": [
                ["S", null, null],
                ["S", null, null],
                [null, null, null]
            ],
            "result": { "item": "stick", "count": 2 }
        }"#,
    )
    .expect("рецепт должен разбираться");
    assert!(!recipe.is_shapeless());
    let pattern = recipe.pattern().unwrap();
    assert_eq!((pattern.width, pattern.height), (1, 2));

    let stack = |kind| Some(ItemStack { kind, count: 2 });
    // Пустая сетка не должна вызывать переполнение при поиске границ.
    assert!(!recipe.matches(&[None; GRID_SLOTS]));
    let mut grid: [Option<ItemStack>; GRID_SLOTS] = [None; GRID_SLOTS];
    // Вертикальная пара в средней колонке: форма найдена со сдвигом.
    grid[4] = stack(ItemKind::Stick);
    grid[7] = stack(ItemKind::Stick);
    assert!(recipe.matches(&grid));

    // Горизонтальная пара — другая форма.
    let mut wrong: [Option<ItemStack>; GRID_SLOTS] = [None; GRID_SLOTS];
    wrong[4] = stack(ItemKind::Stick);
    wrong[5] = stack(ItemKind::Stick);
    assert!(!recipe.matches(&wrong));
}

/// Бесформенный рецепт совпадает по точным количествам, а не по форме;
/// после крафта списывается ровно указанное число ингредиентов.
#[test]
fn shapeless_recipe_matches_counts_and_consumes() {
    use crate::simulation::crafts::{parse_recipe, GRID_SLOTS};
    use crate::simulation::items::ItemKind;
    use crate::simulation::ItemStack;

    let recipe = parse_recipe(
        r#"{
            "id": "plant_fiber_test",
            "parent": "manual",
            "shapeless": true,
            "ingredients": [{ "item": "grass_bunch", "count": 2 }],
            "result": { "item": "plant_fiber", "count": 1 }
        }"#,
    )
    .expect("рецепт должен разбираться");
    assert!(recipe.is_shapeless());

    let grass = |count| Some(ItemStack { kind: ItemKind::GrassBunch, count });
    let mut grid: [Option<ItemStack>; GRID_SLOTS] = [None; GRID_SLOTS];
    grid[0] = grass(1);
    grid[8] = grass(1);
    assert!(recipe.matches(&grid));

    // Три пучка — уже не тот набор.
    grid[8] = grass(2);
    assert!(!recipe.matches(&grid));

    // Ровно два пучка в одном стеке — совпадает и списывает оба.
    let mut stacked: [Option<ItemStack>; GRID_SLOTS] = [None; GRID_SLOTS];
    stacked[3] = grass(2);
    assert!(recipe.matches(&stacked));
    recipe.consume(&mut stacked);
    assert_eq!(stacked[3], None);
}

/// Массовый крафт считает ингредиенты и в сетке, и в инвентаре: двух
/// пучков травы в сетке и четырёх в инвентаре хватает на три волокна.
#[test]
fn bulk_craft_uses_grid_and_inventory_ingredients() {
    use crate::simulation::crafts::{parse_recipe, GRID_SLOTS};
    use crate::simulation::items::{Inventory, ItemKind};
    use crate::simulation::ItemStack;

    let recipe = parse_recipe(
        r#"{
            "id": "fiber_bulk",
            "parent": "manual",
            "shapeless": true,
            "ingredients": [{ "item": "grass_bunch", "count": 2 }],
            "result": { "item": "plant_fiber", "count": 1 }
        }"#,
    )
    .expect("рецепт должен разбираться");

    let mut grid: [Option<ItemStack>; GRID_SLOTS] = [None; GRID_SLOTS];
    grid[0] = Some(ItemStack { kind: ItemKind::GrassBunch, count: 2 });
    let mut inventory = Inventory::default();
    inventory.add(ItemKind::GrassBunch, 4);

    assert_eq!(recipe.max_crafts(&grid, &inventory), 3);
    for _ in 0..3 {
        assert!(recipe.try_craft_one(&mut grid, &mut inventory));
    }
    assert_eq!(inventory.count_of(ItemKind::PlantFiber), 3);
    assert_eq!(inventory.count_of(ItemKind::GrassBunch), 0);
    assert!(grid.iter().all(Option::is_none));
    assert!(!recipe.try_craft_one(&mut grid, &mut inventory));
}

/// Максимум крафта ограничен местом под результат: при полностью занятом
/// инвентаре не крафтится ничего, а сетка не тратится.
#[test]
fn bulk_craft_respects_inventory_space() {
    use crate::simulation::crafts::{parse_recipe, GRID_SLOTS};
    use crate::simulation::items::{Inventory, ItemKind, MISC_SLOTS};
    use crate::simulation::ItemStack;

    let recipe = parse_recipe(
        r#"{
            "id": "arrow_bulk",
            "parent": "manual",
            "shapeless": true,
            "ingredients": [{ "item": "stick", "count": 1 }],
            "result": { "item": "arrow", "count": 1 }
        }"#,
    )
    .expect("рецепт должен разбираться");

    let mut grid: [Option<ItemStack>; GRID_SLOTS] = [None; GRID_SLOTS];
    grid[0] = Some(ItemStack { kind: ItemKind::Stick, count: 1 });
    let mut inventory = Inventory::default();
    for slot in &mut inventory.items {
        *slot = Some(ItemStack { kind: ItemKind::Arrow, count: ItemKind::Arrow.max_stack() });
    }
    assert_eq!(inventory.items.len(), MISC_SLOTS);

    assert_eq!(recipe.max_crafts(&grid, &inventory), 0);
    assert!(!recipe.try_craft_one(&mut grid, &mut inventory));
    assert_eq!(grid[0], Some(ItemStack { kind: ItemKind::Stick, count: 1 }));

    // Один arrow можно доложить — ровно один крафт.
    inventory.items[0].as_mut().unwrap().count -= 1;
    assert_eq!(recipe.max_crafts(&grid, &inventory), 1);
    assert!(recipe.try_craft_one(&mut grid, &mut inventory));
    assert_eq!(grid[0], None);
}

/// Все рецепты из `data/crafts` читаются без ошибок: `parent` определяет
/// группу, папки совпадают с `parent`, рецепты с инструментами (кузнечный
/// стол) читаются, общие рецепты костра и печи не дублируются.
#[test]
fn craft_book_loads_all_recipe_files() {
    use crate::simulation::crafts::{CraftBook, CrafterKind};

    let load = CraftBook::load_dir("data/crafts");
    assert!(load.errors.is_empty(), "ошибки рецептов: {:?}", load.errors);
    assert!(
        load.warnings.is_empty(),
        "предупреждения раскладки: {:?}",
        load.warnings
    );
    let book = load.book;
    // 14 ручных + 4 верстака + 10 печи + 4 плавильни + 8 кузнечного стола.
    // Восемь рецептов еды общие для костра и печи (parent-массив), поэтому
    // отдельных файлов костра нет.
    assert_eq!(book.recipes.len(), 14 + 4 + 10 + 4 + 8);

    let manual_ids: Vec<&str> = book
        .recipes_for(CrafterKind::Manual)
        .map(|recipe| recipe.id.as_str())
        .collect();
    assert_eq!(
        manual_ids,
        vec![
            "arrow",
            "axe",
            "bow",
            "bow_mirrored",
            "cordage",
            "fishing_rod",
            "hoe",
            "knife",
            "pickaxe",
            "plant_fiber",
            "shovel",
            "primal_spear",
            "stick_bundle",
            "stick_bundle_unpack",
        ]
    );
    assert!(
        book.recipes
            .iter()
            .any(|recipe| recipe.result.kind == crate::simulation::items::ItemKind::FishingRod)
    );
    // Станционные группы присутствуют, у кузнечного стола есть рецепты
    // с требованием инструмента.
    assert_eq!(book.recipes_for(CrafterKind::Furnace).count(), 10);
    assert_eq!(book.recipes_for(CrafterKind::BlastFurnace).count(), 4);
    assert_eq!(book.recipes_for(CrafterKind::Campfire).count(), 8);
    assert_eq!(book.recipes_for(CrafterKind::CraftingTable).count(), 4);
    assert!(
        book.recipes_for(CrafterKind::SmithingTable)
            .any(|recipe| !recipe.tools.is_empty())
    );
}

/// Связка палок: 8 палок → связка и обратно, оба рецепта ручные.
#[test]
fn stick_bundle_recipe_is_bidirectional() {
    use crate::simulation::crafts::{CraftBook, GRID_SLOTS};
    use crate::simulation::items::{Inventory, ItemKind};
    use crate::simulation::ItemStack;

    let load = CraftBook::load_dir("data/crafts");
    assert!(load.errors.is_empty(), "ошибки рецептов: {:?}", load.errors);
    let book = load.book;

    let mut grid: [Option<ItemStack>; GRID_SLOTS] = [None; GRID_SLOTS];
    grid[0] = Some(ItemStack { kind: ItemKind::Stick, count: 8 });
    let pack = book.matching_recipe(&grid).expect("8 палок должны давать связку");
    assert_eq!(pack.result, ItemStack { kind: ItemKind::StickBundle, count: 1 });

    let mut inventory = Inventory::default();
    assert!(pack.try_craft_one(&mut grid, &mut inventory));
    assert_eq!(inventory.count_of(ItemKind::StickBundle), 1);

    grid[0] = Some(ItemStack { kind: ItemKind::StickBundle, count: 1 });
    let unpack = book.matching_recipe(&grid).expect("связка должна развязываться");
    assert_eq!(unpack.result, ItemStack { kind: ItemKind::Stick, count: 8 });

    let mut inventory = Inventory::default();
    assert!(unpack.try_craft_one(&mut grid, &mut inventory));
    assert_eq!(inventory.count_of(ItemKind::Stick), 8);
    assert_eq!(inventory.count_of(ItemKind::StickBundle), 0);
}

/// `parent` принимает строку и массив; кулинарная схема использует `input`.
#[test]
fn parent_accepts_one_or_many_and_cooking_schema() {
    use crate::simulation::crafts::{CrafterKind, parse_recipe};
    use crate::simulation::items::ItemKind;

    let single = parse_recipe(
        r#"{
            "id": "apple_baked",
            "parent": "furnace",
            "input": { "item": "apple", "count": 1 },
            "result": { "item": "apple_baked", "count": 1 }
        }"#,
    )
    .expect("одиночный parent должен разбираться");
    assert_eq!(single.parents, vec![CrafterKind::Furnace]);
    assert_eq!(single.cooking_input(), Some((ItemKind::Apple, 1)));

    let shared = parse_recipe(
        r#"{
            "id": "apple_baked",
            "parent": ["campfire", "furnace"],
            "input": { "item": "apple", "count": 1 },
            "result": { "item": "apple_baked", "count": 1 }
        }"#,
    )
    .expect("массив parent должен разбираться");
    assert_eq!(
        shared.parents,
        vec![CrafterKind::Campfire, CrafterKind::Furnace]
    );
}

/// Символы `grid` разворачиваются в `ItemKind`; неизвестный символ — ошибка.
#[test]
fn shaped_recipe_resolves_symbols() {
    use crate::simulation::crafts::parse_recipe;
    use crate::simulation::items::ItemKind;

    let recipe = parse_recipe(
        r#"{
            "id": "symbols",
            "parent": "manual",
            "ingredients": { "0": "rock", "A": "stick" },
            "grid": [
                ["0", "A", null],
                [null, "0", null],
                [null, null, null]
            ],
            "result": { "item": "axe", "count": 1 }
        }"#,
    )
    .expect("символы должны разбираться");
    let pattern = recipe.pattern().expect("это shaped-рецепт");
    assert_eq!(
        pattern.cells,
        vec![
            Some(ItemKind::Rock),
            Some(ItemKind::Stick),
            None,
            Some(ItemKind::Rock),
        ]
    );

    let error = parse_recipe(
        r#"{
            "id": "bad_symbol",
            "parent": "manual",
            "ingredients": { "0": "rock" },
            "grid": [
                ["Z", null, null],
                [null, null, null],
                [null, null, null]
            ],
            "result": { "item": "axe", "count": 1 }
        }"#,
    )
    .expect_err("неизвестный символ должен давать ошибку");
    assert!(error.contains("символ"), "неожиданная ошибка: {error}");
}

/// Схемы сетки и готовки не смешиваются, `parent` проверяется.
#[test]
fn recipe_schema_mismatches_are_errors() {
    use crate::simulation::crafts::parse_recipe;

    let unknown_parent = parse_recipe(
        r#"{
            "id": "x",
            "parent": "microwave",
            "input": { "item": "apple", "count": 1 },
            "result": { "item": "apple_baked", "count": 1 }
        }"#,
    )
    .expect_err("неизвестный крафтер должен давать ошибку");
    assert!(unknown_parent.contains("крафтер"));

    let cooking_with_grid = parse_recipe(
        r#"{
            "id": "x",
            "parent": "furnace",
            "grid": [["apple", null, null], [null, null, null], [null, null, null]],
            "result": { "item": "apple_baked", "count": 1 }
        }"#,
    )
    .expect_err("grid у готовки должен давать ошибку");
    assert!(cooking_with_grid.contains("grid"));

    let mixed_parents = parse_recipe(
        r#"{
            "id": "x",
            "parent": ["manual", "furnace"],
            "input": { "item": "apple", "count": 1 },
            "result": { "item": "apple_baked", "count": 1 }
        }"#,
    )
    .expect_err("смешение схем должно давать ошибку");
    assert!(mixed_parents.contains("смешивает"));
}

/// Верстак наследует ручные рецепты, кузнечный стол — нет.
#[test]
fn crafting_table_inherits_manual_but_smithing_does_not() {
    use crate::simulation::crafts::{CraftBook, CrafterKind, GRID_SLOTS};
    use crate::simulation::items::ItemKind;
    use crate::simulation::ItemStack;

    let load = CraftBook::load_dir("data/crafts");
    assert!(load.errors.is_empty(), "ошибки рецептов: {:?}", load.errors);
    let book = load.book;

    let mut grid: [Option<ItemStack>; GRID_SLOTS] = [None; GRID_SLOTS];
    grid[0] = Some(ItemStack { kind: ItemKind::Stick, count: 8 });
    assert!(
        book.matching_recipe_for(
            CrafterKind::CraftingTable.recipe_chain(),
            &grid,
            &[],
        )
        .is_some(),
        "верстак должен видеть ручную связку палок"
    );
    assert!(
        book.matching_recipe_for(
            CrafterKind::SmithingTable.recipe_chain(),
            &grid,
            &[],
        )
        .is_none(),
        "кузнечный стол не должен видеть ручные рецепты"
    );
}

/// Общий рецепт еды проиндексирован у костра и печи одинаково.
#[test]
fn shared_food_recipe_is_visible_in_both_cookers() {
    use crate::simulation::crafts::{CraftBook, CrafterKind};
    use crate::simulation::items::ItemKind;

    let load = CraftBook::load_dir("data/crafts");
    assert!(load.errors.is_empty(), "ошибки рецептов: {:?}", load.errors);
    let book = load.book;

    let campfire = book
        .cooking_recipe(CrafterKind::Campfire, ItemKind::Apple)
        .expect("костёр должен жарить яблоко");
    let furnace = book
        .cooking_recipe(CrafterKind::Furnace, ItemKind::Apple)
        .expect("печь должна печь яблоко");
    assert_eq!(campfire.result, furnace.result);
    assert_eq!(campfire.id, furnace.id);
    assert!(book.cooking_recipe(CrafterKind::Manual, ItemKind::Apple).is_none());
}

/// Раскладка «папка не из parent» — предупреждение, но рецепт загружается.
#[test]
fn folder_mismatch_warns_but_recipe_loads() {
    use crate::simulation::crafts::{CraftBook, CrafterKind};
    use crate::simulation::items::ItemKind;
    use std::fs;

    let dir = std::env::temp_dir().join(format!("adem_crafts_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let folder = dir.join("furnace");
    fs::create_dir_all(&folder).expect("временная папка");
    fs::write(
        folder.join("apple.json"),
        r#"{
            "id": "mismatched",
            "parent": "campfire",
            "input": { "item": "apple", "count": 1 },
            "result": { "item": "apple_baked", "count": 1 }
        }"#,
    )
    .expect("временный рецепт");

    let load = CraftBook::load_dir(dir.to_str().unwrap());
    assert!(load.errors.is_empty(), "ошибки: {:?}", load.errors);
    assert_eq!(load.warnings.len(), 1, "ожидалось предупреждение о папке");
    assert_eq!(load.book.recipes.len(), 1);
    assert!(
        load.book
            .cooking_recipe(CrafterKind::Campfire, ItemKind::Apple)
            .is_some()
    );

    let _ = fs::remove_dir_all(&dir);
}
