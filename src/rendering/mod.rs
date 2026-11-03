//! Рендеринг мира: меши terrain-блоков с LOD и спрайты объектов.
//!
//! Мир нарезан на блоки 64×64 тайлов. Видимые блоки рисуются одним мешем
//! на блок: все тайлы блока — квады в общей текстуре-атласе. При отдалении
//! камеры соседние тайлы агрегируются в ячейки (LOD), и в ячейке берётся
//! доминирующий тип тайла. Смена LOD атомарна: сцена нового уровня строится
//! на фоне старой, и старая остаётся на экране, пока новая не покроет весь
//! вид — иначе по краям экрана мигали бы чёрные куски недостроенных блоков.
//! Объекты (деревья, кусты, валуны, яблони) остаются спрайтами и спавнятся
//! только для видимых блоков, чтобы не держать в памяти миллионы сущностей.
//!
//! Состав модуля:
//! - `terrain` — сборка мешей блоков, LOD-переходы, dirty-перестройки, атлас;
//! - `sprites` — объекты/постройки/культуры/предметы на тайлах, дроп-сущности
//!   и существа, контуры выделения, вспышки урона, счётчики предметов;
//! - `hologram` — голограмма строительства;
//! - `isometric` — изометрическая проекция и визуальные высоты тайлов;
//! - `textures` — реестр текстур (`TileTextures`);
//! - `combat`, `lighting`, `lenses` — боевые эффекты, оверлей тьмы, линзы карты.

use bevy::prelude::*;
use bevy::sprite_render::Material2dPlugin;

use crate::app::state::SimTick;

pub(crate) mod isometric;

mod combat;
mod hologram;
mod lenses;
mod lighting;
mod sprites;
mod terrain;
mod textures;

// Часть реэкспортов ниже используется только тестами (`src/tests/rendering.rs`)
// и соседями внутри rendering — без `allow` бинарь считал бы их неиспользуемыми.
#[allow(unused_imports)]
pub(crate) use hologram::update_build_hologram;
#[allow(unused_imports)]
pub(crate) use sprites::{
    EntitySpriteVisuals, OUTLINE_THICKNESS_PX, build_outline_image, dilate_mask,
    entity_sprite_size, item_counts_visible, outline_radius_texels, scan_alpha_bounds,
    update_entity_sprite_visuals, update_entity_sprites, update_item_count_labels,
    update_item_entity_sprites, update_selection_outlines,
};
#[allow(unused_imports)]
pub(crate) use terrain::{
    BLOCK_TILES, BlockPos, LodTransition, MAX_LOD, RenderMetrics, TARGET_CELL_PX,
    TILE_KIND_COUNT, TerrainRender, atlas_uv, build_block_mesh, dominant_atlas_index,
    ensure_ground_material, lod_for_scale, lod_transition_for, render_world, tile_atlas_index,
};
#[allow(unused_imports)]
pub(crate) use textures::TileTextures;

pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<lighting::DarknessMaterial>::default())
            .add_systems(Startup, textures::load_tile_textures)
            .init_resource::<TerrainRender>()
            .init_resource::<RenderMetrics>()
            .init_resource::<sprites::EntitySprites>()
            .init_resource::<sprites::ItemEntitySprites>()
            .init_resource::<sprites::WorldSpriteAssets>()
            .init_resource::<EntitySpriteVisuals>()
            .init_resource::<sprites::HurtFlashes>()
            .init_resource::<sprites::SelectionOutlines>()
            .init_resource::<lighting::CampfireLights>()
            .init_resource::<lighting::DarknessOverlay>()
            .add_systems(
                Update,
                (
                    ensure_ground_material,
                    render_world,
                    // Счётчики предметов скрываются/показываются по зуму;
                    // новый LOD мог изменить набор этих сущностей.
                    update_item_count_labels,
                )
                    .chain()
                    .after(crate::app::camera::update_camera_transforms),
            )
            // Реестр костров сверяется с миром до render_world: тот изымает
            // dirty-тайлы, по которым отслеживаются новые и потухшие костры.
            .add_systems(
                Update,
                lighting::update_campfire_positions
                    .after(crate::app::plugin::apply_commands)
                    .before(render_world),
            )
            .add_systems(
                Update,
                (
                    // Кэш видимых областей — до спрайтов и контуров: из него
                    // берутся «ноги на землю» и радиус дилатации.
                    update_entity_sprite_visuals,
                    update_item_entity_sprites,
                    update_entity_sprites,
                    // Контур поверх спрайтов: рисуется по их же трансформам.
                    update_selection_outlines.after(update_entity_sprites),
                )
                    .after(SimTick)
                    .after(crate::app::camera::update_camera_transforms),
            )
            // Оверлей тьмы поверх мира ( lighting): после перестройки блоков,
            // чтобы камера и мир были актуальны.
            .add_systems(
                Update,
                lighting::update_darkness_overlay
                    .after(update_item_count_labels)
                    .after(crate::app::camera::update_camera_transforms),
            )
            .add_systems(Update, update_build_hologram.after(crate::app::camera::update_camera_transforms))
            // Боевые визуализации: след взмаха, снаряды, воткнутые копья.
            .add_plugins(combat::CombatFxPlugin)
            // Линзы карты (редактор): подсветка чанков по флагам.
            .add_plugins(lenses::LensPlugin);
    }
}
