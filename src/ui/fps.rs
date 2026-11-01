//! FPS-счётчик, игровая дата и температура в левом верхнем углу,
//! обновляются раз в 0.5 секунды (GAME_DESIGN 9). F3 переключает
//! расширенный режим: добавляются время тика и метрики рендера
//! (ARCHITECTURE 9), чтобы не занимать экран в обычной игре.

use bevy::input_focus::InputFocus;
use bevy::prelude::*;

use crate::app::state::{ClockRes, GameState, SchedulerRes, TickTiming};
use crate::rendering::RenderMetrics;
use crate::ui::{style, text_input_focused};
use crate::ui::UiFont;

pub struct FpsPlugin;

impl Plugin for FpsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FpsCounter::new())
            .add_systems(Startup, spawn_fps_text)
            .add_systems(Update, update_fps);
    }
}

#[derive(Resource)]
struct FpsCounter {
    frames: u32,
    timer: Timer,
    current_fps: f32,
}

impl FpsCounter {
    fn new() -> Self {
        Self {
            frames: 0,
            timer: Timer::from_seconds(0.5, TimerMode::Repeating),
            current_fps: 0.0,
        }
    }
}

#[derive(Component)]
struct FpsText;

/// Создаёт текстовый узел FPS в верхнем левом углу.
fn spawn_fps_text(mut commands: Commands, font: Res<UiFont>) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            left: Val::Px(8.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(style::background()),
        children![(
            Text::new("FPS: --"),
            TextFont {
                font: font.0.clone().into(),
                font_size: 14.0.into(),
                ..default()
            },
            TextColor(style::text()),
            FpsText,
        )],
    ));
}

/// Считает кадры и раз в 0.5 с обновляет текст. Обычный режим: FPS, дата,
/// температура. Расширенный (F3): плюс время тика и метрики рендера —
/// dirty-события по каналам, перестройки блоков, время сборки мешей,
/// видимые блоки и спрайты.
fn update_fps(
    time: Res<Time>,
    mut fps: ResMut<FpsCounter>,
    clock: Res<ClockRes>,
    timing: Res<TickTiming>,
    state: Res<GameState>,
    sched: Res<SchedulerRes>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    text_inputs: Query<(), With<bevy::ui_widgets::TextInput>>,
    mut metrics: ResMut<RenderMetrics>,
    mut extended: Local<bool>,
    mut text: Query<&mut Text, With<FpsText>>,
) {
    // F3 переключает режим; в текстовых полях не срабатывает (как хоткеи).
    if keys.just_pressed(KeyCode::F3) && !text_input_focused(&focus, &text_inputs) {
        *extended = !*extended;
    }

    fps.frames = fps.frames.saturating_add(1);
    if fps.timer.tick(time.delta()).just_finished() {
        let secs = fps.timer.duration().as_secs_f32();
        fps.current_fps = fps.frames as f32 / secs;
        fps.frames = 0;

        let clock = &clock.0;
        // Температура в месте игрока: пока это общая заглушка мира
        // (ambient_temperature_c); с погодой значение станет локальным.
        let temperature = state
            .world
            .as_ref()
            .map(|world| format!("{:.1} °C", world.ambient_temperature_c))
            .unwrap_or_else(|| "--".to_string());
        // Календарные величины отображаются с единицы: в симуляции они с нуля.
        let mut panel = format!(
            "FPS: {:.1}\nГод {}, {}, день {}, {:02}:{:02}\nТемпература: {temperature}",
            fps.current_fps,
            clock.year() + 1,
            clock.season().label(),
            clock.day_of_month() + 1,
            clock.hour_of_day(),
            clock.minute_of_hour(),
        );
        if *extended {
            // Время тика показываем только после первого исполненного тика.
            if timing.avg_ms > 0.0 {
                panel.push_str(&format!("\nТик: {:.2} мс", timing.avg_ms));
            }
            // Оценка ОЗУ (мир + планировщик) и бюджет на тайл — основной
            // показатель больших карт (ARCHITECTURE 3.1).
            if let Some(world) = state.world.as_ref() {
                let bytes = (world.memory_bytes() + sched.0.memory_bytes()) as f32;
                let tiles = (world.size.tiles() as u64).max(1).pow(2) as f32;
                panel.push_str(&format!(
                    "\nМир: {:.1} МБ (~{:.2} Б/тайл)",
                    bytes / (1024.0 * 1024.0),
                    bytes / tiles,
                ));
            }
            // Накопители рендера пересчитываются в «за секунду» (панель
            // ходит каждые 0.5 с).
            panel.push_str(&format!(
                "\nБлоки: {}, спрайтов: {}\nDirty: рельеф {}/с, декор {}/с, перестроек {}/с, меши {:.1} мс/с",
                metrics.visible_blocks,
                metrics.block_sprites,
                (metrics.terrain_events as f32 / secs).round() as u32,
                (metrics.decor_events as f32 / secs).round() as u32,
                (metrics.rebuilds as f32 / secs).round() as u32,
                metrics.mesh_build_ms / secs,
            ));
        }
        for mut t in &mut text {
            **t = panel.clone();
        }
        // Накопители сбрасываются каждый период независимо от режима:
        // иначе при включении F3 накопленное с прошлого раза соврёт.
        metrics.terrain_events = 0;
        metrics.decor_events = 0;
        metrics.rebuilds = 0;
        metrics.mesh_build_ms = 0.0;
    }
}
