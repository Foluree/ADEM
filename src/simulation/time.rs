//! Календарь и часы игры.
//! Все константы времени собраны здесь, чтобы балансировать в одном месте.

/// Тиков симуляции в реальную секунду (скорость x1).
pub const TICKS_PER_SEC: u32 = 20;
/// Игровых секунд в одном тике (1 тик = 12 игровых секунд, GAME_DESIGN 3.3).
pub const GAME_SECS_PER_TICK: u32 = 12;

pub const SECS_PER_MIN: u32 = 60;
pub const MINS_PER_HOUR: u32 = 60;
pub const HOURS_PER_DAY: u32 = 24;
/// Длина сезона в месяцах (4 сезона по 3 месяца).
pub const SEASON_MONTHS: u32 = 3;
pub const DAYS_PER_MONTH: u32 = 15;
pub const MONTHS_PER_YEAR: u32 = 12;

pub const SECS_PER_HOUR: u64 = (SECS_PER_MIN * MINS_PER_HOUR) as u64;
pub const SECS_PER_DAY: u64 = HOURS_PER_DAY as u64 * SECS_PER_HOUR;
pub const SECS_PER_MONTH: u64 = SECS_PER_DAY * DAYS_PER_MONTH as u64;
pub const SECS_PER_YEAR: u64 = SECS_PER_MONTH * MONTHS_PER_YEAR as u64;
pub const TICKS_PER_DAY: u64 = SECS_PER_DAY / GAME_SECS_PER_TICK as u64;
pub const TICKS_PER_MONTH: u64 = TICKS_PER_DAY * DAYS_PER_MONTH as u64;
#[allow(dead_code)] // используется в тестах и при балансировке
pub const TICKS_PER_YEAR: u64 = TICKS_PER_MONTH * MONTHS_PER_YEAR as u64;

/// Сезон года.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    /// Сезон по номеру месяца года (0..11): весна — месяцы 1–3 и т. д.
    pub fn from_month(month: u32) -> Self {
        match (month / SEASON_MONTHS) % 4 {
            0 => Season::Spring,
            1 => Season::Summer,
            2 => Season::Autumn,
            _ => Season::Winter,
        }
    }

    /// Название сезона для UI.
    pub fn label(self) -> &'static str {
        match self {
            Season::Spring => "Весна",
            Season::Summer => "Лето",
            Season::Autumn => "Осень",
            Season::Winter => "Зима",
        }
    }
}

/// Игровые часы: счётчик тиков и производные календарные величины.
/// Календарь вычисляется из тика по запросу — хранить нужно одно число.
#[derive(Clone, Copy, Debug, Default)]
pub struct GameClock {
    /// Номер текущего тика с начала мира.
    pub tick: u64,
}

impl GameClock {
    /// Продвигает часы на один тик.
    pub fn tick_once(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    /// Игровых секунд с начала мира.
    pub fn game_seconds(&self) -> u64 {
        self.tick * GAME_SECS_PER_TICK as u64
    }

    /// Номер суток с начала мира.
    pub fn day(&self) -> u64 {
        self.game_seconds() / SECS_PER_DAY
    }

    /// День месяца (0..DAYS_PER_MONTH).
    pub fn day_of_month(&self) -> u32 {
        (self.day() % DAYS_PER_MONTH as u64) as u32
    }

    /// Номер месяца с начала мира.
    pub fn month(&self) -> u64 {
        self.game_seconds() / SECS_PER_MONTH
    }

    /// Месяц года (0..11).
    pub fn month_of_year(&self) -> u32 {
        (self.month() % MONTHS_PER_YEAR as u64) as u32
    }

    /// Номер года с начала мира.
    pub fn year(&self) -> u64 {
        self.game_seconds() / SECS_PER_YEAR
    }

    /// Текущий сезон.
    pub fn season(&self) -> Season {
        Season::from_month(self.month_of_year())
    }

    /// Доля суток [0, 1). Используется освещением (rendering/lighting).
    pub fn time_of_day_fraction(&self) -> f32 {
        (self.game_seconds() % SECS_PER_DAY) as f32 / SECS_PER_DAY as f32
    }

    /// Час суток (0..23) для UI.
    pub fn hour_of_day(&self) -> u32 {
        ((self.game_seconds() % SECS_PER_DAY) / SECS_PER_HOUR) as u32
    }

    /// Минута часа (0..59) для UI.
    pub fn minute_of_hour(&self) -> u32 {
        ((self.game_seconds() % SECS_PER_HOUR) / SECS_PER_MIN as u64) as u32
    }
}

/// Час суток, с которого начинается новый мир: утро 8:00.
pub const START_HOUR_OF_DAY: u32 = 8;

/// Стартовый тик мира: `START_HOUR_OF_DAY`:00 первых суток. Часы, события
/// календаря и посев растительности стартуют от него, а не от нуля.
pub fn start_tick() -> u64 {
    START_HOUR_OF_DAY as u64 * SECS_PER_HOUR / GAME_SECS_PER_TICK as u64
}

/// Номер месяца с начала мира по тику (для задач планировщика, у которых
/// нет доступа к часам): тот же расчёт, что у `GameClock::month`.
pub fn month_of_tick(tick: u64) -> u32 {
    (tick * GAME_SECS_PER_TICK as u64 / SECS_PER_MONTH) as u32
}

/// Тик следующей границы часа строго после `after_tick`
/// (момент события `TimeOfDayChanged`).
pub fn next_hour_tick(after_tick: u64) -> u64 {
    let secs = after_tick * GAME_SECS_PER_TICK as u64;
    (secs / SECS_PER_HOUR + 1) * SECS_PER_HOUR / GAME_SECS_PER_TICK as u64
}

/// Тик начала следующего сезона строго после `after_tick`
/// (момент события `SeasonChanged`).
pub fn next_season_tick(after_tick: u64) -> u64 {
    let secs = after_tick * GAME_SECS_PER_TICK as u64;
    let season_secs = SEASON_MONTHS as u64 * SECS_PER_MONTH;
    (secs / season_secs + 1) * season_secs / GAME_SECS_PER_TICK as u64
}
