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
            "grid": [
                ["stick", null, null],
                ["stick", null, null],
                [null, null, null]
            ],
            "result": { "item": "stick", "count": 2 }
        }"#,
    )
    .expect("рецепт должен разбираться");
    assert!(!recipe.shapeless);
    let pattern = recipe.pattern.as_ref().unwrap();
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
            "shapeless": true,
            "ingredients": [{ "item": "grass_bunch", "count": 2 }],
            "result": { "item": "plant_fiber", "count": 1 }
        }"#,
    )
    .expect("рецепт должен разбираться");
    assert!(recipe.shapeless);

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

/// Все рецепты из `data/crafts` читаются без ошибок: корень — ручная
/// категория, подпапки — категории станций, рецепты с инструментами
/// (кузнечный стол) читаются.
#[test]
fn craft_book_loads_all_recipe_files() {
    use crate::simulation::crafts::{CraftBook, MANUAL_CATEGORY};
    use crate::simulation::items::ItemKind;

    let (book, errors) = CraftBook::load_dir("data/crafts");
    assert!(errors.is_empty(), "ошибки рецептов: {errors:?}");
    // 14 ручных рецептов + рецепты станций (верстак, печи, костёр,
    // кузнечный стол; у костра — 8 рецептов жарки еды).
    assert_eq!(book.recipes.len(), 14 + 34);

    let manual_ids: Vec<&str> = book
        .recipes
        .iter()
        .filter(|recipe| recipe.category == MANUAL_CATEGORY)
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
            "spear",
            "stick_bundle",
            "stick_bundle_unpack",
        ]
    );
    assert!(
        book.recipes
            .iter()
            .any(|recipe| recipe.result.kind == ItemKind::FishingRod)
    );
    // Станционные категории присутствуют, у кузнечного стола есть рецепты
    // с требованием инструмента.
    assert!(book
        .recipes
        .iter()
        .any(|recipe| recipe.category == "furnace"));
    assert!(book
        .recipes
        .iter()
        .any(|recipe| recipe.category == "blast_furnace"));
    assert!(book
        .recipes
        .iter()
        .any(|recipe| recipe.category == "campfire"));
    assert!(book
        .recipes
        .iter()
        .any(|recipe| recipe.category == "crafting_table"));
    assert!(book
        .recipes
        .iter()
        .any(|recipe| recipe.category == "smithing_table" && !recipe.tools.is_empty()));
}

/// Связка палок: 8 палок → связка и обратно, оба рецепта ручные.
#[test]
fn stick_bundle_recipe_is_bidirectional() {
    use crate::simulation::crafts::{CraftBook, GRID_SLOTS};
    use crate::simulation::items::{Inventory, ItemKind};
    use crate::simulation::ItemStack;

    let (book, errors) = CraftBook::load_dir("data/crafts");
    assert!(errors.is_empty(), "ошибки рецептов: {errors:?}");

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
