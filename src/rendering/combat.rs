//! Рендер боевой системы: эффекты оружия, снаряды в полёте, воткнутые копья.
//!
//! Бой считается в симуляции (`simulation/combat.rs`); здесь только
//! визуализация. Эффекты оружия — косметика по сообщению `WeaponFx`
//! (дуга лезвия и выпад уходят и на промахе). Снаряды и воткнутые копья
//! рисуются только для видимой области — как существа.

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::camera::{Camera, Camera2d, Projection};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite_render::{AlphaMode2d, ColorMaterial};

use crate::app::input::{WeaponFx, WeaponFxKind};
use crate::app::camera::update_camera_transforms;
use crate::app::state::{GameMode, GameState, PlayerRes, SimTick, TickAccumulator, player_render_pos};
use crate::rendering::isometric::{WORLD_TILE_SIZE, world_position};
use crate::simulation::time::TICKS_PER_SEC;
use crate::simulation::{ItemKind, ProjectileKind, StuckSpear, TILE_SIZE};

use super::textures::TileTextures;

// ---------- Эффекты оружия ----------

/// Длительность анимации взмаха лезвия.
const BLADE_DURATION_SECS: f32 = 0.15;
/// Угол, который проходит след за анимацию (радианы, 90°).
const BLADE_SWEEP: f32 = std::f32::consts::FRAC_PI_2;
/// Угловая ширина сектора следа: втрое уже базовой (45°).
const BLADE_SECTOR: f32 = std::f32::consts::PI / 4.0;
/// Радиусы сектора в мировых пикселях: от игрока наружу.
const SWING_INNER_PX: f32 = TILE_SIZE * 0.6;
const SWING_OUTER_PX: f32 = TILE_SIZE * 0.9;
/// Длительность выпада копья: вперёд и обратно.
const THRUST_DURATION_SECS: f32 = 0.25;
/// Насколько далеко копьё выносится вперёд от центра игрока, мировых px.
const THRUST_REACH_PX: f32 = TILE_SIZE * 1.2;
/// Сколько тиков держится текстура лука после выстрела: ровно кулдаун
/// выстрела (15 тиков = 0.75 с при x1), чтобы анимация не пережидала
/// готовность следующего выстрела.
const BOW_SHOT_FX_TICKS: f32 = 15.0;
/// Дополнительный поворот лука относительно направления выстрела:
/// +90° по часовой стрелке (в математической конвенции — минус 90°).
const BOW_EXTRA_ROTATION: f32 = -std::f32::consts::FRAC_PI_2;
/// Размер текстуры копья в анимации выпада, тайлов (вдвое меньше прежнего
/// показа — решение автора).
const THRUST_SPRITE_TILES: f32 = 0.8;
/// Размер текстуры брошенного копья в полёте, тайлов (в ~1.5 раза меньше
/// прежнего показа 1.4).
const THROWN_SPEAR_SPRITE_TILES: f32 = 0.9;
/// Размер текстуры лука относительно тайла (вдвое меньше прежнего показа).
const BOW_SPRITE_TILES: f32 = 0.5;

/// Подъём оружия «в руках» (копьё, лук) над центром игрока, тайлов:
/// у центра спрайт рисуется в ногах.
const WEAPON_HAND_LIFT: f32 = 5.0;
/// Насколько эффект оружия сдвинут к камере, чтобы перекрыть спрайт игрока.
/// Цвет следа: серо-белый, полупрозрачный (альфа 0.5 в начале взмаха).
const SWING_COLOR: Color = Color::srgba(0.85, 0.85, 0.9, 0.5);

/// Эффект оружия: живёт по своему таймеру, форма анимации зависит от вида.
#[derive(Component)]
struct WeaponFxEffect {
    age: f32,
    duration: f32,
    kind: WeaponFxKind,
    /// Точка, откуда эффект (центр игрока при применении).
    origin: Vec3,
    /// Направление на прицел (радианы).
    aim_angle: f32,
    /// Для лезвия: угол в начале анимации (прицел минус полдуги).
    start_angle: f32,
}

/// Общие ресурсы эффектов: меш сектора строится один раз.
#[derive(Resource, Default)]
struct SwingFx {
    mesh: Option<Handle<Mesh>>,
}

/// Меш сектора кольца, направленного вдоль +X (анимация поворачивает
/// сущность); вершины — в мировых пикселях относительно центра игрока.
fn build_swing_mesh() -> Mesh {
    const SEGMENTS: usize = 4;
    let half = BLADE_SECTOR * 0.5;
    let mut positions = Vec::with_capacity((SEGMENTS + 1) * 2);
    let mut indices = Vec::with_capacity(SEGMENTS * 6);
    for i in 0..=SEGMENTS {
        let angle = -half + BLADE_SECTOR * i as f32 / SEGMENTS as f32;
        let (sin, cos) = angle.sin_cos();
        positions.push([cos * SWING_INNER_PX, sin * SWING_INNER_PX, 0.0]);
        positions.push([cos * SWING_OUTER_PX, sin * SWING_OUTER_PX, 0.0]);
        if i < SEGMENTS {
            let base = (i * 2) as u32;
            indices.extend([base, base + 1, base + 3, base, base + 3, base + 2]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Спавнит эффект по сообщению применения оружия.
fn spawn_weapon_fx(
    mut messages: MessageReader<WeaponFx>,
    state: Res<GameState>,
    mut fx: ResMut<SwingFx>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    textures: Res<TileTextures>,
    cameras: Query<&Transform, With<Camera2d>>,
    overlay: Query<(Entity, &WeaponFxEffect)>,
    mut commands: Commands,
) {
    let mesh = fx
        .mesh
        .get_or_insert_with(|| meshes.add(build_swing_mesh()))
        .clone();
    let Ok(camera_transform) = cameras.single() else {
        return;
    };
    let camera_rotation = camera_transform.rotation;
    for fx_msg in messages.read() {
        let Some(world) = state.world.as_ref() else {
            continue;
        };
        let origin = world_position(world, fx_msg.origin / TILE_SIZE);
        let aim = world_position(world, fx_msg.aim / TILE_SIZE);
        let aim_local = camera_rotation.inverse() * (aim - origin);
        let aim_angle = aim_local.y.atan2(aim_local.x);
        // Предмет поверх игрока (копьё, лук) показывается одним эффектом:
        // новая анимация прерывает предыдущую, иначе при быстрых кликах
        // текстуры плодились бы поверх игрока.
        if matches!(fx_msg.kind, WeaponFxKind::Thrust | WeaponFxKind::Bow) {
            for (entity, effect) in &overlay {
                if matches!(effect.kind, WeaponFxKind::Thrust | WeaponFxKind::Bow) {
                    commands.entity(entity).despawn();
                }
            }
        }
        match fx_msg.kind {
            WeaponFxKind::Blade => {
                let start_angle = aim_angle - BLADE_SWEEP * 0.5;
                let material = materials.add(ColorMaterial {
                    color: SWING_COLOR,
                    alpha_mode: AlphaMode2d::Blend,
                    ..default()
                });
                commands.spawn((
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_translation(origin + camera_rotation * Vec3::Z * FX_Z_LIFT)
                        .with_rotation(camera_rotation * Quat::from_rotation_z(start_angle)),
                    WeaponFxEffect {
                        age: 0.0,
                        duration: BLADE_DURATION_SECS,
                        kind: WeaponFxKind::Blade,
                        origin,
                        aim_angle,
                        start_angle,
                    },
                ));
            }
            WeaponFxKind::Thrust | WeaponFxKind::Bow => {
                // Текстура оружия от центра игрока, повёрнутая к прицелу;
                // выпад двигается в анимации, лук стоит на месте (следует
                // за игроком). У лука — дополнительный поворот на +90° по
                // часовой и вдвое меньший размер.
                let (kind_texture, size, extra_rotation) = match fx_msg.kind {
                    WeaponFxKind::Thrust => (
                        textures.item_texture(ItemKind::Spear),
                        THRUST_SPRITE_TILES,
                        0.0,
                    ),
                    _ => (
                        textures.item_texture(ItemKind::Bow),
                        BOW_SPRITE_TILES,
                        BOW_EXTRA_ROTATION,
                    ),
                };
                let rotation = aim_angle - SPRITE_ANGLE_OFFSET + extra_rotation;
                // Оружие хранит поднятый origin: все дальнейшие кадры
                // анимации (выпад, удержание лука) идут от него же.
                let weapon_origin =
                    origin + camera_rotation * (Vec3::Z * FX_Z_LIFT + Vec3::Y * WEAPON_HAND_LIFT);
                commands.spawn((
                    Sprite {
                        image: kind_texture.clone(),
                        custom_size: Some(Vec2::splat(TILE_SIZE * size)),
                        ..default()
                    },
                    Transform::from_translation(weapon_origin)
                        .with_rotation(camera_rotation * Quat::from_rotation_z(rotation)),
                    WeaponFxEffect {
                        age: 0.0,
                        duration: match fx_msg.kind {
                            WeaponFxKind::Thrust => THRUST_DURATION_SECS,
                            _ => BOW_SHOT_FX_TICKS / TICKS_PER_SEC as f32,
                        },
                        kind: fx_msg.kind,
                        origin: weapon_origin,
                        aim_angle,
                        start_angle: aim_angle,
                    },
                ));
            }
        }
    }
}

/// Анимирует и убирает отжившие эффекты: лезвие — поворот по дуге
/// и растворение; выпад — вынос копья вперёд (по синусоиде, плавно
/// вперёд и назад) с сохранением поворота к прицелу; лук — статичная
/// текстура, привязанная к текущей позиции игрока.
fn update_weapon_fx(
    time: Res<Time>,
    state: Res<GameState>,
    player: Res<PlayerRes>,
    acc: Res<TickAccumulator>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    cameras: Query<&Transform, With<Camera2d>>,
    mut effects: Query<(Entity, &mut WeaponFxEffect, Option<&MeshMaterial2d<ColorMaterial>>)>,
    mut commands: Commands,
) {
    let Ok(camera_transform) = cameras.single() else {
        return;
    };
    let camera_rotation = camera_transform.rotation;
    for (entity, mut effect, material) in &mut effects {
        effect.age += time.delta_secs();
        let progress = (effect.age / effect.duration).clamp(0.0, 1.0);
        // Интерполированная позиция игрока для эффектов, привязанных к нему
        // (лук). Нет игрока в режиме игрока — эффекту не к чему привязываться.
        let player_pos = player
            .0
            .as_ref()
            .filter(|_| state.mode == GameMode::Player)
            .map(|p| player_render_pos(p, &acc));
        match effect.kind {
            WeaponFxKind::Blade => {
                let angle = effect.start_angle + BLADE_SWEEP * progress;
                commands.entity(entity).insert(
                    Transform::from_translation(
                        effect.origin + camera_rotation * Vec3::Z * FX_Z_LIFT,
                    )
                    .with_rotation(camera_rotation * Quat::from_rotation_z(angle)),
                );
                if let Some(material) = material
                    && let Some(mut material) = materials.get_mut(&material.0)
                {
                    material.color = SWING_COLOR.with_alpha(SWING_COLOR.alpha() * (1.0 - progress));
                }
            }
            WeaponFxKind::Thrust => {
                // Синусоида: плавный вынос вперёд и такой же возврат назад.
                // Поворот к прицелу задаётся заново каждый кадр: трансформ
                // перезаписывается целиком, и без этого копьё теряло бы
                // ориентацию после первого кадра анимации.
                let offset = (std::f32::consts::PI * progress).sin() * THRUST_REACH_PX;
                let (sin, cos) = effect.aim_angle.sin_cos();
                // origin уже поднят (см. WEAPON_HAND_LIFT при спавне).
                let position = effect.origin
                    + camera_rotation * Vec3::new(cos * offset, sin * offset, 0.0);
                commands.entity(entity).insert(
                    Transform::from_translation(position).with_rotation(
                        camera_rotation
                            * Quat::from_rotation_z(effect.aim_angle - SPRITE_ANGLE_OFFSET),
                    ),
                );
            }
            WeaponFxKind::Bow => {
                let Some(p) = player_pos else {
                    // Игрок покинул режим: лук не должен висеть в мире один.
                    if let Some(material) = material {
                        materials.remove(&material.0);
                    }
                    commands.entity(entity).despawn();
                    continue;
                };
                let Some(world) = state.world.as_ref() else {
                    continue;
                };
                let position = world_position(world, Vec2::new(p.x, p.y))
                    + camera_rotation * (Vec3::Z * FX_Z_LIFT + Vec3::Y * WEAPON_HAND_LIFT);
                commands.entity(entity).insert(
                    Transform::from_translation(position).with_rotation(
                        camera_rotation
                            * Quat::from_rotation_z(
                                effect.aim_angle - SPRITE_ANGLE_OFFSET + BOW_EXTRA_ROTATION,
                            ),
                    ),
                );
            }
        }
        if progress >= 1.0 {
            if let Some(material) = material {
                materials.remove(&material.0);
            }
            commands.entity(entity).despawn();
        }
    }
}

// ---------- Снаряды и воткнутые копья ----------

/// Соответствие «id снаряда симуляции → Bevy-сущность».
#[derive(Resource, Default)]
struct ProjectileSprites {
    map: HashMap<u64, Entity>,
}

/// Длина линии стрелы в мировых пикселях (на ~10% короче тайла).
const ARROW_LINE_PX: f32 = 10.0;
/// Толщина линии стрелы на ЭКРАНЕ в пикселях — не масштабируется зумом:
/// мировая толщина пересчитывается из масштаба камеры каждый кадр.
const ARROW_LINE_SCREEN_PX: f32 = 0.5;
/// Какую долю высоты полёта показывать как мировую высоту снаряда.
const HEIGHT_LIFT: f32 = 0.75;

/// Оружейные текстуры (копьё, лук) нарисованы направленными вверх-вправо
/// (≈45°); поправка выравнивает рисунок вдоль направления атаки/полёта.
const SPRITE_ANGLE_OFFSET: f32 = std::f32::consts::FRAC_PI_4;
/// Смещение к камере в мировом пространстве, чтобы эффект перекрывал спрайт.
const FX_Z_LIFT: f32 = 0.1;

/// Общие ресурсы снарядов: меш единичного квадрата и материал линии
/// стрелы. Материал кэшируется: создание нового каждый кадр оставляло бы
/// в ассетах десятки неиспользуемых материалов в секунду.
#[derive(Resource, Default)]
struct ProjectileFx {
    arrow_mesh: Option<Handle<Mesh>>,
    arrow_material: Option<Handle<ColorMaterial>>,
}

/// Спрайты воткнутых копьёв: соответствие «копьё → Bevy-сущность». Ключ —
/// битовое представление параметров копья (позиция, угол, отвесность):
/// у `StuckSpear` в симуляции нет стабильного id (список пересобирается
/// `swap_remove` при подборе), а побитово одинаковые копья визуально
/// совпадают — один спрайт честно рисует их оба. Спрайты создаются и
/// убираются точечно по разнице множеств, без пересбора списка каждый кадр.
#[derive(Resource, Default)]
struct StuckSpearSprites {
    map: HashMap<StuckSpearKey, Entity>,
}

/// Ключ спрайта копья: биты четырёх f32-полей записи.
type StuckSpearKey = [u64; 4];

/// Битовый ключ копья (позиция, угол падения, отвесность).
fn spear_key(spear: &StuckSpear) -> StuckSpearKey {
    [
        spear.pos.x.to_bits() as u64,
        spear.pos.y.to_bits() as u64,
        spear.heading.to_bits() as u64,
        spear.pitch.to_bits() as u64,
    ]
}

/// Маркер линии стрелы (масштаб и поворот задаёт трансформ).
#[derive(Component)]
struct ArrowLine;

/// Обновляет спрайты снарядов: создаёт для видимых, двигает по позиции
/// и высоте, убирает закончившие полёт. Стрела — тонкая серо-белая линия
/// (экранная толщина постоянна), копьё — спрайт по направлению полёта.
fn update_projectile_sprites(
    mut commands: Commands,
    state: Res<GameState>,
    cameras: Query<(&Camera, &Transform, &Projection), With<Camera2d>>,
    textures: Res<TileTextures>,
    mut fx: ResMut<ProjectileFx>,
    mut sprites: ResMut<ProjectileSprites>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let Some(world) = state.world.as_ref() else {
        for (_, entity) in sprites.map.drain() {
            commands.entity(entity).despawn();
        }
        return;
    };
    let Ok((camera, camera_transform, projection)) = cameras.single() else {
        return;
    };
    let Projection::Orthographic(ortho) = projection else {
        return;
    };
    let Some(viewport_size) = camera.logical_viewport_size() else {
        return;
    };
    let camera_global = GlobalTransform::from(*camera_transform);
    let camera_rotation = camera_transform.rotation;
    let margin = 2.0 * TILE_SIZE / ortho.scale;
    let arrow_width = (ARROW_LINE_SCREEN_PX * ortho.scale).clamp(0.1, 64.0);

    let arrow_mesh = fx
        .arrow_mesh
        .get_or_insert_with(|| meshes.add(unit_quad_mesh()))
        .clone();
    let arrow_material = fx
        .arrow_material
        .get_or_insert_with(|| {
            materials.add(ColorMaterial {
                color: Color::srgba(0.85, 0.85, 0.9, 0.9),
                alpha_mode: AlphaMode2d::Blend,
                ..default()
            })
        })
        .clone();

    let mut visible = HashSet::new();
    for projectile in &world.projectiles {
        let pos = projectile.pos();
        let mut world_pos = world_position(world, Vec2::new(pos.x, pos.y));
        // Снаряды, выпускаемые «из рук» (стрела, брошенное копьё), стартуют
        // с высоты рук: подъём затухает к посадке, точка падения — земля.
        let hand_lift = match projectile.kind {
            ProjectileKind::Arrow | ProjectileKind::ThrownSpear => {
                WEAPON_HAND_LIFT * (1.0 - projectile.t)
            }
        };
        world_pos.z += (projectile.height() + hand_lift) * WORLD_TILE_SIZE * HEIGHT_LIFT;
        let Ok(viewport_pos) = camera.world_to_viewport(&camera_global, world_pos) else {
            continue;
        };
        if viewport_pos.x < -margin
            || viewport_pos.x > viewport_size.x + margin
            || viewport_pos.y < -margin
            || viewport_pos.y > viewport_size.y + margin
        {
            continue;
        }
        visible.insert(projectile.id);
        let heading = projectile.heading();
        let heading_vector = camera_rotation.inverse() * Vec3::new(heading.cos(), heading.sin(), 0.0);
        let screen_heading = heading_vector.y.atan2(heading_vector.x);
        world_pos += camera_rotation * Vec3::Z * FX_Z_LIFT;

        match projectile.kind {
            ProjectileKind::Arrow => {
                // Линия: единичный квадрат, растянутый трансформом в отрезок
                // вдоль направления полёта. Меш и материал общие.
                let transform = Transform::from_translation(world_pos)
                    .with_rotation(camera_rotation * Quat::from_rotation_z(screen_heading))
                    .with_scale(Vec3::new(ARROW_LINE_PX, arrow_width, 1.0));
                match sprites.map.get(&projectile.id).copied() {
                    Some(entity) => {
                        commands.entity(entity).insert(transform);
                    }
                    None => {
                        let entity = commands
                            .spawn((
                                ArrowLine,
                                Mesh2d(arrow_mesh.clone()),
                                MeshMaterial2d(arrow_material.clone()),
                                transform,
                            ))
                            .id();
                        sprites.map.insert(projectile.id, entity);
                    }
                }
            }
            ProjectileKind::ThrownSpear => {
                let sprite = Sprite {
                    image: textures.item_texture(ItemKind::Spear).clone(),
                    custom_size: Some(Vec2::splat(TILE_SIZE * THROWN_SPEAR_SPRITE_TILES)),
                    ..default()
                };
                let transform = Transform::from_translation(world_pos).with_rotation(
                    camera_rotation
                        * Quat::from_rotation_z(screen_heading - SPRITE_ANGLE_OFFSET),
                );
                match sprites.map.get(&projectile.id).copied() {
                    Some(entity) => {
                        commands.entity(entity).insert((sprite, transform));
                    }
                    None => {
                        let entity = commands.spawn((sprite, transform)).id();
                        sprites.map.insert(projectile.id, entity);
                    }
                }
            }
        }
    }

    let stale: Vec<u64> = sprites
        .map
        .keys()
        .copied()
        .filter(|id| !visible.contains(id))
        .collect();
    for id in stale {
        if let Some(entity) = sprites.map.remove(&id) {
            commands.entity(entity).despawn();
        }
    }
}

/// Обновляет спрайты воткнутых копьёв инкрементально: копья стоят на месте
/// от падения до подбора, поэтому по разнице множеств ключей создаются
/// только новые спрайты и убираются исчезнувшие — каждый кадр ничего
/// не пересобирается.
fn update_stuck_spear_sprites(
    mut commands: Commands,
    state: Res<GameState>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    textures: Res<TileTextures>,
    mut sprites: ResMut<StuckSpearSprites>,
) {
    let Some(world) = state.world.as_ref() else {
        for (_, entity) in sprites.map.drain() {
            commands.entity(entity).despawn();
        }
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    let Some(viewport_size) = camera.logical_viewport_size() else {
        return;
    };
    let camera_global = GlobalTransform::from(*camera_transform);
    let camera_rotation = camera_transform.rotation;
    let margin = 2.0 * TILE_SIZE;

    let mut visible = HashSet::new();
    for spear in &world.stuck_spears {
        let world_pos = world_position(world, Vec2::new(spear.pos.x, spear.pos.y));
        let Ok(viewport_pos) = camera.world_to_viewport(&camera_global, world_pos) else {
            continue;
        };
        if viewport_pos.x < -margin
            || viewport_pos.x > viewport_size.x + margin
            || viewport_pos.y < -margin
            || viewport_pos.y > viewport_size.y + margin
        {
            continue;
        }
        let key = spear_key(spear);
        if !visible.insert(key) {
            // Побитовый дубль (второе копьё ровно на том же месте).
            continue;
        }
        // Воткнувшееся копьё смотрит туда, куда летело; наклон входа
        // показан лёгким «укорачиванием» спрайта: чем отвеснее падение,
        // тем короче видимая проекция.
        let steep = (spear.pitch / std::f32::consts::FRAC_PI_2).clamp(0.0, 1.0);
        let sprite = Sprite {
            image: textures.item_texture(ItemKind::Spear).clone(),
            custom_size: Some(Vec2::splat(TILE_SIZE * 1.4 * (1.0 - steep * 0.35))),
            ..default()
        };
        let heading_vector = camera_rotation.inverse()
            * Vec3::new(spear.heading.cos(), spear.heading.sin(), 0.0);
        let screen_heading = heading_vector.y.atan2(heading_vector.x);
        let transform = Transform::from_translation(world_pos + camera_rotation * Vec3::Z * FX_Z_LIFT)
            .with_rotation(
                camera_rotation * Quat::from_rotation_z(screen_heading - SPRITE_ANGLE_OFFSET),
            );
        match sprites.map.get(&key).copied() {
            Some(entity) => {
                commands.entity(entity).insert((sprite, transform));
            }
            None => {
                let entity = commands.spawn((sprite, transform)).id();
                sprites.map.insert(key, entity);
            }
        }
    }

    let stale: Vec<StuckSpearKey> = sprites
        .map
        .keys()
        .copied()
        .filter(|key| !visible.contains(key))
        .collect();
    for key in stale {
        if let Some(entity) = sprites.map.remove(&key) {
            commands.entity(entity).despawn();
        }
    }
}

/// Единичный квадрат с центром в нуле (для линии стрелы).
fn unit_quad_mesh() -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5_f32, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
    );
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

/// Плагин боевых визуализаций.
pub struct CombatFxPlugin;

impl Plugin for CombatFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SwingFx>()
            .init_resource::<ProjectileFx>()
            .init_resource::<ProjectileSprites>()
            .init_resource::<StuckSpearSprites>()
            .add_systems(
                Update,
                (spawn_weapon_fx, update_weapon_fx)
                    .chain()
                    .after(update_camera_transforms),
            )
            .add_systems(
                Update,
                (update_projectile_sprites, update_stuck_spear_sprites)
                    .after(SimTick)
                    .after(update_camera_transforms),
            );
    }
}
