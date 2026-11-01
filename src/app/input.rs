//! Ввод режима игрока и редактора: переключение режимов, движение WASD,
//! клики ЛКМ по карте (инструменты «бога» в наблюдателе, добыча — в игре).
//!
//! UI не меняет мир напрямую: все действия уходят командами в очередь
//! симуляции (`simulation/commands.rs`).

use bevy::camera::Camera2d;
use bevy::input::mouse::{MouseButton, MouseMotion};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::TextInput;

use crate::app::settings::{ControlAction, Settings};
use crate::app::state::{
    ClockRes, CommandQueueRes, EditorSelection, GameMode, GameState, MenuKind, Tool, ToolCategory,
    WindowFocusState,
};
use crate::simulation::actions;
use crate::simulation::commands::Command;
use crate::simulation::crops::CropKind;
use crate::simulation::items::ItemKind;
use crate::simulation::objects::{ObjectKind, WorldObject};
use crate::simulation::world::WorldPos;
use crate::simulation::{TILE_SIZE, TilePos, TileType, World};
use crate::ui::UiFont;
use crate::rendering::isometric::{
    WORLD_TILE_SIZE, ground_position_from_viewport, terrain_tile_from_viewport, world_position,
};
use crate::rendering::{EntitySpriteVisuals, TileTextures, entity_sprite_size};

/// Радиус выделения существа курсором, тайлов (задача «инспектор»):
/// выбирается ближайшая к курсору сущность не дальше этого радиуса.
pub(crate) const ENTITY_PICK_RADIUS_TILES: f32 = 2.0;
use crate::ui::bottom_panel::{PanelMenu, PanelMenuState};
use crate::ui::pause_menu::BindingCapture;
use crate::ui::{BlocksWorldInput, cursor_over_ui, text_input_focused};

/// Сколько секунд ЛКМ нужно удержать с копьём, чтобы отпускание бросило
/// его, а не укололо. Меньше удержания — удар в ближнем бою.
const SPEAR_THROW_HOLD_SECS: f32 = 0.5;

/// Вид визуального эффекта оружия. Бой и кулдауны считаются в симуляции,
/// сообщение — только сигнал о визуальном событии для рендера.
#[derive(Clone, Copy, PartialEq, Eq, Message)]
pub enum WeaponFxKind {
    /// Дуга взмаха лезвия (нож, в будущем меч).
    Blade,
    /// Выпад копья: текстура от центра игрока вперёд и назад.
    Thrust,
    /// Выстрел из лука: текстура лука в центре игрока, повёрнутая к выстрелу.
    Bow,
}

/// Событие «игрок применил оружие» для рендера эффектов.
#[derive(Message)]
pub struct WeaponFx {
    pub kind: WeaponFxKind,
    /// Точка, откуда эффект (позиция игрока в мировых пикселях).
    pub origin: Vec2,
    /// Точка, куда направлено (прицел в мировых пикселях).
    pub aim: Vec2,
}

/// Направление движения игрока в координатах сетки мира.
/// Ноль — стоит на месте; нормализация и скорость — в симуляции.
/// Читается тиковой системой движения, поэтому направление — состояние,
/// а не поток команд.
#[derive(Resource, Default, Clone, Copy)]
pub struct PlayerInput {
    pub dir: (f32, f32),
}

/// Читает нажатия биндов движения (по умолчанию WASD). Возвращает
/// направление (−1/0/1 по осям) без нормализации. Общая для движения
/// игрока (input.rs) и камеры редактора (camera.rs).
pub(crate) fn move_dir(keys: &ButtonInput<KeyCode>, settings: &Settings) -> (f32, f32) {
    let mut x = 0.0;
    let mut y = 0.0;
    if keys.pressed(settings.controls.key(ControlAction::MoveUp)) {
        y += 1.0;
    }
    if keys.pressed(settings.controls.key(ControlAction::MoveDown)) {
        y -= 1.0;
    }
    if keys.pressed(settings.controls.key(ControlAction::MoveRight)) {
        x += 1.0;
    }
    if keys.pressed(settings.controls.key(ControlAction::MoveLeft)) {
        x -= 1.0;
    }
    (x, y)
}

/// Преобразует экранное направление в диагональные оси изометрической сетки:
/// экранные вверх/вправо соответствуют (x+y, y-x) в координатах мира.
fn player_grid_dir(screen_dir: (f32, f32)) -> (f32, f32) {
    let (screen_x, screen_y) = screen_dir;
    (screen_x + screen_y, screen_y - screen_x)
}

/// Минимальный интервал повторной добычи при зажатой ЛКМ, секунды реального
/// времени (не игрового: ускорение мира не должно превращать удержание
/// в очередь команд).
const INTERACT_REPEAT_SECS: f32 = 0.5;

/// Состояние удержания ЛКМ в режиме игрока: время до следующей добычи.
#[derive(Default)]
pub(crate) struct HarvestHold {
    cooldown: f32,
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerInput>()
            .add_message::<WeaponFx>()
            .add_systems(
                Update,
                (
                    toggle_player_mode,
                    player_move_input,
                    world_click_input,
                    player_drop_input,
                    player_pickup_input,
                    editor_delete_input,
                    editor_entity_drag,
                    player_throw_input,
                )
                    .after(crate::app::camera::update_camera_transforms),
            );
    }
}

/// F1 — захват существа под курсором / отпускание (GAME_DESIGN 4.1).
/// Системная клавиша (как F11), пока не перебиндивается.
///
/// По целевому формату отдельного игрока в мире нет: управление передаётся
/// существу (человек, волк — любая сущность). В наблюдателе F1 захватывает
/// существо под курсором; повторный F1 возвращает наблюдателя. Захват
/// без сущности под курсором — no-op: существо спавнится редактором.
fn toggle_player_mode(
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    mut state: ResMut<GameState>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
    capture: Res<BindingCapture>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
    textures: Res<TileTextures>,
    visuals: Res<EntitySpriteVisuals>,
) {
    if !keys.just_pressed(KeyCode::F1) {
        return;
    }
    // Пока фокус в текстовом поле, идёт захват бинда или открыт пауз-меню,
    // смена режима отключена.
    if capture.0.is_some() || text_input_focused(&focus, &text_inputs) {
        return;
    }
    if state.active_menu == Some(MenuKind::Pause) {
        return;
    }

    match state.mode {
        GameMode::Player => {
            // Отпускание: сброс FSM-ходьбы, иначе освобождённое существо
            // продолжило бы путь к устаревшей точке (цель осталась с
            // момента захвата, а существо ушло далеко).
            if let Some(entity) = state.controlled_entity_mut() {
                reset_fsm_motion(entity);
            }
            state.set_controlled(None);
        }
        GameMode::Observer => {
            let Ok(window) = windows.single() else {
                return;
            };
            if cursor_over_ui(window, &blockers) {
                return;
            }
            let Some(world) = state.world.as_ref() else {
                return;
            };
            let Some(id) = entity_at(world, window, &cameras, &textures, &visuals) else {
                return;
            };
            state.set_controlled(Some(id));
            // Захват перехватывает управление: сброс в стояние. Иначе
            // спрайт анимировал бы FSM-путь, которым существо больше не
            // идёт, а захваченный лежащим оставался бы лежащим до ввода.
            if let Some(entity) = state.controlled_entity_mut() {
                reset_fsm_motion(entity);
            }
        }
    }
}

/// Сброс движения FSM в стояние: цель приравнивается к позиции, счётчики
/// обнуляются. Общая точка захвата/отпускания существа (F1).
fn reset_fsm_motion(entity: &mut crate::simulation::SimEntity) {
    entity.state = crate::simulation::EntityState::Standing;
    entity.state_ticks = 0;
    entity.target = entity.pos;
    entity.stuck_ticks = 0;
}

/// Бинды движения — направление игрока. Пока фокус в текстовом поле
/// или открыто модальное меню (пауза, деревья, творческое), движение
/// замирает: набор цифр и навигация в меню не должны двигать существо.
fn player_move_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<Settings>,
    state: Res<GameState>,
    mut input: ResMut<PlayerInput>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
) {
    let dir = if text_input_focused(&focus, &text_inputs) || state.active_menu.is_some() {
        (0.0, 0.0)
    } else {
        player_grid_dir(move_dir(&keys, &settings))
    };
    input.dir = dir;
}

/// ЛКМ по карте.
///
/// Наблюдатель с выбранным инструментом — размещение: по нажатию и затем
/// при каждой смене тайла под курсором, пока ЛКМ зажата (спам по одному
/// месту без движения мыши исключён). Режим игрока — зависит от предмета
/// в руке: лук стреляет по нажатию (добычи нет), копьё на коротком
/// нажатии бьёт, а на отпускании после удержания бросается в тайл под
/// курсором; остальное — удар/добыча: сразу по нажатию, затем повтор
/// каждые `INTERACT_REPEAT_SECS`. Неразумное захваченное (волк) — только
/// укус по курсору с тем же автоповтором (`Command::PlayerBite`): ломать
/// и добывать он не умеет. Бой и кулдауны считаются в симуляции;
/// след взмаха — косметика, сообщение уходит и на промахе. Публичная для
/// порядка `apply_commands` в главном плагине.
pub(crate) fn world_click_input(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    windows: Query<&Window>,
    state: Res<GameState>,
    clock: Res<ClockRes>,
    mut queue: ResMut<CommandQueueRes>,
    mut fx: MessageWriter<WeaponFx>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
    mut drag: Local<Option<TilePos>>,
    mut hold: Local<HarvestHold>,
    mut spear_hold: Local<Option<f32>>,
    // Тик, с которого снова можно показывать анимацию выпада (в игровых
    // тиках: на паузе лимитер замирает вместе с миром).
    mut thrust_fx_ready_tick: Local<u64>,
) {
    // Наблюдатель: инструменты «бога».
    if state.mode == GameMode::Observer {
        *hold = HarvestHold::default();
        if !mouse.pressed(MouseButton::Left) {
            *drag = None;
            return;
        }
        let Ok(window) = windows.single() else {
            return;
        };
        let Some(world) = state.world.as_ref() else {
            return;
        };
        if cursor_over_ui(window, &blockers) {
            *drag = None;
            return;
        }
        let Some(tool) = state.selected_tool else {
            *drag = None;
            return;
        };
        let Some(tile) = cursor_world_tile(window, &cameras, Some(world)) else {
            return;
        };
        // Ставим по нажатию и при смене тайла под курсором.
        if *drag == Some(tile) {
            return;
        }
        let command = match tool {
            Tool::Tile(kind) => Command::SetTerrain { pos: tile, kind },
            Tool::Object(kind) => Command::SetObject {
                pos: tile,
                object: Some(placed_object(kind)),
            },
            Tool::Building(kind) => Command::SetObject {
                pos: tile,
                object: Some(WorldObject::building(kind)),
            },
            Tool::Entity(kind) => Command::SpawnEntity { kind, pos: tile },
        };
        queue.0.push(command);
        *drag = Some(tile);
        return;
    }

    // Режим игрока: на паузе мир не тикает — и действие невозможно, как и
    // всё, что является частью симуляции. Таймеры удержания сбрасываются:
    // после снятия паузы зажатая ЛКМ начинает цикл заново, а не даёт
    // внеочередную добычу по устаревшему кулдауну.
    if state.paused {
        *spear_hold = None;
        *hold = HarvestHold::default();
        return;
    }
    let Some(world) = state.world.as_ref() else {
        *hold = HarvestHold::default();
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    if cursor_over_ui(window, &blockers) {
        *spear_hold = None;
        *drag = None;
        return;
    }
    let Some(p) = state.controlled_entity() else {
        return;
    };
    let attack_ready = p.mind.as_ref().map_or(u64::MAX, |m| m.attack_ready_tick);
    let held = p.mind.as_ref().and_then(|m| m.selected_item());
    let (actor_pos_x, actor_pos_y) = (p.pos.x, p.pos.y);
    let Some(tile) = cursor_world_tile(window, &cameras, Some(world)) else {
        return;
    };
    // Неразумное захваченное (волк): ЛКМ — укус по курсору с тем же
    // автоповтором зажатия, что у удара человека. Кусь — единственное
    // действие волка: ломать постройки и добывать он не умеет, поэтому
    // ветки ниже (ломание, лук, копьё, добыча) ему недоступны.
    if !p.kind.is_sentient() {
        if !mouse.pressed(MouseButton::Left) {
            *hold = HarvestHold::default();
            return;
        }
        if mouse.just_pressed(MouseButton::Left) {
            queue.0.push(Command::PlayerBite { pos: tile });
            hold.cooldown = INTERACT_REPEAT_SECS;
            return;
        }
        hold.cooldown -= time.delta_secs();
        if hold.cooldown <= 0.0 {
            queue.0.push(Command::PlayerBite { pos: tile });
            hold.cooldown =
                INTERACT_REPEAT_SECS - (-hold.cooldown).rem_euclid(INTERACT_REPEAT_SECS);
        }
        return;
    }
    let aim = cursor_world_pos(window, &cameras, world).unwrap_or_default();
    let origin = Vec2::new(actor_pos_x * TILE_SIZE, actor_pos_y * TILE_SIZE);

    // Постройка под курсором — зажатие ЛКМ ломает её (любым предметом):
    // команда уходит каждый кадр зажатия, тики непрерывности и разрушение
    // считает симуляция. Обычная добыча и удары оружием на тайле постройки
    // недоступны — ЛКМ занята ломанием.
    if world.object_at(tile).is_some_and(|obj| obj.kind.is_building()) {
        if mouse.pressed(MouseButton::Left) {
            queue.0.push(Command::PlayerBreakBuilding { pos: tile });
        }
        return;
    }

    // Лук: стрельба по нажатию. Добычи и ударов нет — луком нельзя ломать.
    // Эффект показывается только при реальном выстреле: на кулдауне
    // симуляция команду отклонит, и текстура лука не должна появляться.
    if held == Some(ItemKind::Bow) {
        if mouse.just_pressed(MouseButton::Left) && clock.0.tick >= attack_ready {
            queue.0.push(Command::PlayerShootBow { target: tile });
            fx.write(WeaponFx { kind: WeaponFxKind::Bow, origin, aim });
        }
        return;
    }

    // Копьё: короткое нажатие — укол (мимо существ — обычная добыча),
    // удержание дольше `SPEAR_THROW_HOLD_SECS` — бросок на отпускание.
    if held == Some(ItemKind::Spear) {
        if mouse.just_pressed(MouseButton::Left) {
            *spear_hold = Some(0.0);
        }
        if let Some(secs) = spear_hold.as_mut()
            && mouse.pressed(MouseButton::Left)
        {
            *secs += time.delta_secs();
        }
        if mouse.just_released(MouseButton::Left) && let Some(secs) = spear_hold.take() {
            if secs < SPEAR_THROW_HOLD_SECS {
                // Укол: анимация выпада только когда кулдаун прошлой атаки
                // вышел (иначе симуляция удар отклонит, а текстура была бы
                // видна). На промахе кулдаун не тратится — выпад виден и там.
                // Свой ограничитель анимации: симуляция списывает кулдаун
                // только при попадании, поэтому без него заклик мимо играл
                // бы выпад на каждое нажатие. Интервал — кулдаун копья.
                let spear_cooldown = ItemKind::Spear
                    .weapon_stats()
                    .map(|stats| stats.cooldown_ticks)
                    .unwrap_or(10);
                if clock.0.tick >= attack_ready && clock.0.tick >= *thrust_fx_ready_tick {
                    fx.write(WeaponFx { kind: WeaponFxKind::Thrust, origin, aim });
                    *thrust_fx_ready_tick = clock.0.tick + spear_cooldown;
                }
                queue.0.push(Command::PlayerInteract { pos: tile });
            } else {
                queue.0.push(Command::PlayerThrowSpear { target: tile });
            }
        }
        if !mouse.pressed(MouseButton::Left) {
            *spear_hold = None;
        }
        return;
    }

    // Остальные предметы и рука: удар/добыча. Первое действие выполняется
    // сразу, последующие — строго с интервалом по реальному времени, даже
    // если курсор перемещается между тайлами.
    if !mouse.pressed(MouseButton::Left) {
        *hold = HarvestHold::default();
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        // След взмаха — только у лезвий (нож; меч появится позже): рука
        // и инструменты без анимации, бой при этом считается всегда.
        // Анимация только когда кулдаун прошлой атаки вышел — как у лука
        // и копья, чтобы визуал не убегал вперёд реального урона.
        if held == Some(ItemKind::Knife) && clock.0.tick >= attack_ready {
            fx.write(WeaponFx { kind: WeaponFxKind::Blade, origin, aim });
        }
        queue.0.push(Command::PlayerInteract { pos: tile });
        hold.cooldown = INTERACT_REPEAT_SECS;
        return;
    }

    hold.cooldown -= time.delta_secs();
    if hold.cooldown <= 0.0 {
        queue.0.push(Command::PlayerInteract { pos: tile });
        hold.cooldown =
            INTERACT_REPEAT_SECS - (-hold.cooldown).rem_euclid(INTERACT_REPEAT_SECS);
    }
}

/// Бинд «Положить предмет» (по умолчанию V) — положить предмет из руки на
/// тайл под курсором. Тайл должен быть в радиусе взаимодействия игрока
/// (как для добычи); финальную проверку радиуса всё равно делает симуляция
/// при применении команды.
fn player_drop_input(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<Settings>,
    windows: Query<&Window>,
    state: Res<GameState>,
    mut queue: ResMut<CommandQueueRes>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
    capture: Res<BindingCapture>,
) {
    if state.mode != GameMode::Player || state.paused {
        return;
    }
    if capture.0.is_some() || text_input_focused(&focus, &text_inputs) {
        return;
    }
    if !keys.just_pressed(settings.controls.key(ControlAction::DropItem)) {
        return;
    }
    let Some(actor) = state.controlled else {
        return;
    };
    let Some(world) = state.world.as_ref() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    if cursor_over_ui(window, &blockers) {
        return;
    }
    let Some(tile) = cursor_world_tile(window, &cameras, Some(world)) else {
        return;
    };
    if !actions::reachable(world, actor, tile) {
        return;
    }
    queue.0.push(Command::PlayerDropItem { pos: tile });
}

/// Клавиша «Бросить предмет»: один предмет из выбранного слота летит в
/// сторону тайла под курсором (дальность ограничена симуляцией). Вне
/// режима игрока, на паузе, при наборе текста и над UI не срабатывает.
fn player_throw_input(
    mut queue: ResMut<CommandQueueRes>,
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<Settings>,
    state: Res<GameState>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
    capture: Res<BindingCapture>,
) {
    if state.mode != GameMode::Player || state.paused {
        return;
    }
    if capture.0.is_some() || text_input_focused(&focus, &text_inputs) {
        return;
    }
    if !keys.just_pressed(settings.controls.key(ControlAction::ThrowItem)) {
        return;
    }
    if state.controlled.is_none() || state.world.is_none() {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    if cursor_over_ui(window, &blockers) {
        return;
    }
    let Some(tile) = cursor_world_tile(window, &cameras, state.world.as_ref()) else {
        return;
    };
    // Shift/Ctrl + бросок — вылетает весь слот (до стака), без — один предмет.
    let whole_slot = keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    if whole_slot
        && let Some(m) = state.controlled_mind()
    {
        let slot = m.selected_slot.min(crate::simulation::MISC_SLOTS - 1) as u8;
        queue.0.push(Command::PlayerThrowStack { target: tile, slot });
        return;
    }
    queue.0.push(Command::PlayerThrowItem { target: tile });
}

/// ПКМ по лежащей стопке предметов — подобрать её; по яблоне с плодами —
/// собрать плоды. Зажатие действует как ЛКМ-добыча: первое действие по
/// нажатию, затем повтор каждые `INTERACT_REPEAT_SECS` по тайлу под
/// курсором. Выбранная постройка в меню строительства перехватывает ПКМ:
/// клик строит, а не подбирает. Яблоко в выбранном слоте хотбара
/// перехватывает ПКМ раньше всего: клик ест его (побочный эффект — с
/// яблоком в руке нельзя подобрать предмет или собрать плоды; еда не
/// повторяется зажатием — у неё свой кулдаун в симуляции).
fn player_pickup_input(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    windows: Query<&Window>,
    state: Res<GameState>,
    construction: Res<crate::ui::construction::ConstructionUi>,
    mut queue: ResMut<CommandQueueRes>,
    mut station: ResMut<crate::ui::stations::StationUi>,
    mut commands: Commands,
    font: Res<UiFont>,
    station_roots: Query<Entity, With<crate::ui::stations::StationRoot>>,
    mut hold: Local<HarvestHold>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
) {
    if state.mode != GameMode::Player || state.paused {
        *hold = HarvestHold::default();
        return;
    }
    if crate::ui::construction::ConstructionUi::building_selected(&construction) {
        *hold = HarvestHold::default();
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    if cursor_over_ui(window, &blockers) {
        *hold = HarvestHold::default();
        return;
    }
    // Еда в руке — ПКМ ест по нажатию (зажатие без повторов: кулдаун
    // поедания всё равно отбрасывал бы команды). НО сажаемый предмет
    // (семена, корнеплоды — тоже еда) по подходящей пашне сажается,
    // а не съедается: проверка как в `rmb_action`/`Player::sow`.
    let held = state.controlled_mind().and_then(|m| m.selected_item());
    let sow_ready = match (held, state.world.as_ref()) {
        (Some(item), Some(world)) => {
            CropKind::from_seed_item(item).is_some()
                && cursor_world_tile(window, &cameras, Some(world)).is_some_and(|tile| {
                    world.terrain_kind(tile) == TileType::Farmland
                        && world.object_at(tile).is_none()
                        && world.crop_at(tile).is_none()
                        && world.item_at(tile).is_none()
                })
        }
        _ => false,
    };
    if !sow_ready && held.is_some_and(|kind| kind.food_value() > 0.0) {
        if mouse.just_pressed(MouseButton::Right) {
            queue.0.push(Command::PlayerEat);
        }
        *hold = HarvestHold::default();
        return;
    }

    // Зажатие ПКМ — как ЛКМ-добыча: действие по нажатию и повтором.
    if !mouse.pressed(MouseButton::Right) {
        *hold = HarvestHold::default();
        return;
    }
    if mouse.just_pressed(MouseButton::Right) {
        rmb_action(
            &mut queue,
            &state,
            &windows,
            &cameras,
            &mut station,
            &mut commands,
            &font,
            &station_roots,
        );
        hold.cooldown = INTERACT_REPEAT_SECS;
        return;
    }
    hold.cooldown -= time.delta_secs();
    if hold.cooldown <= 0.0 {
        rmb_action(
            &mut queue,
            &state,
            &windows,
            &cameras,
            &mut station,
            &mut commands,
            &font,
            &station_roots,
        );
        hold.cooldown =
            INTERACT_REPEAT_SECS - (-hold.cooldown).rem_euclid(INTERACT_REPEAT_SECS);
    }
}

/// Одно действие ПКМ в режиме игрока: открыть GUI станции, посеять
/// культуру в пашню, подобрать стопку предметов с тайла или собрать
/// плоды с яблони. Тайл читается заново на каждый повтор.
fn rmb_action(
    queue: &mut CommandQueueRes,
    state: &GameState,
    windows: &Query<&Window>,
    cameras: &Query<(&Camera, &Transform), With<Camera2d>>,
    station: &mut crate::ui::stations::StationUi,
    commands: &mut Commands,
    font: &UiFont,
    station_roots: &Query<Entity, With<crate::ui::stations::StationRoot>>,
) {
    let Some(actor) = state.controlled else {
        return;
    };
    let held = state.controlled_mind().and_then(|m| m.selected_item());
    let Some(world) = state.world.as_ref() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(tile) = cursor_world_tile(window, cameras, Some(world)) else {
        return;
    };
    // Станция под курсором — открыть её GUI (приоритет над всем остальным),
    // только если она в радиусе взаимодействия с тайлами.
    if let Some(kind) = world
        .object_at(tile)
        .map(|obj| obj.kind)
        .filter(|kind| kind.is_station())
        .filter(|_| actions::reachable(world, actor, tile))
    {
        crate::ui::stations::toggle_station(commands, station, station_roots, font, tile, kind);
        return;
    }
    // Сажаемый предмет в руке и пашня без объекта — посев.
    if held.is_some_and(|kind| CropKind::from_seed_item(kind).is_some())
        && world.terrain_kind(tile) == TileType::Farmland
        && world.object_at(tile).is_none()
        && world.crop_at(tile).is_none()
        && world.item_at(tile).is_none()
    {
        queue.0.push(Command::PlayerSow { pos: tile });
        return;
    }
    if world.item_at(tile).is_some() {
        queue.0.push(Command::PlayerPickupItem { pos: tile });
    } else if world.object_at(tile).is_some_and(|obj| obj.apple_yield().is_some()) {
        // ПКМ по яблоне — собрать плоды (ЛКМ по ней ничего не делает).
        queue.0.push(Command::PlayerHarvestApple { pos: tile });
    }
}

/// Объект, ставимый редактором по виду. У валуна — средний запас из
/// рабочего диапазона генерации (5–15 единиц, GAME_DESIGN приложение A).
fn placed_object(kind: ObjectKind) -> WorldObject {
    match kind {
        ObjectKind::Tree => WorldObject::tree(),
        ObjectKind::Bush => WorldObject::bush(),
        ObjectKind::Rock => WorldObject::rock(10),
        ObjectKind::AppleTree => WorldObject::apple_tree(),
        ObjectKind::Building(kind) => WorldObject::building(kind),
    }
}

/// Что удаляет ПКМ в редакторе. Класс цели задаётся открытым подменю
/// нижней панели: пока подменю категории не открыто, цель не выбрана.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DeleteClass {
    Objects,
    Entities,
}

/// ПКМ в режиме наблюдателя — удалить объект из открытой категории.
///
/// Удаление срабатывает по отпусканию без движения (порог в несколько
/// пикселей): зажатая ПКМ с движением — это панорамирование камеры, и
/// удалять то, что протащили под курсором, нельзя.
pub(crate) fn editor_delete_input(
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    windows: Query<&Window>,
    state: Res<GameState>,
    menu: Res<PanelMenuState>,
    mut queue: ResMut<CommandQueueRes>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
    textures: Res<TileTextures>,
    visuals: Res<EntitySpriteVisuals>,
    mut drag_distance: Local<Option<f32>>,
) {
    if state.mode != GameMode::Observer {
        *drag_distance = None;
        return;
    }

    // Класс цели — только из открытого подменю инструментов. Базовые кнопки,
    // скорость и категория тайлов цель для удаления не задают: ПКМ по карте
    // в этих состояниях ничего не трогает.
    let target_class = match menu.open {
        Some(PanelMenu::Tools(ToolCategory::Objects)) => Some(DeleteClass::Objects),
        Some(PanelMenu::Tools(ToolCategory::Buildings)) => Some(DeleteClass::Objects),
        Some(PanelMenu::Tools(ToolCategory::Entities)) => Some(DeleteClass::Entities),
        _ => None,
    };

    // Дрожание руки при клике не должно отменять удаление: порог в
    // несколько пикселей отделяет клик от осознанного панорамирования.
    const CLICK_DRAG_THRESHOLD: f32 = 4.0;
    let motion_delta: f32 = motion.read().map(|event| event.delta.length()).sum();
    if mouse.just_pressed(MouseButton::Right) {
        *drag_distance = Some(motion_delta);
    } else if let Some(distance) = drag_distance.as_mut() {
        *distance += motion_delta;
    }

    // Отпускание — единственный момент удаления: так быстрый клик
    // (нажатие и отпускание в одном кадре) тоже обрабатывается.
    if !mouse.just_released(MouseButton::Right) {
        return;
    }
    let Some(distance) = drag_distance.take() else {
        return;
    };
    if distance > CLICK_DRAG_THRESHOLD {
        return;
    }
    let Some(target_class) = target_class else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    if cursor_over_ui(window, &blockers) {
        return;
    }
    let Some(world) = state.world.as_ref() else {
        return;
    };
    let Some(tile) = cursor_world_tile(window, &cameras, Some(world)) else {
        return;
    };
    match target_class {
        DeleteClass::Objects => {
            if world.object_at(tile).is_some() {
                queue.0.push(Command::SetObject { pos: tile, object: None });
            }
        }
        DeleteClass::Entities => {
            if let Some(id) = entity_at(world, window, &cameras, &textures, &visuals) {
                queue.0.push(Command::RemoveEntity { id });
            }
        }
    }
}

/// СКМ перетаскивает существо в редакторе: нажатие на него
/// «берёт» существо, пока кнопка зажата — оно следует за курсором,
/// отпускание освобождает. Каждый кадр уходит команда `MoveEntity`.
pub(crate) fn editor_entity_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    windows: Query<&Window>,
    focus: Res<WindowFocusState>,
    state: Res<GameState>,
    mut selection: ResMut<EditorSelection>,
    mut queue: ResMut<CommandQueueRes>,
    cameras: Query<(&Camera, &Transform), With<Camera2d>>,
    blockers: Query<(&ComputedNode, &UiGlobalTransform, &Visibility), With<BlocksWorldInput>>,
    textures: Res<TileTextures>,
    visuals: Res<EntitySpriteVisuals>,
    mut dragged: Local<Option<u64>>,
    // Накопленное движение мыши с момента нажатия СКМ: клик без
    // перетаскивания (порог в пикселях) открывает окно-инспектор.
    mut drag_distance: Local<f32>,
) {
    const CLICK_DRAG_THRESHOLD: f32 = 4.0;

    // Потеря фокуса окна: отпускание кнопки могло не дойти до игры
    // (например, кнопку отпустили в другом окне), поэтому перетаскивание
    // завершаем сразу, не дожидаясь следующего нажатия.
    if state.mode != GameMode::Observer || !focus.focused {
        *dragged = None;
        selection.hovered = None;
        return;
    }

    let Ok(window) = windows.single() else {
        *dragged = None;
        return;
    };

    // Наведение: существо под курсором (не над UI) подсвечивается белым
    // контуром. Пересчёт только в наблюдателе — в игре выделения нет.
    selection.hovered = if cursor_over_ui(window, &blockers) {
        None
    } else {
        state
            .world
            .as_ref()
            .and_then(|world| entity_at(world, window, &cameras, &textures, &visuals))
    };

    // Нажатие на существо начинает перетаскивание. Клик мимо — ничего.
    if mouse.just_pressed(MouseButton::Middle) {
        *dragged = None;
        *drag_distance = 0.0;
        if cursor_over_ui(window, &blockers) {
            return;
        }
        let Some(world) = state.world.as_ref() else {
            return;
        };
        if cursor_world_pos(window, &cameras, world).is_none() {
            return;
        }
        *dragged = entity_at(world, window, &cameras, &textures, &visuals);
    }
    // Дрожание руки при клике не должно открывать инспектор вместо
    // перетаскивания: порог тот же, что у удаления ПКМ.
    let motion_delta: f32 = motion.read().map(|event| event.delta.length()).sum();
    if mouse.pressed(MouseButton::Middle) {
        *drag_distance += motion_delta;
    }

    let Some(id) = *dragged else {
        return;
    };
    let releasing = mouse.just_released(MouseButton::Middle);
    if !mouse.pressed(MouseButton::Middle) && !releasing {
        *dragged = None;
        return;
    }

    // Отпускание без движения — это клик по существу: открыть/закрыть
    // окно-инспектор вместо переноса.
    if releasing && *drag_distance <= CLICK_DRAG_THRESHOLD {
        selection.selected = if selection.selected == Some(id) {
            None
        } else {
            Some(id)
        };
        *dragged = None;
        return;
    }

    // Перенос начинается только когда зажатие переросло клик: раньше
    // команда уходила уже в кадр нажатия, и СКМ-клик по сущности
    // телепортировал её к курсору (радиус выбора — до 2 тайлов) до того,
    // как releasing успевал превратить зажатие в инспектор.
    if *drag_distance > CLICK_DRAG_THRESHOLD
        && mouse.pressed(MouseButton::Middle)
        && !cursor_over_ui(window, &blockers)
        && let Some(world) = state.world.as_ref()
        && let Some(point) = cursor_world_pos(window, &cameras, world)
    {
        // Курсор в пикселях мира, позиция существа — в тайлах. Край мира
        // зажимаем чуть внутрь, чтобы существо не вышло за границу.
        let edge = (world.size.tiles() as f32 - 0.001).max(0.0);
        let pos = WorldPos {
            x: (point.x / TILE_SIZE).clamp(0.0, edge),
            y: (point.y / TILE_SIZE).clamp(0.0, edge),
        };
        queue.0.push(Command::MoveEntity { id, pos });
    }

    if releasing {
        *dragged = None;
    }
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
    let cursor_tiles = WorldPos {
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
        let ground = world_position(world, Vec2::new(entity.pos.x, entity.pos.y));
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

#[cfg(test)]
#[path = "../tests/input.rs"]
mod tests;
