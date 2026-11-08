//! Terrain-блоки: сборка мешей 64×64 тайлов, LOD, dirty-перестройки и атлас.
//!
//! Видимые блоки рисуются одним мешем на блок: все тайлы блока — квады в
//! общей текстуре-атласе. При отдалении камеры соседние тайлы агрегируются
//! в ячейки (LOD), и в ячейке берётся доминирующий тип тайла. Смена LOD
//! атомарна: сцена нового уровня строится на фоне старой, и старая остаётся
//! на экране, пока новая не покроет весь вид — иначе по краям экрана мигали
//! бы чёрные куски недостроенных блоков. Спрайты объектов/предметов блока
//! спавнит `sprites.rs`; здесь блоки только собираются и перестраиваются.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

use crate::app::camera::WorldCamera;
use crate::app::state::GameState;
use crate::simulation::{ChunkPos, TerrainTiles, TilePos, TileType, World, CHUNK_SIZE, TILE_SIZE};
use crate::ui::UiFont;

use super::isometric::{WORLD_TILE_SIZE, tile_elevation};
use super::sprites::{spawn_crops, spawn_items, spawn_objects};
use super::textures::TileTextures;

/// Сторона terrain-блока в тайлах. 64 делит все поддерживаемые размеры мира.
pub(crate) const BLOCK_TILES: i32 = 64;

/// Желаемый размер агрегированной ячейки на экране, в пикселях.
/// Менялось вручную при тюнинге — тест LOD-порогов берёт значение отсюда.
pub(crate) const TARGET_CELL_PX: f32 = 2.0;

/// Максимальный уровень детализации (ячейка 2^MAX_LOD тайлов).
pub(crate) const MAX_LOD: u32 = 6;

/// Размер одного тайла в исходных PNG.
const ATLAS_TILE_PX: u32 = 32;

/// Ширина повтора краёв вокруг тайла в атласе (защита от bleeding).
const ATLAS_GUTTER_PX: u32 = 2;

/// Сколько тайлов в атласе по горизонтали и вертикали.
const ATLAS_COLUMNS: u32 = 4;
const ATLAS_ROWS: u32 = 4;

/// Размер ячейки атласа с учётом повтора краёв.
const ATLAS_CELL_PX: u32 = ATLAS_TILE_PX + ATLAS_GUTTER_PX * 2;

/// Полный размер атласа.
const ATLAS_WIDTH: u32 = ATLAS_CELL_PX * ATLAS_COLUMNS;
const ATLAS_HEIGHT: u32 = ATLAS_CELL_PX * ATLAS_ROWS;

/// Количество непустых видов тайлов (индексы атласа). Варианты идут
/// подряд за `Empty`, порядок совпадает с `TileType` без него.
pub(crate) const TILE_KIND_COUNT: usize = 14;

/// Сколько блоков строить за кадр при LOD 0 (дальше бюджет растёт).
const BUILD_BLOCKS_BASE: usize = 4;

/// Верхняя граница бюджета построения блоков за кадр.
const BUILD_BLOCKS_MAX: usize = 32;

/// Запас вокруг видимой области, в тайлах (деревья выступают за свой тайл).
const VIEW_MARGIN_TILES: f32 = 10.0;

const TERRAIN_LENS_SHADER_PATH: &str = "shaders/terrain_lens.wesl";

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct TerrainLensExtension {}

impl MaterialExtension for TerrainLensExtension {
    fn fragment_shader() -> ShaderRef {
        TERRAIN_LENS_SHADER_PATH.into()
    }

    fn enable_prepass() -> bool {
        false
    }
}

pub(crate) type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainLensExtension>;

/// Построенный terrain-блок: сущность и её меш (если в блоке есть тайлы).
pub(crate) struct BuiltBlock {
    pub(crate) entity: Entity,
    pub(crate) mesh: Option<Handle<Mesh>>,
    pub(crate) lod: u32,
    pub(crate) fallback: bool,
}

/// Координаты terrain-блока в сетке блоков.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct BlockPos {
    pub x: i32,
    pub y: i32,
}

/// Состояние рендера мира: материал, активные блоки и переход LOD.
#[derive(Resource, Default)]
pub(crate) struct TerrainRender {
    pub(crate) material: Option<Handle<TerrainMaterial>>,
    /// Готовая сцена текущего LOD: видимые блоки.
    pub(crate) active: HashMap<BlockPos, BuiltBlock>,
    /// Строящаяся сцена нового LOD; активная остаётся на экране до её готовности.
    pub(crate) incoming: Option<IncomingLod>,
    pub(crate) blocks_per_side: i32,
    pub(crate) lod: u32,
    /// Множитель цвета terrain-тайлов по чанкам, задаваемый линзами.
    pub(crate) lens_tints: Arc<HashMap<ChunkPos, [f32; 4]>>,
}

/// Сцена нового LOD, достраивающаяся на фоне активной.
pub(crate) struct IncomingLod {
    pub(crate) lod: u32,
    pub(crate) blocks: HashMap<BlockPos, BuiltBlock>,
}

/// Счётчики рендера для FPS-панели (GAME_DESIGN 9, ARCHITECTURE 9):
/// события dirty по каналам, перестройки блоков, время сборки мешей,
/// число видимых блоков и спрайтов. События и время накапливаются с
/// прошлого опроса панели; панель сбрасывает их, читая. Только для
/// отладки, на симуляцию не влияет.
#[derive(Resource, Default)]
pub(crate) struct RenderMetrics {
    /// Terrain-события dirty с прошлого опроса панели.
    pub(crate) terrain_events: u32,
    /// Decor-события dirty (объекты/предметы) с прошлого опроса панели.
    pub(crate) decor_events: u32,
    /// Полных перестроек terrain-блоков с прошлого опроса панели.
    pub(crate) rebuilds: u32,
    /// Суммарное время `build_block_mesh` с прошлого опроса панели, мс.
    pub(crate) mesh_build_ms: f32,
    /// Видимых terrain-блоков на текущий кадр.
    pub(crate) visible_blocks: u32,
    /// Спрайтов-детей terrain-блоков на текущий кадр (объекты, предметы,
    /// счётчики).
    pub(crate) block_sprites: u32,
}

/// Метка сущности terrain-блока.
#[derive(Component)]
pub(crate) struct TerrainBlock;

pub(crate) fn ensure_ground_material(
    mut render: ResMut<TerrainRender>,
    textures: Res<TileTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<TerrainMaterial>>,
) {
    if render.material.is_some() {
        return;
    }
    let Some(atlas) = build_atlas_image(&textures, &images) else {
        return;
    };

    let atlas = images.add(atlas);
    render.material = Some(materials.add(ExtendedMaterial {
        base: StandardMaterial {
            base_color_texture: Some(atlas),
            alpha_mode: AlphaMode::Opaque,
            unlit: true,
            ..default()
        },
        extension: TerrainLensExtension {},
    }));
}

/// Собирает атлас 4×4 из четырнадцати 32×32 тайлов с повтором краёв.
/// Возвращает `None`, пока хотя бы одна исходная текстура не загружена.
fn build_atlas_image(textures: &TileTextures, images: &Assets<Image>) -> Option<Image> {
    let mut data = vec![0_u8; (ATLAS_WIDTH * ATLAS_HEIGHT * 4) as usize];

    for (index, handle) in textures.atlas_sources().into_iter().enumerate() {
        let image = images.get(handle)?;
        let pixels = tile_pixels(image);
        let fallback = kind_for_atlas_index(index as u8)
            .map(|kind| color_to_rgba8(kind.placeholder_color()))
            .unwrap_or([255, 0, 255, 255]);
        write_atlas_cell(&mut data, index as u32, pixels, fallback);
    }
    let mut atlas = Image::new(
        Extent3d {
            width: ATLAS_WIDTH,
            height: ATLAS_HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    atlas.sampler = ImageSampler::nearest();
    Some(atlas)
}

/// Проверяет формат и размер изображения и возвращает его RGBA8-пиксели.
fn tile_pixels(image: &Image) -> Option<&[u8]> {
    if image.texture_descriptor.format != TextureFormat::Rgba8UnormSrgb
        || image.width() != ATLAS_TILE_PX || image.height() != ATLAS_TILE_PX
    {
        return None;
    }
    image.data.as_deref()
}

/// Записывает одну ячейку атласа и повторяет её края в гуттере.
/// Если исходных пикселей нет, ячейка заливается запасным цветом.
fn write_atlas_cell(data: &mut [u8], index: u32, pixels: Option<&[u8]>, fallback: [u8; 4]) {
    let cell_x = (index % ATLAS_COLUMNS) * ATLAS_CELL_PX;
    let cell_y = (index / ATLAS_COLUMNS) * ATLAS_CELL_PX;
    let max_src = ATLAS_TILE_PX as i32 - 1;

    for dy in 0..ATLAS_CELL_PX {
        for dx in 0..ATLAS_CELL_PX {
            let sx = (dx as i32 - ATLAS_GUTTER_PX as i32).clamp(0, max_src) as u32;
            let sy = (dy as i32 - ATLAS_GUTTER_PX as i32).clamp(0, max_src) as u32;
            let dst = (((cell_y + dy) * ATLAS_WIDTH + cell_x + dx) * 4) as usize;
            let src = ((sy * ATLAS_TILE_PX + sx) * 4) as usize;
            match pixels.and_then(|pixels| pixels.get(src..src + 4)) {
                Some(pixel) => data[dst..dst + 4].copy_from_slice(pixel),
                None => data[dst..dst + 4].copy_from_slice(&fallback),
            }
        }
    }
}

/// Переводит цвет в байты RGBA8.
fn color_to_rgba8(color: [f32; 4]) -> [u8; 4] {
    [
        (color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[3].clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

/// UV-прямоугольник тайла в атласе: `[u0, v0, u1, v1]`, где v0 — верх.
pub(crate) fn atlas_uv(index: u8) -> [f32; 4] {
    let col = index as u32 % ATLAS_COLUMNS;
    let row = index as u32 / ATLAS_COLUMNS;
    let x0 = col * ATLAS_CELL_PX + ATLAS_GUTTER_PX;
    let y0 = row * ATLAS_CELL_PX + ATLAS_GUTTER_PX;
    [
        x0 as f32 / ATLAS_WIDTH as f32,
        y0 as f32 / ATLAS_HEIGHT as f32,
        (x0 + ATLAS_TILE_PX) as f32 / ATLAS_WIDTH as f32,
        (y0 + ATLAS_TILE_PX) as f32 / ATLAS_HEIGHT as f32,
    ]
}

/// Перерисовывает видимые блоки мира и их деревья.
pub(crate) fn render_world(
    mut commands: Commands,
    mut state: ResMut<GameState>,
    mut render: ResMut<TerrainRender>,
    mut metrics: ResMut<RenderMetrics>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    terrain_materials: Res<Assets<TerrainMaterial>>,
    mut sprite_assets: ResMut<super::sprites::WorldSpriteAssets>,
    images: Res<Assets<Image>>,
    textures: Res<TileTextures>,
    font: Res<UiFont>,
    camera: Query<(&Camera, &Transform, &Projection), With<WorldCamera>>,
    terrain_blocks: Query<&Children, With<TerrainBlock>>,
) {
    // Мир перегенерирован: убираем обе сцены и начинаем с нуля.
    if state.pending_rerender {
        state.pending_rerender = false;
        state.dirty_tiles.clear();
        clear_all_blocks(&mut commands, &mut meshes, &mut render);
        render.blocks_per_side = state
            .world
            .as_ref()
            .map(|world| blocks_per_side(world.size.tiles() as i32))
            .unwrap_or(0);
        render.lod = 0;
    }

    // Изменённые тайлы изымаются до заимствования мира: список
    // обнуляется на месте, «расходуется» рендером.
    let dirty = std::mem::take(&mut state.dirty_tiles);
    metrics.terrain_events += dirty.terrain.len() as u32;
    metrics.decor_events += dirty.decor.len() as u32;

    let Some(world) = state.world.as_ref() else {
        return;
    };
    let Some(material) = render.material.clone() else {
        return;
    };
    if terrain_materials.get(&material).is_none() {
        return;
    }
    let Ok((camera, camera_transform, projection)) = camera.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let lens_tints = Arc::clone(&render.lens_tints);
    // Точечная перестройка terrain-блоков (инструменты редактора, добыча
    // травы, растительность): блок пересобирается целиком, вместе с
    // дочерними объектами.
    if !dirty.terrain.is_empty() {
        rebuild_dirty_blocks(
            &mut commands, &mut meshes, &mut materials, &mut sprite_assets, &images, world,
            &material, camera_transform.rotation, &textures, &font,
            &dirty.terrain, &mut render, &mut metrics, &lens_tints,
        );
    }
    // Объекты и предметы (декор) не меняют геометрию меша: достаточно
    // пересобрать дочерние спрайты затронутых блоков.
    if !dirty.decor.is_empty() {
        refresh_decor(
            &mut commands, &mut meshes, &mut materials, &mut sprite_assets, &images, world,
            camera_transform.rotation, &textures, &font, &dirty.decor, &render,
        );
    }
    if ortho.area.width() <= 0.0 || ortho.area.height() <= 0.0 {
        return;
    }
    let Some(viewport_size) = camera.logical_viewport_size() else {
        return;
    };
    // Проверяем проекцию 3D-границ каждого блока, а не пересечения лучей
    // только по углам viewport. Это остаётся консервативным для любого
    // промежуточного рельефа и не теряет блоки при панорамировании.
    let world_tiles = world.size.tiles() as i32;
    let half_view = viewport_size * (ortho.scale * 0.5);
    let margin = VIEW_MARGIN_TILES * WORLD_TILE_SIZE;
    let mut visible = Vec::new();
    collect_visible_blocks(
        0, 0, render.blocks_per_side, render.blocks_per_side, world_tiles, 
        camera_transform, half_view, margin, ortho.near, ortho.far, &mut visible,
    );
    if visible.is_empty() {
        clear_all_blocks(&mut commands, &mut meshes, &mut render);
        return;
    }

    // Строим недостающие блоки от центра экрана наружу, с бюджетом на кадр.
    let forward = camera_transform.rotation * -Vec3::Z;
    let view_center = camera_transform.translation  +  forward * (-camera_transform.translation.z / forward.z);
    let view_center = view_center.truncate();
    visible.sort_by(|a, b| {
        block_distance_sq(*a, view_center).total_cmp(&block_distance_sq(*b, view_center))
    });
    let visible_set: HashSet<BlockPos> = visible.iter().copied().collect();

    let lenses_enabled = state.lenses.sentients || state.lenses.buildings;
    let lod = lod_for_scale(ortho.scale).min(if lenses_enabled { 4 } else { MAX_LOD });

    // Управление переходом: активная сцена остаётся на экране, пока строится новая.
    match lod_transition_for(lod, render.lod, render.incoming.as_ref().map(|inc| inc.lod)) {
        LodTransition::None | LodTransition::Continue => {}
        LodTransition::Cancel => drop_incoming(&mut commands, &mut meshes, &mut render),
        LodTransition::Start => {
            drop_incoming(&mut commands, &mut meshes, &mut render);
            render.incoming = Some(IncomingLod {
                lod, blocks: HashMap::new(),
            });
        }
    }
    // Новая часть viewport сразу получает дешёвый плоский placeholder,
    // вместо чёрного фона на время построения полноразмерного блока.
    for block in visible.iter().copied() {
        render
            .active
            .entry(block)
            .or_insert_with(|| build_fallback_block(
                &mut commands, &mut meshes, world, &material, block,
            ));
    }

    // Убираем блоки активной сцены, которые перестали быть видимыми.
    if render.incoming.is_none() {
        let stale: Vec<BlockPos> = render
            .active
            .keys()
            .copied()
            .filter(|pos| !visible_set.contains(pos))
            .collect();
        for pos in stale {
            if let Some(block) = render.active.remove(&pos) {
                despawn_block(&mut commands, &mut meshes, block);
            }
        }
    }
    let budget = (BUILD_BLOCKS_BASE << lod.min(3)).min(BUILD_BLOCKS_MAX);
    let mut built_now = 0_usize;

    // Во время перехода активную сцену не наращиваем: она всё равно будет
    // заменена целиком, а бюджет нужен на новую. Достраивание работает,
    // только когда перехода нет.
    if render.incoming.is_none() {
        for block in visible.iter().copied() {
            if built_now >= budget {
                break;
            }
            let needs_rebuild = render
                .active
                .get(&block)
                .is_some_and(|built| built.fallback || built.lod != lod);
            if !needs_rebuild {
                continue;
            }
            if let Some(old) = render.active.remove(&block) {
                despawn_block(&mut commands, &mut meshes, old);
            }
            let built = build_block(
                &mut commands, &mut meshes, &mut materials, &mut sprite_assets, &images, world,
                &material, camera_transform.rotation, &textures, &font,
                &mut metrics, block, lod, &lens_tints,
            );
            render.active.insert(block, built);
            built_now += 1;
        }
    }

    // Достраиваем сцену нового LOD; когда она покрывает весь вид — подменяем
    // активную одним кадром, чтобы на экране не было чёрных кусков.
    // Карта блоков временно изымается, чтобы не конфликтовать с &render.
    let mut swap_ready = false;
    if render.incoming.is_some() {
        let incoming = render.incoming.as_mut().unwrap();
        let incoming_lod = incoming.lod;
        let mut blocks = std::mem::take(&mut incoming.blocks);

        let mut missing = visible
            .iter()
            .filter(|block| !blocks.contains_key(block))
            .count();
        for block in visible.iter().copied() {
            if built_now >= budget {
                break;
            }
            if blocks.contains_key(&block) {
                continue;
            }
            let built = build_block(
                &mut commands, &mut meshes, &mut materials, &mut sprite_assets, &images,
                world, &material,
                camera_transform.rotation, &textures, &font, &mut metrics, block, incoming_lod,
                &lens_tints,
            );
            commands.entity(built.entity).insert(Visibility::Hidden);
            blocks.insert(block, built);
            built_now += 1;
            missing -= 1;
        }
        swap_ready = missing == 0;
        render.incoming.as_mut().unwrap().blocks = blocks;
    }
    if swap_ready {
        promote_incoming(
            &mut commands, &mut meshes, &mut render, &visible_set,
        );
    }

    // Абсолютные счётчики текущего кадра (панель показывает их без пересчёта).
    metrics.visible_blocks = render.active.len() as u32;
    metrics.block_sprites = render
        .active
        .values()
        .filter_map(|block| terrain_blocks.get(block.entity).ok())
        .map(|children| children.len() as u32)
        .sum();
}

/// Удаляет строящуюся сцену нового LOD (отмена перехода или перезапуск).
fn drop_incoming(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    render: &mut TerrainRender,
) {
    if let Some(incoming) = render.incoming.take() {
        for (_, block) in incoming.blocks {
            despawn_block(commands, meshes, block);
        }
    }
}

/// Атомарно подменяет активную сцену строящейся: старая удаляется, новая
/// становится текущей. Блоки за пределами видимой области не переносятся.
fn promote_incoming(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    render: &mut TerrainRender,
    visible: &HashSet<BlockPos>,
) {
    let Some(incoming) = render.incoming.take() else {
        return;
    };
    for (_, block) in render.active.drain() {
        despawn_block(commands, meshes, block);
    }
    for (pos, block) in incoming.blocks {
        if !visible.contains(&pos) {
            despawn_block(commands, meshes, block);
        } else {
            commands.entity(block.entity).insert(Visibility::Visible);
            render.active.insert(pos, block);
        }
    }
    render.lod = incoming.lod;
}

/// Удаляет все сцены (при перегенерации мира или уходе мира с экрана).
fn clear_all_blocks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    render: &mut TerrainRender,
) {
    for (_, block) in render.active.drain() {
        despawn_block(commands, meshes, block);
    }
    drop_incoming(commands, meshes, render);
}

/// Перестраивает terrain-блоки, содержащие изменённые тайлы, и блоки справа
/// или сверху, чьи видимые боковые грани зависят от изменённой высоты.
/// Блок пересобирается целиком: объекты — дети сущности блока, так что и
/// они подхватят изменения. Вне бюджета построения: правок за кадр немного,
/// а видимость результата должна быть мгновенной.
fn rebuild_dirty_blocks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    sprite_assets: &mut super::sprites::WorldSpriteAssets,
    images: &Assets<Image>,
    world: &World,
    material: &Handle<TerrainMaterial>,
    camera_rotation: Quat,
    textures: &TileTextures,
    font: &Res<UiFont>,
    dirty: &[TilePos],
    render: &mut TerrainRender,
    metrics: &mut RenderMetrics,
    lens_tints: &HashMap<ChunkPos, [f32; 4]>,
) {
    let mut blocks: HashSet<BlockPos> = HashSet::with_capacity(dirty.len());
    for tile in dirty {
        let block = BlockPos {
            x: tile.x.div_euclid(BLOCK_TILES),
            y: tile.y.div_euclid(BLOCK_TILES),
        };
        blocks.insert(block);
        let local_x = tile.x.rem_euclid(BLOCK_TILES);
        let local_y = tile.y.rem_euclid(BLOCK_TILES);
        if local_x == BLOCK_TILES - 1 {
            blocks.insert(BlockPos { x: block.x + 1, y: block.y });
        }
        if local_y == BLOCK_TILES - 1 {
            blocks.insert(BlockPos { x: block.x, y: block.y + 1 });
        }
    }

    let stale: Vec<BlockPos> = render
        .active
        .keys()
        .copied()
        .filter(|pos| blocks.contains(pos))
        .collect();
    for pos in stale {
        if let Some(block) = render.active.remove(&pos) {
            despawn_block(commands, meshes, block);
            let built = build_block(
                commands, meshes, materials, sprite_assets, images, world, material,
                camera_rotation, textures, font, metrics, pos, render.lod,
                lens_tints,
            );
            render.active.insert(pos, built);
            metrics.rebuilds += 1;
        }
    }

    if let Some(incoming) = render.incoming.as_mut() {
        let lod = incoming.lod;
        let stale: Vec<BlockPos> = incoming
            .blocks
            .keys()
            .copied()
            .filter(|pos| blocks.contains(pos))
            .collect();
        for pos in stale {
            if let Some(block) = incoming.blocks.remove(&pos) {
                despawn_block(commands, meshes, block);
                let built =
                    build_block(
                        commands, meshes, materials, sprite_assets, images, world,
                        material, camera_rotation, textures, font, metrics, pos, lod,
                        lens_tints,
                    );
                commands.entity(built.entity).insert(Visibility::Hidden);
                incoming.blocks.insert(pos, built);
                metrics.rebuilds += 1;
            }
        }
    }
}

/// Обновляет декор (объекты и предметы) блоков с изменёнными тайлами, не
/// трогая terrain-меш: изменение объекта/предмета не меняет геометрию.
/// Дети блока снимаются целиком и спавнятся заново — тот же путь, что при
/// постройке блока. Декор рисуется только на LOD ≤ 1: на дальних блоках и
/// в fallback-блоках обновлять нечего.
fn refresh_decor(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    sprite_assets: &mut super::sprites::WorldSpriteAssets,
    images: &Assets<Image>,
    world: &World,
    camera_rotation: Quat,
    textures: &TileTextures,
    font: &Res<UiFont>,
    decor: &[TilePos],
    render: &TerrainRender,
) {
    let mut blocks: HashSet<BlockPos> = HashSet::with_capacity(decor.len());
    for tile in decor {
        blocks.insert(BlockPos {
            x: tile.x.div_euclid(BLOCK_TILES),
            y: tile.y.div_euclid(BLOCK_TILES),
        });
    }
    for block in blocks {
        if let Some(built) = render.active.get(&block) {
            refresh_block_decor(
                built, commands, meshes, materials, sprite_assets, images, world,
                camera_rotation, textures, font, block,
            );
        }
        if let Some(incoming) = render.incoming.as_ref() && let Some(built) = incoming.blocks.get(&block) {
            refresh_block_decor(
                built, commands, meshes, materials, sprite_assets, images, world,
                camera_rotation, textures, font, block,
            );
        }
    }
}

/// Пересобирает детей одного блока (объекты и предметы на земле).
fn refresh_block_decor(
    built: &BuiltBlock,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    sprite_assets: &mut super::sprites::WorldSpriteAssets,
    images: &Assets<Image>,
    world: &World,
    camera_rotation: Quat,
    textures: &TileTextures,
    font: &Res<UiFont>,
    block: BlockPos,
) {
    if built.fallback || built.lod > 1 {
        return;
    }
    let Ok(mut entity) = commands.get_entity(built.entity) else {
        return;
    };
    entity.despawn_related::<Children>();
    let origin = block_origin(block);
    spawn_objects(
        &mut entity, meshes, materials, sprite_assets, images, world, block, origin, camera_rotation, textures,
    );
    spawn_crops(
        &mut entity, meshes, materials, sprite_assets, world, block, origin, textures,
    );
    spawn_items(
        &mut entity, meshes, materials, sprite_assets, world, block, origin, camera_rotation, textures, &font.0,
    );
}

/// Деспавнит сущность блока и освобождает его меш.
pub(crate) fn despawn_block(commands: &mut Commands, meshes: &mut Assets<Mesh>, block: BuiltBlock) {
    commands.entity(block.entity).despawn();
    if let Some(mesh) = block.mesh {
        meshes.remove(&mesh);
    }
}

/// Строит один блок: меш по LOD, сущность с материалом и объекты блока.
fn build_block(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    sprite_assets: &mut super::sprites::WorldSpriteAssets,
    images: &Assets<Image>,
    world: &World,
    material: &Handle<TerrainMaterial>,
    camera_rotation: Quat,
    textures: &TileTextures,
    font: &Res<UiFont>,
    metrics: &mut RenderMetrics,
    block: BlockPos,
    lod: u32,
    lens_tints: &HashMap<ChunkPos, [f32; 4]>,
) -> BuiltBlock {
    let started = std::time::Instant::now();
    let mesh = build_block_mesh(world, block, lod).map(|mut mesh| {
        if lod <= 4 && block_has_lens_tint(block, lens_tints) {
            set_block_lens_colors(&mut mesh, block, lens_tints);
        }
        meshes.add(mesh)
    });
    metrics.mesh_build_ms += started.elapsed().as_secs_f32() * 1000.0;
    let origin = block_origin(block);

    let mut entity = commands.spawn((
        Transform::from_translation(origin),
        Visibility::default(),
        TerrainBlock,
    ));
    if let Some(handle) = mesh.clone() {
        entity.insert((Mesh3d(handle), MeshMaterial3d(material.clone())));
    }
    // Объекты рисуются только на ближних LOD: дальше они мельче ячейки сетки.
    if lod <= 1 {
        spawn_objects(
            &mut entity, meshes, materials, sprite_assets, images, world, block, origin, camera_rotation, textures,
        );
        spawn_crops(
            &mut entity, meshes, materials, sprite_assets, world, block, origin, textures,
        );
        spawn_items(
            &mut entity, meshes, materials, sprite_assets, world, block, origin, camera_rotation, textures, &font.0,
        );
    }
    BuiltBlock {
        entity: entity.id(),
        mesh,
        lod,
        fallback: false,
    }
}

fn block_has_lens_tint(block: BlockPos, tints: &HashMap<ChunkPos, [f32; 4]>) -> bool {
    let chunks_per_block = BLOCK_TILES / CHUNK_SIZE as i32;
    let chunk_x0 = (block.x * BLOCK_TILES).div_euclid(CHUNK_SIZE as i32);
    let chunk_y0 = (block.y * BLOCK_TILES).div_euclid(CHUNK_SIZE as i32);
    (0..chunks_per_block).any(|y| {
        (0..chunks_per_block).any(|x| {
            tints.contains_key(&ChunkPos {
                x: (chunk_x0 + x) as u32,
                y: (chunk_y0 + y) as u32,
            })
        })
    })
}

fn build_fallback_block(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    world: &World,
    material: &Handle<TerrainMaterial>,
    block: BlockPos,
) -> BuiltBlock {
    let mesh = build_fallback_block_mesh(world, block).map(|mesh| meshes.add(mesh));
    let mut entity = commands.spawn((
        Transform::from_translation(block_origin(block)),
        Visibility::default(),
        TerrainBlock,
    ));
    if let Some(handle) = mesh.clone() {
        entity.insert((Mesh3d(handle), MeshMaterial3d(material.clone())));
    }
    BuiltBlock {
        entity: entity.id(),
        mesh,
        lod: MAX_LOD,
        fallback: true,
    }
}

fn build_fallback_block_mesh(world: &World, block: BlockPos) -> Option<Mesh> {
    const SAMPLE_STEP: i32 = 8;
    let base_x = block.x * BLOCK_TILES;
    let base_y = block.y * BLOCK_TILES;
    let world_tiles = world.size.tiles() as i32;
    let mut counts = [0_u32; TILE_KIND_COUNT];

    for y in (0..BLOCK_TILES).step_by(SAMPLE_STEP as usize) {
        for x in (0..BLOCK_TILES).step_by(SAMPLE_STEP as usize) {
            let tile = TilePos {
                x: base_x + x,
                y: base_y + y,
            };
            if tile.x >= world_tiles || tile.y >= world_tiles {
                continue;
            }
            if let Some(index) = tile_atlas_index(world.terrain_kind(tile)) {
                counts[index as usize] += 1;
            }
        }
    }
    let kind = kind_for_atlas_index(dominant_atlas_index(&counts)?)?;
    let index = tile_atlas_index(kind)?;
    let elevation = tile_elevation(kind);
    let width_tiles = (world_tiles - base_x).clamp(0, BLOCK_TILES) as f32;
    let height_tiles = (world_tiles - base_y).clamp(0, BLOCK_TILES) as f32;
    let points = [
        [0.0, 0.0, elevation],
        [width_tiles * WORLD_TILE_SIZE, 0.0, elevation],
        [
            width_tiles * WORLD_TILE_SIZE,
            height_tiles * WORLD_TILE_SIZE,
            elevation,
        ],
        [0.0, height_tiles * WORLD_TILE_SIZE, elevation],
    ];
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, points.to_vec());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0; 4]; points.len()]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        atlas_uv_coordinates(index).to_vec(),
    );
    mesh.insert_indices(Indices::U32([0, 1, 2, 0, 2, 3].into()));
    // Нормали не считаем: материал terrain unlit (см. ensure_ground_material),
    // шейдер без атрибута нормали компилируется без VERTEX_NORMALS.
    Some(mesh)
}

/// Сколько блоков по стороне нужно для мира из `tiles` тайлов.
fn blocks_per_side(tiles: i32) -> i32 {
    (tiles + BLOCK_TILES - 1) / BLOCK_TILES
}

fn collect_visible_blocks(
    bx0: i32,
    by0: i32,
    bx1: i32,
    by1: i32,
    world_tiles: i32,
    camera: &Transform,
    half_view: Vec2,
    margin: f32,
    near: f32,
    far: f32,
    visible: &mut Vec<BlockPos>,
) {
    if bx0 >= bx1 || by0 >= by1 || !block_region_intersects_view( bx0, by0, bx1, by1, world_tiles, camera, half_view, margin, near, far ) {
        return;
    }
    if bx1 - bx0 == 1 && by1 - by0 == 1 {
        visible.push(BlockPos { x: bx0, y: by0 });
        return;
    }

    let mid_x = bx0 + (bx1 - bx0 + 1) / 2;
    let mid_y = by0 + (by1 - by0 + 1) / 2;
    let x_ranges = if bx1 - bx0 > 1 {
        [(bx0, mid_x), (mid_x, bx1)]
    } else {
        [(bx0, bx1), (bx1, bx1)]
    };
    let y_ranges = if by1 - by0 > 1 {
        [(by0, mid_y), (mid_y, by1)]
    } else {
        [(by0, by1), (by1, by1)]
    };

    for (x0, x1) in x_ranges {
        for (y0, y1) in y_ranges {
            collect_visible_blocks(
                x0, y0, x1, y1, world_tiles, camera, half_view, margin, near, far, visible,
            );
        }
    }
}

fn block_region_intersects_view(
    bx0: i32,
    by0: i32,
    bx1: i32,
    by1: i32,
    world_tiles: i32,
    camera: &Transform,
    half_view: Vec2,
    margin: f32,
    near: f32,
    far: f32,
) -> bool {
    let min = Vec3::new(
        (bx0 * BLOCK_TILES) as f32 * WORLD_TILE_SIZE,
        (by0 * BLOCK_TILES) as f32 * WORLD_TILE_SIZE,
        0.0,
    );
    let max = Vec3::new(
        (bx1 * BLOCK_TILES).min(world_tiles) as f32 * WORLD_TILE_SIZE,
        (by1 * BLOCK_TILES).min(world_tiles) as f32 * WORLD_TILE_SIZE,
        tile_elevation(TileType::Mountain),
    );
    let camera_position = camera.translation;
    let rotation = camera.rotation;
    let right = rotation * Vec3::X;
    let up = rotation * Vec3::Y;
    let forward = rotation * -Vec3::Z;

    let projected_range = |axis: Vec3| {
        let low = axis.x * if axis.x >= 0.0 { min.x } else { max.x }
            + axis.y * if axis.y >= 0.0 { min.y } else { max.y }
            + axis.z * if axis.z >= 0.0 { min.z } else { max.z }
            - axis.dot(camera_position);
        let high = axis.x * if axis.x >= 0.0 { max.x } else { min.x }
            + axis.y * if axis.y >= 0.0 { max.y } else { min.y }
            + axis.z * if axis.z >= 0.0 { max.z } else { min.z }
            - axis.dot(camera_position);
        (low, high)
    };
    let (screen_left, screen_right) = projected_range(right);
    let (screen_bottom, screen_top) = projected_range(up);
    let (depth_near, depth_far) = projected_range(forward);

    screen_right >= -half_view.x - margin
        && screen_left <= half_view.x + margin
        && screen_top >= -half_view.y - margin
        && screen_bottom <= half_view.y + margin
        && depth_far >= near
        && depth_near <= far
}

/// Начало блока в координатах 3D-сетки.
pub(crate) fn block_origin(block: BlockPos) -> Vec3 {
    Vec3::new(
        (block.x * BLOCK_TILES) as f32,
        (block.y * BLOCK_TILES) as f32,
        0.0,
    ) * WORLD_TILE_SIZE
}

/// Квадрат расстояния от центра блока до точки (для порядка построения).
fn block_distance_sq(block: BlockPos, point: Vec2) -> f32 {
    let base = Vec2::new(
        (block.x * BLOCK_TILES) as f32,
        (block.y * BLOCK_TILES) as f32,
    );
    let center = (base + Vec2::splat(BLOCK_TILES as f32 * 0.5)) * WORLD_TILE_SIZE;
    center.distance_squared(point)
}

/// Подбирает LOD так, чтобы агрегированная ячейка была не меньше `TARGET_CELL_PX`.
pub(crate) fn lod_for_scale(scale: f32) -> u32 {
    let ratio = (TARGET_CELL_PX * scale / TILE_SIZE).max(1.0);
    let lod = ratio.log2().ceil();
    if lod <= 0.0 {
        0
    } else {
        (lod as u32).min(MAX_LOD)
    }
}

/// Решение по переходу LOD на этот кадр.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LodTransition {
    /// Перехода нет: активный LOD совпадает с желаемым.
    None,
    /// Камера вернулась к активному LOD — строящуюся сцену отменяем.
    Cancel,
    /// Продолжаем строить сцену желаемого LOD.
    Continue,
    /// Желаемый LOD сменился в процессе строительства — начинаем заново.
    Start,
}

/// Сопоставляет желаемый LOD камеры, активный LOD и LOD строящейся сцены.
pub(crate) fn lod_transition_for(
    desired: u32,
    active: u32,
    incoming: Option<u32>,
) -> LodTransition {
    match (desired == active, incoming) {
        (true, None) => LodTransition::None,
        (true, Some(_)) => LodTransition::Cancel,
        (false, Some(lod)) if lod == desired => LodTransition::Continue,
        (false, _) => LodTransition::Start,
    }
}

/// Строит верх ячейки и грани перепада высоты, обращённые к фиксированной камере.
pub(crate) fn build_block_mesh(world: &World, block: BlockPos, lod: u32) -> Option<Mesh> {
    let cells = (BLOCK_TILES >> lod) as usize;
    let cell_tiles = 1_i32 << lod;
    let world_tiles = world.size.tiles() as i32;
    let base_x = block.x * BLOCK_TILES;
    let base_y = block.y * BLOCK_TILES;
    let mut kinds = vec![None; cells * cells];

    for cy in 0..cells {
        for cx in 0..cells {
            let x0 = base_x + cx as i32 * cell_tiles;
            let y0 = base_y + cy as i32 * cell_tiles;
            kinds[cy * cells + cx] = cell_kind(world, x0, y0, cell_tiles, world_tiles);
        }
    }
    let mut positions = Vec::with_capacity(cells * cells * 20);
    let mut uvs = Vec::with_capacity(cells * cells * 20);
    let mut indices = Vec::with_capacity(cells * cells * 30);
    let mut push_quad = |points: [[f32; 3]; 4], texture_uvs: [[f32; 2]; 4]| {
        let base = positions.len() as u32;
        positions.extend(points);
        uvs.extend(texture_uvs);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    let point = |x: f32, y: f32, elevation: f32| {
        [x * WORLD_TILE_SIZE, y * WORLD_TILE_SIZE, elevation]
    };
    for cy in 0..cells {
        for cx in 0..cells {
            let Some(kind) = kinds[cy * cells + cx] else {
                continue;
            };
            let Some(index) = tile_atlas_index(kind) else {
                continue;
            };
            let x0 = (base_x + cx as i32 * cell_tiles) as f32;
            let y0 = (base_y + cy as i32 * cell_tiles) as f32;
            let lx0 = cx as f32 * cell_tiles as f32;
            let ly0 = cy as f32 * cell_tiles as f32;
            let lx1 = lx0 + cell_tiles as f32;
            let ly1 = ly0 + cell_tiles as f32;
            let elevation = tile_elevation(kind);

            push_quad(
                [
                    point(lx0, ly0, elevation),
                    point(lx1, ly0, elevation),
                    point(lx1, ly1, elevation),
                    point(lx0, ly1, elevation),
                ],
                atlas_uv_coordinates(index),
            );

            let edges = [
                (cx as i32 - 1, cy as i32, x0 as i32 - cell_tiles, y0 as i32, 0_u8),
                (cx as i32, cy as i32 - 1, x0 as i32, y0 as i32 - cell_tiles, 2),
            ];
            for (nx, ny, neighbor_x, neighbor_y, edge) in edges {
                let neighbor_kind = if nx >= 0
                    && ny >= 0
                    && nx < cells as i32
                    && ny < cells as i32
                {
                    kinds[ny as usize * cells + nx as usize]
                } else {
                    cell_kind(world, neighbor_x, neighbor_y, cell_tiles, world_tiles)
                };
                let neighbor_elevation = neighbor_kind.map(tile_elevation).unwrap_or(0.0);
                if neighbor_elevation >= elevation {
                    continue;
                }
                let wall = match edge {
                    0 => [
                        point(lx0, ly1, neighbor_elevation),
                        point(lx0, ly0, neighbor_elevation),
                        point(lx0, ly0, elevation),
                        point(lx0, ly1, elevation),
                    ],
                    _ => [
                        point(lx0, ly0, neighbor_elevation),
                        point(lx1, ly0, neighbor_elevation),
                        point(lx1, ly0, elevation),
                        point(lx0, ly0, elevation),
                    ],
                };
                push_quad(wall, atlas_uv_coordinates(index));
            }
        }
    }

    if positions.is_empty() {
        return None;
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        // Линзы переокрашивают вершины после extraction, поэтому terrain
        // сохраняет атрибуты в MainWorld до удаления блока.
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    let vertex_count = positions.len();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0; 4]; vertex_count]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    // Нормали не считаем: материал terrain unlit, освещение по нормалям
    // не считается, а без атрибута Bevy не включает VERTEX_NORMALS в шейдере.
    // Это ~2/3 стоимости сборки меша (профиль: compute_normals был 1.38%).
    Some(mesh)
}

/// Обновляет только цвета terrain-квадов, не меняя геометрию, UV или
/// глубину. При включённых линзах LOD не выше размера чанка, поэтому каждый
/// квад полностью принадлежит ровно одному чанку.
pub(crate) fn set_block_lens_colors(
    mesh: &mut Mesh,
    block: BlockPos,
    tints: &HashMap<ChunkPos, [f32; 4]>,
) {
    let positions = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|attribute| attribute.as_float3())
        .expect("terrain mesh has float3 positions");
    assert_eq!(positions.len() % 4, 0, "terrain mesh consists of quads");
    let block_x = (block.x * BLOCK_TILES) as f32;
    let block_y = (block.y * BLOCK_TILES) as f32;
    let mut colors = Vec::with_capacity(positions.len());
    for quad in positions.chunks_exact(4) {
        let center_x = quad.iter().map(|point| point[0]).sum::<f32>()
            / (4.0 * WORLD_TILE_SIZE)
            + block_x
            + 1e-4;
        let center_y = quad.iter().map(|point| point[1]).sum::<f32>()
            / (4.0 * WORLD_TILE_SIZE)
            + block_y
            + 1e-4;
        let tile = TilePos {
            x: center_x.floor() as i32,
            y: center_y.floor() as i32,
        };
        let chunk = ChunkPos {
            x: tile.x.div_euclid(CHUNK_SIZE as i32) as u32,
            y: tile.y.div_euclid(CHUNK_SIZE as i32) as u32,
        };
        let color = tints.get(&chunk).copied().unwrap_or([1.0; 4]);
        colors.extend([color; 4]);
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
}

fn atlas_uv_coordinates(index: u8) -> [[f32; 2]; 4] {
    let [u0, v0, u1, v1] = atlas_uv(index);
    [[u0, v1], [u1, v1], [u1, v0], [u0, v0]]
}

fn cell_kind(world: &World, x0: i32, y0: i32, cell: i32, world_tiles: i32) -> Option<TileType> {
    if let Some(index) = uniform_cell_index(world, x0, y0, cell, world_tiles) {
        return kind_for_atlas_index(index);
    }
    let mut counts = [0_u32; TILE_KIND_COUNT];
    for y in y0..y0 + cell {
        for x in x0..x0 + cell {
            if x < 0 || y < 0 || x >= world_tiles || y >= world_tiles {
                continue;
            }
            if let Some(index) = tile_atlas_index(world.terrain_kind(TilePos { x, y })) {
                counts[index as usize] += 1;
            }
        }
    }
    dominant_atlas_index(&counts).and_then(kind_for_atlas_index)
}

/// Если ячейка `[x0, x0+cell) × [y0, y0+cell)` целиком лежит внутри мира
/// и внутри одного чанка с однородным рельефом, возвращает индекс атласа.
/// Иначе None — ячейку считает общий путь с подсчётом тайлов.
fn uniform_cell_index(world: &World, x0: i32, y0: i32, cell: i32, world_tiles: i32) -> Option<u8> {
    if x0 < 0 || y0 < 0 || x0 + cell > world_tiles || y0 + cell > world_tiles {
        return None;
    }
    let chunk_side = CHUNK_SIZE as i32;
    if cell > chunk_side
        || x0.rem_euclid(chunk_side) + cell > chunk_side
        || y0.rem_euclid(chunk_side) + cell > chunk_side
    {
        return None;
    }
    let cpos = ChunkPos {
        x: x0.div_euclid(chunk_side) as u32,
        y: y0.div_euclid(chunk_side) as u32,
    };
    match &world.chunk(cpos).terrain {
        TerrainTiles::Uniform(kind) => tile_atlas_index(*kind),
        TerrainTiles::Full(_) => None,
    }
}

/// Выбирает самый частый индекс тайла; при равенстве — меньший индекс.
pub(crate) fn dominant_atlas_index(counts: &[u32; TILE_KIND_COUNT]) -> Option<u8> {
    let mut best: Option<(u8, u32)> = None;
    for (index, &count) in counts.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let better = match best {
            None => true,
            Some((_, best_count)) => count > best_count,
        };
        if better {
            best = Some((index as u8, count));
        }
    }
    best.map(|(index, _)| index)
}

/// Индекс тайла в атласе; `Empty` не рисуется.
pub(crate) fn tile_atlas_index(kind: TileType) -> Option<u8> {
    match kind {
        TileType::Empty => None,
        other => Some(other as u8 - 1),
    }
}

/// Обратное преобразование индекса атласа в тип тайла. Индексы идут
/// подряд за `Empty` (`tile_atlas_index` = дискриминант − 1), поэтому
/// обратное отображение выводится из `TileType::from_discriminant` —
/// новый вид тайла не требует правки здесь.
fn kind_for_atlas_index(index: u8) -> Option<TileType> {
    TileType::from_discriminant(index + 1)
}
