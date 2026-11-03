//! Общие помощники ввода: луч курсора через активную камеру и пикинг
//! существ по альфа-границам спрайтов. Используются и режимом игрока,
//! и редактором, а также рендером (голограмма) и UI (меню строительства,
//! окна станций).

use bevy::camera::Camera2d;
use bevy::prelude::*;

use crate::rendering::isometric::{
    WORLD_TILE_SIZE, ground_position_from_viewport, terrain_tile_from_viewport,
};
use crate::rendering::{EntitySpriteVisuals, TileTextures, entity_sprite_size};
use crate::simulation::{TilePos, World, TILE_SIZE};

/// Радиус выделения существа курсором, тайлов (задача «инспектор»):
/// выбирается ближайшая к курсору сущность не дальше этого радиуса.
pub(crate) const ENTITY_PICK_RADIUS_TILES: f32 = 2.0;

/// Позиция курсора в единицах симуляции (`TILE_SIZE` px на тайл, не тайлы:
/// для перевода в тайловые координаты делить на `TILE_SIZE`).
/// `None` — луч камеры не пересёк рельеф мира.
pub(crate) fn cursor_world_pos(
    window: &Window,
    cameras: &Query<(&Camera, &Transform), With<Camera2d>>,
    world: &World,
) -> Option<Vec2> {
    let cursor = window.cursor_position()?;
    let Ok((camera, transform)) = cameras.single() else {
        return None;
    };
    let point = ground_position_from_viewport(
        camera, &GlobalTransform::from(*transform), cursor, world,
    )?;
    Some(point.truncate() / WORLD_TILE_SIZE * TILE_SIZE)
}

/// Тайл мира под курсором окна. `None` — курсор вне окна или за границами
/// мира (камера может смотреть и мимо карты). Публичная: используется
/// рендером (голограмма постройки) и меню строительства.
pub(crate) fn cursor_world_tile(
    window: &Window,
    cameras: &Query<(&Camera, &Transform), With<Camera2d>>,
    world: Option<&World>,
) -> Option<TilePos> {
    let world = world?;
    let cursor = window.cursor_position()?;
    let Ok((camera, transform)) = cameras.single() else {
        return None;
    };
    let tile = terrain_tile_from_viewport(
        camera, &GlobalTransform::from(*transform), cursor, world,
    )?;
    let size = world.size.tiles() as i32;
    (tile.x >= 0 && tile.y >= 0 && tile.x < size && tile.y < size).then_some(tile)
}

/// Выбирает существо по прямоугольнику экранного спрайта: луч пересекает
/// поверхность земли, а не сам спрайт, поднятый над тайлом. Границы рисунка
/// берутся из альфа-канала текстуры (`EntitySpriteVisuals`) — пикинг
/// попадает ровно в тот рисунок, что рисуется, без ручных констант.
/// Берётся ближайшая к курсору сущность в радиусе `ENTITY_PICK_RADIUS_TILES`
/// (общая логика для захвата по F1, инспектора и подсветки).
pub(crate) fn entity_at(
    world: &World,
    window: &Window,
    cameras: &Query<(&Camera, &Transform), With<Camera2d>>,
    textures: &TileTextures,
    visuals: &EntitySpriteVisuals,
) -> Option<u64> {
    let cursor = window.cursor_position()?;
    let Ok((camera, camera_transform)) = cameras.single() else {
        return None;
    };
    // Курсор в тайловых координатах — для ограничения радиуса выбора.
    // `cursor_world_pos` даёт пиксели симуляции (TILE_SIZE на тайл),
    // а `entity.pos` хранится в тайлах: без деления радиус сравнивал
    // единицы разных масштабов и не пропускал ни одно существо.
    let cursor_px = cursor_world_pos(window, cameras, world)?;
    let cursor_tiles = crate::simulation::world::WorldPos {
        x: cursor_px.x / TILE_SIZE,
        y: cursor_px.y / TILE_SIZE,
    };
    let camera_global = GlobalTransform::from(*camera_transform);
    let camera_up = camera_transform.rotation * Vec3::Y;
    let camera_right = camera_transform.rotation * Vec3::X;
    let mut nearest: Option<(u64, f32)> = None;

    for entity in world.entities.iter() {
        // Радиус выделения: только существа рядом с курсором.
        if entity.pos.dist(cursor_tiles) > ENTITY_PICK_RADIUS_TILES {
            continue;
        }
        // Границы ещё не отсканированы (текстура не догрузилась) — существо
        // пропускается до ближайшего кадра: спрайт к этому моменту тоже
        // ещё не отрисован.
        let Some(bounds) = visuals.bounds(textures.entity_texture(entity).id()) else {
            continue;
        };
        let size = entity_sprite_size(entity.kind);
        let half_width = size.x * (bounds.right - bounds.left) * 0.5;
        let half_height = size.y * (bounds.bottom - bounds.top) * 0.5;
        let ground = crate::rendering::isometric::world_position(world, Vec2::new(entity.pos.x, entity.pos.y));
        let alpha_center_x = (bounds.left + bounds.right) * 0.5;
        let alpha_center_y = (bounds.top + bounds.bottom) * 0.5;
        let flip = if entity.facing == crate::simulation::Facing::Left {
            -1.0
        } else {
            1.0
        };
        // Центр рисунка относительно земли: тот же вывод, что у трансформа
        // спрайта с bottom_offset = 1 - bounds.bottom (см. entity_transform).
        let center = ground
            + camera_right * (flip * size.x * (alpha_center_x - 0.5))
            + camera_up * (size.y * (bounds.bottom - alpha_center_y));
        let Ok(center_screen) = camera.world_to_viewport(&camera_global, center) else {
            continue;
        };
        let Ok(right_screen) =
            camera.world_to_viewport(&camera_global, center + camera_right * half_width)
        else {
            continue;
        };
        let Ok(up_screen) =
            camera.world_to_viewport(&camera_global, center + camera_up * half_height)
        else {
            continue;
        };
        let screen_half_width = (right_screen - center_screen).length();
        let screen_half_height = (up_screen - center_screen).length();
        let delta = cursor - center_screen;
        if delta.x.abs() > screen_half_width || delta.y.abs() > screen_half_height {
            continue;
        }
        let distance = delta.length_squared();
        if nearest.is_none_or(|(_, best)| distance < best) {
            nearest = Some((entity.id, distance));
        }
    }

    nearest.map(|(id, _)| id)
}
