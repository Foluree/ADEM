//! Сохранение и загрузка игры в файлы слотов `saves/save_N.adem`.
//!
//! IO живёт здесь (app-слой); сериализация — в `simulation/save.rs`
//! без зависимости от Bevy. UI шлёт запросы-сообщения со слотом, системы
//! ниже выполняют их и обновляют экран слотов.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::app::settings::Settings;
use crate::app::state::{ClockRes, GameState, SchedulerRes, TickAccumulator};
use crate::simulation::save::{self, SaveHeader};
use crate::simulation::GameClock;
use crate::ui::pause_menu::{
    PauseMenuRoot, PauseMenuView, SaveStatusText, SlotLabel, close_menu, set_status_text,
    slot_label,
};

/// Число слотов сохранения (решение автора: save_1..save_4).
pub const SAVE_SLOTS: u8 = 4;

/// Запрос UI: записать текущую симуляцию в слот (нумерация с 1).
#[derive(Message)]
pub struct SaveGameRequest(pub u8);

/// Запрос UI: загрузить симуляцию из слота (нумерация с 1).
#[derive(Message)]
pub struct LoadGameRequest(pub u8);

/// Каталог сохранений: рабочий каталог при `cargo run` (там уже лежит
/// папка `saves`), иначе рядом с exe.
fn saves_dir() -> PathBuf {
    let cwd = PathBuf::from("saves");
    if cwd.is_dir() {
        return cwd;
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("saves")
}

/// Путь к файлу слота (слоты нумеруются с 1).
fn slot_path(slot: u8) -> PathBuf {
    saves_dir().join(format!("save_{slot}.adem"))
}

/// Заголовок слота без чтения тела файла — для подписей кнопок.
/// None — слота нет или файл не читается (в списке он выглядит пустым).
pub fn slot_info(slot: u8) -> Option<SaveHeader> {
    let mut file = fs::File::open(slot_path(slot)).ok()?;
    // Заголовок — несколько десятков байт; читаем префикс с запасом.
    let mut prefix = vec![0_u8; 256];
    let len = file.read(&mut prefix).ok()?;
    prefix.truncate(len);
    save::read_header(&prefix).ok()
}

pub struct SavesPlugin;

impl Plugin for SavesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveGameRequest>()
            .add_message::<LoadGameRequest>()
            .add_systems(Update, (handle_save_request, handle_load_request));
    }
}

/// Кладёт сохранение текущей симуляции в слот; обновляет статус и подпись
/// слота на экране (пересборка не нужна: подписи обновляются на месте).
///
/// Два запроса `Text` не пересекаются: статус-строка без `SlotLabel`,
/// подписи слотов — с ним.
fn handle_save_request(
    mut requests: MessageReader<SaveGameRequest>,
    state: Res<GameState>,
    clock: Res<ClockRes>,
    sched: Res<SchedulerRes>,
    status: Query<Entity, With<SaveStatusText>>,
    mut texts: Query<&mut Text, Without<SlotLabel>>,
    mut slot_labels: Query<(&SlotLabel, &mut Text)>,
) {
    for request in requests.read() {
        let slot = request.0.clamp(1, SAVE_SLOTS);
        let Some(world) = &state.world else {
            set_status_text(&status, &mut texts, "Мир ещё не создан");
            continue;
        };
        let bytes =
            save::save_game(world, &clock.0, &sched.0, crate::app::GAME_VERSION);
        let path = slot_path(slot);
        let result = fs::create_dir_all(saves_dir()).and_then(|_| fs::write(&path, &bytes));
        match result {
            Ok(_) => {
                set_status_text(
                    &status,
                    &mut texts,
                    &format!("Игра сохранена — слот {slot}"),
                );
                let info = slot_info(slot);
                for (label, mut text) in &mut slot_labels {
                    if label.0 == slot {
                        **text = slot_label(slot, info.as_ref());
                    }
                }
            }
            Err(e) => {
                set_status_text(&status, &mut texts, &format!("Ошибка сохранения: {e}"));
            }
        }
    }
}

/// Загружает симуляцию из слота: подменяет мир, часы и планировщик,
/// закрывает меню и снимает паузу. Скорость и пауза — выбор игрока,
/// из файла не читаются.
fn handle_load_request(
    mut requests: MessageReader<LoadGameRequest>,
    mut commands: Commands,
    mut state: ResMut<GameState>,
    mut clock: ResMut<ClockRes>,
    mut sched: ResMut<SchedulerRes>,
    mut acc: ResMut<TickAccumulator>,
    settings: Res<Settings>,
    status: Query<Entity, With<SaveStatusText>>,
    mut texts: Query<&mut Text, Without<SlotLabel>>,
    roots: Query<Entity, With<PauseMenuRoot>>,
    mut view: ResMut<PauseMenuView>,
) {
    for request in requests.read() {
        let slot = request.0.clamp(1, SAVE_SLOTS);
        let result = fs::read(slot_path(slot))
            .map_err(|e| format!("слот {slot} недоступен: {e}"))
            .and_then(|bytes| save::load_game(&bytes).map_err(|e| format!("слот {slot}: {e}")));
        let loaded = match result {
            Ok(saved) => saved,
            Err(message) => {
                set_status_text(
                    &status,
                    &mut texts,
                    &format!("Не удалось загрузить: {message}"),
                );
                continue;
            }
        };

        let size = loaded.world.size;
        state.world = Some(loaded.world);
        state.world_size = size;
        state.seed = loaded.seed;
        clock.0 = GameClock { tick: loaded.tick };
        sched.0 = loaded.scheduler;
        // Аккумулятор старого мира не переносится: тики начинаются с файла.
        acc.seconds = 0.0;
        state.pending_rerender = true;
        // Захваченное существо — часть мира, но после загрузки контроллер
        // теряется: id могли переоткрыться в другом мире. Управление всегда
        // начинается с наблюдателя — существо можно захватить заново по F1.
        state.set_controlled(None);
        close_menu(&mut commands, &roots, &mut state, &mut view, &settings);
    }
}
