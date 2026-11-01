//! Освещение: ночное затемнение мира и «свет» костра.
//!
//! Тьма — один оверлей поверх мира. Фрагментный шейдер затемняет каждый пиксель
//! чёрным с альфой, поэтому фактура и оттенки мира сохраняются. Он вычисляет
//! ослабление тьмы от каждого костра напрямую, без дискретизации сеткой.
//!
//! UI рисуется отдельным проходом поверх мира и не затемняется.

use std::collections::HashSet;

use bevy::asset::RenderAssetUsages;
use bevy::camera::{Camera2d, Projection};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::{render_resource::AsBindGroup, storage::ShaderBuffer};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d};

use crate::app::state::{ClockRes, GameState};
use crate::rendering::isometric::{WORLD_TILE_SIZE, world_position};
use crate::simulation::{BuildingKind, ChunkPos, ObjectKind, TilePos, World, CHUNK_SIZE};

/// Светлота мира глухой ночью как доля дневной освещённости.
pub(crate) const NIGHT_DAYLIGHT: f32 = 0.08;

/// Радиус света костра в тайлах.
pub(crate) const LIGHT_RADIUS_TILES: f32 = 6.0;

/// Радиус света в мировых пикселях.
const LIGHT_RADIUS_PX: f32 = LIGHT_RADIUS_TILES * WORLD_TILE_SIZE;

/// Границы переходов суток в часах: рассвет 5–7, закат 18–20.
const DAWN_START_H: f32 = 5.0;
const DAWN_END_H: f32 = 7.0;
const DUSK_START_H: f32 = 18.0;
const DUSK_END_H: f32 = 20.0;

const DARKNESS_SHADER_PATH: &str = "shaders/darkness.wesl";

/// Светлота мира по доле суток [0, 1): 1.0 днём (7:00–18:00), NIGHT_DAYLIGHT
/// глухой ночью (20:00–5:00), между ними — плавные рассвет и закат (smoothstep).
/// Затенение равно `1 − daylight`.
pub(crate) fn daylight_factor(day_fraction: f32) -> f32 {
    let hour = day_fraction.fract() * 24.0;
    let t = if hour >= DUSK_END_H || hour < DAWN_START_H {
        0.0
    } else if hour < DAWN_END_H {
        smoothstep((hour - DAWN_START_H) / (DAWN_END_H - DAWN_START_H))
    } else if hour < DUSK_START_H {
        1.0
    } else {
        1.0 - smoothstep((hour - DUSK_START_H) / (DUSK_END_H - DUSK_START_H))
    };
    NIGHT_DAYLIGHT + (1.0 - NIGHT_DAYLIGHT) * t
}

/// Классический smoothstep на [0, 1].
fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct DarknessMaterial {
    #[uniform(0)]
    params: Vec4,
    #[storage(1, read_only)]
    fire_positions: Handle<ShaderBuffer>,
}

impl Material2d for DarknessMaterial {
    fn fragment_shader() -> ShaderRef {
        DARKNESS_SHADER_PATH.into()
    }
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// Реестр горящих костров мира (тайлы). Костров мало, а изменения
/// объектов проходят через команды и оставляют dirty-тайлы — реестр
/// сверяется по ним, без обхода мира каждый кадр.
#[derive(Resource, Default)]
pub(crate) struct CampfireLights {
    positions: HashSet<TilePos>,
    /// Счётчик изменений реестра: оверлей тьмы по нему понимает, что набор
    /// огней изменился, не сравнивая множества каждый кадр.
    version: u64,
}

/// Оверлей тьмы: одна Bevy-сущность с мешем и материалом, пересобираемая
/// под видимую область.
#[derive(Resource, Default)]
pub(crate) struct DarknessOverlay {
    entity: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
    material: Option<Handle<DarknessMaterial>>,
    fire_positions: Option<Handle<ShaderBuffer>>,
    /// Кадр, на который собран оверлей. Пока ключ не изменился, меш, буфер
    /// огней и материал не трогаются — иначе каждый ночной кадр получал бы
    /// AssetEvent::Modified и лишнюю подготовку материала.
    last_frame: Option<OverlayFrame>,
}

/// Состояние кадра, от которого зависит содержимое оверлея тьмы. Поворота
/// камеры нет (ARCHITECTURE 5), поэтому достаточно переноса камеры.
#[derive(Clone, PartialEq)]
struct OverlayFrame {
    darkness: f32,
    view_min: Vec2,
    view_max: Vec2,
    camera_translation: Vec3,
    fires_version: u64,
}

/// Держит реестр костров в соответствии с миром. Полный пересбор — после
/// перегенерации мира или загрузки сохранения (`pending_rerender`), иначе
/// точечная сверка по изменённым тайлам обоих каналов (рельеф и декор:
/// костёр — объект, но заодно проверяется и рельеф под ним). Запускается
/// до `render_world`, который изымает dirty-тайлы.
pub(crate) fn update_campfire_positions(
    state: Res<GameState>,
    mut lights: ResMut<CampfireLights>,
) {
    let Some(world) = state.world.as_ref() else {
        if !lights.positions.is_empty() {
            lights.positions.clear();
            lights.version += 1;
        }
        return;
    };
    if state.pending_rerender {
        lights.positions = scan_campfires(world);
        lights.version += 1;
        return;
    }
    let mut changed = false;
    for tile in state
        .dirty_tiles
        .terrain
        .iter()
        .chain(state.dirty_tiles.decor.iter())
    {
        let has_fire = matches!(
            world.object_at(*tile),
            Some(obj) if matches!(obj.kind, ObjectKind::Building(BuildingKind::Campfire))
        );
        changed |= has_fire && lights.positions.insert(*tile)
            || !has_fire && lights.positions.remove(tile);
    }
    if changed {
        lights.version += 1;
    }
}

/// Все костры мира. Порядок обхода не важен: результат — множество.
fn scan_campfires(world: &World) -> HashSet<TilePos> {
    let mut found = HashSet::new();
    for cpy in 0..world.chunks_per_side {
        for cpx in 0..world.chunks_per_side {
            let chunk = world.chunk(ChunkPos { x: cpx, y: cpy });
            for (&idx, obj) in &chunk.objects {
                if matches!(obj.kind, ObjectKind::Building(BuildingKind::Campfire)) {
                    found.insert(TilePos {
                        x: (cpx * CHUNK_SIZE as u32) as i32 + (idx % CHUNK_SIZE as u8) as i32,
                        y: (cpy * CHUNK_SIZE as u32) as i32 + (idx / CHUNK_SIZE as u8) as i32,
                    });
                }
            }
        }
    }
    found
}

/// Пересобирает оверлей тьмы под видимую область и время суток. Запускается
/// после `render_world` (камера и мир уже актуальны). Днём оверлей скрыт.
pub(crate) fn update_darkness_overlay(
    mut commands: Commands,
    state: Res<GameState>,
    clock: Res<ClockRes>,
    lights: Res<CampfireLights>,
    cameras: Query<(&Camera, &Transform, &Projection), With<Camera2d>>,
    mut overlay: ResMut<DarknessOverlay>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DarknessMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let daylight = daylight_factor(clock.0.time_of_day_fraction());
    let darkness = (1.0 - daylight).clamp(0.0, 1.0);

    let Some(world) = state.world.as_ref() else {
        despawn_overlay(&mut commands, &mut overlay);
        return;
    };
    if darkness <= 0.0 {
        if let Some(entity) = overlay.entity {
            commands.entity(entity).insert(Visibility::Hidden);
        }
        // После скрытия оверлей собирается заново, даже если кадр совпадёт.
        overlay.last_frame = None;
        return;
    }

    // Видимая область с запасом на свет костров у края экрана.
    let Ok((camera, camera_transform, projection)) = cameras.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let Some(viewport_size) = camera.logical_viewport_size() else {
        return;
    };
    let half_size = viewport_size * (ortho.scale * 0.5);
    let margin = Vec2::splat((LIGHT_RADIUS_TILES + 2.0) * WORLD_TILE_SIZE);
    let view_min = -half_size - margin;
    let view_max = half_size + margin;

    // Кадр не изменился (стоячая камера, стабильная тьма, костры те же):
    // собранный оверлей уже корректен и видим — ничего не трогаем.
    let frame = OverlayFrame {
        darkness,
        view_min,
        view_max,
        camera_translation: camera_transform.translation,
        fires_version: lights.version,
    };
    if overlay.last_frame.as_ref() == Some(&frame) {
        return;
    }
    let world_to_camera = camera_transform.to_matrix().inverse();

    // Центры костров в мировых пикселях; костры вне области с запасом
    // света всё равно не дают вклада — отсекаем их до построения сетки.
    // Сортировка по X: фрагментный шейдер ищёт окно огней бинарным
    // поиском, а не перебирает все огни на каждый пиксель экрана.
    let mut fires: Vec<Vec2> = lights
        .positions
        .iter()
        .map(|tile| {
            let position = world_position(
                world, Vec2::new(tile.x as f32 + 0.5, tile.y as f32 + 0.5),
            );
            world_to_camera.transform_point3(position).truncate()
        })
        .filter(|fire| {
            fire.x >= view_min.x - LIGHT_RADIUS_PX
                && fire.x <= view_max.x + LIGHT_RADIUS_PX
                && fire.y >= view_min.y - LIGHT_RADIUS_PX
                && fire.y <= view_max.y + LIGHT_RADIUS_PX
        })
        .collect();
    fires.sort_by(|a, b| a.x.total_cmp(&b.x));

    let mesh = overlay
        .mesh
        .get_or_insert_with(|| meshes.add(Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )));
    let handle = mesh.clone();
    let mut mesh = meshes
        .get_mut(&handle)
        .expect("darkness overlay mesh asset must exist");
    rebuild_darkness_mesh(&mut mesh, view_min, view_max);

    if overlay.fire_positions.is_none() {
        overlay.fire_positions = Some(buffers.add(ShaderBuffer::from(vec![[0.0; 4]])));
    }
    let fire_positions = overlay
        .fire_positions
        .as_ref()
        .expect("fire position buffer was just initialized")
        .clone();
    let mut fire_buffer = buffers
        .get_mut(&fire_positions)
        .expect("darkness fire position buffer asset must exist");
    fire_buffer.clear();
    if fires.is_empty() {
        // Storage buffers cannot be bound with zero bytes.
        fire_buffer.extend([[0.0; 4]]);
    } else {
        fire_buffer.extend(fires.iter().map(|fire| [fire.x, fire.y, 1.0, 0.0]));
    }

    if overlay.material.is_none() {
        overlay.material = Some(materials.add(DarknessMaterial {
            params: Vec4::new(darkness, LIGHT_RADIUS_PX, 0.0, 0.0),
            fire_positions,
        }));
    }
    let material = overlay
        .material
        .as_ref()
        .expect("darkness material was just initialized")
        .clone();
    materials
        .get_mut(&material)
        .expect("darkness material asset must exist")
        .params = Vec4::new(darkness, LIGHT_RADIUS_PX, 0.0, 0.0);

    let transform = *camera_transform;
    match overlay.entity {
        Some(entity) => {
            commands.entity(entity).insert((transform, Visibility::Visible));
        }
        None => {
            let entity = commands
                .spawn((
                    Mesh2d(handle),
                    MeshMaterial2d(material),
                    transform,
                    Visibility::Visible,
                ))
                .id();
            overlay.entity = Some(entity);
        }
    }
    overlay.last_frame = Some(frame);
}

/// Деспавнит оверлей (мир отсутствует).
fn despawn_overlay(commands: &mut Commands, overlay: &mut DarknessOverlay) {
    if let Some(entity) = overlay.entity.take() {
        commands.entity(entity).despawn();
    }
    overlay.last_frame = None;
}

/// Четыре вершины и два треугольника покрывают видимую область с запасом.
/// Локальные UV передают шейдеру точные координаты камеры для расчёта света.
fn rebuild_darkness_mesh(mesh: &mut Mesh, view_min: Vec2, view_max: Vec2) {
    let corners = [
        view_min,
        Vec2::new(view_max.x, view_min.y),
        view_max,
        Vec2::new(view_min.x, view_max.y),
    ];
    let positions = corners
        .iter()
        .map(|point| [point.x, point.y, -1.0])
        .collect::<Vec<_>>();
    let uvs = corners
        .iter()
        .map(|point| [point.x, point.y])
        .collect::<Vec<_>>();

    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
}
