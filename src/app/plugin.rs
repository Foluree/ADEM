//! Главный плагин проекта: собирает вместе камеру, рендер, UI и генерирует мир.

use bevy::anti_alias::fxaa::Fxaa;
use bevy::anti_alias::smaa::Smaa;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::render::view::Msaa;
use bevy::ui_widgets::TextInput;
use bevy::window::{MonitorSelection, PresentMode, VideoModeSelection, WindowEvent, WindowMode};
use bevy::winit::{UpdateMode, WinitSettings};
use std::time::Duration;

use crate::app::audio::AudioPlugin;
use crate::app::camera::{CameraPlugin, OverlayCamera, WorldCamera};
use crate::app::input::InputPlugin;
use crate::app::saves::SavesPlugin;
use crate::app::settings::{
    AntiAlias, ControlAction, Settings, WindowModeSetting, settings_path,
};
use crate::app::state::{
    ClockRes, CommandQueueRes, EditorSelection, FocusPause, GameMode, GameState, MenuKind,
    SchedulerRes, SimSpeed, SimTick, TickAccumulator, TickTiming, WindowFocusState,
};
use crate::app::window_icon::set_window_icon;
use crate::rendering::RenderingPlugin;
use crate::simulation::climate;
use crate::simulation::combat;
use crate::simulation::containers;
use crate::simulation::crops;
use crate::simulation::generation::generate_world;
use crate::simulation::scheduler::{Task, TaskKind};
use crate::simulation::time::{TICKS_PER_SEC, next_hour_tick, next_season_tick, start_tick};
use crate::simulation::vegetation;
use crate::ui::CraftBookRes;
use crate::ui::pause_menu::BindingCapture;
use crate::ui::{UiPlugin, text_input_focused};

pub struct AdemPlugin;

impl Plugin for AdemPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameState>()
            .init_resource::<ClockRes>()
            .init_resource::<SchedulerRes>()
            .init_resource::<TickAccumulator>()
            .init_resource::<TickTiming>()
            .init_resource::<WindowFocusState>()
            .init_resource::<FocusPause>()
            .init_resource::<CommandQueueRes>()
            .init_resource::<EditorSelection>()
            .add_message::<TickEvent>()
            .add_plugins(CameraPlugin)
            .add_plugins(RenderingPlugin)
            .add_plugins(UiPlugin)
            .add_plugins(InputPlugin)
            .add_plugins(AudioPlugin)
            .add_plugins(SavesPlugin)
            .add_systems(Startup, (seed_calendar, set_window_icon))
            .add_systems(PostStartup, generate_initial_world)
            .add_systems(
                Update,
                (
                    handle_global_keys,
                    tick_world.in_set(SimTick),
                    // Ввод ставит команды в очередь — применение строго после.
                    apply_commands
                        .after(crate::app::input::world_click_input)
                        .after(crate::app::input::editor_delete_input)
                        .after(crate::app::input::editor_entity_drag),
                    track_window_state,
                    auto_pause_on_focus_loss,
                    apply_window_mode,
                    apply_present_mode,
                    apply_antialias,
                    apply_window_update_mode,
                    toggle_borderless_f11,
                ),
            )
            .add_systems(Last, save_settings_on_exit);
    }
}

/// Сгенерировать стартовый мир по текущим параметрам GameState и запустить
/// первичные цепочки растительности. Посев идёт от стартового тика часов:
/// цепочки планируются в будущее относительно момента начала мира.
fn generate_initial_world(
    mut state: ResMut<GameState>,
    clock: Res<ClockRes>,
    mut sched: ResMut<SchedulerRes>,
) {
    let mut world = generate_world(state.world_size, state.seed);
    world.ambient_temperature_c = climate::ambient_temperature_c(clock.0.tick);
    vegetation::seed_all(&world, &mut sched.0, clock.0.tick);
    state.world = Some(world);
    state.pending_rerender = true;
}

/// Засевает периодические календарные события при старте.
/// Дальше их перезапускает tick_world после каждого срабатывания.
/// Мир начинается утром в 8:00: часы сразу стоят на стартовом тике,
/// календарные события планируются от него же.
fn seed_calendar(mut clock: ResMut<ClockRes>, mut sched: ResMut<SchedulerRes>) {
    clock.0.tick = start_tick();
    sched.0.schedule_at(next_hour_tick(clock.0.tick), TaskKind::TimeOfDayChanged);
    sched.0.schedule_at(next_season_tick(clock.0.tick), TaskKind::SeasonChanged);
}

/// Нижняя граница FPS, на которую рассчитан догоняющий лимит тиков.
/// При падении FPS ниже неё симуляция всё же замедляется (иначе аккумулятор
/// рос бы бесконечно), но любое окно от 5 FPS идёт в реальном темпе —
/// в том числе неактивное (10 FPS в фоне, см. `apply_window_update_mode`).
const MIN_SIM_FPS: f32 = 5.0;

/// Максимум тиков за один кадр для данной скорости: столько тиков нужно,
/// чтобы выдержать множитель даже на `MIN_SIM_FPS` кадрах в секунду
/// (x1 → 4, x100 → 400). Решение автора: симуляция не замедляется вместе
/// с FPS (прежний фиксированный лимит 8 тиков тормозил мир на низком FPS).
pub(crate) fn max_ticks_for_frame(speed: SimSpeed) -> u32 {
    (speed.multiplier() * TICKS_PER_SEC as f32 / MIN_SIM_FPS)
        .ceil()
        .max(1.0) as u32
}

/// Чистая логика фикс-степа: сколько тиков исполнить и остаток аккумулятора.
/// При исчерпании бюджета (только ниже `MIN_SIM_FPS`) остаток отбрасывается.
pub(crate) fn step_ticks(
    acc_seconds: f32,
    speed: f32,
    delta_secs: f32,
    max_ticks: u32,
) -> (u32, f32) {
    let mut acc = acc_seconds + delta_secs * speed;
    let step = 1.0 / TICKS_PER_SEC as f32;
    let mut ticks = 0;
    while acc >= step && ticks < max_ticks {
        acc -= step;
        ticks += 1;
    }
    if ticks == max_ticks {
        acc = 0.0;
    }
    (ticks, acc)
}

/// Фикс-степ тик мира: продвигает часы, исполняет задачи планировщика
/// и двигает игрока, если он в мире.
///
/// Задачи растительности меняют тайлы мира: их позиции уходят в
/// `state.dirty_tiles` (dirty-флаги), по которым рендер перестраивает
/// terrain-блоки. Без отметки изменения были бы видны только после
/// ручной правки карты (командами).
fn tick_world(
    time: Res<Time>,
    mut state: ResMut<GameState>,
    input: Res<crate::app::input::PlayerInput>,
    mut acc: ResMut<TickAccumulator>,
    mut clock: ResMut<ClockRes>,
    mut sched: ResMut<SchedulerRes>,
    mut timing: ResMut<TickTiming>,
    book: Res<CraftBookRes>,
    mut fired: Local<Vec<(u64, Task)>>,
    mut events: MessageWriter<TickEvent>,
) {
    // На паузе тики не генерируются и аккумулятор не растёт (GAME_DESIGN 3.3):
    // стоит и захваченное существо — оно часть мира.
    if state.paused || state.world.is_none() {
        return;
    }
    let (ticks, rest) = step_ticks(
        acc.seconds,
        state.speed.multiplier(),
        time.delta_secs(),
        max_ticks_for_frame(state.speed),
    );
    acc.seconds = rest;
    if ticks == 0 {
        return;
    }

    // Захваченное существо могло погибнуть в мире (бой) или быть убрано
    // редактором — контроль теряется сразу.
    if state.controlled.is_some() && state.controlled_entity().is_none() {
        state.set_controlled(None);
    }

    // Захваченное существо движется по тикам симуляции: скорость задана
    // видом, на ускорении мира оно движется быстрее в реальном времени.
    // Тут же подбирает воткнутые рядом копья (автоподбор по близости) и
    // тратит сытость. Здоровье, упавшее в ноль (голод, бой), — смерть:
    // лут инвентаря выпадает, управление возвращается наблюдателю.
    // Полноценная смерть с телом — отдельная задача (ROADMAP).
    //
    // При включённом ИИ (кнопка «ИИ» в HUD) движение и голод ведёт
    // симуляция: `step_direction`/`tick_hunger` не вызываются — иначе
    // голод тикал бы дважды, а ввод рвал бы ИИ-цели.
    if state.mode == GameMode::Player
        && let Some(actor) = state.controlled
    {
        let ai_driven = state.controlled_ai;
        let mut died = false;
        if let Some(world) = state.world.as_mut() {
            if !ai_driven {
                for _ in 0..ticks {
                    crate::simulation::actions::step_direction(world, actor, input.dir);
                    if !crate::simulation::actions::tick_hunger(world, actor, input.dir != (0.0, 0.0))
                    {
                        died = true;
                        break;
                    }
                }
            }
            combat::pickup_stuck_spears(world, actor);
        }
        if died {
            // Дроп инвентаря и удаление сущности из мира.
            if let Some(world) = state.world.as_mut() {
                combat::kill_entity(world, actor, clock.0.tick);
            }
            state.set_controlled(None);
        }
    }

    // Dirty-флаги изымаются до мутации мира: события растительности
    // дописывают позиции изменённых тайлов (с каналом причины), рендер
    // «расходует» их за кадр.
    let mut dirty = std::mem::take(&mut state.dirty_tiles);
    let started = std::time::Instant::now();
    // Список исполненных задач переиспользуется между кадрами: ёмкость
    // переживает кадр, аллокаций на ровном месте нет.
    fired.clear();
    for _ in 0..ticks {
        clock.0.tick_once();
        let tick = clock.0.tick;
        sched.0.run_tick(tick, |task| fired.push((tick, task)));
        // Существа двигаются и принимают решения на каждом тике — так же,
        // как захваченное существо: их поведение часть мира, а не рендера.
        // Снаряды летят и бьют в том же тике. Захваченному ИИ и FSM не
        // ведут — кроме случая, когда игрок включил ему автономность
        // (кнопка «ИИ»): тогда существо живёт как обычное.
        let player_controlled = if state.controlled_ai { None } else { state.controlled };
        if let Some(world) = state.world.as_mut() {
            world.tick_entities(tick, player_controlled);
            world.tick_projectiles(tick);
            // Дроп-сущности: автоподбор людьми и исчезновение по возрасту.
            world.tick_item_entities(tick);
        }
    }

    // Температура окружения — чистая функция от календаря (месяц + время
    // суток, simulation/climate.rs): O(1) арифметики на кадр, планировщик
    // не нужен. Значение одно на весь мир — локальный климат появится
    // вместе с погодой.
    if let Some(world) = state.world.as_mut() {
        world.ambient_temperature_c = climate::ambient_temperature_c(clock.0.tick);
    }

    // Периодические календарные события перезапускаются на следующую границу.
    // Час/сезон не могут сработать дважды за один кадр (максимум 8 тиков =
    // 96 игровых секунд < часа), поэтому перезапуск по одному разу на каждое
    // сработавшее событие корректен.
    // События растительности применяют правило и сами планируют продолжение.
    for (fired_tick, task) in &fired {
        match &task.kind {
            TaskKind::TimeOfDayChanged => {
                sched.0.schedule_at(next_hour_tick(clock.0.tick), TaskKind::TimeOfDayChanged);
            }
            TaskKind::SeasonChanged => {
                sched.0.schedule_at(next_season_tick(clock.0.tick), TaskKind::SeasonChanged);
            }
            kind @ (TaskKind::GrassSpread { .. }
            | TaskKind::BushGrowth { .. }
            | TaskKind::WaterErosion { .. }) => {
                let Some(world) = state.world.as_mut() else {
                    continue;
                };
                vegetation::apply_task(kind, world, &mut sched.0, *fired_tick, &mut dirty);
            }
            TaskKind::CropGrowth { chunk, local_index } => {
                let Some(world) = state.world.as_mut() else {
                    continue;
                };
                crops::apply_growth(world, &mut sched.0, *fired_tick, &mut dirty, *chunk, *local_index);
            }
            TaskKind::FarmlandRevert { chunk, local_index } => {
                let Some(world) = state.world.as_mut() else {
                    continue;
                };
                crops::apply_farmland_revert(
                    world,
                    &mut sched.0,
                    *fired_tick,
                    &mut dirty,
                    *chunk,
                    *local_index,
                );
            }
            TaskKind::StationTick { chunk, local_index } => {
                let Some(world) = state.world.as_mut() else {
                    continue;
                };
                containers::apply_station_tick(
                    world,
                    &mut sched.0,
                    *fired_tick,
                    &mut dirty,
                    &book.0,
                    *chunk,
                    *local_index,
                );
            }
            _ => {}
        }
    }
    state.dirty_tiles = dirty;

    // Шина событий: исполненные задачи уходят слушателям (появятся позже).
    // drain оставляет ёмкость вектора на следующий кадр.
    for (tick, task) in fired.drain(..) {
        events.write(TickEvent { tick, task });
    }

    // Время тика для UI: среднее по исполненным тикам кадра, сглаженное по кадрам.
    let ms_per_tick = started.elapsed().as_secs_f32() * 1000.0 / ticks as f32;
    timing.avg_ms = if timing.avg_ms <= 0.0 {
        ms_per_tick
    } else {
        timing.avg_ms * 0.9 + ms_per_tick * 0.1
    };
}

/// Событие Bevy: задача планировщика исполнена на тике `tick`.
/// Слушатели (поведение существ, эффекты) появятся позже; шина уже работает.
#[allow(dead_code)] // полей нет читателей, пока нет слушателей
#[derive(Message)]
pub struct TickEvent {
    pub tick: u64,
    pub task: Task,
}

/// Применяет команды контроллеров (UI, «бог», игрок) к миру.
///
/// Команды применяются каждый кадр в порядке FIFO — редактор работает и на
/// паузе. Изменённые тайлы уходят в `dirty_tiles` с каналом причины, по ним
/// рендер пересобирает меш блока или только его декор; вокруг них
/// перезапускаются цепочки растительности.
pub(crate) fn apply_commands(
    mut queue: ResMut<CommandQueueRes>,
    mut state: ResMut<GameState>,
    clock: Res<ClockRes>,
    mut sched: ResMut<SchedulerRes>,
    book: Res<CraftBookRes>,
) {
    if queue.0.is_empty() {
        return;
    }
    let mut dirty = std::mem::take(&mut state.dirty_tiles);
    let actor = state.controlled;
    if let Some(world) = state.world.as_mut() {
        // Действия игрока (Player*) исполняет захваченное существо;
        // в наблюдателе actor = None и они пропускаются.
        queue.0.apply_all(
            world,
            actor,
            &mut dirty,
            &mut sched.0,
            clock.0.tick,
            &book.0,
        );
    }
    state.dirty_tiles = dirty;
}

/// Глобальные горячие клавиши: пауза — по бинду из настроек.
///
/// ESC обрабатывается в `pause_menu::handle_escape` как единая цепочка
/// для всех модальных меню. Пока фокус в текстовом поле, идёт захват
/// бинда или открыт пауз-меню (игра уже на паузе), хоткей не срабатывает.
fn handle_global_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<GameState>,
    settings: Res<Settings>,
    capture: Res<BindingCapture>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<TextInput>>,
) {
    let typing = text_input_focused(&focus, &text_inputs);
    let pause_key = settings.controls.key(ControlAction::Pause);
    if keys.just_pressed(pause_key)
        && !typing
        && capture.0.is_none()
        && state.active_menu != Some(MenuKind::Pause)
    {
        state.paused = !state.paused;
    }
}

// --- Окно: фокус, пауза, режимы, FPS-лимит ---

/// Отслеживает фокус и перекрытие окна из событий winit.
fn track_window_state(mut events: MessageReader<WindowEvent>, mut focus: ResMut<WindowFocusState>) {
    for ev in events.read() {
        match ev {
            WindowEvent::WindowFocused(e) => focus.focused = e.focused,
            WindowEvent::WindowOccluded(e) => focus.occluded = e.occluded,
            _ => {}
        }
    }
}

/// Ставит игру на паузу при потере фокуса (настройка «Геймплей»).
///
/// При возврате фокуса пауза снимается только если её поставил именно
/// этот механизм (`FocusPause`): пауза из меню/пробела остаётся.
fn auto_pause_on_focus_loss(
    focus: Res<WindowFocusState>,
    settings: Res<Settings>,
    mut state: ResMut<GameState>,
    mut focus_pause: ResMut<FocusPause>,
    mut prev_focused: Local<bool>,
) {
    if focus.focused == *prev_focused {
        return;
    }
    *prev_focused = focus.focused;
    if focus.focused {
        if focus_pause.active && state.active_menu != Some(MenuKind::Pause) {
            state.paused = false;
        }
        focus_pause.active = false;
    } else if settings.pause_on_focus_loss && !state.paused {
        state.paused = true;
        focus_pause.active = true;
    }
}

/// Применяет режим окна из настроек.
///
/// Идемпотентно: пишем только при расхождении, поэтому систему можно
/// держать включённой всегда (включая стартовый кадр — начальный режим
/// из settings.json применится сам).
fn apply_window_mode(settings: Res<Settings>, mut windows: Query<&mut Window>) {
    let want = match settings.window_mode {
        WindowModeSetting::Windowed => WindowMode::Windowed,
        WindowModeSetting::Borderless => {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        }
        WindowModeSetting::Exclusive => {
            WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current)
        }
    };
    for mut window in &mut windows {
        if window.mode != want {
            window.mode = want;
        }
    }
}

/// Применяет режим сглаживания к обеим камерам сцены.
///
/// Идемпотентно: пишем только при расхождении, поэтому систему можно
/// держать включённой всегда — настройка из settings.json применится
/// на первом кадре. Режимы взаимоисключающие: FXAA и SMAA — постпроцесс-
/// компоненты на камере, MSAA — счётчик сэмплов; постпроцесс требует
/// выключенного MSAA. Смена `Msaa` перестраивает цель рендера на кадр.
fn apply_antialias(
    settings: Res<Settings>,
    mut commands: Commands,
    cameras: Query<
        (Entity, Option<&Msaa>, Has<Fxaa>, Has<Smaa>),
        Or<(With<WorldCamera>, With<OverlayCamera>)>,
    >,
) {
    for (entity, msaa, has_fxaa, has_smaa) in &cameras {
        let (want_msaa, want_fxaa, want_smaa) = match settings.antialias {
            AntiAlias::Off => (Msaa::Off, false, false),
            AntiAlias::Fxaa => (Msaa::Off, true, false),
            AntiAlias::Msaa => (Msaa::Sample4, false, false),
            AntiAlias::Smaa => (Msaa::Off, false, true),
        };
        if msaa != Some(&want_msaa) {
            commands.entity(entity).insert(want_msaa);
        }
        if has_fxaa != want_fxaa {
            if want_fxaa {
                commands.entity(entity).insert(Fxaa::default());
            } else {
                commands.entity(entity).remove::<Fxaa>();
            }
        }
        if has_smaa != want_smaa {
            if want_smaa {
                commands.entity(entity).insert(Smaa::default());
            } else {
                commands.entity(entity).remove::<Smaa>();
            }
        }
    }
}

/// Числовой лимит FPS снимает VSync, «VSync» возвращает его.
/// Иначе на 60-герцовом мониторе лимит 120 никогда не достигался бы,
/// а счётчик FPS показывал бы не то, что реально рисуется.
fn apply_present_mode(settings: Res<Settings>, mut windows: Query<&mut Window>) {
    let want = if settings.fps_limit.target_fps().is_some() {
        PresentMode::AutoNoVsync
    } else {
        PresentMode::AutoVsync
    };
    for mut window in &mut windows {
        if window.present_mode != want {
            window.present_mode = want;
        }
    }
}

/// Режим цикла winit: числовой кап или VSync, плюс 10 FPS в фоне.
///
/// Неактивное (в фоне) и свёрнутое окно теряет фокус — действует
/// `unfocused_mode`: не чаще кадра раз в 100 мс (10 FPS).
///
/// В `Reactive` все реакции на события выключены намеренно: иначе
/// движение мыши и служебные события окна будят цикл чаще таймера,
/// и лимит 15 FPS превращается в 60–150. События при этом не теряются —
/// они копятся и обрабатываются на ближайшем кадре.
///
/// Для неактивного окна нельзя брать `reactive_low_power`: он игнорирует
/// только device-события, но всё ещё просыпается от window-событий.
/// Курсор, наведённый на неактивное окно, генерирует `CursorMoved`,
/// цикл крутится на 120–180 FPS и жарит железо.
fn apply_window_update_mode(settings: Res<Settings>, mut winit: ResMut<WinitSettings>) {
    // 10 FPS: неактивное/свёрнутое окно не рендерит чаще, чем раз в 100 мс.
    let background = UpdateMode::Reactive {
        wait: Duration::from_millis(100),
        react_to_device_events: false,
        react_to_user_events: false,
        react_to_window_events: false,
    };
    let focused = match settings.fps_limit.target_fps() {
        Some(fps) => UpdateMode::Reactive {
            wait: Duration::from_secs_f64(1.0 / fps as f64),
            react_to_device_events: false,
            react_to_user_events: false,
            react_to_window_events: false,
        },
        None => UpdateMode::Continuous,
    };
    // Пишем только при расхождении: лишняя мутация ресурса заставляет
    // winit перечитывать настройки и сбрасывать расписание каждый кадр.
    if winit.focused_mode != focused {
        winit.focused_mode = focused;
    }
    if winit.unfocused_mode != background {
        winit.unfocused_mode = background;
    }
}

/// F11 — переключение в безрамочный полноэкранный и обратно в оконный.
///
/// Режим пишется прямо в настройки: `apply_window_mode` применит его,
/// а файл обновится при закрытии меню/выходе. F11 не входит в
/// перебиндиваемые клавиши, но во время захвата бинда не срабатывает,
/// чтобы случайное нажатие не переключало окно.
fn toggle_borderless_f11(keyboard: Res<ButtonInput<KeyCode>>, mut settings: ResMut<Settings>, capture: Res<BindingCapture>) {
    if !keyboard.just_pressed(KeyCode::F11) || capture.0.is_some() {
        return;
    }
    settings.window_mode = match settings.window_mode {
        WindowModeSetting::Borderless => WindowModeSetting::Windowed,
        _ => WindowModeSetting::Borderless,
    };
}

/// Настройки пишутся на диск один раз — при выходе из игры. Пока игра
/// работает, файл перезаписывает закрытие пауз-меню (см. pause_menu).
fn save_settings_on_exit(mut events: MessageReader<AppExit>, settings: Res<Settings>) {
    if events.read().next().is_some() {
        settings.save(&settings_path());
    }
}
