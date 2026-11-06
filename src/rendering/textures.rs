//! Игровые текстуры рендера: рельеф, объекты, постройки, предметы, сущности.
//!
//! Вынесены из `rendering/mod.rs` в отдельный файл: видов текстур много и
//! будет больше, а мешам блоков нужен только доступ к хэндлам по виду
//! (`object_texture`, `item_texture`, …).

use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::prelude::*;

use crate::simulation::{
    BuildingKind, CropKind, EntityKind, EntityState, ItemKind, ObjectKind, WorldObject,
    CROP_KIND_COUNT, ITEM_KIND_COUNT, MAX_CROP_STAGES,
};

/// Хэндлы текстур для всех типов тайлов, объектов и игрока.
#[derive(Resource)]
pub(crate) struct TileTextures {
    water: Handle<Image>,
    sand: Handle<Image>,
    dirt: Handle<Image>,
    grass: Handle<Image>,
    forest_grass: Handle<Image>,
    stone: Handle<Image>,
    clay: Handle<Image>,
    farmland: Handle<Image>,
    deep_water: Handle<Image>,
    ocean: Handle<Image>,
    mountain: Handle<Image>,
    moss: Handle<Image>,
    mud: Handle<Image>,
    podzol: Handle<Image>,
    tree: Handle<Image>,
    bush: Handle<Image>,
    rock: Handle<Image>,
    apple_tree: Handle<Image>,
    apple_tree_empty: Handle<Image>,
    /// Крест костра: две вертикальные грани вдоль середин сторон тайла.
    campfire_cross: Handle<Image>,
    /// Склад: плоская текстура, заполняющая поверхность тайла.
    storage_flat: Handle<Image>,
    /// Текстуры станций по порядку `STATION_KINDS`.
    stations: [StationTextures; STATION_KIND_COUNT],
    /// Стадии культур: плотный массив `[культура][стадия]` по
    /// `crop_stage_slot` (несуществующие стадии — дубликат последней).
    crops: [Handle<Image>; CROP_TEXTURE_COUNT],
    /// Иконки предметов по дискриминанту `ItemKind`.
    items: [Handle<Image>; ITEM_KIND_COUNT],
    player: Handle<Image>,
    wolf_standing: Handle<Image>,
    wolf_lying: Handle<Image>,
    wolf_walk: [Handle<Image>; 2],
    /// Кролик: сидит (покой), лежит и три прыжковых кадра ходьбы.
    rabbit_standing: Handle<Image>,
    rabbit_lying: Handle<Image>,
    rabbit_walk: [Handle<Image>; 3],
}

/// Текстуры одной станции-блока: верх и две видимые боковые грани
/// (−X — «фронт», −Y — «бок»). Для печей `front_on` — рабочая фронтальная
/// текстура (горит/готовит).
pub(crate) struct StationTextures {
    pub(crate) top: Handle<Image>,
    pub(crate) side: Handle<Image>,
    pub(crate) front: Handle<Image>,
    pub(crate) front_on: Option<Handle<Image>>,
}

/// Станции с изометрическим блоком (порядок = порядок в `stations`).
pub(crate) const STATION_KINDS: [BuildingKind; 5] = [
    BuildingKind::CraftingTable,
    BuildingKind::Furnace,
    BuildingKind::Barrel,
    BuildingKind::BlastFurnace,
    BuildingKind::SmithingTable,
];

pub(crate) const STATION_KIND_COUNT: usize = STATION_KINDS.len();

/// Всего слотов текстур стадий культур (плотная раскладка по культурам).
pub(crate) const CROP_TEXTURE_COUNT: usize = CROP_KIND_COUNT * MAX_CROP_STAGES;

/// Смещение стадии культуры в плотном массиве текстур: `[культура][стадия]`.
/// Стадия зажимается до последней существующей (у культур с меньшим числом
/// стадий «лишние» слоты дублируют зрелую стадию). Чистая функция — без
/// графики проверяется тестом.
pub(crate) fn crop_stage_slot(kind: CropKind, stage: u8) -> usize {
    let stage = (stage as usize).min(kind.stage_count() - 1);
    kind as usize * MAX_CROP_STAGES + stage
}

impl TileTextures {
    /// Загружает все текстуры из `assets/textures/...`.
    pub(crate) fn load(assets: &AssetServer) -> Self {
        // Все игровые текстуры — пиксель-арт: грузим с nearest-семплированием,
        // иначе при масштабировании квадов (объекты, существа, культуры,
        // предметы на земле) рисунок размывается линейной фильтрацией.
        // Приём тот же, что у UI-иконок (`UiIcons::load`).
        let nearest = |path: String| {
            assets
                .load_builder()
                .with_settings::<ImageLoaderSettings>(move |settings| {
                    settings.sampler = ImageSampler::nearest();
                })
                .load(path)
        };
        // Стадии культур грузим в плотную раскладку: слот каждой культуры
        // и стадии заполнен реальной текстурой, недостающие стадии —
        // дубликатом последней существующей (см. `crop_stage_slot`).
        let crop_files: [&str; CROP_KIND_COUNT] = [
            "wheat",
            "beetroots",
            "carrots",
            "potatoes",
            "cherry",
        ];
        let mut crops: Vec<Handle<Image>> = Vec::with_capacity(CROP_TEXTURE_COUNT);
        for (kind, base) in crop_files.iter().enumerate() {
            let kind = CropKind::from_discriminant(kind as u8).expect("культуры 0..COUNT валидны");
            let stage_count = kind.stage_count();
            for stage in 0..MAX_CROP_STAGES {
                // Существующие стадии — свои файлы; остальные — дубликат зрелой.
                let file_stage = stage.min(stage_count - 1);
                crops.push(nearest(format!("textures/crops/{base}_stage{file_stage}.png")));
            }
        }
        let crops: [Handle<Image>; CROP_TEXTURE_COUNT] = crops
            .try_into()
            .expect("все слоты культур заполнены");

        Self {
            water: nearest("textures/tiles/water.png".into()),
            sand: nearest("textures/tiles/sand.png".into()),
            dirt: nearest("textures/tiles/dirt.png".into()),
            grass: nearest("textures/tiles/grass.png".into()),
            forest_grass: nearest("textures/tiles/forest_grass.png".into()),
            stone: nearest("textures/tiles/stone.png".into()),
            clay: nearest("textures/tiles/clay.png".into()),
            farmland: nearest("textures/tiles/farmland.png".into()),
            deep_water: nearest("textures/tiles/deep_water.png".into()),
            ocean: nearest("textures/tiles/ocean.png".into()),
            mountain: nearest("textures/tiles/mountain.png".into()),
            moss: nearest("textures/tiles/moss.png".into()),
            mud: nearest("textures/tiles/mud.png".into()),
            podzol: nearest("textures/tiles/podzol.png".into()),
            tree: nearest("textures/objects/tree.png".into()),
            bush: nearest("textures/objects/bush.png".into()),
            rock: nearest("textures/objects/rock.png".into()),
            apple_tree: nearest("textures/objects/apple_tree.png".into()),
            apple_tree_empty: nearest("textures/objects/apple_tree_empty.png".into()),
            campfire_cross: nearest("textures/buildings/campfire_cross.png".into()),
            storage_flat: nearest("textures/buildings/storage.png".into()),
            stations: [
                // Верстак: боковины одинаковы, фронтальной грани нет.
                StationTextures {
                    top: nearest("textures/buildings/crafting_table_top.png".into()),
                    side: nearest("textures/buildings/crafting_table_side.png".into()),
                    front: nearest("textures/buildings/crafting_table_side.png".into()),
                    front_on: None,
                },
                StationTextures {
                    top: nearest("textures/buildings/furnace_top.png".into()),
                    side: nearest("textures/buildings/furnace_side.png".into()),
                    front: nearest("textures/buildings/furnace_front.png".into()),
                    front_on: Some(nearest("textures/buildings/furnace_front_on.png".into())),
                },
                StationTextures {
                    top: nearest("textures/buildings/barrel_top.png".into()),
                    side: nearest("textures/buildings/barrel_side.png".into()),
                    front: nearest("textures/buildings/barrel_side.png".into()),
                    front_on: None,
                },
                StationTextures {
                    top: nearest("textures/buildings/blast_furnace_top.png".into()),
                    side: nearest("textures/buildings/blast_furnace_side.png".into()),
                    front: nearest("textures/buildings/blast_furnace_front.png".into()),
                    front_on: Some(nearest("textures/buildings/blast_furnace_front_on.png".into())),
                },
                StationTextures {
                    top: nearest("textures/buildings/smithing_table_top.png".into()),
                    side: nearest("textures/buildings/smithing_table_side.png".into()),
                    front: nearest("textures/buildings/smithing_table_front.png".into()),
                    front_on: None,
                },
            ],
            crops,
            items: [
                nearest("textures/items/stick.png".into()),
                nearest("textures/items/rock.png".into()),
                nearest("textures/items/food/apple.png".into()),
                nearest("textures/items/grass_bunch_raw.png".into()),
                nearest("textures/items/plant_fiber.png".into()),
                nearest("textures/items/cordage_fiber.png".into()),
                nearest("textures/items/tools/axe_primal.png".into()),
                nearest("textures/items/tools/pickaxe_primal.png".into()),
                nearest("textures/items/tools/hoe_primal.png".into()),
                nearest("textures/items/tools/shovel_primal.png".into()),
                nearest("textures/items/weapon/spear_primal.png".into()),
                nearest("textures/items/tools/knife_stone.png".into()),
                nearest("textures/items/tools/fishing_rod_primal.png".into()),
                nearest("textures/items/weapon/arrow.png".into()),
                nearest("textures/items/weapon/bow_wood.png".into()),
                nearest("textures/items/armor/helmet_chainmail.png".into()),
                nearest("textures/items/armor/chestplate_chainmail.png".into()),
                nearest("textures/items/armor/leggings_chainmail.png".into()),
                nearest("textures/items/armor/boots_chainmail.png".into()),
                nearest("textures/items/bone.png".into()),
                nearest("textures/items/shard/shard_bone.png".into()),
                nearest("textures/items/arrow_fletching.png".into()),
                nearest("textures/items/feather.png".into()),
                nearest("textures/items/string.png".into()),
                nearest("textures/items/stick_bundle.png".into()),
                nearest("textures/items/book.png".into()),
                nearest("textures/items/paper.png".into()),
                nearest("textures/items/paper_pulp.png".into()),
                nearest("textures/items/clods/clod_clay.png".into()),
                nearest("textures/items/brick_clay_unfired.png".into()),
                nearest("textures/items/brick_clay_baked.png".into()),
                nearest("textures/items/seeds/seeds_wheat.png".into()),
                nearest("textures/items/wheat.png".into()),
                nearest("textures/items/tools/hammer_stone.png".into()),
                nearest("textures/items/nugget/nugget_copper.png".into()),
                nearest("textures/items/nugget/nugget_gold.png".into()),
                nearest("textures/items/nugget/nugget_silver.png".into()),
                nearest("textures/items/ingot/ingot_copper.png".into()),
                nearest("textures/items/ingot/ingot_bronze.png".into()),
                nearest("textures/items/ingot/ingot_tin.png".into()),
                nearest("textures/items/ingot/ingot_gold.png".into()),
                nearest("textures/items/ingot/ingot_silver.png".into()),
                nearest("textures/items/gem/gem_quartz.png".into()),
                nearest("textures/items/gem/gem_flint.png".into()),
                nearest("textures/items/gem/gem_emerald.png".into()),
                nearest("textures/items/gem/gem_ruby.png".into()),
                nearest("textures/items/dust/dust_bronze.png".into()),
                nearest("textures/items/dust/dust_charcoal.png".into()),
                nearest("textures/items/dust/dust_copper.png".into()),
                nearest("textures/items/dust/dust_flour.png".into()),
                nearest("textures/items/dust/dust_gold.png".into()),
                nearest("textures/items/dust/dust_pit_ash.png".into()),
                nearest("textures/items/dust/dust_silver.png".into()),
                nearest("textures/items/dust/dust_sugar.png".into()),
                nearest("textures/items/dust/dust_sulfur.png".into()),
                nearest("textures/items/dust/dust_tin.png".into()),
                nearest("textures/items/dust/dust_wood_chips.png".into()),
                nearest("textures/items/food/apple_baked.png".into()),
                nearest("textures/items/food/carrot.png".into()),
                nearest("textures/items/food/carrot_baked.png".into()),
                nearest("textures/items/food/egg.png".into()),
                nearest("textures/items/food/egg_roasted.png".into()),
                nearest("textures/items/food/lard.png".into()),
                nearest("textures/items/food/meat_bird_raw.png".into()),
                nearest("textures/items/food/meat_bird_cooked.png".into()),
                nearest("textures/items/food/meat_herbivore_raw.png".into()),
                nearest("textures/items/food/meat_herbivore_cooked.png".into()),
                nearest("textures/items/food/meat_predator_raw.png".into()),
                nearest("textures/items/food/meat_predator_cooked.png".into()),
                nearest("textures/items/food/potato.png".into()),
                nearest("textures/items/food/potato_baked.png".into()),
                nearest("textures/items/food/strange_tuber.png".into()),
                nearest("textures/items/raw_ore/raw_copper.png".into()),
                nearest("textures/items/raw_ore/raw_gold.png".into()),
                nearest("textures/items/raw_ore/raw_silver.png".into()),
                nearest("textures/items/raw_ore/raw_tin.png".into()),
                nearest("textures/items/tools/axe_copper.png".into()),
                nearest("textures/items/tools/hammer_copper.png".into()),
                nearest("textures/items/tools/hoe_copper.png".into()),
                nearest("textures/items/tools/pickaxe_copper.png".into()),
                nearest("textures/items/tools/saw_copper.png".into()),
                nearest("textures/items/tools/shears_copper.png".into()),
                nearest("textures/items/tools/shovel_copper.png".into()),
                nearest("textures/items/tools/sickle_copper.png".into()),
                nearest("textures/items/seeds/seeds_beetroot.png".into()),
                nearest("textures/items/food/beetroot.png".into()),
                nearest("textures/items/food/beetroot_roasted.png".into()),
                nearest("textures/items/food/cherry.png".into()),
                nearest("textures/items/coal.png".into()),
                nearest("textures/items/charcoal.png".into()),
            ],
            player: nearest("textures/entities/player.png".into()),
            wolf_standing: nearest("textures/entities/wolf/wolf_standing.png".into()),
            wolf_lying: nearest("textures/entities/wolf/wolf_lying.png".into()),
            wolf_walk: [
                nearest("textures/entities/wolf/wolf_walk1.png".into()),
                nearest("textures/entities/wolf/wolf_walk2.png".into()),
            ],
            rabbit_standing: nearest("textures/entities/rabit/rabit_sit.png".into()),
            rabbit_lying: nearest("textures/entities/rabit/rabit_lying.png".into()),
            rabbit_walk: [
                nearest("textures/entities/rabit/rabbit_jump_2.png".into()),
                nearest("textures/entities/rabit/rabbit_standing.png".into()),
                nearest("textures/entities/rabit/rabit_jump.png".into()),
            ],
        }
    }

    /// Хэндл текстуры по объекту. Пустая яблоня (без плодов) рисуется
    /// отдельной текстурой, но остаётся тем же видом объекта.
    pub(crate) fn object_texture(&self, obj: &WorldObject) -> &Handle<Image> {
        match obj.kind {
            ObjectKind::AppleTree if obj.charge == 0 => &self.apple_tree_empty,
            kind => self.kind_texture(kind),
        }
    }

    /// Хэндл текстуры по виду объекта (яблоня — плодоносящая).
    pub(crate) fn kind_texture(&self, kind: ObjectKind) -> &Handle<Image> {
        match kind {
            ObjectKind::Tree => &self.tree,
            ObjectKind::Bush => &self.bush,
            ObjectKind::Rock => &self.rock,
            ObjectKind::AppleTree => &self.apple_tree,
            // Плоская текстура склада-декаля; остальное — первая попавшаяся
            // грань станции (для меню строительства/голограммы).
            ObjectKind::Building(BuildingKind::Storage) => &self.storage_flat,
            ObjectKind::Building(BuildingKind::Campfire) => &self.campfire_cross,
            ObjectKind::Building(station) => &self.station_textures(station).front,
        }
    }

    /// Текстуры станции-блока (верх/бок/фронт/фронт-в-работе).
    /// Порядок станций — `STATION_KINDS`; станция вне списка (костёр,
    /// склад) рисуется не блоком, поэтому сюда не попадает.
    pub(crate) fn station_textures(&self, kind: BuildingKind) -> &StationTextures {
        let index = STATION_KINDS
            .iter()
            .position(|k| *k == kind)
            .expect("все блочные станции должны быть в STATION_KINDS");
        &self.stations[index]
    }

    /// Хэндл креста костра.
    pub(crate) fn campfire_cross(&self) -> &Handle<Image> {
        &self.campfire_cross
    }

    /// Хэндл плоской текстуры склада.
    pub(crate) fn storage_flat(&self) -> &Handle<Image> {
        &self.storage_flat
    }

    /// Хэндл текстуры стадии культуры (плотная раскладка — `crop_stage_slot`).
    pub(crate) fn crop_texture(&self, kind: CropKind, stage: u8) -> &Handle<Image> {
        &self.crops[crop_stage_slot(kind, stage)]
    }

    /// Хэндл текстуры предмета.
    pub(crate) fn item_texture(&self, kind: ItemKind) -> &Handle<Image> {
        &self.items[kind as usize]
    }

    /// Текстура существа по виду, состоянию и кадру анимации ходьбы.
    /// Исходники нарисованы смотрящими вправо; зеркалит рендер.
    /// Человек временно рисуется текстурой игрока (решение автора).
    pub(crate) fn entity_texture(&self, entity: &crate::simulation::SimEntity) -> &Handle<Image> {
        match entity.kind {
            EntityKind::Wolf => match entity.state {
                EntityState::Standing => &self.wolf_standing,
                EntityState::Lying => &self.wolf_lying,
                EntityState::Walking => &self.wolf_walk[entity.walk_frame() as usize],
            },
            EntityKind::Human => &self.player,
            EntityKind::Rabbit => match entity.state {
                EntityState::Standing => &self.rabbit_standing,
                EntityState::Lying => &self.rabbit_lying,
                EntityState::Walking => &self.rabbit_walk[entity.walk_frame() as usize],
            },
        }
    }

    /// Все текстуры существ: скан видимых областей (`EntitySpriteVisuals`)
    /// должен покрыть каждую — пикинг и рендер обязаны видеть один рисунок.
    /// Новый вид существ добавляет свою текстуру сюда же.
    pub(crate) fn entity_textures(&self) -> impl Iterator<Item = &Handle<Image>> {
        [
            &self.player,
            &self.wolf_standing,
            &self.wolf_lying,
            &self.wolf_walk[0],
            &self.wolf_walk[1],
            &self.rabbit_standing,
            &self.rabbit_lying,
            &self.rabbit_walk[0],
            &self.rabbit_walk[1],
            &self.rabbit_walk[2],
        ]
        .into_iter()
    }

    /// Хэндлы тайлов в порядке индексов атласа: Water … Podzol.
    pub(crate) fn atlas_sources(&self) -> [&Handle<Image>; 14] {
        [
            &self.water,
            &self.sand,
            &self.dirt,
            &self.grass,
            &self.forest_grass,
            &self.stone,
            &self.clay,
            &self.farmland,
            &self.deep_water,
            &self.ocean,
            &self.mountain,
            &self.moss,
            &self.mud,
            &self.podzol,
        ]
    }
}

/// Загружает текстуры при старте.
pub(crate) fn load_tile_textures(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(TileTextures::load(&assets));
}
