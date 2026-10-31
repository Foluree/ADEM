//! Фиксированная изометрическая 3D-камера: зум, панорамирование и следование за игроком.

use bevy::camera::{Camera2d, Camera3d, OrthographicProjection, Projection};
use bevy::input::mouse::{MouseButton, MouseMotion, MouseWheel};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::TextInput;

use crate::app::input::move_dir;
use crate::app::settings::Settings;
use crate::app::state::{GameMode, GameState, PlayerRes, SimTick, TickAccumulator, player_render_pos};
use crate::rendering::isometric::{WORLD_TILE_SIZE, world_position};
use crate::ui::pause_menu::BindingCapture;
use crate::ui::text_input_focused;

const ZOOM_MIN: f32 = 0.08;
const ZOOM_MAX: f32 = 150.0;
const ZOOM_STEP: f32 = 0.12;
const DEFAULT_SCALE: f32 = 2.0;
const CAMERA_YAW: f32 = 5.0 * std::f32::consts::FRAC_PI_4;
const CAMERA_PITCH: f32 = std::f32::consts::FRAC_PI_6;
const CAMERA_SPEED_SCREEN_FRACTION: f32 = 0.5;
const CAMERA_HEIGHT_SPRING_OMEGA: f32 = 20.0;

/// Точка, на которую направлены камеры; ориентация камеры фиксирована.
#[derive(Resource)]
pub(crate) struct CameraTarget {
    pub target: Vec3,
    height_velocity: f32,
    following_player: bool,
}

impl CameraTarget {
    fn new(world_size: u32) -> Self {
        let center = world_size as f32 * WORLD_TILE_SIZE * 0.5;
        Self {
            target: Vec3::new(center, center, 0.0),
            height_velocity: 0.0,
            following_player: false,
        }
    }

    fn camera_transform(&self, distance: f32) -> Transform {
        let horizontal = CAMERA_PITCH.cos();
        let offset = Vec3::new(
            CAMERA_YAW.cos() * horizontal,
            CAMERA_YAW.sin() * horizontal,
            CAMERA_PITCH.sin(),
        ) * distance;
        Transform::from_translation(self.target + offset).looking_at(self.target, Vec3::Z)
    }
}

#[derive(Component)]
pub(crate) struct WorldCamera;

#[derive(Component)]
pub(crate) struct OverlayCamera;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CameraTarget::new(256))
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (
                    zoom_system,
                    editor_move_system,
                    pan_system,
                    follow_player,
                    update_camera_transforms,
                )
                    .chain()
                    .after(SimTick),
            );
    }
}

fn modal_menu_open(state: &GameState) -> bool {
    state.active_menu.is_some()
}

fn spawn_camera(
    mut commands: Commands,
    state: Res<GameState>,
    mut camera_target: ResMut<CameraTarget>,
) {
    *camera_target = CameraTarget::new(state.world_size.tiles());
    let distance = state.world_size.tiles() as f32 * WORLD_TILE_SIZE * 2.0;
    let transform = camera_target.camera_transform(distance);
    let far = distance * 3.0;
    let projection = OrthographicProjection {
        scale: DEFAULT_SCALE, near: 0.1, far,
        ..OrthographicProjection::default_3d()
    };
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            ..default()
        },
        Projection::Orthographic(projection.clone()),
        transform,
        WorldCamera,
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Orthographic(projection),
        transform,
        OverlayCamera,
    ));
}

fn zoom_system(
    mut query: Query<&mut Projection, With<WorldCamera>>,
    mut wheel: MessageReader<MouseWheel>,
    state: Res<GameState>,
) {
    if modal_menu_open(&state) {
        return;
    }
    let delta: f32 = wheel.read().map(|event| event.y).sum();
    if delta.abs() < f32::EPSILON {
        return;
    }
    for mut projection in &mut query {
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scale = (ortho.scale * (1.0 - delta * ZOOM_STEP)).clamp(ZOOM_MIN, ZOOM_MAX);
        }
    }
}

fn editor_move_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<Settings>,
    state: Res<GameState>,
    capture: Res<BindingCapture>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
    cameras: Query<(&Camera, &Transform, &Projection), With<WorldCamera>>,
    mut camera_target: ResMut<CameraTarget>,
) {
    if state.mode != GameMode::Observer || modal_menu_open(&state) {
        return;
    }
    if capture.0.is_some() || text_input_focused(&focus, &text_inputs) {
        return;
    }
    let (dx, dy) = move_dir(&keys, &settings);
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    let Ok((camera, transform, projection)) = cameras.single() else {
        return;
    };
    let (right, up) = camera_ground_axes(transform);
    let scale = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => DEFAULT_SCALE,
    };
    let viewport_height = camera
        .logical_viewport_size()
        .map(|size| size.y * scale)
        .unwrap_or(0.0);
    if viewport_height <= 0.0 {
        return;
    }
    camera_target.target +=
        (right * dx + up * dy) * (CAMERA_SPEED_SCREEN_FRACTION * viewport_height * time.delta_secs());
}

fn pan_system(
    mut motion: MessageReader<MouseMotion>,
    buttons: Res<ButtonInput<MouseButton>>,
    state: Res<GameState>,
    cameras: Query<(&Transform, &Projection), With<WorldCamera>>,
    mut camera_target: ResMut<CameraTarget>,
) {
    if modal_menu_open(&state) || state.mode == GameMode::Player || !buttons.pressed(MouseButton::Right)
    {
        for _ in motion.read() {}
        return;
    }
    let delta: Vec2 = motion.read().map(|event| event.delta).sum();
    if delta == Vec2::ZERO {
        return;
    }
    let Ok((transform, projection)) = cameras.single() else {
        return;
    };
    let scale = match projection {
        Projection::Orthographic(ortho) => ortho.scale,
        _ => 1.0,
    };
    let (right, up) = camera_ground_axes(transform);
    camera_target.target += right * (-delta.x * scale) + up * (delta.y * scale);
}

fn follow_player(
    state: Res<GameState>,
    player: Res<PlayerRes>,
    acc: Res<TickAccumulator>,
    time: Res<Time>,
    mut camera_target: ResMut<CameraTarget>,
) {
    if state.mode != GameMode::Player {
        camera_target.following_player = false;
        camera_target.height_velocity = 0.0;
        return;
    }
    let Some(world) = state.world.as_ref() else {
        camera_target.following_player = false;
        camera_target.height_velocity = 0.0;
        return;
    };
    let Some(player) = player.0.as_ref() else {
        camera_target.following_player = false;
        camera_target.height_velocity = 0.0;
        return;
    };
    let player_position = world_position(world, {
        let pos = player_render_pos(player, &acc);
        Vec2::new(pos.x, pos.y)
    });
    if !camera_target.following_player {
        camera_target.target = player_position;
        camera_target.height_velocity = 0.0;
        camera_target.following_player = true;
        return;
    }

    camera_target.target.x = player_position.x;
    camera_target.target.y = player_position.y;
    (camera_target.target.z, camera_target.height_velocity) = critically_damped_step(
        camera_target.target.z,
        camera_target.height_velocity,
        player_position.z,
        time.delta_secs(),
        CAMERA_HEIGHT_SPRING_OMEGA,
    );
}

fn critically_damped_step(
    position: f32,
    velocity: f32,
    target: f32,
    delta_secs: f32,
    omega: f32,
) -> (f32, f32) {
    let delta_secs = delta_secs.max(0.0);
    let offset = position - target;
    let decay = (-omega * delta_secs).exp();
    let c = velocity + omega * offset;
    let next_offset = (offset + c * delta_secs) * decay;
    let next_velocity = (velocity - omega * c * delta_secs) * decay;
    let next_position = target + next_offset;
    let next_position = if target >= position {
        next_position.clamp(position, target)
    } else {
        next_position.clamp(target, position)
    };
    let next_velocity = if next_position == target || next_position == position {
        0.0
    } else {
        next_velocity
    };
    (next_position, next_velocity)
}

pub(crate) fn update_camera_transforms(
    state: Res<GameState>,
    camera_target: Res<CameraTarget>,
    mut world_camera: Query<
        (&mut Transform, &Projection),
        (With<WorldCamera>, Without<OverlayCamera>),
    >,
    mut overlay_camera: Query<
        (&mut Transform, &mut Projection),
        (With<OverlayCamera>, Without<WorldCamera>),
    >,
) {
    let distance = state.world_size.tiles() as f32 * WORLD_TILE_SIZE * 2.0;
    let transform = camera_target.camera_transform(distance);
    let Ok((mut world_transform, world_projection)) = world_camera.single_mut() else {
        return;
    };
    *world_transform = transform;
    let Projection::Orthographic(_) = world_projection else {
        return;
    };
    let Ok((mut overlay_transform, mut overlay_projection)) = overlay_camera.single_mut() else {
        return;
    };
    *overlay_transform = transform;
    *overlay_projection = world_projection.clone();
}

fn camera_ground_axes(transform: &Transform) -> (Vec3, Vec3) {
    let right = (transform.rotation * Vec3::X).with_z(0.0).normalize_or_zero();
    let up = (transform.rotation * Vec3::Y).with_z(0.0).normalize_or_zero();
    (right, up)
}

#[cfg(test)]
#[path = "../tests/camera.rs"]
mod tests;
