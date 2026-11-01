//! Тесты инфраструктуры симуляции: календарь, планировщик, фиксированный
//! шаг тика, скорости симуляции и lazy-потребности.

/// Часы: тики → игровые секунды → календарные величины корректны.
#[test]
fn clock_calendar_transitions() {
    use crate::simulation::time::*;

    let mut c = GameClock::default();
    assert_eq!(c.year(), 0);
    assert_eq!(c.month_of_year(), 0);
    assert_eq!(c.season(), Season::Spring);
    // Перескок на 1 год + 4 месяца вперёд.
    c.tick = TICKS_PER_YEAR + 4 * TICKS_PER_MONTH;
    assert_eq!(c.year(), 1);
    assert_eq!(c.month_of_year(), 4);
    assert_eq!(c.season(), Season::Summer);
    // Переход через сутки: день сменился, время суток обнулилось.
    c.tick = TICKS_PER_DAY;
    assert_eq!(c.day(), 1);
    assert_eq!(c.day_of_month(), 1);
    assert!(c.time_of_day_fraction() < 1e-6);
    // Половина суток — 12 часов ровно.
    c.tick = TICKS_PER_DAY / 2;
    assert_eq!(c.hour_of_day(), 12);
    assert_eq!(c.minute_of_hour(), 0);
}

/// Границы часа и сезона: следующий тик строго больше текущего и совпадает с календарной сеткой.
#[test]
fn calendar_next_boundaries() {
    use crate::simulation::time::*;

    // Час = 300 тиков: с середины часа — до ближайшей границы.
    assert_eq!(next_hour_tick(0), 300);
    assert_eq!(next_hour_tick(299), 300);
    assert_eq!(next_hour_tick(300), 600);
    // Сезон = 3 месяца = 324 000 тиков.
    assert_eq!(next_season_tick(0), TICKS_PER_MONTH * 3);
    assert_eq!(
        next_season_tick(TICKS_PER_MONTH * 3),
        TICKS_PER_MONTH * 6
    );
}

/// Планировщик: задачи одного тика — по id, отмена работает, дальняя задача
/// доезжает ровно на своём тике.
#[test]
fn scheduler_orders_cancels_and_migrates() {
    use crate::simulation::scheduler::{Scheduler, TaskKind};

    let mut s = Scheduler::default();
    s.schedule_at(10, TaskKind::Marker(1));
    s.schedule_at(10, TaskKind::Marker(2));
    s.schedule_at(1_000, TaskKind::Marker(3));
    let cancelled = s.schedule_at(1_000, TaskKind::Marker(4));
    s.cancel(cancelled);

    let mut got = Vec::new();
    for t in 0..=1_000 {
        s.run_tick(t, |task| got.push(task.kind));
    }
    // Отменённая задача не исполнилась, порядок — по id регистрации.
    assert_eq!(
        got,
        vec![TaskKind::Marker(1), TaskKind::Marker(2), TaskKind::Marker(3)]
    );
}

/// Задача, назначенная «в прошлое», исполняется на ближайшем следующем тике и ровно один раз.
#[test]
fn scheduler_past_task_runs_next_tick_once() {
    use crate::simulation::scheduler::{Scheduler, TaskKind};

    let mut s = Scheduler::default();
    s.schedule_at(0, TaskKind::Marker(9));

    let mut got = Vec::new();
    s.run_tick(1, |task| got.push(task.kind));
    assert_eq!(got, vec![TaskKind::Marker(9)]);

    let mut again = Vec::new();
    s.run_tick(2, |task| again.push(task.kind));
    assert!(again.is_empty());
}

/// При перегенерации задачи старого мира удаляются, а календарные и прочие
/// независимые задачи, включая их состояние отмены, сохраняются.
#[test]
fn scheduler_clears_world_tasks_without_dropping_global_tasks() {
    use crate::simulation::scheduler::{Scheduler, TaskKind};
    use crate::simulation::world::ChunkPos;

    let mut scheduler = Scheduler::default();
    scheduler.schedule_at(
        3,
        TaskKind::GrassSpread {
            chunk: ChunkPos { x: 20, y: 20 },
            local_index: 0,
        },
    );
    let cancelled_calendar = scheduler.schedule_at(3, TaskKind::TimeOfDayChanged);
    scheduler.cancel(cancelled_calendar);
    scheduler.schedule_at(4, TaskKind::TimeOfDayChanged);
    scheduler.schedule_at(
        100,
        TaskKind::WaterErosion {
            chunk: ChunkPos { x: 30, y: 30 },
            local_index: 0,
        },
    );
    scheduler.schedule_at(100, TaskKind::SeasonChanged);
    scheduler.schedule_at(100, TaskKind::Marker(7));

    scheduler.clear_world_tasks();

    let mut fired = Vec::new();
    scheduler.run_tick(100, |task| fired.push(task.kind));
    assert_eq!(
        fired,
        vec![
            TaskKind::TimeOfDayChanged,
            TaskKind::SeasonChanged,
            TaskKind::Marker(7)
        ]
    );
}

/// Фикс-степ: лимит тиков на кадр, перенос остатка, сброс при исчерпании бюджета.
#[test]
fn fixed_step_limits_ticks_per_frame() {
    use crate::app::plugin::step_ticks;

    // Ровно один шаг — один тик без остатка.
    assert_eq!(step_ticks(0.0, 1.0, 0.05, 4), (1, 0.0));
    // Секунда при x1: бюджет 4 исчерпан, остаток отброшен (мир замедляется, не виснет).
    assert_eq!(step_ticks(0.0, 1.0, 1.0, 4), (4, 0.0));
    // Остаток меньше шага переносится на следующий кадр.
    let (n, acc) = step_ticks(0.0, 1.0, 0.07, 4);
    assert_eq!(n, 1);
    assert!((acc - 0.02).abs() < 1e-6);
    // Три полных шага из 0.15 с; хвост на грани погрешности f32 не даёт лишнего тика.
    let (n, acc) = step_ticks(0.0, 1.0, 0.15, 4);
    assert_eq!(n, 3);
    assert!(acc < 1e-5);
    // Ускорение x5: 0.1 с превращаются в полсекунды симуляции — бюджет исчерпан.
    assert_eq!(step_ticks(0.0, 5.0, 0.1, 4), (4, 0.0));
}

/// Все запрошенные скорости доступны в меню и имеют корректные подписи.
#[test]
fn simulation_speed_options_include_high_speeds() {
    use crate::app::state::SimSpeed;

    let high_speeds = &SimSpeed::all()[6..];
    assert_eq!(
        high_speeds
            .iter()
            .map(|speed| speed.label())
            .collect::<Vec<_>>(),
        [
            "x7.5", "x10", "x12.5", "x15", "x20", "x25", "x30", "x35", "x40", "x45", "x50",
            "x60", "x70", "x80", "x90", "x100"
        ]
    );
    assert_eq!(
        high_speeds
            .iter()
            .map(|speed| speed.multiplier())
            .collect::<Vec<_>>(),
        [
            7.5, 10.0, 12.5, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 60.0, 70.0, 80.0,
            90.0, 100.0
        ]
    );
}

/// Lazy-потребность: значение восстанавливается по запросу, закрывается
/// перед сменой скорости, порог достижим ровно через расчётное число тиков.
#[test]
fn lazy_need_accumulates_on_demand() {
    use crate::simulation::needs::Need;

    let mut hunger = Need::new(100.0, -0.5);
    assert_eq!(hunger.value_at(10), 95.0);

    // После close накопленное зафиксировано; новая скорость считается от него.
    hunger.close(10);
    hunger.set_rate(10, -1.0);
    assert_eq!(hunger.value_at(20), 85.0);

    // Порог считается от значения на тике последнего close (95 на тике 10):
    // до 80 — 15 тиков. После close на тике 20 (значение 85) — уже 5.
    assert_eq!(hunger.ticks_until(80.0), Some(15));
    hunger.close(20);
    assert_eq!(hunger.ticks_until(80.0), Some(5));

    // Нулевая скорость и порог в недостижимую сторону событий не дают.
    assert_eq!(Need::new(50.0, 0.0).ticks_until(30.0), None);
    assert_eq!(Need::new(50.0, 1.0).ticks_until(30.0), None);
}
