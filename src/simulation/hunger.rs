//! Голод как общая потребность существа (GAME_DESIGN 3.5).
//!
//! Система не привязана к игроку: любое существо хранит `Hunger` в своих
//! данных и на каждом своём тике вызывает `tick` с текущей активностью,
//! применяя возвращённый урон к своему здоровью. Тик O(1) на существо —
//! планировщик не нужен; lazy-механика (`needs.rs`) подключится, когда
//! существ станет много.

/// Максимум шкалы голода (сытости): 100 — сыт, 0 — голодает.
pub const HUNGER_MAX: f32 = 100.0;

/// Расход сытости стоя на месте, единиц за тик.
const DRAIN_IDLE: f32 = 0.0005;
/// Расход сытости при ходьбе, единиц за тик.
const DRAIN_WALK: f32 = 0.004;
/// Расход сытости вплавь, единиц за тик.
const DRAIN_SWIM: f32 = 0.006;

/// Порог голода: ниже него существо получает урон.
const STARVE_HUNGER_THRESHOLD: f32 = 5.0;
/// Урон от голода за один голодный интервал.
const STARVE_DAMAGE: f32 = 1.0;
/// Длина голодного интервала: урон раз в столько тиков при голоде.
const STARVE_DAMAGE_INTERVAL_TICKS: u32 = 50;

/// Кулдаун поедания еды в тиках: следующая еда не раньше этого смещения.
pub const EAT_COOLDOWN_TICKS: u64 = 50;

/// Интервал пассивной регенерации, тиков: сытый организм раз в столько
/// тиков тратит сытость на восстановление здоровья.
const REGEN_INTERVAL_TICKS: u32 = 200;
/// Порог сытости для регенерации: восстанавливаться может только сытость
/// выше него.
const REGEN_HUNGER_THRESHOLD: f32 = 80.0;
/// Цена 1.0 HP в единицах сытости.
const REGEN_HUNGER_COST: f32 = 2.0;

/// Активность существа за тик — определяет расход сытости.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Activity {
    /// Стоит на месте.
    Idle,
    /// Движется по суше.
    Walking,
    /// Плывёт (тайл воды под ногами).
    Swimming,
}

/// Расход сытости за тик по активности.
fn drain_per_tick(activity: Activity) -> f32 {
    match activity {
        Activity::Idle => DRAIN_IDLE,
        Activity::Walking => DRAIN_WALK,
        Activity::Swimming => DRAIN_SWIM,
    }
}

/// Состояние голода одного существа: текущая сытость, счётчик тиков
/// непрерывного голода с последнего урона и счётчик до следующей
/// пассивной регенерации. Копия — потребности простые.
#[derive(Clone, Copy, Debug)]
pub struct Hunger {
    /// Текущая сытость, всегда в [0, HUNGER_MAX].
    value: f32,
    /// Тики непрерывного голода (ниже порога) с последнего урона.
    starve_ticks: u32,
    /// Тики до следующей попытки регенерации. Transient: в сейв не
    /// пишется (после загрузки отсчёт начинается заново — погрешность
    /// в несколько секунд несущественна).
    regen_ticks: u32,
}

/// Равенство — только по сытости: счётчики тиков transient (в сейв не
/// пишутся), сравнивать roundtrip сохранения по ним нельзя.
impl PartialEq for Hunger {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Hunger {
    /// Новый сытый голод.
    pub fn new() -> Self {
        Self { value: HUNGER_MAX, starve_ticks: 0, regen_ticks: 0 }
    }

    /// Голод с заданным начальным значением (восстановление сохранения).
    /// Значение зажимается в допустимый диапазон.
    pub fn with_value(value: f32) -> Self {
        Self { value: value.clamp(0.0, HUNGER_MAX), starve_ticks: 0, regen_ticks: 0 }
    }

    /// Текущая сытость.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Тик потребности: списывает сытость по активности. Возвращает урон
    /// от голода за этот тик (0.0, если сытость выше порога или голодный
    /// интервал ещё не истёк); вызывающий применяет его к своему здоровью.
    pub fn tick(&mut self, activity: Activity) -> f32 {
        self.value = (self.value - drain_per_tick(activity)).max(0.0);
        if self.value < STARVE_HUNGER_THRESHOLD {
            self.starve_ticks += 1;
            if self.starve_ticks >= STARVE_DAMAGE_INTERVAL_TICKS {
                self.starve_ticks = 0;
                return STARVE_DAMAGE;
            }
        } else {
            self.starve_ticks = 0;
        }
        0.0
    }

    /// Восстанавливает сытость едой; излишек сверх максимума пропадает.
    /// Счётчик урона сбросится сам на ближайшем тике выше порога.
    pub fn eat(&mut self, food_value: f32) {
        self.value = (self.value + food_value).min(HUNGER_MAX);
    }

    /// Пассивная регенерация сытого организма: при сытости выше порога и
    /// неполном здоровье раз в `REGEN_INTERVAL_TICKS` тратит
    /// `REGEN_HUNGER_COST` сытости на 1.0 HP (полное HP — скип).
    /// Вызывается раз за тик существа тем же кодом, что и `tick`.
    /// Возвращает `true`, если восстановил.
    pub fn regen(&mut self, hp: &mut f32, max_hp: f32) -> bool {
        self.regen_ticks += 1;
        if self.regen_ticks < REGEN_INTERVAL_TICKS {
            return false;
        }
        self.regen_ticks = 0;
        if self.value <= REGEN_HUNGER_THRESHOLD || *hp >= max_hp {
            return false;
        }
        self.value = (self.value - REGEN_HUNGER_COST).max(0.0);
        *hp = (*hp + 1.0).min(max_hp);
        true
    }
}
