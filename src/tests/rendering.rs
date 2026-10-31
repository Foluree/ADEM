//! Тесты рендера: LOD и переходы, агрегация тайлов, атлас, меши блоков,
//! а также UI-виджеты, не требующие графики (EditableText).

use crate::rendering::{
    BlockPos, TILE_KIND_COUNT, atlas_uv, build_block_mesh, dominant_atlas_index, lod_for_scale,
    tile_atlas_index,
};
use crate::simulation::generation::generate_world;
use crate::simulation::tile::TileType;
use crate::simulation::world::WorldSize;

/// LOD растёт при отдалении камеры: 0, пока ячейка не доросла до тайла,
/// затем +1 на каждое удвоение масштаба, и не выходит за предел.
/// Пороги вычисляются из TARGET_CELL_PX, чтобы тест не зависел от тюнинга.
#[test]
fn lod_grows_with_camera_scale() {
    use crate::rendering::{MAX_LOD, TARGET_CELL_PX};
    use crate::simulation::TILE_SIZE;

    let step = TILE_SIZE / TARGET_CELL_PX;
    assert_eq!(lod_for_scale(0.25), 0);
    assert_eq!(lod_for_scale(1.0), 0);
    assert_eq!(lod_for_scale(step), 0);
    assert_eq!(lod_for_scale(step * 2.0), 1);
    assert_eq!(lod_for_scale(step * 4.0), 2);
    assert_eq!(lod_for_scale(step * 8.0), 3);
    assert_eq!(lod_for_scale(step * 4096.0), MAX_LOD);
    assert_eq!(lod_for_scale(1_000_000.0), MAX_LOD);
}

/// Счётчик предмета скрывается, когда он меньше четырёх экранных пикселей,
/// и всегда скрыт при укрупнённом LOD.
#[test]
fn item_count_labels_obey_screen_size_and_lod() {
    use crate::rendering::item_counts_visible;

    assert!(item_counts_visible(1.0), "ровно 4 px — показываем");
    assert!(item_counts_visible(0.5), "при приближении текст читаем");
    assert!(!item_counts_visible(1.01), "меньше 4 px — скрываем");
    assert!(
        !item_counts_visible(crate::simulation::TILE_SIZE),
        "на LOD выше нулевого текст скрыт"
    );
}

/// Доминирующий тайл игнорирует пустые клетки, при равенстве берёт меньший индекс.
#[test]
fn dominant_tile_aggregation() {
    let mut counts = [0_u32; TILE_KIND_COUNT];
    assert_eq!(dominant_atlas_index(&counts), None);

    counts[3] = 2;
    counts[6] = 5;
    assert_eq!(dominant_atlas_index(&counts), Some(6));

    counts[3] = 5;
    assert_eq!(dominant_atlas_index(&counts), Some(3));
}

/// Индексы атласа: Empty не рисуется, остальные виды идут подряд.
#[test]
fn tile_types_map_to_atlas_slots() {
    assert_eq!(tile_atlas_index(TileType::Empty), None);
    assert_eq!(tile_atlas_index(TileType::Water), Some(0));
    assert_eq!(tile_atlas_index(TileType::Farmland), Some(7));
    assert_eq!(tile_atlas_index(TileType::DeepWater), Some(8));
    assert_eq!(tile_atlas_index(TileType::Ocean), Some(9));
    assert_eq!(tile_atlas_index(TileType::Mountain), Some(10));
}

/// UV-прямоугольники тайлов лежат внутри атласа и не вырождены.
#[test]
fn atlas_uv_stays_inside_texture() {
    for index in 0..TILE_KIND_COUNT {
        let [u0, v0, u1, v1] = atlas_uv(index as u8);
        assert!(0.0 <= u0 && u0 < u1 && u1 <= 1.0, "битый U-диапазон: {index}");
        assert!(0.0 <= v0 && v0 < v1 && v1 <= 1.0, "битый V-диапазон: {index}");
    }
}

/// Меш блока содержит по кваду на ячейку сетки LOD — и на однородных чанках
/// (быстрый путь), и на смешанных (общий путь с подсчётом).
#[test]
fn block_mesh_quad_count_matches_lod() {
    let world = generate_world(WorldSize::new(256), 42);

    let mesh = build_block_mesh(&world, BlockPos { x: 0, y: 0 }, 0).expect("LOD 0");
    assert!(mesh.count_vertices() >= 64 * 64 * 4);
    assert!(
        mesh.indices().is_some_and(|indices| indices.len() >= 64 * 64 * 6)
    );

    let mesh = build_block_mesh(&world, BlockPos { x: 0, y: 0 }, 3).expect("LOD 3");
    assert!(mesh.count_vertices() >= 8 * 8 * 4);
    assert!(
        mesh.indices().is_some_and(|indices| indices.len() >= 8 * 8 * 6)
    );
}

#[test]
fn terrain_elevation_is_derived_from_tile_type() {
    use crate::rendering::isometric::{WORLD_TILE_SIZE, tile_elevation, world_position};
    use crate::simulation::{TilePos, World};

    let mut world = World::new(WorldSize::new(256), 1);
    world.set_terrain(TilePos { x: 2, y: 3 }, TileType::Mountain);
    let position = world_position(&world, bevy::math::Vec2::new(2.5, 3.5));

    assert_eq!(position.x, 2.5 * WORLD_TILE_SIZE);
    assert_eq!(position.y, 3.5 * WORLD_TILE_SIZE);
    assert_eq!(position.z, tile_elevation(TileType::Mountain));
    assert_eq!(tile_elevation(TileType::Water), tile_elevation(TileType::Sand));
    assert!(
        tile_elevation(TileType::Grass) < tile_elevation(TileType::ForestGrass)
            && tile_elevation(TileType::ForestGrass) < tile_elevation(TileType::Stone)
            && tile_elevation(TileType::Stone) < tile_elevation(TileType::Mountain)
    );
}

#[test]
fn elevated_tile_mesh_has_top_and_camera_facing_sides() {
    use crate::simulation::{TilePos, World};

    let mut world = World::new(WorldSize::new(256), 1);
    world.set_terrain(TilePos { x: 0, y: 0 }, TileType::Mountain);
    let mesh = build_block_mesh(&world, BlockPos { x: 0, y: 0 }, 0).expect("mountain tile");

    // The fixed camera sees only the negative X and negative Y walls.
    assert_eq!(mesh.count_vertices(), 12);
    assert_eq!(mesh.indices().map(|indices| indices.len()), Some(18));
}

#[test]
fn adjacent_equal_height_tiles_do_not_create_internal_walls() {
    use crate::simulation::TilePos;

    let mut world = crate::simulation::World::new(WorldSize::new(256), 1);
    world.set_terrain(TilePos { x: 63, y: 0 }, TileType::Mountain);
    world.set_terrain(TilePos { x: 64, y: 0 }, TileType::Mountain);

    let left = build_block_mesh(&world, BlockPos { x: 0, y: 0 }, 0).expect("left mountain");
    let right = build_block_mesh(&world, BlockPos { x: 1, y: 0 }, 0).expect("right mountain");

    // Two tops and three visible outside walls; the shared edge crosses a block boundary.
    assert_eq!(left.count_vertices() + right.count_vertices(), 20);
    let index_count = |mesh: &bevy::mesh::Mesh| mesh.indices().map(|indices| indices.len()).unwrap_or(0);
    assert_eq!(index_count(&left) + index_count(&right), 30);
}

/// Переход LOD: продолжение стройки, отмена при возврате, перезапуск при смене.
#[test]
fn lod_transition_decisions() {
    use crate::rendering::{LodTransition, lod_transition_for};
    assert_eq!(lod_transition_for(0, 0, None), LodTransition::None);
    assert_eq!(lod_transition_for(0, 0, Some(2)), LodTransition::Cancel);
    assert_eq!(lod_transition_for(2, 0, Some(2)), LodTransition::Continue);
    assert_eq!(lod_transition_for(3, 0, Some(2)), LodTransition::Start);
    assert_eq!(lod_transition_for(1, 0, None), LodTransition::Start);
}

/// Частичный блок у края мира не читает тайлы за его границей.
#[test]
fn block_mesh_handles_partial_edge_block() {
    // 288 тайлов = 4.5 блока по 64: последний ряд и колонка частичные.
    let world = generate_world(WorldSize::new(288), 7);
    let mesh = build_block_mesh(&world, BlockPos { x: 4, y: 4 }, 0);
    assert!(mesh.is_some(), "частичный блок должен строиться");
}

/// Сброс текста поля правит буфер на месте и не трогает настройки виджета:
/// пересоздание компонента ломало отрисовку текста и курсора.
#[test]
fn editable_reset_keeps_widget_state() {
    use crate::ui::bottom_panel::reset_editable;
    use bevy::text::{EditableText, TextEdit};

    let mut editable = EditableText::new("256");
    editable.max_characters = Some(5);

    reset_editable(&mut editable, "1024");
    assert_eq!(editable.value().to_string(), "1024");
    assert_eq!(editable.max_characters, Some(5));
    assert!(editable
        .pending_edits
        .iter()
        .any(|edit| matches!(edit, TextEdit::TextEnd(_))));
}
