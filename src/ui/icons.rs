//! Хэндлы текстур UI: иконки тайлов, объектов и предметов
//! (нижняя панель, инвентарь), а также иконки play/pause.
//! Игровые иконки грузятся с nearest-семплированием: исходники —
//! пиксель-арт, при растяжении они не должны размываться. Иконки
//! управления — обычные картинки, им оставлен линейный фильтр.

use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::prelude::*;

use crate::simulation::items::BodySlot;
use crate::simulation::{EntityKind, ItemKind, ObjectKind, TileType, ITEM_KIND_COUNT};

/// Число видов тайлов с иконками (Water..Podzol; Empty в UI не нужен).
const TILE_ICON_COUNT: usize = 14;

/// Число специализаций брони разумного существа с текстурой пустого слота.
const EMPTY_ARMOR_SLOT_COUNT: usize = 4;

/// Иконки UI. Индексация: тайлы — `TileType as u8 - 1`, объекты и
/// предметы — `as usize` дискриминанта.
#[derive(Resource)]
pub struct UiIcons {
    tiles: [Handle<Image>; TILE_ICON_COUNT],
    /// По дискриминанту `ObjectKind` (включая постройки и станции).
    objects: [Handle<Image>; 11],
    items: [Handle<Image>; ITEM_KIND_COUNT],
    entities: [Handle<Image>; 2],
    /// Текстуры пустых слотов брони в порядке `ARMOR_LAYOUT`
    /// (head, chest, legs, boots).
    empty_armor_slots: [Handle<Image>; EMPTY_ARMOR_SLOT_COUNT],
    /// Текстура слота-мусорки в окне инвентаря (видна, когда слот пуст).
    trash_can: Handle<Image>,
    /// «Play»: мир стоит на паузе, клик запустит симуляцию.
    play: Handle<Image>,
    /// «Pause»: мир идёт, клик поставит на паузу.
    pause: Handle<Image>,
}

impl UiIcons {
    /// Загружает все иконки из `assets/textures/...`.
    /// Пути — 'static: у `LoadBuilder::load` строка должна жить вечно,
    /// литералы это дают.
    pub fn load(assets: &AssetServer) -> Self {
        let nearest = |path: &'static str| {
            assets
                .load_builder()
                .with_settings::<ImageLoaderSettings>(|settings| {
                    settings.sampler = ImageSampler::nearest();
                })
                .load(path)
        };
        Self {
            tiles: [
                nearest("textures/tiles/water.png"),
                nearest("textures/tiles/sand.png"),
                nearest("textures/tiles/dirt.png"),
                nearest("textures/tiles/grass.png"),
                nearest("textures/tiles/forest_grass.png"),
                nearest("textures/tiles/stone.png"),
                nearest("textures/tiles/clay.png"),
                nearest("textures/tiles/farmland.png"),
                nearest("textures/tiles/deep_water.png"),
                nearest("textures/tiles/ocean.png"),
                nearest("textures/tiles/mountain.png"),
                nearest("textures/tiles/moss.png"),
                nearest("textures/tiles/mud.png"),
                nearest("textures/tiles/podzol.png"),
            ],
            objects: [
                nearest("textures/objects/tree.png"),
                nearest("textures/objects/bush.png"),
                nearest("textures/objects/rock.png"),
                nearest("textures/objects/apple_tree.png"),
                nearest("textures/buildings/campfire_cross.png"),
                nearest("textures/buildings/crafting_table_top.png"),
                nearest("textures/buildings/furnace_front.png"),
                nearest("textures/buildings/barrel_side.png"),
                nearest("textures/buildings/blast_furnace_front.png"),
                nearest("textures/buildings/storage.png"),
                nearest("textures/buildings/smithing_table_front.png"),
            ],
            items: [
                nearest("textures/items/stick.png"),
                nearest("textures/items/rock.png"),
                nearest("textures/items/food/apple.png"),
                nearest("textures/items/grass_bunch_raw.png"),
                nearest("textures/items/plant_fiber.png"),
                nearest("textures/items/cordage_fiber.png"),
                nearest("textures/items/tools/axe_primal.png"),
                nearest("textures/items/tools/pickaxe_primal.png"),
                nearest("textures/items/tools/hoe_primal.png"),
                nearest("textures/items/tools/shovel_primal.png"),
                nearest("textures/items/weapon/spear_primal.png"),
                nearest("textures/items/tools/knife_stone.png"),
                nearest("textures/items/tools/fishing_rod_primal.png"),
                nearest("textures/items/weapon/arrow.png"),
                nearest("textures/items/weapon/bow_wood.png"),
                nearest("textures/items/armor/helmet_chainmail.png"),
                nearest("textures/items/armor/chestplate_chainmail.png"),
                nearest("textures/items/armor/leggings_chainmail.png"),
                nearest("textures/items/armor/boots_chainmail.png"),
                nearest("textures/items/bone.png"),
                nearest("textures/items/shard/shard_bone.png"),
                nearest("textures/items/arrow_fletching.png"),
                nearest("textures/items/feather.png"),
                nearest("textures/items/string.png"),
                nearest("textures/items/stick_bundle.png"),
                nearest("textures/items/book.png"),
                nearest("textures/items/paper.png"),
                nearest("textures/items/paper_pulp.png"),
                nearest("textures/items/clods/clod_clay.png"),
                nearest("textures/items/brick_clay_unfired.png"),
                nearest("textures/items/brick_clay_baked.png"),
                nearest("textures/items/seeds/seeds_wheat.png"),
                nearest("textures/items/wheat.png"),
                nearest("textures/items/tools/hammer_stone.png"),
                nearest("textures/items/nugget/nugget_copper.png"),
                nearest("textures/items/nugget/nugget_gold.png"),
                nearest("textures/items/nugget/nugget_silver.png"),
                nearest("textures/items/ingot/ingot_copper.png"),
                nearest("textures/items/ingot/ingot_bronze.png"),
                nearest("textures/items/ingot/ingot_tin.png"),
                nearest("textures/items/ingot/ingot_gold.png"),
                nearest("textures/items/ingot/ingot_silver.png"),
                nearest("textures/items/gem/gem_quartz.png"),
                nearest("textures/items/gem/gem_flint.png"),
                nearest("textures/items/gem/gem_emerald.png"),
                nearest("textures/items/gem/gem_ruby.png"),
                nearest("textures/items/dust/dust_bronze.png"),
                nearest("textures/items/dust/dust_charcoal.png"),
                nearest("textures/items/dust/dust_copper.png"),
                nearest("textures/items/dust/dust_flour.png"),
                nearest("textures/items/dust/dust_gold.png"),
                nearest("textures/items/dust/dust_pit_ash.png"),
                nearest("textures/items/dust/dust_silver.png"),
                nearest("textures/items/dust/dust_sugar.png"),
                nearest("textures/items/dust/dust_sulfur.png"),
                nearest("textures/items/dust/dust_tin.png"),
                nearest("textures/items/dust/dust_wood_chips.png"),
                nearest("textures/items/food/apple_baked.png"),
                nearest("textures/items/food/carrot.png"),
                nearest("textures/items/food/carrot_baked.png"),
                nearest("textures/items/food/egg.png"),
                nearest("textures/items/food/egg_roasted.png"),
                nearest("textures/items/food/lard.png"),
                nearest("textures/items/food/meat_bird_raw.png"),
                nearest("textures/items/food/meat_bird_cooked.png"),
                nearest("textures/items/food/meat_herbivore_raw.png"),
                nearest("textures/items/food/meat_herbivore_cooked.png"),
                nearest("textures/items/food/meat_predator_raw.png"),
                nearest("textures/items/food/meat_predator_cooked.png"),
                nearest("textures/items/food/potato.png"),
                nearest("textures/items/food/potato_baked.png"),
                nearest("textures/items/food/strange_tuber.png"),
                nearest("textures/items/raw_ore/raw_copper.png"),
                nearest("textures/items/raw_ore/raw_gold.png"),
                nearest("textures/items/raw_ore/raw_silver.png"),
                nearest("textures/items/raw_ore/raw_tin.png"),
                nearest("textures/items/tools/axe_copper.png"),
                nearest("textures/items/tools/hammer_copper.png"),
                nearest("textures/items/tools/hoe_copper.png"),
                nearest("textures/items/tools/pickaxe_copper.png"),
                nearest("textures/items/tools/saw_copper.png"),
                nearest("textures/items/tools/shears_copper.png"),
                nearest("textures/items/tools/shovel_copper.png"),
                nearest("textures/items/tools/sickle_copper.png"),
                nearest("textures/items/seeds/seeds_beetroot.png"),
                nearest("textures/items/food/beetroot.png"),
                nearest("textures/items/food/beetroot_roasted.png"),
                nearest("textures/items/food/cherry.png"),
                nearest("textures/items/coal.png"),
                nearest("textures/items/charcoal.png"),
                nearest("textures/items/torch.png"),
            ],
            // Человек временно рисуется текстурой игрока (как в рендере).
            entities: [
                nearest("textures/entities/wolf/wolf_standing.png"),
                nearest("textures/entities/player.png"),
            ],
            empty_armor_slots: [
                nearest("textures/ui/inventory/empty_armor_slot_helmet.png"),
                nearest("textures/ui/inventory/empty_armor_slot_chestplate.png"),
                nearest("textures/ui/inventory/empty_armor_slot_leggings.png"),
                nearest("textures/ui/inventory/empty_armor_slot_boots.png"),
            ],
            trash_can: nearest("textures/ui/inventory/trash_can.png"),
            play: assets.load("textures/ui/play.png"),
            pause: assets.load("textures/ui/pause.png"),
        }
    }

    /// Иконка тайла рельефа.
    pub fn tile(&self, kind: TileType) -> &Handle<Image> {
        &self.tiles[(kind as u8 - 1) as usize]
    }

    /// Иконка объекта. Массив проиндексирован дискриминантом `ObjectKind`:
    /// природные виды 0–3, постройки 4–10 (`Building(k)` → 4 + k).
    pub fn object(&self, kind: ObjectKind) -> &Handle<Image> {
        &self.objects[kind.discriminant() as usize]
    }

    /// Иконка предмета.
    pub fn item(&self, kind: ItemKind) -> &Handle<Image> {
        &self.items[kind as usize]
    }

    /// Иконка существа.
    pub fn entity(&self, kind: EntityKind) -> &Handle<Image> {
        &self.entities[kind as usize]
    }

    /// Текстура пустого слота брони по части тела. `Body` (броня
    /// неразумных существ) своей текстуры не имеет.
    pub fn empty_armor_slot(&self, slot: BodySlot) -> Option<&Handle<Image>> {
        match slot {
            BodySlot::Head => Some(&self.empty_armor_slots[0]),
            BodySlot::Chest => Some(&self.empty_armor_slots[1]),
            BodySlot::Legs => Some(&self.empty_armor_slots[2]),
            BodySlot::Boots => Some(&self.empty_armor_slots[3]),
            BodySlot::Body => None,
        }
    }

    /// Текстура слота-мусорки (пустое состояние).
    pub fn trash_can(&self) -> &Handle<Image> {
        &self.trash_can
    }

    /// Иконка «play» (показать, когда мир на паузе).
    pub fn play(&self) -> &Handle<Image> {
        &self.play
    }

    /// Иконка «pause» (показать, когда мир идёт).
    pub fn pause(&self) -> &Handle<Image> {
        &self.pause
    }
}
