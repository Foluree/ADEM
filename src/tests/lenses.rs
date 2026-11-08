use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Mesh, PrimitiveTopology, VertexAttributeValues};
use std::collections::HashMap;

use crate::rendering::isometric::WORLD_TILE_SIZE;
use crate::rendering::lenses::lens_color;
use crate::rendering::terrain::{build_block_mesh, set_block_lens_colors, BlockPos};
use crate::simulation::{ChunkPos, TilePos, TileType, World, WorldSize};

#[test]
fn lens_categories_combine_in_the_terrain_tint() {
    let combined = lens_color(true, true);
    let sentient_only = lens_color(true, false);
    let building_only = lens_color(false, true);

    for channel in 0..3 {
        assert!(
            combined[channel] >= sentient_only[channel].min(building_only[channel])
                && combined[channel] <= sentient_only[channel].max(building_only[channel])
        );
    }
    assert_eq!(combined[3], 1.0);
}

#[test]
fn terrain_tints_follow_chunk_boundaries_without_changing_geometry() {
    let boundary_x = 16.0 * WORLD_TILE_SIZE;
    let positions = vec![
        [15.0 * WORLD_TILE_SIZE, WORLD_TILE_SIZE, 0.0],
        [boundary_x, WORLD_TILE_SIZE, 0.0],
        [boundary_x, 2.0 * WORLD_TILE_SIZE, 0.0],
        [15.0 * WORLD_TILE_SIZE, 2.0 * WORLD_TILE_SIZE, 0.0],
        [boundary_x, WORLD_TILE_SIZE, 10.0],
        [17.0 * WORLD_TILE_SIZE, WORLD_TILE_SIZE, 10.0],
        [17.0 * WORLD_TILE_SIZE, 2.0 * WORLD_TILE_SIZE, 10.0],
        [boundary_x, 2.0 * WORLD_TILE_SIZE, 10.0],
    ];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
    mesh.insert_indices(bevy::mesh::Indices::U32(vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]));
    let tints = HashMap::from([(ChunkPos { x: 0, y: 0 }, [0.4, 0.6, 0.8, 1.0])]);

    set_block_lens_colors(&mut mesh, BlockPos { x: 0, y: 0 }, &tints);

    let actual_positions = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|attribute| attribute.as_float3())
        .expect("terrain positions remain float3");
    assert_eq!(actual_positions, positions);
    let Some(VertexAttributeValues::Float32x4(colors)) =
        mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("terrain colors are float4");
    };
    assert_eq!(colors.len(), 8);
    assert!(colors[..4].iter().all(|color| *color == [0.4, 0.6, 0.8, 1.0]));
    assert!(colors[4..].iter().all(|color| *color == [1.0; 4]));
}

#[test]
fn terrain_mesh_keeps_vertex_attributes_in_main_world_for_lens_updates() {
    let mut world = World::new(WorldSize::new(256), 1);
    for y in 0..16 {
        for x in 0..16 {
            world.set_terrain(TilePos { x, y }, TileType::Grass);
        }
    }
    let mesh = build_block_mesh(&world, BlockPos { x: 0, y: 0 }, 4)
        .expect("configured world has terrain");

    assert!(
        mesh.asset_usage
            .contains(bevy::asset::RenderAssetUsages::MAIN_WORLD)
    );
    assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
}
