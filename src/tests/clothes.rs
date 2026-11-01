//! Тесты системы одежды: тепло предметов, сбор тепла только из надетых
//! слотов одежды и правила слотов одежды в инвентаре.

use crate::simulation::clothes::{warmth, warmth_of};
use crate::simulation::items::{Inventory, ItemKind, ItemStack, SlotArea, SlotRef};

/// Пока предметов одежды нет, тепло у всех существующих предметов
/// нулевое (при появлении первой одежды тест уточняется по частям).
#[test]
fn no_clothes_items_have_no_warmth() {
    for kind in ItemKind::ALL {
        assert_eq!(warmth_of(kind), 0.0, "{kind:?} — не одежда");
    }
}

/// Тепло собирается только из надетых слотов одежды: пустой инвентарь и
/// предметы в misc/мусорке тепла не дают. Прямая кладка в слот одежды
/// (механизм `put` минует `accepts`) учитывается — но пока предметов
/// одежды нет, любой предмет даёт 0.0.
#[test]
fn warmth_counts_equipped_clothes_slots_only() {
    let mut inv = Inventory::default();
    assert_eq!(warmth(&inv), 0.0, "пустой инвентарь — тепла нет");

    // Предметы в основных слотах и мусорке не надеты.
    inv.add(ItemKind::Stick, 3);
    inv.trash = Some(ItemStack { kind: ItemKind::Rock, count: 1 });
    assert_eq!(warmth(&inv), 0.0);

    // Слот одежды с не-одеждой (кладка напрямую, мимо `accepts`) тоже
    // даёт ноль: тепло считается по `warmth_of`.
    inv.put(
        SlotRef { area: SlotArea::Clothes, index: 2 },
        ItemStack { kind: ItemKind::Apple, count: 1 },
    );
    assert_eq!(warmth(&inv), 0.0, "яблоко в слоте одежды не греет");
}
