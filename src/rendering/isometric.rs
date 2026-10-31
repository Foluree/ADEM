use bevy::camera::Camera;
use bevy::math::{Vec2, Vec3};
use bevy::prelude::GlobalTransform;

use crate::simulation::{TilePos, TileType, World, TILE_SIZE};

/// Grid spacing is tuned so a 30°-above-horizon, 45° azimuth camera
/// reproduces the previous 2:1 isometric diamond.
pub(crate) const WORLD_TILE_SIZE: f32 = TILE_SIZE * std::f32::consts::SQRT_2;
const HEIGHT_STEP: f32 = TILE_SIZE * 0.125 / 0.866_025_4;

pub(crate) fn tile_elevation(kind: TileType) -> f32 {
    let levels = match kind {
        TileType::Empty
        | TileType::Water
        | TileType::DeepWater
        | TileType::Ocean
        | TileType::Sand => 0.0,
        TileType::Dirt | TileType::Grass | TileType::Farmland | TileType::Clay => 1.0,
        TileType::ForestGrass => 2.0,
        TileType::Stone => 3.0,
        TileType::Mountain => 8.0,
    };
    levels * HEIGHT_STEP
}

pub(crate) fn terrain_elevation(world: &World, tile: TilePos) -> f32 {
    let size = world.size.tiles() as i32;
    if tile.x < 0 || tile.y < 0 || tile.x >= size || tile.y >= size {
        return 0.0;
    }
    tile_elevation(world.terrain_kind(tile))
}

pub(crate) fn world_position(world: &World, grid: Vec2) -> Vec3 {
    let tile = TilePos {
        x: grid.x.floor() as i32,
        y: grid.y.floor() as i32,
    };
    Vec3::new(
        grid.x * WORLD_TILE_SIZE,
        grid.y * WORLD_TILE_SIZE,
        terrain_elevation(world, tile),
    )
}

pub(crate) fn ground_position_from_viewport(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    viewport: Vec2,
    world: &World,
) -> Option<Vec3> {
    let ray = camera.viewport_to_world(camera_transform, viewport).ok()?;
    Some(intersect_terrain(ray.origin, ray.direction.as_vec3(), world)?.point)
}

pub(crate) fn terrain_tile_from_viewport(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    viewport: Vec2,
    world: &World,
) -> Option<TilePos> {
    let ray = camera.viewport_to_world(camera_transform, viewport).ok()?;
    Some(intersect_terrain(ray.origin, ray.direction.as_vec3(), world)?.tile)
}

#[derive(Clone, Copy)]
struct TerrainHit {
    point: Vec3,
    tile: TilePos,
}

fn intersect_terrain(origin: Vec3, direction: Vec3, world: &World) -> Option<TerrainHit> {
    if direction.z.abs() < f32::EPSILON {
        return None;
    }
    let mut nearest: Option<(f32, Vec3, TilePos)> = None;
    for level in [0.0, 1.0, 2.0, 3.0, 8.0] {
        let height = level * HEIGHT_STEP;
        let distance = (height - origin.z) / direction.z;
        if distance < 0.0 {
            continue;
        }
        let point = origin + direction * distance;
        let tile = TilePos {
            x: (point.x / WORLD_TILE_SIZE).floor() as i32,
            y: (point.y / WORLD_TILE_SIZE).floor() as i32,
        };
        if (terrain_elevation(world, tile) - height).abs() <= f32::EPSILON
            && nearest.is_none_or(|(best_distance, _, _)| distance < best_distance)
        {
            nearest = Some((distance, point, tile));
        }
    }
    let (top_distance, top_point, top_tile) = nearest?;

    // Проверяем боковые грани рядом с первым попаданием в горизонтальную
    // поверхность. При фиксированном наклоне камеры высота горы ограничивает
    // этот участок несколькими тайлами, поэтому не требуется обходить мир.
    let side_reach = (tile_elevation(TileType::Mountain) * direction.truncate().length()
        / direction.z.abs() / WORLD_TILE_SIZE).ceil() as i32 + 1;
    let center = TilePos {
        x: (top_point.x / WORLD_TILE_SIZE).floor() as i32,
        y: (top_point.y / WORLD_TILE_SIZE).floor() as i32,
    };
    let size = world.size.tiles() as i32;
    let mut best = (top_distance, top_point, top_tile);
    let epsilon = 1e-4;

    for y in center.y - side_reach..=center.y + side_reach {
        for x in center.x - side_reach..=center.x + side_reach {
            if x < 0 || y < 0 || x >= size || y >= size {
                continue;
            }
            let tile = TilePos { x, y };
            let elevation = terrain_elevation(world, tile);
            if elevation <= 0.0 {
                continue;
            }
            if x > 0 {
                let lower = terrain_elevation(world, TilePos { x: x - 1, y });
                if elevation > lower && direction.x.abs() >= f32::EPSILON {
                    let distance = (x as f32 * WORLD_TILE_SIZE - origin.x) / direction.x;
                    if distance >= 0.0 && distance < best.0 {
                        let point = origin + direction * distance;
                        let y0 = y as f32 * WORLD_TILE_SIZE;
                        let y1 = y0 + WORLD_TILE_SIZE;
                        if point.y >= y0 - epsilon
                            && point.y <= y1 + epsilon
                            && point.z >= lower - epsilon
                            && point.z <= elevation + epsilon
                        {
                            best = (distance, point, tile);
                        }
                    }
                }
            }
            if y > 0 {
                let lower = terrain_elevation(world, TilePos { x, y: y - 1 });
                if elevation > lower && direction.y.abs() >= f32::EPSILON {
                    let distance = (y as f32 * WORLD_TILE_SIZE - origin.y) / direction.y;
                    if distance >= 0.0 && distance < best.0 {
                        let point = origin + direction * distance;
                        let x0 = x as f32 * WORLD_TILE_SIZE;
                        let x1 = x0 + WORLD_TILE_SIZE;
                        if point.x >= x0 - epsilon
                            && point.x <= x1 + epsilon
                            && point.z >= lower - epsilon
                            && point.z <= elevation + epsilon
                        {
                            best = (distance, point, tile);
                        }
                    }
                }
            }
        }
    }
    Some(TerrainHit {
        point: best.1,
        tile: best.2,
    })
}



#[cfg(test)]
#[path = "../tests/isometric.rs"]
mod tests;
