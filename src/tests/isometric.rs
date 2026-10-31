use super::*;
use crate::simulation::{TileType, WorldSize};

#[test]
fn terrain_ray_hits_the_tile_surface_height() {
    let mut world = World::new(WorldSize::new(256), 1);
    world.set_terrain(TilePos { x: 2, y: 3 }, TileType::Mountain);
    let point = intersect_terrain(
        Vec3::new(2.5 * WORLD_TILE_SIZE, 3.5 * WORLD_TILE_SIZE, 100.0),
        Vec3::new(0.0, 0.0, -1.0),
        &world,
    )
    .expect("ray should hit the mountain top");

    assert!((point.point.z - tile_elevation(TileType::Mountain)).abs() < 1e-4);
    assert_eq!(point.tile, TilePos { x: 2, y: 3 });
}

#[test]
fn terrain_ray_outside_world_uses_the_editable_base_plane() {
    let world = World::new(WorldSize::new(256), 1);
    let point = intersect_terrain(
        Vec3::new(-WORLD_TILE_SIZE, WORLD_TILE_SIZE, 10.0),
        Vec3::new(0.0, 0.0, -1.0),
        &world,
    )
    .expect("ray should hit the base plane outside the map");

    assert_eq!(point.point.z, 0.0);
}

#[test]
fn terrain_ray_hits_a_visible_mountain_side() {
    let mut world = World::new(WorldSize::new(256), 1);
    world.set_terrain(TilePos { x: 3, y: 2 }, TileType::Mountain);
    let point = intersect_terrain(
        Vec3::new(
            2.5 * WORLD_TILE_SIZE,
            2.5 * WORLD_TILE_SIZE,
            tile_elevation(TileType::Mountain) * 1.25,
        ),
        Vec3::new(0.866_025_4, 0.0, -0.5),
        &world,
    )
    .expect("ray should hit the mountain side before the lower tile");

    assert_eq!(point.tile, TilePos { x: 3, y: 2 });
    assert!((point.point.z > 0.0) && (point.point.z < tile_elevation(TileType::Mountain)));
}

#[test]
fn terrain_ray_hits_the_other_visible_mountain_side() {
    let mut world = World::new(WorldSize::new(256), 1);
    world.set_terrain(TilePos { x: 2, y: 3 }, TileType::Mountain);
    let point = intersect_terrain(
        Vec3::new(
            2.5 * WORLD_TILE_SIZE,
            2.5 * WORLD_TILE_SIZE,
            tile_elevation(TileType::Mountain) * 1.25,
        ),
        Vec3::new(0.0, 0.866_025_4, -0.5),
        &world,
    )
    .expect("ray should hit the mountain's other visible side");

    assert_eq!(point.tile, TilePos { x: 2, y: 3 });
    assert!((point.point.z > 0.0) && (point.point.z < tile_elevation(TileType::Mountain)));
}

#[test]
fn clay_has_the_same_elevation_as_dirt() {
    assert_eq!(tile_elevation(TileType::Clay), tile_elevation(TileType::Dirt));
}
