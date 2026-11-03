//! Инвентарь и слоты (GAME_DESIGN 3.4/3.7): секции слотов (misc, броня,
//! одежда, косметика), стеки, правила размещения. Каталог видов предметов
//! (`ItemKind`, инструменты/оружие) — в `catalog.rs`; правила совместимости
//! слотов — `accepts`/`transfer` здесь.

// Каталог видов предметов — реэкспорт, чтобы путь `items::ItemKind`
// (и вся внешняя поверхность модуля) не изменился.
mod catalog;
pub use catalog::{
    HAND_ATTACK, ITEM_KIND_COUNT, ItemKind, ToolClass,
};


/// Класс предмета — правило размещения по слотам.
#[allow(dead_code)] // Clothes и Bauble появятся вместе с первыми предметами этих классов
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemClass {
    /// Обычный предмет: лежит в основных слотах `misc`.
    Misc,
    /// Броня конкретной части тела: свой слот брони (misc тоже принимает —
    /// «в основном слоте может лежать что угодно»).
    Armor(BodySlot),
    /// Одежда конкретной части тела: свой слот одежды (misc тоже
    /// принимает). Защита от среды (холод и тому подобное), не карман.
    Clothes(BodySlot),
    /// Косметика: любой из слотов косметики (misc тоже принимает).
    Bauble,
}

/// Часть тела — специализация слота и предмета снаряжения (броня, одежда).
/// Порядок первых четырёх — порядок слотов брони и одежды разумного
/// существа (GAME_DESIGN 3.4: head, chest, legs, boots); `Body` — единая
/// специализация на всё тело для неразумных существ.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum BodySlot {
    Head,
    Chest,
    Legs,
    Boots,
    /// Неразумные существа: единое снаряжение на тело (без деления по частям).
    Body,
}

/// Максимум единиц одного предмета в слоте.
pub const MAX_STACK: u32 = 64;

/// Число слотов предметов (misc) разумного существа (GAME_DESIGN 3.4: 8).
pub const MISC_SLOTS: usize = 8;
/// Число слотов брони разумного существа (GAME_DESIGN 3.4: 4).
pub const ARMOR_SLOTS: usize = 4;
/// Число слотов одежды разумного существа: по части тела, как у брони.
pub const CLOTHES_SLOTS: usize = 4;
/// Число слотов косметики разумного существа (GAME_DESIGN 3.4: 4).
pub const BAUBLE_SLOTS: usize = 4;

/// Части тела слотов брони разумного существа — порядок фиксирован:
/// head, chest, legs, boots. Индекс в секции брони = индекс в этом списке.
pub const ARMOR_LAYOUT: [BodySlot; ARMOR_SLOTS] =
    [BodySlot::Head, BodySlot::Chest, BodySlot::Legs, BodySlot::Boots];

/// Части тела слотов одежды разумного существа — совпадает с `ARMOR_LAYOUT`
/// (одежда делится по тем же частям тела, что и броня).
pub const CLOTHES_LAYOUT: [BodySlot; CLOTHES_SLOTS] =
    [BodySlot::Head, BodySlot::Chest, BodySlot::Legs, BodySlot::Boots];

/// Специализации слотов брони неразумного существа (GAME_DESIGN 3.4:
/// «1 слот инвентаря + 2 слота особой брони»: head и body). Задел на
/// будущих существ; инвентарь игрока пока разумный.
#[allow(dead_code)] // задел до первого неразумного существа
pub const ARMOR_LAYOUT_UNSENTIENT: [BodySlot; 2] = [BodySlot::Head, BodySlot::Body];

/// Секция инвентаря — глобальная типизация слотов.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotArea {
    /// Основные предметы (misc): любой предмет может лежать в любом слоте.
    Misc,
    /// Броня: слот принимает только броню своей части тела.
    Armor,
    /// Одежда: слот принимает только одежду своей части тела.
    Clothes,
    /// Косметика (baubles): слоты равнозначны, порядок произвольный.
    Bauble,
    /// Мусорка UI удаления предметов: один слот, принимает что угодно,
    /// каждая новая кладка затирает старое (стек можно забрать обратно,
    /// пока его не затёрли). Не входит в `ALL`: предмет в мусорке не
    /// считается лежащим в инвентаре (не тратится, не съедается).
    Trash,
}

impl SlotArea {
    /// Секции в стабильном порядке (UI, обходы). Мусорка — служебный
    /// слот удаления, в обычные обходы инвентаря не входит.
    pub const ALL: [SlotArea; 4] =
        [SlotArea::Misc, SlotArea::Armor, SlotArea::Clothes, SlotArea::Bauble];

    /// Число слотов секции у разумного существа (GAME_DESIGN 3.4).
    pub fn slots_count(self) -> usize {
        match self {
            SlotArea::Misc => MISC_SLOTS,
            SlotArea::Armor => ARMOR_SLOTS,
            SlotArea::Clothes => CLOTHES_SLOTS,
            SlotArea::Bauble => BAUBLE_SLOTS,
            SlotArea::Trash => 1,
        }
    }
}

/// Адрес конкретного слота инвентаря: секция + индекс в ней.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotRef {
    pub area: SlotArea,
    pub index: usize,
}

/// Стек предметов одного вида в слоте.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemStack {
    pub kind: ItemKind,
    pub count: u32,
}

/// Инвентарь: секции слотов (misc, броня, одежда, косметика).
/// Слот пуст (None) или хранит стек предметов одного вида.
/// Секции брони и одежды разумного существа индексируются по
/// `ARMOR_LAYOUT`/`CLOTHES_LAYOUT`.
/// Для неразумных существ (ARMOR_LAYOUT_UNSENTIENT) отдельный инвентарь.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Inventory {
    /// Основные предметы.
    pub items: [Option<ItemStack>; MISC_SLOTS],
    /// Надеваемая броня (не используется на первом срезе).
    pub armor: [Option<ItemStack>; ARMOR_SLOTS],
    /// Надеваемая одежда: защита от среды (холод и т.п.), предметов пока нет.
    pub clothes: [Option<ItemStack>; CLOTHES_SLOTS],
    /// Косметика (не используется на первом срезе).
    pub baubles: [Option<ItemStack>; BAUBLE_SLOTS],
    /// Мусорка UI: последний положенный «на удаление» стек. Затирается
    /// следующей кладкой, поэтому хранит ровно один слот.
    pub trash: Option<ItemStack>,
}

impl Default for Inventory {
    /// Пустой инвентарь: все слоты свободны.
    fn default() -> Self {
        Self {
            items: [None; MISC_SLOTS],
            armor: [None; ARMOR_SLOTS],
            clothes: [None; CLOTHES_SLOTS],
            baubles: [None; BAUBLE_SLOTS],
            trash: None,
        }
    }
}

impl Inventory {
    /// Добавляет предметы в секцию misc: доливает в существующий стек
    /// того же вида или занимает пустые слоты. Предел стака — по виду
    /// (`ItemKind::max_stack`): инструменты и оружие расходуют по слоту
    /// на единицу, расходники доливаются до 64. Возвращает, сколько
    /// единиц не поместилось.
    pub fn add(&mut self, kind: ItemKind, count: u32) -> u32 {
        let cap = kind.max_stack();
        let mut left = count;

        // Сначала доливаем частично заполненные стеки того же вида.
        for slot in self.items.iter_mut().flatten() {
            if left == 0 {
                break;
            }
            if slot.kind == kind && slot.count < cap {
                let take = (cap - slot.count).min(left);
                slot.count += take;
                left -= take;
            }
        }

        // Остаток занимает пустые слоты — тоже с пределом вида.
        for slot in self.items.iter_mut() {
            if left == 0 {
                break;
            }
            if slot.is_none() {
                let take = cap.min(left);
                *slot = Some(ItemStack { kind, count: take });
                left -= take;
            }
        }

        left
    }

    /// Поместятся ли `count` единиц предмета вида `kind` в секцию misc
    /// целиком (предел стака — по виду). Используется перед действиями,
    /// которые тратят ресурс мира: добыча «в никуда» не должна
    /// уничтожать объект.
    pub fn can_fit(&self, kind: ItemKind, count: u32) -> bool {
        let cap = kind.max_stack();
        let mut space = 0_u32;
        for slot in &self.items {
            match slot {
                Some(stack) if stack.kind == kind => space += cap - stack.count,
                None => space += cap,
                Some(_) => {}
            }
        }
        space >= count
    }

    /// Сколько единиц предмета вида `kind` лежит в инвентаре (все секции,
    /// хотя на первом срезе добыча кладёт предмет только в `items`).
    pub fn count_of(&self, kind: ItemKind) -> u32 {
        let mut total = 0_u32;
        for area in SlotArea::ALL {
            for slot in self.area(area).iter().flatten() {
                if slot.kind == kind {
                    total += slot.count;
                }
            }
        }
        total
    }

    /// Списывает `count` единиц предмета `kind` из секции misc (по стекам,
    /// начиная с первого слота). `false` — предметов не хватает, тогда
    /// ничего не списано. Нужно строительству: плата берётся со всех
    /// стеков инвентаря, а не только с выбранного слота.
    pub fn take_items(&mut self, kind: ItemKind, count: u32) -> bool {
        let mut available = 0_u32;
        for slot in self.items.iter().flatten() {
            if slot.kind == kind {
                available += slot.count;
            }
        }
        if available < count {
            return false;
        }
        let mut left = count;
        for slot in self.items.iter_mut() {
            if left == 0 {
                break;
            }
            let Some(stack) = slot else { continue };
            if stack.kind != kind {
                continue;
            }
            let take = stack.count.min(left);
            stack.count -= take;
            left -= take;
            if stack.count == 0 {
                *slot = None;
            }
        }
        true
    }

    /// Слоты секции (view для UI и обходов).
    pub fn area(&self, area: SlotArea) -> &[Option<ItemStack>] {
        match area {
            SlotArea::Misc => &self.items,
            SlotArea::Armor => &self.armor,
            SlotArea::Clothes => &self.clothes,
            SlotArea::Bauble => &self.baubles,
            SlotArea::Trash => std::slice::from_ref(&self.trash),
        }
    }

    /// Мутабельные слоты секции.
    pub fn area_mut(&mut self, area: SlotArea) -> &mut [Option<ItemStack>] {
        match area {
            SlotArea::Misc => &mut self.items,
            SlotArea::Armor => &mut self.armor,
            SlotArea::Clothes => &mut self.clothes,
            SlotArea::Bauble => &mut self.baubles,
            SlotArea::Trash => std::slice::from_mut(&mut self.trash),
        }
    }

    /// Стек слота (нечитаемый адрес — None).
    pub fn get(&self, slot: SlotRef) -> Option<ItemStack> {
        self.area(slot.area).get(slot.index).copied().flatten()
    }

    /// Берёт стек из слота, очищая его. Нечитаемый адрес — None.
    pub fn take(&mut self, slot: SlotRef) -> Option<ItemStack> {
        self.area_mut(slot.area).get_mut(slot.index).and_then(Option::take)
    }

    /// Ставит стек в пустой слот. Нечитаемый адрес — стек теряется:
    /// вызывающий обязан проверить совместимость через `accepts` заранее
    /// (внутренний метод, правила размещения — в `accepts` и `transfer`).
    pub fn put(&mut self, slot: SlotRef, stack: ItemStack) {
        if let Some(cell) = self.area_mut(slot.area).get_mut(slot.index) {
            debug_assert!(cell.is_none(), "слот занят: запись только в пустой");
            *cell = Some(stack);
        }
    }

    /// Правило размещения: можно ли предмет вида `kind` держать в слоте
    /// `area[index]`. Misc принимает любой предмет; свой слот брони — только
    /// броню той же части тела; одежды — только одежду той же части тела;
    /// косметику — только косметика; мусорка — что угодно (старый стек
    /// затирается).
    pub fn accepts(kind: ItemKind, area: SlotArea, index: usize) -> bool {
        if index >= area.slots_count() {
            return false;
        }
        match area {
            SlotArea::Misc => true,
            SlotArea::Armor => {
                matches!(kind.class(), ItemClass::Armor(slot) if slot == ARMOR_LAYOUT[index])
            }
            SlotArea::Clothes => {
                matches!(kind.class(), ItemClass::Clothes(slot) if slot == CLOTHES_LAYOUT[index])
            }
            SlotArea::Bauble => matches!(kind.class(), ItemClass::Bauble),
            SlotArea::Trash => true,
        }
    }

    /// Переносит содержимое слота в другой слот с учётом правил размещения.
    /// Целевой слот занят тем же видом (misc-стеки) — доливает, остаток
    /// возвращается в исходный слот; занят другим предметом — меняет их
    /// местами, если встречный предмет допустим в исходном слоте.
    /// Целевой слот — мусорка: принимает что угодно, старый стек затирается
    /// (предмет удаляется — это и есть путь удаления через UI). Исключение
    /// до общей логики: ни слияния, ни обмена с мусоркой не бывает.
    /// `true` — содержимое изменилось.
    pub fn transfer(&mut self, from: SlotRef, to: SlotRef) -> bool {
        if from == to || !self.accepts_slot_count(to) {
            return false;
        }
        let Some(source) = self.take(from) else {
            return false;
        };
        if to.area == SlotArea::Trash {
            self.take(to); // прежнее содержимое удаляется
            self.put(to, source);
            return true;
        }
        if !Self::accepts(source.kind, to.area, to.index) {
            self.put(from, source);
            return false;
        }

        let target = self.take(to);
        match target {
            None => {
                self.put(to, source);
                true
            }
            Some(t) if t.kind == source.kind => {
                // Доливаем целевой стек до предела вида, остаток остаётся
                // в исходном слоте.
                let cap = t.kind.max_stack();
                let free = cap.saturating_sub(t.count).min(source.count);
                if free > 0 {
                    self.put(to, ItemStack { kind: t.kind, count: t.count + free });
                    if source.count > free {
                        self.put(from, ItemStack { kind: t.kind, count: source.count - free });
                    }
                    true
                } else {
                    // Некуда доливать: перенос невозможен, всё на месте.
                    self.put(to, t);
                    self.put(from, source);
                    false
                }
            }
            Some(t) => {
                if Self::accepts(t.kind, from.area, from.index) {
                    // Обмен предметами между слотами.
                    self.put(to, source);
                    self.put(from, t);
                    true
                } else {
                    self.put(to, t);
                    self.put(from, source);
                    false
                }
            }
        }
    }

    /// Адрес в границах секции.
    fn accepts_slot_count(&self, slot: SlotRef) -> bool {
        slot.index < slot.area.slots_count()
    }
}
