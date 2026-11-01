//! Тесты игрока и его окружения: движение, инвентарь, добыча и сбор,
//! очередь команд симуляции.

use crate::simulation::dirty::DirtyTiles;
use crate::simulation::objects::{ObjectKind, WorldObject};
use crate::simulation::tile::TileType;
use crate::simulation::world::{TilePos, World, WorldSize};

/// Инвентарь: предметы складываются в стеки, переполнение уходит в новый
/// слот, лишнее возвращается вызывающему. can_fit учитывает и стеки, и
/// пустые слоты.
#[test]
fn inventory_add_stacks_and_overflows() {
    use crate::simulation::items::{Inventory, ItemKind, ItemStack, MAX_STACK, MISC_SLOTS};

    let mut inv = Inventory::default();
    assert_eq!(inv.add(ItemKind::Stick, 100), 0);
    // Сто палок: первый стек до MAX_STACK, остаток — во второй слот.
    assert_eq!(
        inv.items[0],
        Some(ItemStack { kind: ItemKind::Stick, count: MAX_STACK })
    );
    assert_eq!(
        inv.items[1],
        Some(ItemStack {
            kind: ItemKind::Stick,
            count: 100 - MAX_STACK
        })
    );
    assert_eq!(inv.count_of(ItemKind::Stick), 100);

    // Забитый инвентарь того же вида ничего не принимает.
    let mut full = Inventory::default();
    assert_eq!(full.add(ItemKind::Rock, MISC_SLOTS as u32 * MAX_STACK), 0);
    assert!(!full.can_fit(ItemKind::Rock, 1));
    assert_eq!(full.add(ItemKind::Rock, 1), 1);

    // Частично заполненный стек того же вида учитывается в can_fit.
    // Чужой вид место в стеке не занимает, но пустые слоты принимает:
    // остаётся 7 пустых слотов по MAX_STACK.
    let mut half = Inventory::default();
    half.add(ItemKind::Stick, MAX_STACK - 1);
    assert!(half.can_fit(ItemKind::Stick, MAX_STACK));
    assert!(half.can_fit(ItemKind::Rock, (MISC_SLOTS as u32 - 1) * MAX_STACK));
    assert!(!half.can_fit(ItemKind::Rock, MISC_SLOTS as u32 * MAX_STACK));

    // Ресурс мира превращается в предмет: камень и палка есть, руды — нет.
    use crate::simulation::ResourceKind;
    assert_eq!(
        ItemKind::from_resource(ResourceKind::Stone),
        Some(ItemKind::Rock)
    );
    assert_eq!(
        ItemKind::from_resource(ResourceKind::Stick),
        Some(ItemKind::Stick)
    );
    assert_eq!(ItemKind::from_resource(ResourceKind::Copper), None);
}

/// Предел стака по виду: инструменты и оружие занимают по слоту на
/// единицу (как в Minecraft), расходники доливаются в один стек.
#[test]
fn inventory_respects_per_kind_max_stack() {
    use crate::simulation::items::{
        Inventory, ItemKind, ItemStack, SlotArea, SlotRef, MAX_STACK,
    };

    // Три копья — три слота по единице, слияния в один стек нет.
    let mut inv = Inventory::default();
    assert_eq!(inv.add(ItemKind::Spear, 3), 0);
    assert_eq!(
        inv.items[0],
        Some(ItemStack { kind: ItemKind::Spear, count: 1 })
    );
    assert_eq!(
        inv.items[1],
        Some(ItemStack { kind: ItemKind::Spear, count: 1 })
    );
    assert_eq!(
        inv.items[2],
        Some(ItemStack { kind: ItemKind::Spear, count: 1 })
    );
    assert_eq!(inv.count_of(ItemKind::Spear), 3);

    // Больше слотов нет — остаток возвращается, can_fit согласован.
    assert_eq!(inv.add(ItemKind::Spear, 10), 5);
    assert!(!inv.can_fit(ItemKind::Spear, 1));

    // Расходники ведут как раньше: стрелы доливаются в один стек до 64.
    let mut stash = Inventory::default();
    assert_eq!(stash.add(ItemKind::Arrow, MAX_STACK + 1), 0);
    assert_eq!(
        stash.items[0],
        Some(ItemStack { kind: ItemKind::Arrow, count: MAX_STACK })
    );
    assert_eq!(stash.items[1], Some(ItemStack { kind: ItemKind::Arrow, count: 1 }));

    // Перенос единичного предмета между слотами и обмен с другим видом.
    let from = SlotRef { area: SlotArea::Misc, index: 0 };
    let to = SlotRef { area: SlotArea::Misc, index: 5 };
    assert!(stash.transfer(from, to));
    assert_eq!(stash.items[5], Some(ItemStack { kind: ItemKind::Arrow, count: MAX_STACK }));
    assert_eq!(stash.items[0], None);
}

/// Движение игрока: скорость 0.2 тайла/тик по каждому направлению,
/// глубокая вода непроходима с суши, но не мешает скользить вдоль берега
/// по другой оси; границы мира зажимаются.
#[test]
fn player_step_moves_and_respects_water() {
    use crate::simulation::player::{Player, WorldPos};
    use crate::simulation::tile::TileType;

    let mut world = World::new(WorldSize::new(256), 1);
    // Стена океана по x=10, тайлы y 4..8: глубже мелководья с суши не войти.
    for y in 4..8 {
        world.set_terrain(TilePos { x: 10, y }, TileType::Ocean);
    }

    // Прямое движение: 0.2 тайла за тик.
    let mut p = Player::new(WorldPos { x: 5.5, y: 5.5 });
    for _ in 0..10 {
        p.step((1.0, 0.0), &world);
    }
    assert!((p.pos.x - 7.5).abs() < 1e-4, "x = {}", p.pos.x);

    // Подход к глубокой воде вплотную: в тайл океана нога не ступает.
    let mut p = Player::new(WorldPos { x: 9.5, y: 5.5 });
    p.step((1.0, 0.0), &world);
    assert!((p.pos.x - 9.7).abs() < 1e-4);
    p.step((1.0, 0.0), &world);
    assert!((p.pos.x - 9.9).abs() < 1e-4);
    p.step((1.0, 0.0), &world);
    assert!((p.pos.x - 9.9).abs() < 1e-4, "океан блокирует: x = {}", p.pos.x);

    // Скольжение вдоль берега: по X стена, по Y движение продолжается.
    let mut p = Player::new(WorldPos { x: 9.9, y: 5.0 });
    p.step((1.0, 1.0), &world);
    assert!((p.pos.x - 9.9).abs() < 1e-4, "x упёрся в океан");
    assert!(p.pos.y > 5.0, "y идёт вдоль берега");

    // Границы мира: ноль не нарушается.
    let mut p = Player::new(WorldPos { x: 0.05, y: 0.05 });
    p.step((-1.0, -1.0), &world);
    assert!(p.pos.x >= 0.0 && p.pos.y >= 0.0);
    assert!(p.pos.x < 0.2, "за границу не вышел: x = {}", p.pos.x);
}

/// Вплавь по мелководью: войти с суши можно, скорость втрое ниже;
/// глубокая вода и океан с суши по-прежнему непроходимы.
#[test]
fn player_wades_shallow_water_only_and_slower() {
    use crate::simulation::player::{Player, WorldPos};
    use crate::simulation::tile::TileType;

    let mut world = World::new(WorldSize::new(256), 1);
    for y in 4..8 {
        world.set_terrain(TilePos { x: 10, y }, TileType::Water);
        world.set_terrain(TilePos { x: 30, y }, TileType::DeepWater);
        world.set_terrain(TilePos { x: 40, y }, TileType::Ocean);
    }

    // В мелководье входят с суши.
    let mut p = Player::new(WorldPos { x: 9.5, y: 5.5 });
    p.step((1.0, 0.0), &world);
    assert!((p.pos.x - 9.7).abs() < 1e-4);
    p.step((1.0, 0.0), &world);
    assert!((p.pos.x - 9.9).abs() < 1e-4);
    p.step((1.0, 0.0), &world);
    assert!(p.pos.x > 9.9, "мелководье проходимо: x = {}", p.pos.x);

    // Вплавь втрое медленнее: три тика на воде дают 3 × 0.2/3 = 0.2 тайла.
    let mut p = Player::new(WorldPos { x: 21.5, y: 5.5 });
    world.set_terrain(TilePos { x: 21, y: 5 }, TileType::Water);
    for _ in 0..3 {
        p.step((1.0, 0.0), &world);
    }
    assert!((p.pos.x - 21.7).abs() < 1e-4, "x = {}", p.pos.x);

    // Глубокая вода и океан с суши не проходимы.
    for wall_x in [30, 40] {
        let mut p = Player::new(WorldPos { x: wall_x as f32 - 0.5, y: 5.5 });
        for _ in 0..3 {
            p.step((1.0, 0.0), &world);
        }
        assert!(
            (p.pos.x - (wall_x as f32 - 0.1)).abs() < 1e-4,
            "x = {wall_x} не блокирует: x = {}",
            p.pos.x
        );
    }
}

/// Гора — общее правило мира: игрок замедляется вдвое так же, как существа.
#[test]
fn player_mountain_slows_down() {
    use crate::simulation::player::{Player, WorldPos};

    let mut world = World::new(WorldSize::new(256), 1);
    let mountain = TilePos { x: 50, y: 50 };
    world.set_terrain(mountain, TileType::Mountain);

    let mut p = Player::new(WorldPos::of_tile_center(mountain));
    p.step((1.0, 0.0), &world);
    assert!(
        (p.pos.x - 50.6).abs() < 1e-4,
        "на горе шаг должен быть вдвое короче, x = {}",
        p.pos.x
    );
}

/// Вброд в глубину не уйти: стоя на мелководье, игрок не может войти
/// в средневодную или океан; с глубины (размещён «богом») можно выбраться
/// на берег.
#[test]
fn player_cannot_swim_from_shallows_into_deep_water() {
    use crate::simulation::player::{Player, WorldPos};
    use crate::simulation::tile::TileType;

    let mut world = World::new(WorldSize::new(256), 1);
    // Полоса: суша x=9, мелководье x=10, средневодная x=11, океан x=12.
    for y in 4..8 {
        world.set_terrain(TilePos { x: 9, y }, TileType::Grass);
        world.set_terrain(TilePos { x: 10, y }, TileType::Water);
        world.set_terrain(TilePos { x: 11, y }, TileType::DeepWater);
        world.set_terrain(TilePos { x: 12, y }, TileType::Ocean);
    }

    // Стоя на мелководье, вплавь в глубину не входит.
    let mut p = Player::new(WorldPos { x: 10.8, y: 5.5 });
    for _ in 0..5 {
        p.step((1.0, 0.0), &world);
    }
    assert!(p.pos.x < 11.0, "средневодная должны быть недоступна: x = {}", p.pos.x);

    // «Бог» разместил игрока посреди океана: движение разрешено только
    // как выбирание на берег.
    let mut p = Player::new(WorldPos { x: 12.5, y: 5.5 });
    for _ in 0..30 {
        p.step((-1.0, 0.0), &world);
    }
    assert!(p.pos.x < 10.0, "с глубины можно выбраться на сушу: x = {}", p.pos.x);
    // ... но обратно в глубину с мелководья не вернуться.
    for _ in 0..10 {
        p.step((1.0, 0.0), &world);
    }
    assert!(p.pos.x < 11.0, "с мелководья в глубину не войти: x = {}", p.pos.x);
}

/// Инвентарь: перенос и обмен стеков между слотами с учётом типов слотов
/// (misc принимает любой предмет, чужой предмет в бронь не кладётся).
#[test]
fn inventory_transfer_respects_slot_types() {
    use crate::simulation::items::{Inventory, ItemKind, ItemStack, SlotArea, SlotRef, MAX_STACK};

    let refs = |area, index| SlotRef { area, index };
    let misc = SlotArea::Misc;

    // Перенос стека в пустой слот секции misc.
    let mut inv = Inventory::default();
    inv.add(ItemKind::Stick, 10);
    assert!(inv.transfer(refs(misc, 0), refs(misc, 3)));
    assert_eq!(inv.items[0], None);
    assert_eq!(inv.items[3], Some(ItemStack { kind: ItemKind::Stick, count: 10 }));

    // Обмен предметами между двумя занятыми слотами.
    inv.items[4] = Some(ItemStack { kind: ItemKind::Rock, count: 5 });
    assert!(inv.transfer(refs(misc, 3), refs(misc, 4)));
    assert_eq!(inv.items[3], Some(ItemStack { kind: ItemKind::Rock, count: 5 }));
    assert_eq!(inv.items[4], Some(ItemStack { kind: ItemKind::Stick, count: 10 }));

    // Долив в частично заполненный стек того же вида: цель дорастает до
    // предела, остаток возвращается в исходный слот.
    let mut inv2 = Inventory::default();
    inv2.items[0] = Some(ItemStack { kind: ItemKind::Rock, count: 3 });
    inv2.items[1] = Some(ItemStack { kind: ItemKind::Rock, count: MAX_STACK });
    assert!(inv2.transfer(refs(misc, 1), refs(misc, 0)));
    assert_eq!(inv2.items[0], Some(ItemStack { kind: ItemKind::Rock, count: MAX_STACK }));
    assert_eq!(inv2.items[1], Some(ItemStack { kind: ItemKind::Rock, count: 3 }));

    // Некуда доливать (целевой стек полон): перенос отклоняется без изменений.
    assert!(!inv2.transfer(refs(misc, 1), refs(misc, 0)));
    assert_eq!(inv2.items[1], Some(ItemStack { kind: ItemKind::Rock, count: 3 }));

    // Обмен стеков разных видов: misc-слот лоялен к любому предмету,
    // встречный обмен допускается.
    let mut inv3 = Inventory::default();
    inv3.add(ItemKind::Stick, 1);
    inv3.add(ItemKind::Rock, 1);
    assert!(inv3.transfer(refs(misc, 0), refs(misc, 1)));

    // Перенос одного и того же слот-адреса — ничего не делает.
    let mut inv4 = Inventory::default();
    inv4.add(ItemKind::Stick, 1);
    assert!(!inv4.transfer(refs(misc, 0), refs(misc, 0)));
    assert_eq!(inv4.count_of(ItemKind::Stick), 1);

    // Перенос из пустого слота — отказ.
    assert!(!inv4.transfer(refs(misc, 2), refs(misc, 0)));
}

/// Правила типов слотов: misc-предмет не кладётся в бронь, одежду и
/// косметику; предмет брони принимается только своим слотом (шлем не
/// надевается на ноги и тому подобное), одежды — только своим слотом одежды,
/// косметика — любой косметический слот.
#[test]
fn inventory_slot_type_rules() {
    use crate::simulation::items::{
        BodySlot, Inventory, ItemClass, ItemKind, SlotArea, SlotRef, ARMOR_LAYOUT, BAUBLE_SLOTS,
        CLOTHES_LAYOUT,
    };

    let misc = SlotArea::Misc;
    let armor = SlotArea::Armor;
    let clothes = SlotArea::Clothes;
    let bauble = SlotArea::Bauble;

    // Каждый предмет принимается только слотами своего класса.
    for kind in ItemKind::ALL {
        match kind.class() {
            ItemClass::Misc => {
                assert!(!Inventory::accepts(kind, armor, 0));
                assert!(!Inventory::accepts(kind, clothes, 0));
                assert!(!Inventory::accepts(kind, bauble, 0));
                assert!(Inventory::accepts(kind, misc, 0), "misc принимает любой предмет");
            }
            ItemClass::Armor(slot) => {
                // Свой слот принимает, чужие — нет.
                for (index, layout_slot) in ARMOR_LAYOUT.iter().enumerate() {
                    let want = *layout_slot == slot;
                    assert_eq!(
                        Inventory::accepts(kind, armor, index),
                        want,
                        "{kind:?} в слоте {layout_slot:?}"
                    );
                }
                assert!(!Inventory::accepts(kind, clothes, 0));
                assert!(!Inventory::accepts(kind, bauble, 0));
            }
            ItemClass::Clothes(slot) => {
                // Свой слот одежды принимает, чужие — нет.
                for (index, layout_slot) in CLOTHES_LAYOUT.iter().enumerate() {
                    let want = *layout_slot == slot;
                    assert_eq!(
                        Inventory::accepts(kind, clothes, index),
                        want,
                        "{kind:?} в слоте одежды {layout_slot:?}"
                    );
                }
                assert!(!Inventory::accepts(kind, armor, 0));
            }
            ItemClass::Bauble => {
                assert!(!Inventory::accepts(kind, armor, 0));
                assert!(!Inventory::accepts(kind, clothes, 0));
                assert!(Inventory::accepts(kind, bauble, 0));
            }
        }
    }

    // Кольчуга: защита слотов — шлем/кираса/поножи/ботинки по порядку.
    assert!(Inventory::accepts(ItemKind::HelmetChainmail, armor, 0));
    assert!(Inventory::accepts(ItemKind::ChestplateChainmail, armor, 1));
    assert!(Inventory::accepts(ItemKind::LeggingsChainmail, armor, 2));
    assert!(Inventory::accepts(ItemKind::BootsChainmail, armor, 3));
    assert_eq!(ARMOR_LAYOUT[0], BodySlot::Head);
    assert_eq!(ARMOR_LAYOUT[3], BodySlot::Boots);

    // Одежда делится по тем же частям тела, что и броня.
    assert_eq!(CLOTHES_LAYOUT, ARMOR_LAYOUT, "части тела одежды и брони");
    assert_eq!(clothes.slots_count(), 4, "слотов одежды у разумного существа");

    // Перенос misc-предмета в слот брони, одежды и косметики — отказ без
    // изменений.
    let mut inv = Inventory::default();
    inv.add(ItemKind::Stick, 1);
    let (from, to_armor) = (SlotRef { area: misc, index: 0 }, SlotRef { area: armor, index: 0 });
    let to_clothes = SlotRef { area: clothes, index: 0 };
    let to_bauble = SlotRef { area: bauble, index: BAUBLE_SLOTS - 1 };
    assert!(!inv.transfer(from, to_armor));
    assert_eq!(inv.count_of(ItemKind::Stick), 1, "предмет не пропал");
    assert!(!inv.transfer(from, to_clothes));
    assert_eq!(inv.count_of(ItemKind::Stick), 1, "предмет не пропал");
    assert!(!inv.transfer(from, to_bauble));
    assert_eq!(inv.count_of(ItemKind::Stick), 1, "предмет не пропал");

    // Шлем в слот поножей — отказ без изменений.
    inv.items[0] = Some(crate::simulation::ItemStack {
        kind: ItemKind::HelmetChainmail,
        count: 1,
    });
    let to_legs = SlotRef { area: armor, index: 2 };
    assert!(!inv.transfer(from, to_legs));
    assert_eq!(inv.count_of(ItemKind::HelmetChainmail), 1, "шлем не пропал");

    // Адрес за границей секции отклонён.
    assert!(!Inventory::accepts(ItemKind::Stick, misc, 8));
    assert!(!Inventory::accepts(ItemKind::Rock, clothes, 4));
    assert!(!Inventory::accepts(ItemKind::Rock, bauble, BAUBLE_SLOTS));
}

/// Голод тратится по активности: стояние — медленнее ходьбы, плавание —
/// быстрее всего. Тайл воды под ногами считается плаванием.
#[test]
fn hunger_drains_by_activity() {
    use crate::simulation::hunger::{Hunger, HUNGER_MAX};
    use crate::simulation::player::{Player, WorldPos};
    use crate::simulation::tile::TileType;

    let mut world = World::new(WorldSize::new(256), 1);
    let water = TilePos { x: 10, y: 10 };
    world.set_terrain(water, TileType::Water);
    // Суша под «стоящим» и «идущим» игроком: тайл сгенерированного мира
    // мог бы оказаться водой и испортить замер.
    let land = TilePos { x: 20, y: 20 };
    world.set_terrain(land, TileType::Grass);

    // Стояние: 0.0005 за тик (допуск с запасом на f32-накопление).
    let mut p = Player::new(WorldPos::of_tile_center(land));
    for _ in 0..100 {
        p.tick_hunger(false, &world);
    }
    assert!((p.hunger.value() - (HUNGER_MAX - 0.05)).abs() < 1e-3, "hunger = {}", p.hunger.value());

    // Ходьба: 0.004 за тик.
    let mut p = Player::new(WorldPos::of_tile_center(land));
    for _ in 0..100 {
        p.tick_hunger(true, &world);
    }
    assert!((p.hunger.value() - (HUNGER_MAX - 0.4)).abs() < 1e-3, "hunger = {}", p.hunger.value());

    // Плавание: 0.006 за тик (в т.ч. без движения — тайл решает).
    let mut p = Player::new(WorldPos::of_tile_center(water));
    for _ in 0..100 {
        p.tick_hunger(false, &world);
    }
    assert!((p.hunger.value() - (HUNGER_MAX - 0.6)).abs() < 1e-3, "hunger = {}", p.hunger.value());

    // Сытость не уходит в минус.
    p.hunger = Hunger::with_value(0.001);
    for _ in 0..100 {
        p.tick_hunger(false, &world);
    }
    assert_eq!(p.hunger.value(), 0.0);
}

/// Голод ниже порога бьёт по здоровью раз в 50 тиков; еда восстанавливает
/// сытость и сбрасывает счётчик урона.
#[test]
fn starvation_damages_and_eating_restores() {
    use crate::simulation::hunger::Hunger;
    use crate::simulation::items::ItemKind;
    use crate::simulation::player::{Player, WorldPos};

    let mut world = World::new(WorldSize::new(256), 1);
    let land = TilePos { x: 20, y: 20 };
    world.set_terrain(land, TileType::Grass);
    let mut p = Player::new(WorldPos::of_tile_center(land));
    p.hunger = Hunger::with_value(4.0);

    // 49 тиков голода — ещё без урона; на 50-м — первый удар.
    for _ in 0..49 {
        p.tick_hunger(false, &world);
    }
    assert!((p.hp - 100.0).abs() < 1e-4, "hp = {}", p.hp);
    p.tick_hunger(false, &world);
    assert!((p.hp - 99.0).abs() < 1e-4, "hp = {}", p.hp);

    // Ещё 50 тиков голода — второй удар.
    for _ in 0..50 {
        p.tick_hunger(false, &world);
    }
    assert!((p.hp - 98.0).abs() < 1e-4, "hp = {}", p.hp);

    // Яблоко в выбранном слоте: +10 сытости (4.0 − 100·0.0005 = 3.95), предмет списан.
    p.inventory.add(ItemKind::Apple, 2);
    p.selected_slot = 0;
    assert!(p.eat_selected());
    assert_eq!(p.inventory.count_of(ItemKind::Apple), 1);
    assert!((p.hunger.value() - 13.95).abs() < 1e-3, "hunger = {}", p.hunger.value());

    // Сытость выше порога: урона нет, счётчик сброшен.
    for _ in 0..50 {
        p.tick_hunger(false, &world);
    }
    assert!((p.hp - 98.0).abs() < 1e-4, "hp = {}", p.hp);

    // Не еда в руке — есть нельзя.
    p.inventory.items[0] = None;
    p.inventory.items[1] = Some(crate::simulation::ItemStack {
        kind: ItemKind::Stick,
        count: 1,
    });
    p.selected_slot = 1;
    assert!(!p.eat_selected());

    // Сытость зажимается максимумом.
    p.inventory.items[1] = Some(crate::simulation::ItemStack {
        kind: ItemKind::Apple,
        count: 1,
    });
    p.hunger = Hunger::with_value(99.0);
    assert!(p.eat_selected());
    assert!((p.hunger.value() - 100.0).abs() < 1e-4, "hunger = {}", p.hunger.value());
}

/// Команда PlayerEat ест яблоко из выбранного слота, не еда — игнор,
/// кулдаун (50 тиков) отклоняет слишком частые попытки.
#[test]
fn command_player_eat_consumes_apple() {
    use crate::simulation::commands::{Command, CommandQueue};
    use crate::simulation::hunger::Hunger;
    use crate::simulation::items::ItemKind;
    use crate::simulation::player::{Player, WorldPos};

    let mut world = World::new(WorldSize::new(256), 1);
    let mut queue = CommandQueue::default();
    let mut sched = crate::simulation::Scheduler::default();
    let mut p = Player::new(WorldPos { x: 20.5, y: 20.5 });
    p.inventory.add(ItemKind::Apple, 3);
    p.selected_slot = 0;
    p.hunger = Hunger::with_value(50.0);

    // Две команды на одном тике: вторая отбита кулдауном.
    queue.push(Command::PlayerEat);
    queue.push(Command::PlayerEat);
    let mut dirty = DirtyTiles::default();
    queue.apply_all(&mut world, Some(&mut p), &mut dirty, &mut sched, 0);

    assert_eq!(p.inventory.count_of(ItemKind::Apple), 2, "второй укус отброшен кулдауном");
    assert!((p.hunger.value() - 60.0).abs() < 1e-4, "hunger = {}", p.hunger.value());

    // На тике 49 ещё рано (команда применена, но отброшена кулдауном),
    // на тике 50 — можно: снова кладём команду в очередь.
    queue.push(Command::PlayerEat);
    queue.apply_all(&mut world, Some(&mut p), &mut dirty, &mut sched, 49);
    assert_eq!(p.inventory.count_of(ItemKind::Apple), 2);
    queue.push(Command::PlayerEat);
    queue.apply_all(&mut world, Some(&mut p), &mut dirty, &mut sched, 50);
    assert_eq!(p.inventory.count_of(ItemKind::Apple), 1);

    // Не еда в руке: команда ничего не меняет.
    p.inventory.items[0] = Some(crate::simulation::ItemStack {
        kind: ItemKind::Rock,
        count: 1,
    });
    let hunger_before = p.hunger.value();
    queue.push(Command::PlayerEat);
    queue.apply_all(&mut world, Some(&mut p), &mut dirty, &mut sched, 500);
    assert_eq!(p.hunger.value(), hunger_before);
    assert_eq!(p.inventory.count_of(ItemKind::Rock), 1);
}

/// Мусорка принимает что угодно, каждая новая кладка затирает старый стек
/// (удаление), а предмет из мусорки ещё можно забрать обратно. В обычные
/// обходы инвентаря (`count_of`) мусорка не входит.
#[test]
fn trash_slot_overwrites_and_allows_retrieval() {
    use crate::simulation::items::{Inventory, ItemKind, ItemStack, SlotArea, SlotRef};

    let trash = SlotRef { area: SlotArea::Trash, index: 0 };
    let misc0 = SlotRef { area: SlotArea::Misc, index: 0 };
    let misc1 = SlotRef { area: SlotArea::Misc, index: 1 };

    let mut inv = Inventory::default();
    inv.add(ItemKind::Stick, 10);
    inv.add(ItemKind::Rock, 5);

    // Кладём палки в мусорку: стек уходит целиком.
    assert!(inv.transfer(misc0, trash));
    assert_eq!(inv.trash, Some(ItemStack { kind: ItemKind::Stick, count: 10 }));
    assert_eq!(inv.count_of(ItemKind::Stick), 0, "в мусорке предмет не считается");

    // Кладём камни поверх: старый стек затёрт.
    assert!(inv.transfer(misc1, trash));
    assert_eq!(inv.trash, Some(ItemStack { kind: ItemKind::Rock, count: 5 }));
    assert_eq!(inv.count_of(ItemKind::Rock), 0);

    // Забрать случайно положенное можно обратно.
    assert!(inv.transfer(trash, misc0));
    assert_eq!(inv.items[0], Some(ItemStack { kind: ItemKind::Rock, count: 5 }));
    assert_eq!(inv.trash, None);
    assert_eq!(inv.count_of(ItemKind::Rock), 5);

    // Мусорка принимает любой предмет (в том числе всё, что не бронь/косметика).
    for kind in ItemKind::ALL {
        assert!(Inventory::accepts(kind, SlotArea::Trash, 0));
    }
    // Слот мусорки один: индекс 1 отклонён.
    assert!(!Inventory::accepts(ItemKind::Stick, SlotArea::Trash, 1));
}

/// Голод и инвентарь: предметы в мусорке не тратятся и не съедаются.
#[test]
fn trash_items_are_not_consumed() {
    use crate::simulation::items::{Inventory, ItemKind, SlotArea, SlotRef};

    let mut inv = Inventory {
        trash: Some(crate::simulation::ItemStack { kind: ItemKind::Apple, count: 5 }),
        ..Default::default()
    };
    assert_eq!(inv.count_of(ItemKind::Apple), 0, "в мусорке предметы не считаются");
    assert!(!inv.take_items(ItemKind::Apple, 1), "из мусорки не платят");
    // Забрать из мусорки можно только переносом слота.
    assert_eq!(
        inv.get(SlotRef { area: SlotArea::Trash, index: 0 }),
        Some(crate::simulation::ItemStack { kind: ItemKind::Apple, count: 5 })
    );
}

/// Рендер интерполирует движение между тиками: после шага позиция кадра
/// лежит между прошлой и текущей, а без движения сегмент схлопывается.
#[test]
fn player_render_pos_interpolates_and_settles() {
    use crate::app::state::{TickAccumulator, player_render_pos};
    use crate::simulation::player::{Player, WorldPos};

    let world = World::new(WorldSize::new(256), 1);
    let mut p = Player::new(WorldPos { x: 5.0, y: 5.0 });
    p.step((1.0, 0.0), &world);
    assert_eq!(p.pos.x, 5.2);

    // Половина тика: ровно середина отрезка прошлого шага.
    let half = TickAccumulator { seconds: 0.025 };
    let mid = player_render_pos(&p, &half);
    assert!((mid.x - 5.1).abs() < 1e-4, "x = {}", mid.x);

    // Аккумулятор за пределами шага зажимается: не улетаем за позицию тика.
    let over = TickAccumulator { seconds: 1.0 };
    let end = player_render_pos(&p, &over);
    assert!((end.x - 5.2).abs() < 1e-4);

    // Стоящий игрок: сегмент схлопнут, позиция кадра не дрейфует обратно
    // к точке старта шага.
    let mut still = Player::new(WorldPos { x: 5.0, y: 5.0 });
    still.step((1.0, 0.0), &world);
    still.step((0.0, 0.0), &world);
    let frozen = player_render_pos(&still, &half);
    assert_eq!((frozen.x, frozen.y), (5.2, 5.0));
}

/// Добыча: куст ломается и даёт палку, у валуна падает запас и он исчезает
/// на нуле; дальний тайл действие отменяет.
#[test]
fn player_interact_mines_objects() {
    use crate::simulation::items::ItemKind;
    use crate::simulation::objects::WorldObject;
    use crate::simulation::player::{Player, WorldPos};
    use crate::simulation::tile::TileType;

    let mut world = World::new(WorldSize::new(256), 1);
    let pos = TilePos { x: 10, y: 10 };
    world.set_terrain(pos, TileType::Grass);
    world.set_object(pos, Some(WorldObject::rock(2)));

    // Соседний тайл — в радиусе взаимодействия. Для добычи камня нужна палка в руке.
    let mut p = Player::new(WorldPos::of_tile_center(TilePos { x: 11, y: 11 }));
    p.inventory.add(ItemKind::Stick, 1);
    p.selected_slot = 0;
    assert!(p.interact(pos, &mut world));
    assert_eq!(p.inventory.count_of(ItemKind::Rock), 1);
    assert_eq!(world.object_at(pos).unwrap().charge, 1);
    assert!(p.interact(pos, &mut world));
    assert_eq!(p.inventory.count_of(ItemKind::Rock), 2);
    assert!(world.object_at(pos).is_none(), "пустой валун исчезает");

    // Куст ломается и даёт две палки.
    let bush_pos = TilePos { x: 12, y: 10 };
    world.set_object(bush_pos, Some(WorldObject::bush()));
    assert!(p.interact(bush_pos, &mut world));
    assert_eq!(p.inventory.count_of(ItemKind::Stick), 3);
    assert!(world.object_at(bush_pos).is_none());

    // Дальний тайл — отказ, объект не тронут.
    let far = TilePos { x: 20, y: 10 };
    world.set_object(far, Some(WorldObject::bush()));
    assert!(!p.interact(far, &mut world));
    assert!(world.object_at(far).is_some());

    // Тайл без объекта — отказ.
    assert!(!p.interact(TilePos { x: 11, y: 10 }, &mut world));
}

/// Полный инвентарь: добыча невозможна и не расходует объект — ресурс
/// мира не должен пропадать «в никуда».
#[test]
fn player_interact_blocked_when_inventory_full() {
    use crate::simulation::items::{Inventory, ItemKind, ItemStack, MAX_STACK, MISC_SLOTS};
    use crate::simulation::objects::WorldObject;
    use crate::simulation::player::{Player, WorldPos};

    let mut world = World::new(WorldSize::new(256), 1);
    let pos = TilePos { x: 10, y: 10 };
    world.set_object(pos, Some(WorldObject::rock(2)));

    let mut p = Player::new(WorldPos::of_tile_center(TilePos { x: 11, y: 11 }));
    let mut full = Inventory::default();
    for slot in full.items.iter_mut() {
        *slot = Some(ItemStack { kind: ItemKind::Rock, count: MAX_STACK });
    }
    p.inventory = full;
    assert_eq!(MISC_SLOTS, p.inventory.items.len());

    assert!(!p.interact(pos, &mut world));
    assert_eq!(world.object_at(pos).unwrap().charge, 2, "валун не тронут");
    assert_eq!(p.inventory.count_of(ItemKind::Rock), MISC_SLOTS as u32 * MAX_STACK);
}

/// Команды применяются в порядке поступления; изменённые тайлы уходят
/// в dirty-список для рендера; команды с тайлами вне мира пропускаются.
#[test]
fn commands_apply_fifo_and_report_dirty() {
    use crate::simulation::commands::{Command, CommandQueue};
    use crate::simulation::objects::WorldObject;
    use crate::simulation::tile::TileType;

    let mut world = World::new(WorldSize::new(256), 1);
    let mut queue = CommandQueue::default();
    let mut sched = crate::simulation::Scheduler::default();
    let a = TilePos { x: 3, y: 4 };
    let b = TilePos { x: 5, y: 6 };
    queue.push(Command::SetTerrain { pos: a, kind: TileType::Sand });
    queue.push(Command::SetObject { pos: b, object: Some(WorldObject::bush()) });
    // Вне мира — пропускается, но считается применённой.
    queue.push(Command::SetTerrain { pos: TilePos { x: -1, y: 0 }, kind: TileType::Sand });

    let mut dirty = DirtyTiles::default();
    let applied = queue.apply_all(&mut world, None, &mut dirty, &mut sched, 0);
    assert_eq!(applied, 3);
    assert_eq!(world.terrain_kind(a), TileType::Sand);
    assert!(world.object_at(b).is_some());
    // Канал причины: рельеф и объект попадают в разные списки рендера.
    assert_eq!(dirty.terrain, vec![a]);
    assert_eq!(dirty.decor, vec![b]);
    assert!(queue.is_empty());
}

/// Сброс стека (остатков сетки крафта) на землю: стек того же вида доливается
/// в лежащую стопку, стек другого вида ложится на ближайший свободный тайл
/// по кольцам вокруг указанного. Предмет не должен пропадать «в никуда»,
/// когда тайл под игроком занят.
#[test]
fn command_drop_stack_finds_free_tile() {
    use crate::simulation::commands::{Command, CommandQueue};
    use crate::simulation::items::{ItemKind, ItemStack};

    let mut world = World::new(WorldSize::new(256), 1);
    let mut queue = CommandQueue::default();
    let mut sched = crate::simulation::Scheduler::default();
    let center = TilePos { x: 10, y: 10 };

    // На центральном тайле лежат палки: доливка того же вида, обход чужого.
    world.set_item(center, Some(ItemStack { kind: ItemKind::Stick, count: 10 }));

    queue.push(Command::PlayerDropStack {
        pos: center,
        stack: ItemStack { kind: ItemKind::Stick, count: 5 },
    });
    queue.push(Command::PlayerDropStack {
        pos: center,
        stack: ItemStack { kind: ItemKind::Rock, count: 3 },
    });

    let mut dirty = DirtyTiles::default();
    queue.apply_all(&mut world, None, &mut dirty, &mut sched, 0);

    // Палки долились в центральную стопку.
    assert_eq!(world.item_at(center).map(|s| (s.kind, s.count)), Some((ItemKind::Stick, 15)));
    // Камни легли на первый свободный тайл кольца радиуса 1: обход кольца
    // начинается с (dx, dy) = (-1, -1), то есть тайл (9, 9).
    assert_eq!(
        world.item_at(TilePos { x: 9, y: 9 }).map(|s| (s.kind, s.count)),
        Some((ItemKind::Rock, 3))
    );
    // Оба изменённых тайла ушли в decor-канал рендера (предметы на земле).
    assert!(dirty.decor.contains(&center));
    assert!(dirty.decor.contains(&TilePos { x: 9, y: 9 }));
}

/// Сброс стека не теряет предметы, когда всё вокруг занято чужими видами:
/// поиск по кольцам расширяется до края мира.
#[test]
fn command_drop_stack_searches_until_free_tile() {
    use crate::simulation::commands::{Command, CommandQueue};
    use crate::simulation::items::{ItemKind, ItemStack};

    let mut world = World::new(WorldSize::new(256), 1);
    let mut queue = CommandQueue::default();
    let mut sched = crate::simulation::Scheduler::default();

    // Замусориваем кольца радиусами 1–2 вокруг центра камнями.
    let center = TilePos { x: 20, y: 20 };
    for dy in -2..=2_i32 {
        for dx in -2..=2_i32 {
            let pos = TilePos { x: center.x + dx, y: center.y + dy };
            world.set_item(pos, Some(ItemStack { kind: ItemKind::Rock, count: 1 }));
        }
    }

    queue.push(Command::PlayerDropStack {
        pos: center,
        stack: ItemStack { kind: ItemKind::Stick, count: 2 },
    });
    let mut dirty = DirtyTiles::default();
    queue.apply_all(&mut world, None, &mut dirty, &mut sched, 0);

    // Первый свободный тайл — на кольце радиуса 3, обход начинается
    // с угла (dx, dy) = (-3, -3).
    assert_eq!(
        world.item_at(TilePos { x: 17, y: 17 }).map(|s| (s.kind, s.count)),
        Some((ItemKind::Stick, 2))
    );
}

/// Каждое действие собирает с яблони одно яблоко; пустое дерево остаётся
/// объектом и больше не даёт плодов.
#[test]
fn apple_harvest_gives_one_fruit_per_action() {
    use crate::simulation::items::ItemKind;
    use crate::simulation::player::{Player, WorldPos};

    let mut world = World::new(WorldSize::new(256), 1);
    let pos = TilePos { x: 10, y: 10 };
    world.set_terrain(pos, TileType::Grass);
    world.set_object(pos, Some(WorldObject::apple_tree_with_fruit(7)));

    let mut player = Player::new(WorldPos::of_tile_center(pos));
    for remaining in (0..7).rev() {
        assert!(player.harvest_apple(pos, &mut world));
        assert_eq!(
            player.inventory.count_of(ItemKind::Apple),
            u32::from(7 - remaining)
        );
        let tree = world.object_at(pos).copied().expect("яблоня остаётся");
        assert_eq!(tree.kind, ObjectKind::AppleTree);
        assert_eq!(tree.charge, remaining);
    }
    assert!(!player.harvest_apple(pos, &mut world), "плодов больше нет");
    assert_eq!(player.inventory.count_of(ItemKind::Apple), 7);
}

/// Сбор травы пустой рукой даёт ровно один пучок и превращает тайл в землю.
/// Предмет в руке (кроме ножа) сбор блокирует.
#[test]
fn grass_gathering_yields_one_and_leaves_dirt() {
    use crate::simulation::items::ItemKind;
    use crate::simulation::player::{Player, WorldPos};

    let mut world = World::new(WorldSize::new(256), 42);
    let pos = TilePos { x: 20, y: 20 };
    world.set_terrain(pos, TileType::Grass);
    let mut player = Player::new(WorldPos::of_tile_center(pos));

    assert!(player.interact(pos, &mut world));
    let collected = player.inventory.count_of(ItemKind::GrassBunch);
    assert_eq!(collected, 1, "за один тайл выдаётся ровно один пучок");
    assert_eq!(world.terrain_kind(pos), TileType::Dirt);
    assert!(!player.interact(pos, &mut world), "с земли трава не берётся");

    // С палкой в руке трава не собирается: нужна пустая рука или нож.
    let blocked = TilePos { x: 21, y: 20 };
    world.set_terrain(blocked, TileType::ForestGrass);
    player.inventory.items[0] = Some(crate::simulation::ItemStack {
        kind: ItemKind::Stick,
        count: 1,
    });
    player.selected_slot = 0;
    assert!(!player.interact(blocked, &mut world));
    assert_eq!(world.terrain_kind(blocked), TileType::ForestGrass);

    // С ножом — можно.
    player.inventory.items[0] = Some(crate::simulation::ItemStack {
        kind: ItemKind::Knife,
        count: 1,
    });
    assert!(player.interact(blocked, &mut world));
    assert_eq!(world.terrain_kind(blocked), TileType::Dirt);
}

/// Реестр предметов: дискриминанты совпадают с порядком `ALL`, строковые id
/// уникальны, а обратные преобразования (`from_id`, `from_discriminant`)
/// возвращают тот же вид. Проверка ловит рассинхрон массивов иконок/текстур.
#[test]
fn item_registry_is_consistent() {
    use crate::simulation::items::{ItemKind, ITEM_KIND_COUNT};
    use std::collections::HashSet;

    assert_eq!(ItemKind::ALL.len(), ITEM_KIND_COUNT);
    let mut ids = HashSet::new();
    for (index, kind) in ItemKind::ALL.into_iter().enumerate() {
        assert_eq!(kind as usize, index, "дискриминант {kind:?} не совпадает с ALL");
        assert!(ids.insert(kind.id()), "дубликат строкового id: {}", kind.id());
        assert_eq!(ItemKind::from_id(kind.id()), Some(kind));
        assert_eq!(ItemKind::from_discriminant(kind as u8), Some(kind));
    }
    assert_eq!(ItemKind::from_discriminant(ITEM_KIND_COUNT as u8), None);
}
