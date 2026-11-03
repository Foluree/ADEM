//! Голограмма строительства: полупрозрачное предсказание будущей постройки.
//! Геометрия — та же, что у реальной постройки в `sprites.rs` (крест костра,
//! изометрический блок станции, декаль склада), но из квадов с тинтом:
//! зелёная — поставить можно, красная — нельзя. Пересчитывается только при
//! изменении ввода (курсор, камера).

use bevy::prelude::*;

use crate::app::input::cursor_world_tile;
use crate::app::state::{GameMode, GameState};
use crate::simulation::actions::reachable;
use crate::simulation::commands::buildable_at;
use crate::simulation::objects::build_cost;
use crate::simulation::{BuildingKind, TILE_SIZE};
use crate::ui::construction::ConstructionUi;

use super::isometric::{WORLD_TILE_SIZE, world_position};
use super::sprites::{DECAL_Z_OFFSET, STATION_BLOCK_HEIGHT, STATION_BLOCK_SCALE, world_sprite_quad};
use super::textures::TileTextures;

/// Маркер голограммы выбранной постройки.
#[derive(Component)]
pub(crate) struct BuildHologram;

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
pub(crate) fn update_build_hologram(
    mut commands: Commands,
    state: Res<GameState>,
    construction: Res<ConstructionUi>,
    textures: Res<TileTextures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sprite_assets: ResMut<super::sprites::WorldSpriteAssets>,
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
        && reachable(world, actor, tile);

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
    sprite_assets: &mut super::sprites::WorldSpriteAssets,
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
    sprite_assets: &mut super::sprites::WorldSpriteAssets,
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
