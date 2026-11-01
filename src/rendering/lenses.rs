//! Линзы карты (редактор): полупрозрачные подсветки чанков по флагам
//! из `GameState::lenses` — «есть разумные существа» и «есть рукотворные
//! постройки». Данные сканируются не чаще раза в секунду и только по
//! видимым блокам (O(сущностей) и O(тайлы видимых чанков) соответственно);
//! изменение подсвеченного множества перестраивает квады дельтой, яркость
//! слоя сглаженно анимируется (GAME_DESIGN: обновление раз в секунду,
//! сглаженно).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::app::state::GameState;
use crate::simulation::objects::ObjectKind;
use crate::simulation::{CHUNK_SIZE, ChunkPos, TilePos, World as SimWorld, split_tile};
use super::isometric::world_position;
use super::{BLOCK_TILES, BlockPos, TerrainRender, render_world};

/// Период сканирования мира линзами, секунд.
const SCAN_INTERVAL_SECS: f32 = 1.0;

/// Целевая непрозрачность подсветки чанка.
const LENS_ALPHA: f32 = 0.3;
/// Скорость сглаживания яркости (экспоненциальное затухание).
const ALPHA_SMOOTH_PER_SEC: f32 = 8.0;
/// Порог, ниже которого слой считается выключенным (квады снимаются).
const ALPHA_EPSILON: f32 = 0.005;

/// Цвет линзы разумных существ (тёплый оранжевый).
const SENTIENTS_COLOR: Color = Color::srgb(1.0, 0.62, 0.2);
/// Цвет линзы построек (холодный синий).
const BUILDINGS_COLOR: Color = Color::srgb(0.3, 0.7, 1.0);

/// Состояние рендера линз: квад на подсвеченный чанк, общие материалы,
/// сглаженная яркость слоёв и таймер сканирования.
#[derive(Resource, Default)]
struct LensRender {
    sentients: HashMap<ChunkPos, Entity>,
    buildings: HashMap<ChunkPos, Entity>,
    mesh: Option<Handle<Mesh>>,
    mat_sentients: Option<Handle<StandardMaterial>>,
    mat_buildings: Option<Handle<StandardMaterial>>,
    /// Текущая (сглаженная) непрозрачность слоёв.
    alpha_sentients: f32,
    alpha_buildings: f32,
    scan_cooldown: f32,
}

pub struct LensPlugin;

impl Plugin for LensPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LensRender>()
            .add_systems(Update, update_map_lenses.after(render_world));
    }
}

impl SimWorld {
    /// Множество чанков с разумными существами: один проход по списку
    /// существ (O(сущностей), порядок обхода не влияет на множество).
    fn lens_sentient_chunks(&self) -> std::collections::HashSet<ChunkPos> {
        let mut set = std::collections::HashSet::new();
        for entity in self.entities.iter() {
            if entity.mind.is_some() {
                set.insert(split_tile(TilePos {
                    x: entity.pos.x.floor() as i32,
                    y: entity.pos.y.floor() as i32,
                })
                .0);
            }
        }
        set
    }

    /// Есть ли в чанке постройка (рукотворный объект). Тайлы чанка —
    /// разреженный слой: обход только занятых тайлов.
    fn chunk_has_building(&self, chunk: ChunkPos) -> bool {
        if chunk.x >= self.chunks_per_side || chunk.y >= self.chunks_per_side {
            return false;
        }
        self.chunk(chunk)
            .objects
            .values()
            .any(|obj| matches!(obj.kind, ObjectKind::Building(_)))
    }
}

/// Обновляет линзы: скан раз в секунду, дельта-обновление квадов,
/// сглаживание яркости материалов. Слои обновляются последовательно
/// (по одному заёму полей состояния за раз).
fn update_map_lenses(
    mut commands: Commands,
    time: Res<Time>,
    state: Res<GameState>,
    render: Res<TerrainRender>,
    mut lenses: ResMut<LensRender>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(world) = state.world.as_ref() else {
        // Мир закрыт (перегенерация): слои гасятся, квадов нет.
        clear_layer(&mut commands, &mut lenses.sentients);
        clear_layer(&mut commands, &mut lenses.buildings);
        lenses.alpha_sentients = 0.0;
        lenses.alpha_buildings = 0.0;
        return;
    };

    // Скан не чаще `SCAN_INTERVAL_SECS`; результаты скана живут до
    // следующего — между сканами только сглаживание яркости.
    lenses.scan_cooldown -= time.delta_secs();
    let scan_due = lenses.scan_cooldown <= 0.0;
    if scan_due {
        lenses.scan_cooldown = SCAN_INTERVAL_SECS;
    }

    let chunk_world_size = CHUNK_SIZE as f32 * super::isometric::WORLD_TILE_SIZE;
    // Общий квадрат-меш единичного размера: масштаб задаётся чанком.
    let mesh = lenses
        .mesh
        .get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0)))
        .clone();
    // Разборка полей: через DerefMut ResMut поля заёмно не разделяются.
    let LensRender {
        mat_sentients,
        mat_buildings,
        alpha_sentients,
        alpha_buildings,
        sentients,
        buildings,
        ..
    } = &mut *lenses;

    // Видимые чанки — из активных блоков рендера (линза покрывает только
    // то, что пользователь видит: скан и число квадов ограничены).
    let mut visible_chunks: Vec<ChunkPos> = Vec::new();
    for block in render.active.keys() {
        push_block_chunks(world, block, &mut visible_chunks);
    }

    // Результаты скана (только на скане и только для включённых линз).
    let sentients_want = if scan_due && state.lenses.sentients {
        let set = world.lens_sentient_chunks();
        Some(
            visible_chunks
                .iter()
                .copied()
                .filter(|chunk| set.contains(chunk))
                .collect::<Vec<ChunkPos>>(),
        )
    } else {
        None
    };
    let buildings_want = if scan_due && state.lenses.buildings {
        Some(
            visible_chunks
                .iter()
                .copied()
                .filter(|chunk| world.chunk_has_building(*chunk))
                .collect::<Vec<ChunkPos>>(),
        )
    } else {
        None
    };

    // Слой разумных существ.
    update_lens_layer(
        &mut commands,
        world,
        time.delta_secs(),
        state.lenses.sentients,
        SENTIENTS_COLOR,
        sentients_want,
        chunk_world_size,
        &mesh,
        &mut materials,
        mat_sentients,
        alpha_sentients,
        sentients,
    );
    // Слой построек.
    update_lens_layer(
        &mut commands,
        world,
        time.delta_secs(),
        state.lenses.buildings,
        BUILDINGS_COLOR,
        buildings_want,
        chunk_world_size,
        &mesh,
        &mut materials,
        mat_buildings,
        alpha_buildings,
        buildings,
    );
}

/// Обновляет один слой линзы: сглаживание яркости, дельта-обновление
/// квадов по результатам скана (`None` — скана не было, менять множество
/// не нужно).
#[allow(clippy::too_many_arguments)]
fn update_lens_layer(
    commands: &mut Commands,
    world: &SimWorld,
    delta_secs: f32,
    enabled: bool,
    color: Color,
    want: Option<Vec<ChunkPos>>,
    chunk_world_size: f32,
    mesh: &Handle<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    material: &mut Option<Handle<StandardMaterial>>,
    alpha: &mut f32,
    quads: &mut HashMap<ChunkPos, Entity>,
) {
    // Сглаживание яркости: к цели (включена — LENS_ALPHA, выключена — 0).
    let target = if enabled { LENS_ALPHA } else { 0.0 };
    let k = 1.0 - (-ALPHA_SMOOTH_PER_SEC * delta_secs).exp();
    *alpha += (target - *alpha) * k;
    if *alpha < ALPHA_EPSILON {
        *alpha = 0.0;
    }

    // Материал слоя: цвет фиксирован, альфа анимируется.
    let material = material.get_or_insert_with(|| materials.add(unlit_material(color)));
    if let Some(mut mat) = materials.get_mut(&*material) {
        let mut base = color;
        base.set_alpha(*alpha);
        if mat.base_color != base {
            mat.base_color = base;
        }
    }

    // Линза выключена (или ещё гаснет): квадов не держим.
    if !enabled || *alpha <= ALPHA_EPSILON {
        clear_layer(commands, quads);
        return;
    }
    let Some(want) = want else {
        return;
    };
    // Снятие квадов чанков, которые больше не подсвечиваются (или ушли
    // с экрана).
    let stale: Vec<ChunkPos> = quads
        .keys()
        .copied()
        .filter(|chunk| !want.contains(chunk))
        .collect();
    for chunk in stale {
        if let Some(entity) = quads.remove(&chunk) {
            commands.entity(entity).despawn();
        }
    }
    // Новые подсвеченные чанки: плоский квад на центр чанка.
    for chunk in want {
        if quads.contains_key(&chunk) {
            continue;
        }
        let center_x = (chunk.x as i32 * CHUNK_SIZE as i32) as f32 + CHUNK_SIZE as f32 / 2.0;
        let center_y = (chunk.y as i32 * CHUNK_SIZE as i32) as f32 + CHUNK_SIZE as f32 / 2.0;
        let anchor = world_position(world, Vec2::new(center_x, center_y));
        let transform = Transform::from_translation(anchor + Vec3::Z * LENS_Z_OFFSET)
            .with_scale(Vec3::new(chunk_world_size, chunk_world_size, 1.0));
        let entity = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                transform,
            ))
            .id();
        quads.insert(chunk, entity);
    }
}

/// Небольшой z-сдвиг подсветки над рельефом против z-fighting (больше
/// декаля культур: линза поверх пашни тоже).
const LENS_Z_OFFSET: f32 = 0.06;

/// Материал линзы: без света, с блендингом (полупрозрачная заливка).
fn unlit_material(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    }
}

/// Снимает все квадраты слоя.
fn clear_layer(commands: &mut Commands, quads: &mut HashMap<ChunkPos, Entity>) {
    for (_, entity) in quads.drain() {
        commands.entity(entity).despawn();
    }
}

/// Добавляет чанки блока (64×64 тайлов = 4×4 чанков) в список видимых.
fn push_block_chunks(world: &SimWorld, block: &BlockPos, out: &mut Vec<ChunkPos>) {
    let bx0 = block.x * BLOCK_TILES;
    let by0 = block.y * BLOCK_TILES;
    let cps = world.chunks_per_side as i32;
    let cpx0 = bx0.div_euclid(CHUNK_SIZE as i32);
    let cpy0 = by0.div_euclid(CHUNK_SIZE as i32);
    for cy in 0..(BLOCK_TILES / CHUNK_SIZE as i32) {
        for cx in 0..(BLOCK_TILES / CHUNK_SIZE as i32) {
            let (cpx, cpy) = (cpx0 + cx, cpy0 + cy);
            if cpx >= 0 && cpy >= 0 && cpx < cps && cpy < cps {
                out.push(ChunkPos { x: cpx as u32, y: cpy as u32 });
            }
        }
    }
}
