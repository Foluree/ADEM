//! Центральная очередь запланированных событий по тикам.
//! Внутри — кольцевой буфер на ближнее окно и BinaryHeap на дальнее
//! (ARCHITECTURE 2.5). Задачи дальних событий переезжают в буфер,
//! когда входят в окно; отменённые id пропускаются при исполнении.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use super::needs::NeedKind;
use super::world::ChunkPos;

/// Ширина ближнего окна в тиках. Больше — реже переносы из кучи, больше ОЗУ.
const NEAR_WINDOW: u64 = 64;

/// Идентификатор задачи. Выдаётся последовательно и задаёт порядок
/// исполнения задач одного тика (детерминизм, ARCHITECTURE 2.8).
pub type TaskId = u64;

/// Задача планировщика: момент исполнения определяется позицией в очереди.
/// Ord нужен только куче дальних задач: там сравнивается кортеж (тик, id, задача),
/// и до сравнения kind дело не доходит — пара (тик, id) уникальна.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Task {
    pub id: TaskId,
    pub kind: TaskKind,
}

/// Вид задачи: встроенные процессы мира. Реестр пользовательских
/// процессов появится позже. Неиспользуемые пока варианты — задел
/// по ROADMAP (рост объектов, потребности существ).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskKind {
    /// Смена часа игрового времени (24 раза за игровые сутки).
    TimeOfDayChanged,
    /// Смена сезона (раз в SEASON_MONTHS месяцев).
    SeasonChanged,
    /// Рост объекта: чанк и линейный индекс тайла в чанке.
    ObjectGrowth { chunk: ChunkPos, local_index: u8 },
    /// Расползание травы: тайл травы пробует занять соседнюю землю.
    GrassSpread { chunk: ChunkPos, local_index: u8 },
    /// Вырастить куст в лесу чанка (с пределом кустов на чанк).
    BushGrowth { chunk: ChunkPos },
    /// Эрозия: тайл воды подтачивает эродируемых соседей.
    WaterErosion { chunk: ChunkPos, local_index: u8 },
    /// Стадия культуры на пашне: чанк и линейный индекс тайла в чанке.
    CropGrowth { chunk: ChunkPos, local_index: u8 },
    /// Рабочий цикл станции (печь, плавильня, костёр): событие готовки
    /// или прогорания топлива.
    StationTick { chunk: ChunkPos, local_index: u8 },
    /// Проверка пустой пашни (месяц без культуры — возврат в землю).
    FarmlandRevert { chunk: ChunkPos, local_index: u8 },
    /// Порог потребности достигнут; owner — id существа (появится вместе с существами).
    NeedThreshold { owner: u64, need: NeedKind },
    /// Маркер для тестов планировщика.
    Marker(u32),
}

/// Планировщик: задачи ближайших тиков лежат в кольцевом буфере, дальние — в куче. Исполнение строго по тикам.
pub struct Scheduler {
    /// Тик, до которого включительно задачи уже исполнены.
    current_tick: u64,
    /// Ближнее окно: бакет `tick % NEAR_WINDOW` содержит задачи этого тика.
    near: Vec<Vec<Task>>,
    /// Дальние задачи (за пределами окна), упорядочены по (тик, id).
    far: BinaryHeap<Reverse<(u64, TaskId, Task)>>,
    /// Следующий неиспользованный id.
    next_id: TaskId,
    /// Отменённые задачи: id пропускается при исполнении и удаляется из набора.
    /// Набор ограничен числом неисполненных отмен, отдельная очистка не нужна:
    /// полная очистка вернула бы к исполнению ещё не дошедшие отмены.
    cancelled: HashSet<TaskId>,
}

impl Default for Scheduler {
    /// Пустой планировщик, старт с тика 0.
    fn default() -> Self {
        Self {
            current_tick: 0,
            near: vec![Vec::new(); NEAR_WINDOW as usize],
            far: BinaryHeap::new(),
            next_id: 1,
            cancelled: HashSet::new(),
        }
    }
}

impl Scheduler {
    /// Оценка расхода ОЗУ планировщиком в байтах (для F3-панели): бакеты
    /// ближнего окна с их ёмкостью, дальняя куча, набор отменённых.
    /// Приближение без учёта аллокатора: ёмкости × размер записи.
    pub fn memory_bytes(&self) -> u64 {
        let near: u64 = self
            .near
            .iter()
            .map(|bucket| (bucket.capacity() as u64) * std::mem::size_of::<Task>() as u64)
            .sum();
        let far = (self.far.len() as u64) * std::mem::size_of::<(u64, TaskId, Task)>() as u64;
        // HashSet: бакет = ключ + байт управления, загрузка ≤ 7/8.
        let cancelled = (self.cancelled.len() as u64)
            * (std::mem::size_of::<TaskId>() as u64 + 2) * 8 / 7;
        near + far + cancelled
    }

    /// Удаляет все задачи, привязанные к содержимому мира. Календарные
    /// события и прочие независимые задачи сохраняются при перегенерации.
    pub fn clear_world_tasks(&mut self) {
        fn is_world_task(kind: &TaskKind) -> bool {
            matches!(
                kind,
                TaskKind::ObjectGrowth { .. }
                    | TaskKind::GrassSpread { .. }
                    | TaskKind::BushGrowth { .. }
                    | TaskKind::WaterErosion { .. }
                    | TaskKind::CropGrowth { .. }
                    | TaskKind::StationTick { .. }
                    | TaskKind::FarmlandRevert { .. }
                    | TaskKind::NeedThreshold { .. }
            )
        }

        for bucket in &mut self.near {
            bucket.retain(|task| !is_world_task(&task.kind));
        }
        self.far
            .retain(|Reverse((_, _, task))| !is_world_task(&task.kind));

        let retained_ids: HashSet<_> = self
            .near
            .iter()
            .flatten()
            .map(|task| task.id)
            .chain(self.far.iter().map(|Reverse((_, id, _))| *id))
            .collect();
        self.cancelled.retain(|id| retained_ids.contains(id));
    }

    /// Ставит задачу на конкретный тик и возвращает id для отмены.
    /// Задача «в прошлом» исполняется на ближайшем следующем тике.
    pub fn schedule_at(&mut self, tick: u64, kind: TaskKind) -> TaskId {
        let id = self.next_id;
        self.next_id += 1;
        let tick = tick.max(self.current_tick + 1);
        let task = Task { id, kind };
        if tick - self.current_tick < NEAR_WINDOW {
            self.near[(tick % NEAR_WINDOW) as usize].push(task);
        } else {
            self.far.push(Reverse((tick, id, task)));
        }
        id
    }

    /// Отменяет задачу по id, если она ещё не исполнена.
    /// Потребители отмены — потребности и рост объектов; API покрыт тестами.
    #[allow(dead_code)]
    pub fn cancel(&mut self, id: TaskId) {
        self.cancelled.insert(id);
    }

    /// Исполняет задачи всех тиков до `now` включительно и продвигает счётчик.
    /// Продвижение идёт по одному тику, поэтому даже при перескоке `now` вперёд
    /// пропущенные тики исполняются с опозданием, но в правильном порядке.
    /// Задачи одного тика исполняются по возрастанию id (порядок регистрации).
    /// `emit` отдаёт исполненные задачи наружу; планировать новые изнутри
    /// нельзя — новые задачи ставят после `run_tick`.
    pub fn run_tick<F: FnMut(Task)>(&mut self, now: u64, mut emit: F) {
        while self.current_tick < now {
            self.current_tick += 1;
            let tick = self.current_tick;

            // Переносим дальние задачи, вошедшие в ближнее окно. Опоздавшие
            // (far_tick <= tick) исполняем текущим бакетом, чтобы они не
            // зависли до следующего оборота кольца.
            while let Some(Reverse((far_tick, _, _))) = self.far.peek() {
                if *far_tick >= tick + NEAR_WINDOW {
                    break;
                }
                let Reverse((far_tick, _, task)) = self.far.pop().unwrap();
                let bucket = far_tick.min(tick) % NEAR_WINDOW;
                self.near[bucket as usize].push(task);
            }

            // Исполняем бакет текущего тика в стабильном порядке по id.
            let idx = (tick % NEAR_WINDOW) as usize;
            let mut bucket = std::mem::take(&mut self.near[idx]);
            bucket.sort_by_key(|task| task.id);
            for task in bucket {
                if self.cancelled.remove(&task.id) {
                    continue;
                }
                emit(task);
            }
        }
    }
}

/// Логическое содержимое планировщика для сохранения (simulation/save.rs):
/// все незавершённые задачи, счётчик id и отмены. Списки отсортированы —
/// байты сохранения детерминированы для одинакового состояния.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SchedulerSnapshot {
    /// Тик, до которого включительно задачи исполнены. Совпадает с тиком часов.
    pub current_tick: u64,
    /// Ближнее окно: NEAR_WINDOW бакетов по порядку индексов.
    pub near: Vec<Vec<Task>>,
    /// Дальние задачи, отсортированы по (тик, id).
    pub far: Vec<(u64, TaskId, Task)>,
    /// Следующий неиспользованный id.
    pub next_id: TaskId,
    /// Отменённые id, отсортированы.
    pub cancelled: Vec<TaskId>,
}

impl Scheduler {
    /// Снимок состояния для записи в сохранение.
    pub(crate) fn snapshot(&self) -> SchedulerSnapshot {
        let mut far: Vec<(u64, TaskId, Task)> =
            self.far.iter().map(|entry| entry.0.clone()).collect();
        far.sort();
        let mut cancelled: Vec<TaskId> = self.cancelled.iter().copied().collect();
        cancelled.sort_unstable();
        SchedulerSnapshot {
            current_tick: self.current_tick,
            near: self.near.clone(),
            far,
            next_id: self.next_id,
            cancelled,
        }
    }

    /// Восстанавливает планировщик из снимка сохранения. Внутренняя
    /// компоновка кучи может отличаться от исходной — порядок исполнения
    /// задаётся парами (тик, id) и не меняется.
    pub(crate) fn from_snapshot(snapshot: SchedulerSnapshot) -> Self {
        let mut near = vec![Vec::new(); NEAR_WINDOW as usize];
        for (idx, bucket) in snapshot.near.into_iter().enumerate() {
            // Лишние бакеты в файле — признак повреждения; save.rs проверяет
            // число бакетов, здесь просто игнорируем сверх окна.
            if idx >= near.len() {
                break;
            }
            near[idx] = bucket;
        }
        let mut far = BinaryHeap::new();
        for entry in snapshot.far {
            far.push(Reverse(entry));
        }
        Self {
            current_tick: snapshot.current_tick,
            near,
            far,
            next_id: snapshot.next_id,
            cancelled: snapshot.cancelled.into_iter().collect(),
        }
    }
}
