//! Боевая система: ближний бой игрока, метательное копьё и лук.
//!
//! Заготовка глобальной системы боя (роадмап «Инструменты и выживание»):
//! урон наносят атаки игрока и снаряды. ИИ существ пока не сражается —
//! волк получает урон, но не атакует в ответ. Смерть существа — исчезновение
//! (трупы/падаль появятся вместе с механикой добычи, см. роадмап).
//!
//! Снаряды летят по баллистической дуге: горизонтальная скорость постоянна,
//! высота — парабола `arc·4·t·(1−t)` (нулевая на старте и в точке падения).
//! Небольшая высота дуги = «смягчённое» движение вверх/вниз. Всё
//! детерминировано, без RNG; порядок в `Vec` стабилен для сохранений.

use super::items::{self, ItemKind};
use super::player::{Player, WorldPos};
use super::world::{TilePos, World};

/// Дальность броска копья, тайлов.
pub const SPEAR_THROW_RANGE_TILES: f32 = 12.0;

/// Дальность выстрела из лука, тайлов.
pub const BOW_RANGE_TILES: f32 = 20.0;

/// Кулдаун выстрела из лука, тиков.
pub const BOW_COOLDOWN_TICKS: u64 = 15;

/// Скорость полёта стрелы, тайлов за тик (20 тиков/с при x1 → ~16 тайлов/с).
const ARROW_SPEED_TILES_PER_TICK: f32 = 0.8;

/// Скорость полёта брошенного копья, тайлов за тик (тяжелее стрелы).
const SPEAR_THROW_SPEED_TILES_PER_TICK: f32 = 0.5;

/// Радиус попадания снаряда по существу, тайлов (от центра существа).
const PROJECTILE_HIT_RADIUS_TILES: f32 = 0.6;

/// Радиус попадания удара в ближнем бою, тайлов (от точки прицела до
/// центра существа).
const MELEE_HIT_RADIUS_TILES: f32 = 0.75;

/// Высота дуги броска копья: доля от дальности, с пределами — «мягкая»
/// баллистика без отвесных траекторий.
const SPEAR_ARC_FRACTION: f32 = 0.25;
const SPEAR_ARC_MIN_TILES: f32 = 1.0;
const SPEAR_ARC_MAX_TILES: f32 = 3.5;

/// Стрела летит настильно: дуга заметно ниже копья (иначе пролетает
/// над существами — порог попадания по высоте 1 тайл).
const ARROW_ARC_FRACTION: f32 = 0.05;
const ARROW_ARC_MIN_TILES: f32 = 0.2;
const ARROW_ARC_MAX_TILES: f32 = 1.0;

/// Радиус автоматического подбора воткнутого копья, тайлов.
pub const SPEAR_PICKUP_RADIUS_TILES: f32 = 1.0;

/// Пределы угла воткнутого копья от горизонтали (для рендера): слишком
/// пологое и слишком отвесное положение выглядит неправдоподобно.
const STUCK_PITCH_MIN: f32 = 10.0_f32.to_radians();
const STUCK_PITCH_MAX: f32 = 80.0_f32.to_radians();

/// Вид снаряда. Значения стабильны для сохранений.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ProjectileKind {
    /// Стрела из лука: при попадании исчезает.
    Arrow,
    /// Брошенное копьё: втыкается в землю, можно подобрать.
    ThrownSpear,
}

impl ProjectileKind {
    /// Вид снаряда по стабильному значению u8 (сохранения).
    pub fn from_discriminant(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Arrow),
            1 => Some(Self::ThrownSpear),
            _ => None,
        }
    }
}

/// Снаряд в полёте. Движение параметрическое: доля пути `t` равномерно
/// растёт до 1, позиция и высота вычисляются по запросу — хранить скорость
/// не нужно, и сохранение остаётся коротким.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Projectile {
    /// Стабильный идентификатор в пределах мира (для рендера).
    pub id: u64,
    pub kind: ProjectileKind,
    /// Точка старта и точка падения (тайлы, непрерывные координаты).
    pub origin: WorldPos,
    pub target: WorldPos,
    /// Пройденная доля пути [0, 1].
    pub t: f32,
    /// Полная длительность полёта в тиках.
    pub duration_ticks: u64,
    /// Высота дуги в тайлах (максимум подъёма в середине пути).
    pub arc_height: f32,
    /// Урон при попадании.
    pub damage: f32,
}

impl Projectile {
    /// Текущая позиция (горизонтальная проекция), интерполяция по `t`.
    pub fn pos(&self) -> WorldPos {
        WorldPos {
            x: self.origin.x + (self.target.x - self.origin.x) * self.t,
            y: self.origin.y + (self.target.y - self.origin.y) * self.t,
        }
    }

    /// Текущая высота над землёй, тайлов: парабола, 0 на концах пути.
    pub fn height(&self) -> f32 {
        self.arc_height * 4.0 * self.t * (1.0 - self.t)
    }

    /// Горизонтальный угол полёта (радианы, atan2(dy, dx)).
    pub fn heading(&self) -> f32 {
        (self.target.y - self.origin.y).atan2(self.target.x - self.origin.x)
    }

    /// Тангенс наклона траектории в точке падения: вертикальная и
    /// горизонтальная скорости на единицу `t` равны `4·arc` и `D`.
    pub fn landing_pitch(&self) -> f32 {
        let distance = self.hypot_distance();
        if distance <= f32::EPSILON || self.arc_height <= 0.0 {
            return STUCK_PITCH_MIN;
        }
        let slope = 4.0 * self.arc_height / distance;
        slope.atan().clamp(STUCK_PITCH_MIN, STUCK_PITCH_MAX)
    }

    /// Дальность полёта, тайлов.
    fn hypot_distance(&self) -> f32 {
        let dx = self.target.x - self.origin.x;
        let dy = self.target.y - self.origin.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Создаёт снаряд от `origin` к `target` с параметрами вида.
    fn new(id: u64, kind: ProjectileKind, origin: WorldPos, target: WorldPos) -> Self {
        let distance = target.dist(origin);
        let (speed, arc) = match kind {
            ProjectileKind::Arrow => (
                ARROW_SPEED_TILES_PER_TICK,
                (distance * ARROW_ARC_FRACTION).clamp(ARROW_ARC_MIN_TILES, ARROW_ARC_MAX_TILES),
            ),
            ProjectileKind::ThrownSpear => (
                SPEAR_THROW_SPEED_TILES_PER_TICK,
                (distance * SPEAR_ARC_FRACTION).clamp(SPEAR_ARC_MIN_TILES, SPEAR_ARC_MAX_TILES),
            ),
        };
        Self {
            id,
            kind,
            origin,
            target,
            t: 0.0,
            duration_ticks: (distance / speed).ceil().max(1.0) as u64,
            arc_height: arc,
            damage: match kind {
                ProjectileKind::Arrow => 10.0,
                ProjectileKind::ThrownSpear => 5.0,
            },
        }
    }
}

/// Копьё, воткнувшееся в землю. Ведёт себя как маленькая «сущность» мира:
/// живёт до подбора игроком, рендерится под углом падения.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StuckSpear {
    /// Точка, куда воткнулось (тайлы, непрерывные координаты).
    pub pos: WorldPos,
    /// Горизонтальный угол полёта в момент падения (куда смотрит древко).
    pub heading: f32,
    /// Угол входа от горизонтали (насколько отвесно воткнулось).
    pub pitch: f32,
}

impl World {
    /// Создаёт снаряд от `origin` к `target` (точка падения клампится к
    /// границам мира) и возвращает его id.
    pub fn spawn_projectile(&mut self, kind: ProjectileKind, origin: WorldPos, target: WorldPos) -> u64 {
        let side = self.size.tiles() as f32;
        let target = WorldPos {
            x: target.x.clamp(0.0, side),
            y: target.y.clamp(0.0, side),
        };
        if self.next_projectile_id == 0 {
            self.next_projectile_id = 1;
        }
        let id = self.next_projectile_id;
        self.next_projectile_id += 1;
        self.projectiles.push(Projectile::new(id, kind, origin, target));
        id
    }

    /// Продвигает все снаряды на один тик: движение, попадания по существам,
    /// падение. Снаряд-стрела при попадании исчезает, копьё втыкается.
    /// Смерть существа — удаление из мира (см. шапку модуля).
    pub fn tick_projectiles(&mut self) {
        // Попадания собираются по id, чтобы не держать заём существ при
        // мутации снарядов.
        let mut hits: Vec<(u64, u64)> = Vec::new(); // (id снаряда, id существа)
        for projectile in &mut self.projectiles {
            projectile.t += 1.0 / projectile.duration_ticks as f32;
            let pos = projectile.pos();
            // Существо высотой около тайла: снаряд выше него пролетает мимо.
            if projectile.height() <= 1.0 {
                for entity in self.entities.iter() {
                    if entity.pos.dist(pos) <= PROJECTILE_HIT_RADIUS_TILES {
                        hits.push((projectile.id, entity.id));
                        break;
                    }
                }
            }
        }

        for (projectile_id, entity_id) in hits {
            let Some(projectile) = self.projectiles.iter().find(|p| p.id == projectile_id) else {
                continue;
            };
            let (damage, kind, pos, heading) = (
                projectile.damage,
                projectile.kind,
                projectile.pos(),
                projectile.heading(),
            );
            // Урон и смерть.
            let died = {
                let entity = self.entities.entities.iter_mut().find(|e| e.id == entity_id);
                match entity {
                    Some(entity) => {
                        entity.hp = (entity.hp - damage).max(0.0);
                        entity.hp <= 0.0
                    }
                    None => false,
                }
            };
            if died {
                self.entities.remove(entity_id);
            }
            // Дальше снаряд не летит: стрела исчезает, копьё втыкается.
            if kind == ProjectileKind::ThrownSpear {
                self.stuck_spears.push(StuckSpear {
                    pos,
                    heading,
                    pitch: projectile.landing_pitch(),
                });
            }
            self.projectiles.retain(|p| p.id != projectile_id);
        }

        // Падение в землю: копьё остаётся, стрела пропадает.
        let mut landed: Vec<StuckSpear> = Vec::new();
        self.projectiles.retain(|projectile| {
            if projectile.t < 1.0 {
                return true;
            }
            if projectile.kind == ProjectileKind::ThrownSpear {
                landed.push(StuckSpear {
                    pos: projectile.target,
                    heading: projectile.heading(),
                    pitch: projectile.landing_pitch(),
                });
            }
            false
        });
        self.stuck_spears.extend(landed);
    }
}

/// Пытается ударить существо в ближнем бою из точки прицела `target`.
///
/// Возвращает `true`, если удар состоялся (кулдаун списан). Кулдаун
/// списывается только при реальном попадании: промах не мешает добыче
/// и не наказывает игрока за пустой клик. Дальность — от атакующего до
/// существа; попадание — от точки прицела до центра существа.
pub fn try_melee_attack(
    world: &mut World,
    player: &mut Player,
    target: TilePos,
    tick: u64,
) -> bool {
    if tick < player.attack_ready_tick {
        return false;
    }
    let held = player.selected_item();
    let stats = held
        .and_then(ItemKind::weapon_stats)
        .unwrap_or(items::HAND_ATTACK);
    let point = WorldPos::of_tile_center(target);

    let victim = world
        .entities
        .iter()
        .filter(|entity| entity.pos.dist(player.pos) <= stats.range_tiles)
        .filter(|entity| entity.pos.dist(point) <= MELEE_HIT_RADIUS_TILES)
        .min_by(|a, b| {
            a.pos
                .dist(point)
                .total_cmp(&b.pos.dist(point))
        })
        .map(|entity| entity.id);
    let Some(victim) = victim else {
        return false;
    };

    player.attack_ready_tick = tick + stats.cooldown_ticks;
    apply_damage(world, victim, stats.damage);
    true
}

/// Наносит урон существу; смерть (hp ≤ 0) удаляет его из мира.
fn apply_damage(world: &mut World, entity_id: u64, damage: f32) {
    let died = {
        let entity = world
            .entities
            .entities
            .iter_mut()
            .find(|entity| entity.id == entity_id);
        match entity {
            Some(entity) => {
                entity.hp = (entity.hp - damage).max(0.0);
                entity.hp <= 0.0
            }
            None => false,
        }
    };
    if died {
        world.entities.remove(entity_id);
    }
}

/// Автоподбор воткнутых копьёв: всё, что ближе `SPEAR_PICKUP_RADIUS_TILES`
/// к игроку и влезает в инвентарь, возвращается как предмет. Вызывается
/// после шага игрока (раз в тик в режиме игрока).
pub fn pickup_stuck_spears(world: &mut World, player: &mut Player) -> usize {
    let mut picked = 0;
    let mut i = 0;
    while i < world.stuck_spears.len() {
        if world.stuck_spears[i].pos.dist(player.pos) <= SPEAR_PICKUP_RADIUS_TILES
            && player.inventory.can_fit(ItemKind::Spear, 1)
        {
            player.inventory.add(ItemKind::Spear, 1);
            world.stuck_spears.swap_remove(i);
            picked += 1;
        } else {
            i += 1;
        }
    }
    picked
}

#[cfg(test)]
#[path = "../tests/combat.rs"]
mod tests;
