//! Ввод режима игрока (захваченного существа): захват по F1, движение,
//! ЛКМ (добыча/бой/ломание построек/лук/копьё), ПКМ (станции, посев,
//! подбор, сбор плодов, еда), выброс и бросок предметов.

use bevy::camera::Camera2d;
use bevy::input::mouse::MouseButton;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui_widgets::TextInput;

use crate::app::settings::{ControlAction, Settings};
use crate::app::state::{ClockRes, CommandQueueRes, GameMode, GameState, MenuKind};
use crate::app::input::{PlayerInput, WeaponFx, WeaponFxKind, move_dir, player_grid_dir};
use crate::rendering::{EntitySpriteVisuals, TileTextures};
use crate::simulation::actions;
use crate::simulation::commands::Command;
use crate::simulation::crops::CropKind;
use crate::simulation::items::ItemKind;
use crate::simulation::{TILE_SIZE, TilePos, TileType};
use crate::ui::UiFont;
use crate::ui::pause_menu::BindingCapture;
use crate::ui::{BlocksWorldInput, cursor_over_ui, text_input_focused};

use super::helpers::{cursor_world_pos, cursor_world_tile, entity_at};

/// Сколько секунд ЛКМ нужно удержать с копьём, чтобы отпускание бросило
/// его, а не укололо. Меньше удержания — удар в ближнем бою.
const SPEAR_THROW_HOLD_SECS: f32 = 0.5;

/// Минимальный интервал повторной добычи при зажатой ЛКМ, секунды реального
/// времени (не игрового: ускорение мира не должно превращать удержание
/// в очередь команд).
const INTERACT_REPEAT_SECS: f32 = 0.5;

/// Состояние удержания ЛКМ в режиме игрока: время до следующей добычи.
#[derive(Default)]
pub(crate) struct HarvestHold {
    cooldown: f32,
}

/// F1 — захват существа под курсором / отпускание (GAME_DESIGN 4.1).
/// Системная клавиша (как F11), пока не перебиндивается.
///
/// По целевому формату отдельного игрока в мире нет: управление передаётся
/// существу (человек, волк — любая сущность). В наблюдателе F1 захватывает
/// существо под курсором; повторный F1 возвращает наблюдателя. Захват
/// без сущности под курсором — no-op: существо спавнится редактором.
pub(crate) fn toggle_player_mode(
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
pub(crate) fn player_move_input(
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
            crate::app::state::Tool::Tile(kind) => Command::SetTerrain { pos: tile, kind },
            crate::app::state::Tool::Object(kind) => Command::SetObject {
                pos: tile,
                object: Some(super::editor::placed_object(kind)),
            },
            crate::app::state::Tool::Building(kind) => Command::SetObject {
                pos: tile,
                object: Some(crate::simulation::objects::WorldObject::building(kind)),
            },
            crate::app::state::Tool::Entity(kind) => Command::SpawnEntity { kind, pos: tile },
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
pub(crate) fn player_drop_input(
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
pub(crate) fn player_throw_input(
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
pub(crate) fn player_pickup_input(
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
