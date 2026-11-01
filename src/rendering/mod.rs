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

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite_render::Material2dPlugin;

use crate::app::camera::WorldCamera;
use crate::app::input::cursor_world_tile;
use crate::app::state::{EditorSelection, GameMode, GameState, SimTick, TickAccumulator, interpolate_pos};
use crate::simulation::commands::buildable_at;
use crate::simulation::objects::build_cost;
use crate::simulation::{BuildingKind, ChunkPos, EntityKind, Facing, ObjectKind, TerrainTiles, TilePos, TileType, World, CHUNK_SIZE, TILE_SIZE, WorldPos};
use crate::ui::UiFont;
use crate::ui::construction::ConstructionUi;

pub(crate) mod isometric;

mod combat;
mod lenses;
mod lighting;
mod textures;

use isometric::{WORLD_TILE_SIZE, tile_elevation, world_position};
pub(crate) use textures::TileTextures;

/// Сторона terrain-блока в тайлах. 64 делит все поддерживаемые размеры мира.
const BLOCK_TILES: i32 = 64;

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

pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(Material2dPlugin::<lighting::DarknessMaterial>::default())
            .add_systems(Startup, textures::load_tile_textures)
            .init_resource::<TerrainRender>()
            .init_resource::<RenderMetrics>()
            .init_resource::<EntitySprites>()
            .init_resource::<ItemEntitySprites>()
            .init_resource::<WorldSpriteAssets>()
            .init_resource::<EntitySpriteVisuals>()
            .init_resource::<HurtFlashes>()
            .init_resource::<SelectionOutlines>()
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

/// Построенный terrain-блок: сущность и её меш (если в блоке есть тайлы).
struct BuiltBlock {
    entity: Entity,
    mesh: Option<Handle<Mesh>>,
    lod: u32,
    fallback: bool,
}

/// Координаты terrain-блока в сетке блоков.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct BlockPos {
    pub x: i32,
    pub y: i32,
}

/// Состояние рендера мира: материал, активные блоки и переход LOD.
#[derive(Resource, Default)]
struct TerrainRender {
    material: Option<Handle<StandardMaterial>>,
    /// Готовая сцена текущего LOD: видимые блоки.
    active: HashMap<BlockPos, BuiltBlock>,
    /// Строящаяся сцена нового LOD; активная остаётся на экране до её готовности.
    incoming: Option<IncomingLod>,
    blocks_per_side: i32,
    lod: u32,
}

/// Сцена нового LOD, достраивающаяся на фоне активной.
struct IncomingLod {
    lod: u32,
    blocks: HashMap<BlockPos, BuiltBlock>,
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
struct TerrainBlock;

/// Метка динамического спрайта существа (животного, человека).
#[derive(Component)]
struct EntitySprite;

/// Метка счётчика количества у лежащего предмета. Видимость пересчитывается
/// каждый кадр по зуму: на крупном LOD текста нет, а внутри LOD 0 он
/// скрывается, когда становится мельче `ITEM_COUNT_MIN_SCREEN_PX`.
#[derive(Component)]
struct ItemCountLabel;

/// Соответствие «id существа симуляции → Bevy-сущность спрайта».
/// Спрайты создаются только для видимых существ и удаляются, когда
/// существо уходит с экрана. `textures` хранит текстуру текущего
/// материала: она меняется только со сменой состояния/кадра анимации,
/// поэтому `MeshMaterial3d` пере-инсертится лишь тогда.
#[derive(Resource, Default)]
struct EntitySprites {
    map: HashMap<u64, Entity>,
    textures: HashMap<u64, bevy::asset::AssetId<Image>>,
}

/// Соответствие «id item-сущности дропа → (спрайт, последняя позиция)».
/// Лежащий дроп статичен, брошенный летит — позиция спрайта обновляется
/// при изменении координат сущности.
#[derive(Resource, Default)]
struct ItemEntitySprites {
    map: HashMap<u64, (Entity, [f32; 2])>,
}

/// Общий quad и материалы для world-space спрайтов с depth test.
/// `object_sizes` кэширует размеры спрайтов по текстуре: чтение размеров
/// из `Assets<Image>` на каждый спавн заменяется lookup'ом. Кэшируется
/// только загруженное изображение — до загрузки размер считается заново.
#[derive(Resource, Default)]
struct WorldSpriteAssets {
    quad: Option<Handle<Mesh>>,
    materials: HashMap<(AssetId<Image>, bool), Handle<StandardMaterial>>,
    /// Кэш контуров выделения: материал + пад-факторы квада по исходной
    /// текстуре (одна запись вместо двух согласованных кэшей).
    outlines: HashMap<AssetId<Image>, OutlineAsset>,
    /// Кэш материалов голограммы: (текстура, зелёная/красная сторона).
    hologram_materials: HashMap<(AssetId<Image>, bool), Handle<StandardMaterial>>,
    object_sizes: HashMap<AssetId<Image>, Vec2>,
}

/// Контур выделения для одной текстуры: материал белого дилатированного
/// силуэта и пад-факторы квада — во сколько раз канвас контура шире/выше
/// исходной текстуры. Внутренняя область канваса при таком масштабе ложится
/// на спрайт один в один, кольцо выходит ровно на радиус дилатации.
#[derive(Clone)]
struct OutlineAsset {
    material: Handle<StandardMaterial>,
    pad: Vec2,
}

/// Сколько секунд после удара существо подсвечивается красным.
const HURT_FLASH_SECS: f32 = 0.33;
/// Подсветка урона: красный поверх текстуры с 60% альфы.
const HURT_TINT: Color = Color::srgba(1.0, 0.0, 0.0, 0.6);

/// Вспышки урона существ: предыдущие HP (для детекта удара), таймеры
/// подсветки и overlay-спрайты «краснее» поверх основной текстуры.
#[derive(Resource, Default)]
struct HurtFlashes {
    prev_hp: HashMap<u64, f32>,
    timers: HashMap<u64, f32>,
    overlays: HashMap<u64, Entity>,
}

/// Контур выделения существа в редакторе (белая рамка): соответствие
/// «id существа → Bevy-сущность квадрата-контура». Контуры живут, пока
/// сущность подсвечена (`EditorSelection`) и видна.
#[derive(Resource, Default)]
struct SelectionOutlines {
    map: HashMap<u64, Entity>,
    /// Текстура, из которой собран материал контура. Смена состояния/кадра
    /// анимации меняет текстуру спрайта — без трекинга силуэт оставался бы
    /// от прежнего состояния и рассинхронизировался с целью.
    textures: HashMap<u64, bevy::asset::AssetId<Image>>,
}

/// Толщина белого контура выделения в мировых пикселях (с каждой стороны).
/// Одна для всех существ: кольцо одинаково выглядит на одном зуме независимо
/// от плотности пикселей текстуры.
pub(crate) const OUTLINE_THICKNESS_PX: f32 = 0.33;

/// Размер спрайта волка в пикселях мира. Текстура 128×128, но рисунок
/// занимает её центр (тело — около 1/3 кадра), поэтому два тайла дают
/// зверя длиной примерно в тайл, сопоставимо с игроком.
pub(crate) const WOLF_SPRITE_PX: f32 = TILE_SIZE * 2.0;

/// Размер спрайта игрока в пикселях мира (текстура 256×256 масштабируется
/// до тайла; объекты масштабируются отдельно — см. `object_sprite_size`).
const PLAYER_SPRITE_PX: f32 = TILE_SIZE;

/// Непрозрачная область рисунка в текстуре, в долях ширины/высоты
/// (правая и нижняя границы — исключающие). Считается по альфа-каналу
/// один раз на текстуру; из неё выводятся «ноги на землю», зона пикинга
/// курсором и радиус дилатации контура выделения — ручные константы
/// протухали при каждой перерисовке спрайтов.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SpriteAlphaBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl SpriteAlphaBounds {
    /// Сдвиг спрайта вниз, в долях высоты, чтобы нижние пиксели рисунка
    /// стояли на земле: доля прозрачной полосы под рисунком.
    pub fn bottom_offset(self) -> f32 {
        1.0 - self.bottom
    }
}

/// Кэш видимых областей спрайтов существ по альфа-каналу их текстур.
/// Заполняется по мере загрузки ассетов (`update_entity_sprite_visuals`);
/// рендер и пикинг обязаны видеть один и тот же рисунок, поэтому источник
/// один — фактические пиксели текстуры.
#[derive(Resource, Default)]
pub(crate) struct EntitySpriteVisuals {
    map: HashMap<AssetId<Image>, Option<SpriteAlphaBounds>>,
    /// Все текстуры существ отсканированы — дальше система бесплатна.
    warm: bool,
}

impl EntitySpriteVisuals {
    /// Границы рисунка текстуры. `None` — текстура ещё не отсканирована
    /// или непрозрачных пикселей в ней нет.
    pub(crate) fn bounds(&self, texture: AssetId<Image>) -> Option<SpriteAlphaBounds> {
        self.map.get(&texture).copied().flatten()
    }

    /// Сдвиг «ног на землю» для текстуры; 0.0, пока границы неизвестны.
    pub(crate) fn bottom_offset(&self, texture: AssetId<Image>) -> f32 {
        self.bounds(texture).map_or(0.0, SpriteAlphaBounds::bottom_offset)
    }
}

/// Непрозрачные границы рисунка по альфа-каналу (порог 128 — тот же, что
/// у контура выделения). `None` — формат не RGBA8 или рисунка нет.
pub(crate) fn scan_alpha_bounds(image: &Image) -> Option<SpriteAlphaBounds> {
    if image.texture_descriptor.format != TextureFormat::Rgba8UnormSrgb {
        return None;
    }
    let pixels = image.data.as_deref()?;
    let width = image.width();
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0, 0);
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        if pixel[3] < 128 {
            continue;
        }
        let x = index as u32 % width;
        let y = index as u32 / width;
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    (max_x >= min_x).then(|| SpriteAlphaBounds {
        left: min_x as f32 / width as f32,
        top: min_y as f32 / image.height() as f32,
        right: (max_x + 1) as f32 / width as f32,
        bottom: (max_y + 1) as f32 / image.height() as f32,
    })
}

/// Скан видимых областей текстур существ: кэш наполняется по мере загрузки
/// ассетов, после полного прогрева система сводится к раннему выходу.
/// Хэндлы текстур известны на старте, ассеты грузятся асинхронно — недостающие
/// досканиваются на следующих кадрах.
fn update_entity_sprite_visuals(
    textures: Res<TileTextures>,
    images: Res<Assets<Image>>,
    mut visuals: ResMut<EntitySpriteVisuals>,
) {
    if visuals.warm {
        return;
    }
    let mut scanned = 0;
    let mut total = 0;
    for handle in textures.entity_textures() {
        total += 1;
        let id = handle.id();
        if visuals.map.contains_key(&id) {
            scanned += 1;
            continue;
        }
        if let Some(image) = images.get(handle) {
            visuals.map.insert(id, scan_alpha_bounds(image));
            scanned += 1;
        }
    }
    visuals.warm = scanned == total;
}

/// Кегль счётчика лежащих предметов и его масштаб. Растр крупный
/// (40 px × 0.1): при мелком кегле глифы состоят из нескольких пикселей
/// и выглядят мылом при любом зуме.
const ITEM_COUNT_FONT_SIZE: f32 = 40.0;
const ITEM_COUNT_SCALE: f32 = 0.1;

/// Номинальная высота счётчика в пикселях мира.
const ITEM_COUNT_WORLD_PX: f32 = ITEM_COUNT_FONT_SIZE * ITEM_COUNT_SCALE;

/// Минимальный размер счётчика на экране, px. Мельче — это уже мыльный
/// пиксель, текст не показывается (и не спавнится на крупных LOD).
const ITEM_COUNT_MIN_SCREEN_PX: f32 = 4.0;

fn world_sprite_quad(
    meshes: &mut Assets<Mesh>,
    sprite_assets: &mut WorldSpriteAssets,
) -> Handle<Mesh> {
    sprite_assets
        .quad
        .get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0)))
        .clone()
}

/// Материал контура выделения для текстуры: белый силуэт, дилатированный
/// на радиус, соответствующий `OUTLINE_THICKNESS_PX` при данном размере
/// спрайта. `None` — текстура ещё не загружена (контур появится на кадре
/// после загрузки; система вызывается на каждый кадр выделения).
fn world_outline_asset(
    images: &mut Assets<Image>,
    texture: &Handle<Image>,
    sprite_size: Vec2,
    sprite_assets: &mut WorldSpriteAssets,
    materials: &mut Assets<StandardMaterial>,
) -> Option<OutlineAsset> {
    if let Some(asset) = sprite_assets.outlines.get(&texture.id()) {
        return Some(asset.clone());
    }
    // Размеры и канвас извлекаются до вставки новой текстуры: мутация
    // `Assets<Image>` несовместима с живым заёмом источника.
    let (width, height, radius_x, radius_y, image) = {
        let source = images.get(texture)?;
        let radius_x = outline_radius_texels(source.width(), sprite_size.x);
        let radius_y = outline_radius_texels(source.height(), sprite_size.y);
        let image = build_outline_image(source, radius_x, radius_y)?;
        (source.width(), source.height(), radius_x, radius_y, image)
    };
    let asset = OutlineAsset {
        material: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(images.add(image)),
            alpha_mode: AlphaMode::Mask(0.5),
            cull_mode: None,
            unlit: true,
            ..default()
        }),
        pad: Vec2::new(
            (width + 2 * radius_x) as f32 / width as f32,
            (height + 2 * radius_y) as f32 / height as f32,
        ),
    };
    sprite_assets.outlines.insert(texture.id(), asset.clone());
    Some(asset)
}

/// Радиус дилатации в текселях, дающий `OUTLINE_THICKNESS_PX` мировых
/// пикселей. Плотность пикселей у текстур разная (человек — 256 текселей
/// на 16 мировых, волк — 128 на 32), радиус её компенсирует.
pub(crate) fn outline_radius_texels(texture_px: u32, sprite_world_px: f32) -> u32 {
    (OUTLINE_THICKNESS_PX * texture_px as f32 / sprite_world_px)
        .round()
        .max(1.0) as u32
}

/// Белый силуэт рисунка, дилатированный на заданный радиус: канвас на
/// 2·radius шире и выше исходника, непрозрачные пиксели — белые. Контур
/// рисуется этим канвасом с трансформом спрайта (масштаб — на канвасные
/// доли, см. `OutlineAsset::pad`), поэтому кольцо везде одной толщины
/// и не зависит от того, где в текстуре лежит рисунок. `None` — формат
/// не RGBA8 или рисунка нет (строить контур не из чего).
pub(crate) fn build_outline_image(source: &Image, radius_x: u32, radius_y: u32) -> Option<Image> {
    if source.texture_descriptor.format != TextureFormat::Rgba8UnormSrgb {
        return None;
    }
    let pixels = source.data.as_deref()?;
    let width = source.width();
    let height = source.height();
    let mut mask = Vec::with_capacity((width * height) as usize);
    for pixel in pixels.as_chunks::<4>().0 {
        mask.push(pixel[3] >= 128);
    }
    let dilated = dilate_mask(&mask, width, height, radius_x, radius_y);

    let canvas_width = width + 2 * radius_x;
    let canvas_height = height + 2 * radius_y;
    let mut data = vec![0_u8; (canvas_width * canvas_height * 4) as usize];
    for (index, &set) in dilated.iter().enumerate() {
        if !set {
            continue;
        }
        let x = index as u32 % width;
        let y = index as u32 / width;
        let offset = ((y + radius_y) * canvas_width + x + radius_x) as usize * 4;
        data[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
    }
    let mut outline = Image::new(
        Extent3d {
            width: canvas_width,
            height: canvas_height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    // Рисунок пиксель-арта: жёсткие края кольца без размытия.
    outline.sampler = ImageSampler::nearest();
    Some(outline)
}

/// Дилатация бинарной маски прямоугольником (2·radius+1)², сепарабельно:
/// горизонтальный и вертикальный проходы со скользящим окном пересчёта
/// счётчика — O(W·H) независимо от радиуса. Углы кольца прямые; для
/// контура в несколько мировых пикселей это неотличимо от круга.
pub(crate) fn dilate_mask(mask: &[bool], width: u32, height: u32, radius_x: u32, radius_y: u32) -> Vec<bool> {
    let width = width as usize;
    let height = height as usize;
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let rx = radius_x as usize;
    let ry = radius_y as usize;

    // Горизонтальный проход: окно [x - rx, x + rx] по строке, счётчик
    // непрозрачных пикселей в нём обновляется инкрементально.
    let mut horizontal = vec![false; mask.len()];
    for y in 0..height {
        let row = y * width;
        let mut count = mask[row..row + rx.min(width - 1) + 1]
            .iter()
            .filter(|pixel| **pixel)
            .count();
        for x in 0..width {
            horizontal[row + x] = count > 0;
            if x >= rx && mask[row + x - rx] {
                count -= 1;
            }
            let added = x + rx + 1;
            if added < width && mask[row + added] {
                count += 1;
            }
        }
    }

    // Вертикальный проход: то же окно по столбцу результата.
    let mut dilated = vec![false; mask.len()];
    for x in 0..width {
        let mut count = (0..=ry.min(height - 1))
            .filter(|offset| horizontal[offset * width + x])
            .count();
        for y in 0..height {
            dilated[y * width + x] = count > 0;
            if y >= ry && horizontal[(y - ry) * width + x] {
                count -= 1;
            }
            let added = y + ry + 1;
            if added < height && horizontal[added * width + x] {
                count += 1;
            }
        }
    }
    dilated
}

fn world_sprite_material(
    texture: &Handle<Image>,
    hurt_tint: bool,
    sprite_assets: &mut WorldSpriteAssets,
    materials: &mut Assets<StandardMaterial>,
) -> Handle<StandardMaterial> {
    sprite_assets
        .materials
        .entry((texture.id(), hurt_tint))
        .or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: if hurt_tint { HURT_TINT } else { Color::WHITE },
                base_color_texture: Some(texture.clone()),
                alpha_mode: if hurt_tint {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Mask(0.5)
                },
                cull_mode: None,
                unlit: true,
                ..default()
            })
        })
        .clone()
}

/// Собирает атлас из загруженных PNG и создаёт общий материал.
/// Пока не все текстуры загружены, материал не создаётся.
fn ensure_ground_material(
    mut render: ResMut<TerrainRender>,
    textures: Res<TileTextures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if render.material.is_some() {
        return;
    }
    let Some(atlas) = build_atlas_image(&textures, &images) else {
        return;
    };

    let atlas = images.add(atlas);
    render.material = Some(materials.add(StandardMaterial {
        base_color_texture: Some(atlas),
        alpha_mode: AlphaMode::Opaque,
        unlit: true,
        ..default()
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
fn render_world(
    mut commands: Commands,
    mut state: ResMut<GameState>,
    mut render: ResMut<TerrainRender>,
    mut metrics: ResMut<RenderMetrics>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sprite_assets: ResMut<WorldSpriteAssets>,
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
    let Ok((camera, camera_transform, projection)) = camera.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    // Точечная перестройка terrain-блоков (инструменты редактора, добыча
    // травы, растительность): блок пересобирается целиком, вместе с
    // дочерними объектами.
    if !dirty.terrain.is_empty() {
        rebuild_dirty_blocks(
            &mut commands, &mut meshes, &mut materials, &mut sprite_assets, &images, world,
            &material, camera_transform.rotation, &textures, &font, &dirty.terrain, &mut render,
            &mut metrics,
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

    let lod = lod_for_scale(ortho.scale);

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
                &material, camera_transform.rotation, &textures, &font, &mut metrics, block, lod,
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
                &mut commands, &mut meshes, &mut materials, &mut sprite_assets, &images, world, &material,
                camera_transform.rotation, &textures, &font, &mut metrics, block, incoming_lod,
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
    sprite_assets: &mut WorldSpriteAssets,
    images: &Assets<Image>,
    world: &World,
    material: &Handle<StandardMaterial>,
    camera_rotation: Quat,
    textures: &TileTextures,
    font: &Res<UiFont>,
    dirty: &[TilePos],
    render: &mut TerrainRender,
    metrics: &mut RenderMetrics,
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
                commands, meshes, materials, sprite_assets, images, world, material, camera_rotation, textures, font, metrics, pos, render.lod,
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
                        commands, meshes, materials, sprite_assets, images, world, material, camera_rotation, textures, font, metrics, pos, lod,
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
    sprite_assets: &mut WorldSpriteAssets,
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
    sprite_assets: &mut WorldSpriteAssets,
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

/// Спрайт игрока больше не нужен отдельной сущностью: по целевому формату
/// игрок — это захваченное существо мира и рисуется общим кодом спрайтов
/// существ (`update_entity_sprites`), текстура — прежняя `player.png`.
/// Синхронизирует спрайты item-сущностей дропа с `world.item_entities`:
/// новый дроп — спрайт, подбор/исчезновение — снятие спрайта. Позиция
/// статична (дроп не двигается),Billboard повёрнут к камере при спавне.
fn update_item_entity_sprites(
    mut commands: Commands,
    state: Res<GameState>,
    textures: Res<TileTextures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sprite_assets: ResMut<WorldSpriteAssets>,
    mut sprites: ResMut<ItemEntitySprites>,
    camera: Query<&Transform, (With<Camera2d>, Without<EntitySprite>)>,
    sprite_transforms: Query<&mut Transform, With<EntitySprite>>,
) {
    let Some(world) = state.world.as_ref() else {
        for (_, (entity, _)) in sprites.map.drain() {
            commands.entity(entity).despawn();
        }
        return;
    };
    let Ok(camera_transform) = camera.single() else {
        return;
    };
    let mut sprite_transforms: Query<&mut Transform, With<EntitySprite>> = sprite_transforms;
    // Снятие спрайтов подобранных/исчезнувших предметов.
    let stale: Vec<u64> = sprites
        .map
        .keys()
        .copied()
        .filter(|id| world.item_entity(*id).is_none())
        .collect();
    for id in stale {
        if let Some((entity, _)) = sprites.map.remove(&id) {
            commands.entity(entity).despawn();
        }
    }
    let quad = world_sprite_quad(&mut meshes, &mut sprite_assets);
    let camera_up = camera_transform.rotation * Vec3::Y;
    let height = TILE_SIZE * 0.3;
    for entity in &world.item_entities.items {
        let pos_xy = [entity.pos.x, entity.pos.y];
        if let Some(&(sprite, last_pos)) = sprites.map.get(&entity.id) {
            // Лежащий предмет не двигается; летящий — обновляем позицию
            // спрайта (интерполяция не нужна: шаг тика и так плавный).
            if (last_pos[0] - entity.pos.x).abs() > 1e-4 || (last_pos[1] - entity.pos.y).abs() > 1e-4 {
                if let Ok(mut transform) = sprite_transforms.get_mut(sprite) {
                    transform.translation = world_position(world, Vec2::new(entity.pos.x, entity.pos.y)) + camera_up * height;
                }
                sprites.map.insert(entity.id, (sprite, pos_xy));
            }
            continue;
        }
        let pos = world_position(world, Vec2::new(entity.pos.x, entity.pos.y)) + camera_up * height;
        let texture = textures.item_texture(entity.stack.kind);
        let material = world_sprite_material(texture, false, &mut sprite_assets, &mut materials);
        let size = Vec2::splat(TILE_SIZE * 0.5);
        let mut transform = Transform::from_translation(pos)
            .with_rotation(camera_transform.rotation);
        transform.scale = Vec3::new(size.x, size.y, 1.0);
        let sprite = commands
            .spawn((Mesh3d(quad.clone()), MeshMaterial3d(material), transform, EntitySprite))
            .id();
        sprites.map.insert(entity.id, (sprite, pos_xy));
    }
}

/// Обновляет спрайты животных: создаёт их для видимых существ, двигает
/// интерполяцией между тиками, меняет текстуру по состоянию и кадру
/// анимации, зеркалит при взгляде влево. Невидимые существа не держат
/// Bevy-сущностей — рендер читает только видимую область.
fn update_entity_sprites(
    mut commands: Commands,
    state: Res<GameState>,
    acc: Res<TickAccumulator>,
    time: Res<Time>,
    textures: Res<TileTextures>,
    visuals: Res<EntitySpriteVisuals>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sprite_assets: ResMut<WorldSpriteAssets>,
    mut sprites: ResMut<EntitySprites>,
    mut flashes: ResMut<HurtFlashes>,
    camera: Query<(&Camera, &Transform, &Projection), With<Camera2d>>,
) {
    let Some(world) = state.world.as_ref() else {
        for (_, entity) in sprites.map.drain() {
            commands.entity(entity).despawn();
        }
        sprites.textures.clear();
        for (_, overlay) in flashes.overlays.drain() {
            commands.entity(overlay).despawn();
        }
        flashes.prev_hp.clear();
        flashes.timers.clear();
        return;
    };
    let Ok((camera, camera_transform, projection)) = camera.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let Some(viewport_size) = camera.logical_viewport_size() else {
        return;
    };
    let sprite_margin_px = WOLF_SPRITE_PX / ortho.scale;
    let quad = world_sprite_quad(&mut meshes, &mut sprite_assets);
    let camera_global = GlobalTransform::from(*camera_transform);

    // Таймеры вспышек урона гаснут по реальному времени.
    let delta = time.delta_secs();
    flashes.timers.retain(|_, timer| {
        *timer -= delta;
        *timer > 0.0
    });

    let mut visible = HashSet::with_capacity(world.entities.len());
    for entity in world.entities.iter() {
        let pos = interpolate_pos(entity.prev_pos, entity.pos, &acc);
        let world_pos = world_position(world, Vec2::new(pos.x, pos.y));
        let Ok(viewport_pos) = camera.world_to_viewport(&camera_global, world_pos) else {
            continue;
        };
        if viewport_pos.x < -sprite_margin_px
            || viewport_pos.x > viewport_size.x + sprite_margin_px
            || viewport_pos.y < -sprite_margin_px
            || viewport_pos.y > viewport_size.y + sprite_margin_px
        {
            continue;
        }
        visible.insert(entity.id);

        // Детект удара: HP стало меньше, чем на прошлом кадре — вспышка.
        let previous = flashes.prev_hp.insert(entity.id, entity.hp);
        if previous.is_some_and(|prev| entity.hp < prev) {
            flashes.timers.insert(entity.id, HURT_FLASH_SECS);
        }

        let texture = textures.entity_texture(entity);
        let transform = entity_transform(
            &pos,
            world,
            camera_transform.rotation,
            entity_sprite_size(entity.kind),
            visuals.bottom_offset(texture.id()),
            entity.facing == Facing::Left,
        );

        match sprites.map.get(&entity.id).copied() {
            Some(bevy_entity) => {
                // Материал меняется только со сменой текстуры (состояние/кадр
                // анимации): без смены — двигаем спрайт, не трогая компонент.
                if sprites.textures.get(&entity.id) != Some(&texture.id()) {
                    let material =
                        world_sprite_material(texture, false, &mut sprite_assets, &mut materials);
                    commands
                        .entity(bevy_entity)
                        .insert((MeshMaterial3d(material), transform));
                    sprites.textures.insert(entity.id, texture.id());
                } else {
                    commands.entity(bevy_entity).insert(transform);
                }
            }
            None => {
                let material = world_sprite_material(texture, false, &mut sprite_assets, &mut materials);
                let bevy_entity = commands
                    .spawn((Mesh3d(quad.clone()), MeshMaterial3d(material), transform, EntitySprite))
                    .id();
                sprites.map.insert(entity.id, bevy_entity);
                sprites.textures.insert(entity.id, texture.id());
            }
        }

        // Красная подсветка: копия текстуры с красной 30% альфой поверх —
        // краснеет именно рисунок, а не прямоугольник спрайта.
        let hurt = flashes.timers.contains_key(&entity.id);
        match flashes.overlays.get(&entity.id).copied() {
            Some(overlay) if hurt => {
                let material =
                    world_sprite_material(texture, true, &mut sprite_assets, &mut materials);
                commands
                    .entity(overlay)
                    .insert((MeshMaterial3d(material), transform));
            }
            Some(overlay) => {
                flashes.overlays.remove(&entity.id);
                commands.entity(overlay).despawn();
            }
            None if hurt => {
                let material =
                    world_sprite_material(texture, true, &mut sprite_assets, &mut materials);
                let overlay = commands
                    .spawn((Mesh3d(quad.clone()), MeshMaterial3d(material), transform))
                    .id();
                flashes.overlays.insert(entity.id, overlay);
            }
            None => {}
        }
    }

    let stale: Vec<u64> = sprites
        .map
        .keys()
        .copied()
        .filter(|id| !visible.contains(id))
        .collect();
    for id in stale {
        if let Some(entity) = sprites.map.remove(&id) {
            commands.entity(entity).despawn();
        }
        sprites.textures.remove(&id);
        // Существо покинуло экран или умерло: вспышка и overlay не нужны.
        flashes.prev_hp.remove(&id);
        flashes.timers.remove(&id);
        if let Some(overlay) = flashes.overlays.remove(&id) {
            commands.entity(overlay).despawn();
        }
    }
}

/// Белый контур выделения существа в редакторе: подсветка под курсором
/// (`EditorSelection::hovered`) и открытое окно-инспектор (`selected`).
/// Контур — дилатированный белый силуэт текстуры (`world_outline_asset`),
/// нарисованный на канвасе с паддингом тем же трансформом, что у спрайта:
/// кольцо везде одной мировой толщины и не смещается при рисунке,
/// нецентрованном в текстуре. Рисуется только в наблюдателе; существо
/// ушло — контур снят.
fn update_selection_outlines(
    mut commands: Commands,
    state: Res<GameState>,
    selection: Res<EditorSelection>,
    acc: Res<TickAccumulator>,
    textures: Res<TileTextures>,
    visuals: Res<EntitySpriteVisuals>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sprite_assets: ResMut<WorldSpriteAssets>,
    mut outlines: ResMut<SelectionOutlines>,
    camera: Query<(&Camera, &Transform), With<Camera2d>>,
) {
    let Some(world) = state.world.as_ref() else {
        for (_, entity) in outlines.map.drain() {
            commands.entity(entity).despawn();
        }
        outlines.textures.clear();
        return;
    };
    // Список существ с контуром: hovered и selected (без дубликатов),
    // только живые и только в режиме редактора.
    let mut want: Vec<u64> = Vec::new();
    if state.mode == GameMode::Observer {
        for id in [selection.hovered, selection.selected].into_iter().flatten() {
            if world.entities.get(id).is_some() && !want.contains(&id) {
                want.push(id);
            }
        }
    }
    let stale: Vec<u64> = outlines
        .map
        .keys()
        .copied()
        .filter(|id| !want.contains(id))
        .collect();
    for id in stale {
        if let Some(entity) = outlines.map.remove(&id) {
            commands.entity(entity).despawn();
        }
        outlines.textures.remove(&id);
    }
    if want.is_empty() {
        return;
    }
    let Ok((_, camera_transform)) = camera.single() else {
        return;
    };
    let quad = world_sprite_quad(&mut meshes, &mut sprite_assets);
    // Контур рисуется чуть глубже сцены: он за спрайтом существа, но
    // перед рельефом.
    let back = camera_transform.rotation * -Vec3::Z * 0.05;
    for id in want {
        let Some(entity) = world.entities.get(id) else {
            continue;
        };
        let pos = interpolate_pos(entity.prev_pos, entity.pos, &acc);
        let texture = textures.entity_texture(entity);
        // Трансформ ровно тот же, что у спрайта; масштаб домножается на
        // пад-факторы канваса контура (перевод текселей канваса в тексели
        // исходника), поэтому внутренняя область ложится на спрайт 1:1,
        // а кольцо везде ровно OUTLINE_THICKNESS_PX.
        let size = entity_sprite_size(entity.kind);
        let mut transform = entity_transform(
            &pos,
            world,
            camera_transform.rotation,
            size,
            visuals.bottom_offset(texture.id()),
            entity.facing == Facing::Left,
        );
        transform.translation += back;
        match outlines.map.get(&id).copied() {
            Some(bevy_entity) => {
                // Материал меняется вместе с текстурой (состояние/кадр
                // анимации): волк лёг — силуэт обязан смениться на лежачего,
                // иначе контур останется от прежнего состояния.
                if outlines.textures.get(&id) != Some(&texture.id()) {
                    let Some(asset) = world_outline_asset(
                        &mut images,
                        texture,
                        size,
                        &mut sprite_assets,
                        &mut materials,
                    ) else {
                        continue;
                    };
                    transform.scale.x *= asset.pad.x;
                    transform.scale.y *= asset.pad.y;
                    commands
                        .entity(bevy_entity)
                        .insert((MeshMaterial3d(asset.material), transform));
                    outlines.textures.insert(id, texture.id());
                } else {
                    if let Some(asset) = sprite_assets.outlines.get(&texture.id()) {
                        transform.scale.x *= asset.pad.x;
                        transform.scale.y *= asset.pad.y;
                    }
                    commands.entity(bevy_entity).insert(transform);
                }
            }
            None => {
                // Текстура не загружена — силуэт построить не из чего:
                // контур появится на следующем кадре выделения.
                let Some(asset) = world_outline_asset(
                    &mut images,
                    texture,
                    size,
                    &mut sprite_assets,
                    &mut materials,
                ) else {
                    continue;
                };
                transform.scale.x *= asset.pad.x;
                transform.scale.y *= asset.pad.y;
                let bevy_entity = commands
                    .spawn((Mesh3d(quad.clone()), MeshMaterial3d(asset.material), transform))
                    .id();
                outlines.map.insert(id, bevy_entity);
                outlines.textures.insert(id, texture.id());
            }
        }
    }
}

/// Спрайт существа развёрнут к камере и стоит на поверхности тайла.
/// `bottom_offset` опускает квадрат, если у текстуры снизу прозрачная
/// полоса (видимые пиксели должны касаться земли).
fn entity_transform(
    pos: &WorldPos,
    world: &World,
    camera_rotation: Quat,
    size: Vec2,
    bottom_offset: f32,
    flip_x: bool,
) -> Transform {
    let location = world_position(world, Vec2::new(pos.x, pos.y));
    let camera_up = camera_rotation * Vec3::Y;
    let mut transform =
        Transform::from_translation(location + camera_up * (size.y * (0.5 - bottom_offset)))
            .with_rotation(camera_rotation);
    transform.scale = Vec3::new(if flip_x { -size.x } else { size.x }, size.y, 1.0);
    transform
}

/// Размер спрайта существа в пикселях мира.
pub(crate) fn entity_sprite_size(kind: EntityKind) -> Vec2 {
    match kind {
        EntityKind::Wolf => Vec2::splat(WOLF_SPRITE_PX),
        // Человек временно рисуется текстурой игрока (решение автора):
        // квадрат в один тайл, как раньше был спрайт игрока.
        EntityKind::Human => Vec2::splat(PLAYER_SPRITE_PX),
    }
}

/// Деспавнит сущность блока и освобождает его меш.
fn despawn_block(commands: &mut Commands, meshes: &mut Assets<Mesh>, block: BuiltBlock) {
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
    sprite_assets: &mut WorldSpriteAssets,
    images: &Assets<Image>,
    world: &World,
    material: &Handle<StandardMaterial>,
    camera_rotation: Quat,
    textures: &TileTextures,
    font: &Res<UiFont>,
    metrics: &mut RenderMetrics,
    block: BlockPos,
    lod: u32,
) -> BuiltBlock {
    let started = std::time::Instant::now();
    let mesh = build_block_mesh(world, block, lod).map(|mesh| meshes.add(mesh));
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

fn build_fallback_block(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    world: &World,
    material: &Handle<StandardMaterial>,
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
fn block_origin(block: BlockPos) -> Vec3 {
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

/// Нужно ли рисовать счётчики лежащих предметов при данном масштабе камеры:
/// только на LOD 0 и только если на экране текст не мельче
/// `ITEM_COUNT_MIN_SCREEN_PX`. На крупных LOD текст не показывается вовсе.
pub(crate) fn item_counts_visible(scale: f32) -> bool {
    lod_for_scale(scale) == 0 && ITEM_COUNT_WORLD_PX / scale >= ITEM_COUNT_MIN_SCREEN_PX
}

fn update_item_count_labels(
    cameras: Query<&Projection, With<Camera2d>>,
    mut labels: Query<&mut Visibility, With<ItemCountLabel>>,
) {
    let Ok(projection) = cameras.single() else {
        return;
    };
    let Projection::Orthographic(orthographic) = projection else {
        return;
    };
    let visibility = if item_counts_visible(orthographic.scale) {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut label in &mut labels {
        *label = visibility;
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
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    // Нормали не считаем: материал terrain unlit, освещение по нормалям
    // не считается, а без атрибута Bevy не включает VERTEX_NORMALS в шейдере.
    // Это ~2/3 стоимости сборки меша (профиль: compute_normals был 1.38%).
    Some(mesh)
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

/// Спавнит объекты блока как дочерние спрайты/блоки. Обычные объекты —
/// camera-facing спрайты; станции-«блоки» — изометрический набор квадов
/// (верх + две видимые боковые грани −X/−Y); костёр — крест из двух
/// вертикальных квадов вдоль середин сторон тайла; склад — плоский декаль
/// на всю поверхность тайла. Идёт по разреженному слою объектов четырёх
/// чанков блока — стоимость пропорциональна числу объектов, а не тайлам.
fn spawn_objects(
    parent: &mut EntityCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    sprite_assets: &mut WorldSpriteAssets,
    images: &Assets<Image>,
    world: &World,
    block: BlockPos,
    origin: Vec3,
    camera_rotation: Quat,
    textures: &TileTextures,
) {
    let bx0 = block.x * BLOCK_TILES;
    let by0 = block.y * BLOCK_TILES;
    // Блок 64×64 покрывает ровно 4×4 чанков (сетка блоков кратна чанку);
    // у края мира часть чанков за границей — они пропускаются.
    let chunks_per_block = BLOCK_TILES / CHUNK_SIZE as i32;
    let cps = world.chunks_per_side as i32;
    let quad = world_sprite_quad(meshes, sprite_assets);

    parent.with_children(|parent| {
        for cy in 0..chunks_per_block {
            for cx in 0..chunks_per_block {
                let cpx = bx0.div_euclid(CHUNK_SIZE as i32) + cx;
                let cpy = by0.div_euclid(CHUNK_SIZE as i32) + cy;
                if cpx < 0 || cpy < 0 || cpx >= cps || cpy >= cps {
                    continue;
                }
                let chunk = world.chunk(ChunkPos { x: cpx as u32, y: cpy as u32 });
                for (&idx, obj) in &chunk.objects {
                    let tile = TilePos {
                        x: cpx * CHUNK_SIZE as i32 + (idx % CHUNK_SIZE as u8) as i32,
                        y: cpy * CHUNK_SIZE as i32 + (idx / CHUNK_SIZE as u8) as i32,
                    };
                    let tile_world = world_position(
                        world,
                        Vec2::new(tile.x as f32 + 0.5, tile.y as f32 + 0.5),
                    );
                    let anchor = tile_world - origin;
                    match obj.kind {
                        // Костёр — раньше станции-блоков: он станция, но
                        // рисуется крестом из двух вертикальных квадов
                        // вдоль осей X и Y тайла, а не изометрическим блоком.
                        ObjectKind::Building(BuildingKind::Campfire) => {
                            spawn_campfire_cross(
                                parent, &quad, sprite_assets, materials, textures, anchor,
                            );
                        }
                        // Склад: плоская текстура-декаль поверх тайла.
                        ObjectKind::Building(BuildingKind::Storage) => {
                            spawn_flat_decal(
                                parent, &quad, sprite_assets, materials,
                                textures.storage_flat(), anchor, 1.0,
                            );
                        }
                        // Станции-блоки: верх + боковые грани с текстурами
                        // top/front/side; горящая печь — front_on.
                        ObjectKind::Building(kind) => {
                            let lit = crate::simulation::containers::container_at(world, tile)
                                .is_some_and(|c| c.is_lit());
                            spawn_station_block(
                                parent, &quad, sprite_assets, materials, textures, anchor, kind,
                                lit,
                            );
                        }
                        _ => {
                            let handle = textures.object_texture(obj);
                            let size =
                                object_sprite_size_cached(obj.kind, handle, images, sprite_assets);
                            // Спрайт центром по X над тайлом; низ прижат к нижней границе родного тайла (не ниже и не в воздухе).
                            let height = size.y * 0.5;
                            let camera_up = camera_rotation * Vec3::Y;
                            let material =
                                world_sprite_material(handle, false, sprite_assets, materials);
                            let mut transform =
                                Transform::from_translation(anchor + camera_up * height)
                                    .with_rotation(camera_rotation);
                            transform.scale = Vec3::new(size.x, size.y, 1.0);
                            parent.spawn((
                                Mesh3d(quad.clone()),
                                MeshMaterial3d(material),
                                transform,
                            ));
                        }
                    }
                }
            }
        }
    });
}

/// Высота блока станции, мировых единиц. Станции вдвое меньше тайла:
/// стоят на нём, а не занимают его целиком.
const STATION_BLOCK_HEIGHT: f32 = TILE_SIZE * 0.5;
/// Ширина блока станции как доля ромба тайла.
const STATION_BLOCK_SCALE: f32 = 0.5;

/// Небольшой z-сдвиг декаля над рельефом против z-fighting.
const DECAL_Z_OFFSET: f32 = 0.02;

/// Изометрический блок станции: горизонтальный верхний квад + две видимые
/// боковые грани (−X — «фронт», −Y — «бок»). Материалы кэшируются по
/// текстурам; у печей фронтальная текстура меняется на `front_on`, когда
/// контейнер горит (готовит/держит топливо).
fn spawn_station_block(
    parent: &mut ChildSpawnerCommands,
    quad: &Handle<Mesh>,
    sprite_assets: &mut WorldSpriteAssets,
    materials: &mut Assets<StandardMaterial>,
    textures: &TileTextures,
    anchor: Vec3,
    kind: BuildingKind,
    lit: bool,
) {
    let station = textures.station_textures(kind);
    let front = if lit {
        station.front_on.as_ref().unwrap_or(&station.front)
    } else {
        &station.front
    };
    let edge = WORLD_TILE_SIZE * STATION_BLOCK_SCALE;
    let half = edge * 0.5;
    let height = STATION_BLOCK_HEIGHT;

    // Верх: квад по умолчанию лежит в плоскости земли (нормаль +Z).
    let top_material = world_sprite_material(&station.top, false, sprite_assets, materials);
    parent.spawn((
        Mesh3d(quad.clone()),
        MeshMaterial3d(top_material),
        Transform::from_translation(anchor + Vec3::Z * height)
            .with_scale(Vec3::new(edge, edge, 1.0)),
    ));
    // Бок (−Y): стена, повёрнутая вокруг X на +90°: нормаль −Y, ширина
    // текстуры вдоль мирового X, высота — вверх по Z.
    let side_material = world_sprite_material(&station.side, false, sprite_assets, materials);
    parent.spawn((
        Mesh3d(quad.clone()),
        MeshMaterial3d(side_material),
        Transform::from_translation(anchor + Vec3::new(0.0, -half, height * 0.5))
            .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
            .with_scale(Vec3::new(edge, height, 1.0)),
    ));
    // Фронт (−X): та же стена, довернутая вокруг Z на −90° (нормаль −X,
    // ширина текстуры вдоль мирового Y, высота по-прежнему вверх).
    let front_material = world_sprite_material(front, false, sprite_assets, materials);
    parent.spawn((
        Mesh3d(quad.clone()),
        MeshMaterial3d(front_material),
        Transform::from_translation(anchor + Vec3::new(-half, 0.0, height * 0.5))
            .with_rotation(
                Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            )
            .with_scale(Vec3::new(edge, height, 1.0)),
    ));
}

/// Крест из двух вертикальных квадов вдоль осей X и Y через центр точки
/// (из середин противоположных сторон): двойная сторона и маска-альфа
/// материалов спрайтов дают правильный вид с фиксированной камеры.
/// Общая основа костра и культур.
fn spawn_cross(
    parent: &mut ChildSpawnerCommands,
    quad: &Handle<Mesh>,
    sprite_assets: &mut WorldSpriteAssets,
    materials: &mut Assets<StandardMaterial>,
    texture: &Handle<Image>,
    anchor: Vec3,
    edge: f32,
    height: f32,
) {
    let material = world_sprite_material(texture, false, sprite_assets, materials);
    let center = anchor + Vec3::Z * height * 0.5;
    // Квад 1: стена вдоль оси X (нормаль −Y, ширина по X, высота по Z).
    parent.spawn((
        Mesh3d(quad.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(center)
            .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
            .with_scale(Vec3::new(edge, height, 1.0)),
    ));
    // Квад 2: та же стена, довернутая вокруг Z на 90° — вдоль оси Y.
    parent.spawn((
        Mesh3d(quad.clone()),
        MeshMaterial3d(material),
        Transform::from_translation(center)
            .with_rotation(
                Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            )
            .with_scale(Vec3::new(edge, height, 1.0)),
    ));
}

/// Крест костра в тайле.
fn spawn_campfire_cross(
    parent: &mut ChildSpawnerCommands,
    quad: &Handle<Mesh>,
    sprite_assets: &mut WorldSpriteAssets,
    materials: &mut Assets<StandardMaterial>,
    textures: &TileTextures,
    anchor: Vec3,
) {
    spawn_cross(
        parent, quad, sprite_assets, materials,
        textures.campfire_cross(), anchor,
        WORLD_TILE_SIZE, TILE_SIZE * 0.6,
    );
}

/// Плоский декаль на поверхность тайла (культуры, склад): горизонтальный
/// квад, заполняющий ромб тайла, с z-сдвигом против z-fighting. `scale`
/// — доля тайла (1.0 — весь ромб).
fn spawn_flat_decal(
    parent: &mut ChildSpawnerCommands,
    quad: &Handle<Mesh>,
    sprite_assets: &mut WorldSpriteAssets,
    materials: &mut Assets<StandardMaterial>,
    texture: &Handle<Image>,
    anchor: Vec3,
    scale: f32,
) {
    let material = world_sprite_material(texture, false, sprite_assets, materials);
    let edge = WORLD_TILE_SIZE * scale;
    parent.spawn((
        Mesh3d(quad.clone()),
        MeshMaterial3d(material),
        Transform::from_translation(anchor + Vec3::Z * DECAL_Z_OFFSET)
            .with_scale(Vec3::new(edge, edge, 1.0)),
    ));
}

/// Размер спрайта объекта из размеров текстуры. 64 px текстуры
/// отображаются в 1.2 тайла (объекты чуть перекрывают свой тайл), поэтому
/// текстура 64×64 даёт 1.2×1.2 тайла, а высокая 64×128 (деревья) —
/// 1.2×2.4 тайла. Постройки (текстуры 16×16) занимают ровно свой тайл.
fn object_sprite_size(kind: ObjectKind, image: &Image) -> Vec2 {
    if kind.is_building() {
        return Vec2::splat(TILE_SIZE);
    }
    const TILES_PER_64PX: f32 = 1.2;
    let scale = TILES_PER_64PX * TILE_SIZE / 64.0;
    Vec2::new(image.width() as f32 * scale, image.height() as f32 * scale)
}

/// Размер спрайта объекта с кэшем по текстуре: пересчёт из `Assets<Image>`
/// только при первом спавне объекта с этой текстурой (и после загрузки —
/// незагруженная текстура в кэш не попадает, до загрузки тайл-плейсхолдер).
fn object_sprite_size_cached(
    kind: ObjectKind,
    handle: &Handle<Image>,
    images: &Assets<Image>,
    sprite_assets: &mut WorldSpriteAssets,
) -> Vec2 {
    if kind.is_building() {
        return Vec2::splat(TILE_SIZE);
    }
    match sprite_assets.object_sizes.get(&handle.id()) {
        Some(size) => *size,
        None => match images.get(handle) {
            Some(image) => {
                let size = object_sprite_size(kind, image);
                sprite_assets.object_sizes.insert(handle.id(), size);
                size
            }
            None => Vec2::splat(TILE_SIZE),
        },
    }
}

/// Высота креста культуры, мировых единиц (как у костра — часть тайла).
const CROP_HEIGHT: f32 = TILE_SIZE * 0.55;

/// Спавнит культуры блока крестами из двух вертикальных квадов (как костёр).
/// Идёт по разреженному слою культур четырёх чанков блока.
fn spawn_crops(
    parent: &mut EntityCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    sprite_assets: &mut WorldSpriteAssets,
    world: &World,
    block: BlockPos,
    origin: Vec3,
    textures: &TileTextures,
) {
    let bx0 = block.x * BLOCK_TILES;
    let by0 = block.y * BLOCK_TILES;
    let chunks_per_block = BLOCK_TILES / CHUNK_SIZE as i32;
    let cps = world.chunks_per_side as i32;
    let quad = world_sprite_quad(meshes, sprite_assets);

    parent.with_children(|parent| {
        for cy in 0..chunks_per_block {
            for cx in 0..chunks_per_block {
                let cpx = bx0.div_euclid(CHUNK_SIZE as i32) + cx;
                let cpy = by0.div_euclid(CHUNK_SIZE as i32) + cy;
                if cpx < 0 || cpy < 0 || cpx >= cps || cpy >= cps {
                    continue;
                }
                let chunk = world.chunk(ChunkPos { x: cpx as u32, y: cpy as u32 });
                for (&idx, crop) in &chunk.crops {
                    let tile = TilePos {
                        x: cpx * CHUNK_SIZE as i32 + (idx % CHUNK_SIZE as u8) as i32,
                        y: cpy * CHUNK_SIZE as i32 + (idx / CHUNK_SIZE as u8) as i32,
                    };
                    let tile_world = world_position(
                        world, Vec2::new(tile.x as f32 + 0.5, tile.y as f32 + 0.5),
                    );
                    spawn_cross(
                        parent, &quad, sprite_assets, materials,
                        textures.crop_texture(crop.kind, crop.stage),
                        tile_world - origin,
                        WORLD_TILE_SIZE, CROP_HEIGHT,
                    );
                }
            }
        }
    });
}

/// Спавнит предметы на тайлах блока (дроп) как дочерние спрайты.
fn spawn_items(
    parent: &mut EntityCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    sprite_assets: &mut WorldSpriteAssets,
    world: &World,
    block: BlockPos,
    origin: Vec3,
    camera_rotation: Quat,
    textures: &TileTextures,
    font: &Handle<Font>,
) {
    let bx0 = block.x * BLOCK_TILES;
    let by0 = block.y * BLOCK_TILES;
    let chunks_per_block = BLOCK_TILES / CHUNK_SIZE as i32;
    let cps = world.chunks_per_side as i32;
    let quad = world_sprite_quad(meshes, sprite_assets);

    parent.with_children(|parent| {
        for cy in 0..chunks_per_block {
            for cx in 0..chunks_per_block {
                let cpx = bx0.div_euclid(CHUNK_SIZE as i32) + cx;
                let cpy = by0.div_euclid(CHUNK_SIZE as i32) + cy;
                if cpx < 0 || cpy < 0 || cpx >= cps || cpy >= cps {
                    continue;
                }
                let chunk = world.chunk(ChunkPos { x: cpx as u32, y: cpy as u32 });
                for (&idx, stack) in &chunk.items {
                    let tile = TilePos {
                        x: cpx * CHUNK_SIZE as i32 + (idx % CHUNK_SIZE as u8) as i32,
                        y: cpy * CHUNK_SIZE as i32 + (idx / CHUNK_SIZE as u8) as i32,
                    };
                    let tile_world = world_position(
                        world,
                        Vec2::new(tile.x as f32 + 0.5, tile.y as f32 + 0.5),
                    );
                    let local = tile_world - origin;
                    let item_anchor = local + Vec3::Z;
                    let item_height = TILE_SIZE * 0.3;
                    let texture = textures.item_texture(stack.kind);
                    let material = world_sprite_material(texture, false, sprite_assets, materials);
                    let item_size = Vec2::splat(TILE_SIZE * 0.6);
                    let mut transform = Transform::from_translation(
                        item_anchor + camera_rotation * Vec3::Y * item_height,
                    )
                    .with_rotation(camera_rotation);
                    transform.scale = Vec3::new(item_size.x, item_size.y, 1.0);
                    parent.spawn((
                        Mesh3d(quad.clone()),
                        MeshMaterial3d(material),
                        transform,
                    ));
                    if stack.count > 1 {
                        // Счётчик растрируется крупным шрифтом и уменьшается
                        // трансформом: при font_size = 4 глифы состоят из
                        // нескольких пикселей и на экране выглядят мылом
                        // при любом зуме. 40 px с масштабом 0.1 дают тот же
                        // размер на экране, но с чёткой растеризацией.
                        let label_anchor = local + Vec3::new(TILE_SIZE * 0.25, -TILE_SIZE * 0.25, 2.0);
                        parent.spawn((
                            Text2d::new(stack.count.to_string()),
                            Visibility::Visible,
                            ItemCountLabel,
                            TextFont {
                                font: font.clone().into(),
                                font_size: ITEM_COUNT_FONT_SIZE.into(),
                                ..default()
                            },
                            TextColor(Color::WHITE),
                            Transform::from_translation(label_anchor)
                            .with_rotation(camera_rotation)
                            .with_scale(Vec3::splat(ITEM_COUNT_SCALE)),
                        ));
                    }
                }
            }
        }
    });
}

/// Маркер голограммы выбранной постройки.
#[derive(Component)]
struct BuildHologram;

/// Голограмма постройки из меню строительства: та же геометрия, что у
/// реальной постройки (крест костра, изометрический блок станции, декаль
/// склада — см. `spawn_objects`), но из полупрозрачных квадов. Зелёная —
/// поставить можно (тайл свободен, не вода, в радиусе взаимодействия и
/// хватает предметов), красная — нельзя.
///
/// Пересчёт только при изменении ввода (курсор, камера): выбор тайла —
/// луч по рельефу, а он одинаков, пока ничего не двигалось. Голограмма
/// пересобирается целиком (единицы квадов за редкий кадр). Финальную
/// корректность всё равно проверяет симуляция при применении команды,
/// поэтому устаревший цвет после подбора предмета с неподвижным курсором
/// поправится при первом же движении мыши.
fn update_build_hologram(
    mut commands: Commands,
    state: Res<GameState>,
    construction: Res<ConstructionUi>,
    textures: Res<TileTextures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sprite_assets: ResMut<WorldSpriteAssets>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &Transform, &Projection), With<Camera2d>>,
    // Отдельный запрос 2-компонентный под сигнатуру cursor_world_tile.
    pick_cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    existing: Query<Entity, With<BuildHologram>>,
    mut last_input: Local<Option<(Option<Vec2>, Vec3, f32)>>,
) {
    let selected = if construction.open && state.mode == GameMode::Player {
        construction.selected
    } else {
        None
    };
    let (Some(kind), Some(world), Some(mind)) =
        (selected, state.world.as_ref(), state.controlled_mind())
    else {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
        return;
    };

    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((_, camera_transform, projection)) = cameras.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    // Рай-пик и вся логика — только когда двигался курсор или камера.
    let frame_input = (window.cursor_position(), camera_transform.translation, ortho.scale);
    if *last_input == Some(frame_input) && existing.iter().next().is_some() {
        return;
    }
    *last_input = Some(frame_input);

    // Курсор вне окна или за границами мира: голограмму рисовать не на чем.
    let Some(tile) = cursor_world_tile(window, &pick_cameras, Some(world)) else {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
        return;
    };

    // Правило установки дублирует проверку команды: голограмма —
    // предсказание результата, а не источник истины (финальную проверку
    // всё равно делает симуляция при применении команды).
    let affordable =
        build_cost(kind).is_none_or(|(item, count)| mind.inventory.count_of(item) >= count);
    let actor = state.controlled.unwrap_or(0);
    let valid = affordable
        && buildable_at(world, tile)
        && crate::simulation::actions::reachable(world, actor, tile);

    // Пересборка целиком: геометрия зависит от вида, цвет — от допустимости.
    let anchor = world_position(world, Vec2::new(tile.x as f32 + 0.5, tile.y as f32 + 0.5));
    for entity in &existing {
        commands.entity(entity).despawn_related::<Children>();
        commands.entity(entity).despawn();
    }
    let quad = world_sprite_quad(&mut meshes, &mut sprite_assets);
    commands
        .spawn((
            Transform::from_translation(anchor),
            // Корень обязан иметь Visibility: Mesh3d-квады (дети) получают
            // Visibility автоматически, и без него InheritedVisibility детей
            // оказывается при родителе без неё — предупреждение B0004
            // («parent without InheritedVisibility») и сломанное наследование.
            Visibility::default(),
            BuildHologram,
        ))
        .with_children(|root| {
            spawn_hologram_building(root, &quad, &mut sprite_assets, &mut materials, &textures, kind, valid);
        });
}

/// Материал голограммы: текстура постройки с полупрозрачным тинтом
/// подсказки. Кэшируется по (текстура, зелёная/красная) — без кэша
/// каждое движение курсора создавало бы новые материалы в `Assets`.
fn hologram_material(
    sprite_assets: &mut WorldSpriteAssets,
    materials: &mut Assets<StandardMaterial>,
    texture: &Handle<Image>,
    valid: bool,
) -> Handle<StandardMaterial> {
    sprite_assets
        .hologram_materials
        .entry((texture.id(), valid))
        .or_insert_with(|| {
            let tint = if valid {
                Color::srgba(0.45, 1.0, 0.45, 0.55)
            } else {
                Color::srgba(1.0, 0.35, 0.35, 0.55)
            };
            materials.add(StandardMaterial {
                base_color: tint,
                base_color_texture: Some(texture.clone()),
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                unlit: true,
                ..default()
            })
        })
        .clone()
}

/// Квад голограммы в координатах относительно корня (центра тайла).
fn hologram_quad(
    parent: &mut ChildSpawnerCommands,
    quad: &Handle<Mesh>,
    material: Handle<StandardMaterial>,
    transform: Transform,
) {
    parent.spawn((Mesh3d(quad.clone()), MeshMaterial3d(material), transform));
}

/// Геометрия голограммы — та же, что у реальной постройки в
/// `spawn_objects`, но с материалами-тинтами.
fn spawn_hologram_building(
    parent: &mut ChildSpawnerCommands,
    quad: &Handle<Mesh>,
    sprite_assets: &mut WorldSpriteAssets,
    materials: &mut Assets<StandardMaterial>,
    textures: &TileTextures,
    kind: BuildingKind,
    valid: bool,
) {
    match kind {
        // Костёр — крест из двух вертикальных квадов вдоль осей X и Y.
        BuildingKind::Campfire => {
            let material = hologram_material(sprite_assets, materials, textures.campfire_cross(), valid);
            let edge = WORLD_TILE_SIZE;
            let height = TILE_SIZE * 0.6;
            let center = Vec3::Z * (height * 0.5);
            hologram_quad(
                parent, quad, material.clone(),
                Transform::from_translation(center)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::new(edge, height, 1.0)),
            );
            hologram_quad(
                parent, quad, material,
                Transform::from_translation(center)
                    .with_rotation(
                        Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
                            * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    )
                    .with_scale(Vec3::new(edge, height, 1.0)),
            );
        }
        // Склад — плоская декаль на поверхности тайла.
        BuildingKind::Storage => {
            let material = hologram_material(sprite_assets, materials, textures.storage_flat(), valid);
            let edge = WORLD_TILE_SIZE;
            hologram_quad(
                parent, quad, material,
                Transform::from_translation(Vec3::Z * DECAL_Z_OFFSET)
                    .with_scale(Vec3::new(edge, edge, 1.0)),
            );
        }
        // Остальные станции — изометрический блок: верх + видимые грани.
        kind => {
            let station = textures.station_textures(kind);
            let edge = WORLD_TILE_SIZE * STATION_BLOCK_SCALE;
            let half = edge * 0.5;
            let height = STATION_BLOCK_HEIGHT;
            hologram_quad(
                parent, quad, hologram_material(sprite_assets, materials, &station.top, valid),
                Transform::from_translation(Vec3::Z * height)
                    .with_scale(Vec3::new(edge, edge, 1.0)),
            );
            hologram_quad(
                parent, quad, hologram_material(sprite_assets, materials, &station.side, valid),
                Transform::from_translation(Vec3::new(0.0, -half, height * 0.5))
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
                    .with_scale(Vec3::new(edge, height, 1.0)),
            );
            hologram_quad(
                parent, quad, hologram_material(sprite_assets, materials, &station.front, valid),
                Transform::from_translation(Vec3::new(-half, 0.0, height * 0.5))
                    .with_rotation(
                        Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2)
                            * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                    )
                    .with_scale(Vec3::new(edge, height, 1.0)),
            );
        }
    }
}
