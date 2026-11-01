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

/// Тестовая RGBA8-текстура с непрозрачными точками по списку координат.
fn test_image(width: u32, height: u32, opaque: &[(u32, u32)]) -> bevy::image::Image {
    let mut data = vec![0_u8; (width * height * 4) as usize];
    for &(x, y) in opaque {
        let offset = ((y * width + x) * 4) as usize;
        data[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
    }
    bevy::image::Image::new(
        bevy::render::render_resource::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    )
}

/// Видимые области по альфе считаются в долях текстуры, правая и нижняя
/// границы — исключающие; пустая (полностью прозрачная) текстура — None.
#[test]
fn alpha_bounds_scan_fractions() {
    use crate::rendering::scan_alpha_bounds;

    let image = test_image(8, 4, &[(2, 1), (5, 2)]);
    let bounds = scan_alpha_bounds(&image).expect("есть непрозрачные пиксели");
    assert_eq!(bounds.left, 2.0 / 8.0);
    assert_eq!(bounds.top, 1.0 / 4.0);
    assert_eq!(bounds.right, 6.0 / 8.0);
    assert_eq!(bounds.bottom, 3.0 / 4.0);
    // «Ноги на землю»: под рисунком одна прозрачная строка из четырёх.
    assert_eq!(bounds.bottom_offset(), 0.25);

    assert_eq!(scan_alpha_bounds(&test_image(4, 4, &[])), None);
}

/// Дилатация прямоугольником: непрозрачная точка расширяется на радиус
/// во все стороны, окно у краёв обрезается, прозрачная маска не растёт.
#[test]
fn dilate_mask_expands_by_radius() {
    use crate::rendering::dilate_mask;

    let mut mask = vec![false; 25];
    mask[2 * 5 + 2] = true; // точка (2, 2) в сетке 5×5
    let dilated = dilate_mask(&mask, 5, 5, 1, 1);
    for y in 1..=3 {
        for x in 1..=3 {
            assert!(dilated[y * 5 + x], "точка ({x}, {y}) должна быть закрашена");
        }
    }
    // Углы сетки за пределами радиуса остаются прозрачными.
    assert!(!dilated[0] && !dilated[4] && !dilated[20] && !dilated[24]);

    // Радиус больше текстуры: закрашено всё, выхода за границы нет.
    assert!(dilate_mask(&mask, 5, 5, 7, 7).iter().all(|pixel| *pixel));
    assert!(dilate_mask(&[false; 9], 3, 3, 2, 2)
        .iter()
        .all(|pixel| !pixel));
}

/// Канвас контура больше исходника на двойной радиус с каждой стороны,
/// непрозрачные пиксели — белые и лежат со сдвигом на радиус.
#[test]
fn outline_image_canvas_padding() {
    use crate::rendering::build_outline_image;

    // 9×5, одна точка (4, 2); радиус 2×1 → канвас 13×7.
    let source = test_image(9, 5, &[(4, 2)]);
    let outline = build_outline_image(&source, 2, 1).expect("RGBA8-источник");
    assert_eq!(outline.width(), 13);
    assert_eq!(outline.height(), 7);

    let data = outline.data.as_deref().expect("данные канваса");
    let pixel = |x: u32, y: u32| {
        let offset = ((y * 13 + x) * 4) as usize;
        (data[offset], data[offset + 3])
    };
    // Точка канваса (4+2, 2+1) = (6, 3); дилатация даёт прямоугольник
    // x 4..=8, y 2..=4.
    for y in 2..=4 {
        for x in 4..=8 {
            assert_eq!(pixel(x, y), (255, 255), "кольцо ({x}, {y})");
        }
    }
    // Вне кольца и по углам — прозрачный чёрный.
    assert_eq!(pixel(0, 0), (0, 0));
    assert_eq!(pixel(3, 2), (0, 0));
    assert_eq!(pixel(9, 4), (0, 0));
    assert_eq!(pixel(12, 6), (0, 0));
}

/// Радиус дилатации компенсирует плотность пикселей текстуры: контур
/// одной мировой толщины у человека (256 текселей на 16 мировых) и волка
/// (128 текселей на 32 мировых), минимум — один тексель.
#[test]
fn outline_radius_matches_texel_density() {
    use crate::rendering::{OUTLINE_THICKNESS_PX, outline_radius_texels};

    let human = outline_radius_texels(256, 16.0);
    let wolf = outline_radius_texels(128, 32.0);
    assert_eq!(human, (OUTLINE_THICKNESS_PX * 16.0).round() as u32);
    assert_eq!(wolf, (OUTLINE_THICKNESS_PX * 4.0).round() as u32);
    assert!(human > wolf, "более плотная текстура — больший радиус");
    assert_eq!(outline_radius_texels(1, 1000.0), 1, "минимум один тексель");
}
