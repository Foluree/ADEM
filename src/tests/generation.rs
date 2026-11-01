//! Тесты процедурной генерации: детерминизм, биомы, глубины воды, реки,
//! посев объектов и заселение существ.

use crate::simulation::generation::generate_world;
use crate::simulation::objects::{ObjectKind, WorldObject};
use crate::simulation::tile::TileType;
use crate::simulation::world::{TilePos, World, WorldSize};

/// Считает объекты мира по видам. Обход HashMap не детерминирован,
/// поэтому миры сравниваются по счётчикам, а не по порядку объектов.
fn count_objects(world: &World) -> [u32; ObjectKind::ALL.len()] {
    let mut counts = [0_u32; ObjectKind::ALL.len()];
    for chunk in &world.chunks {
        for obj in chunk.objects.values() {
            counts[obj.kind.discriminant() as usize] += 1;
        }
    }
    counts
}

/// Генерация с одинаковым seed детерминирована по тайлам и объектам.
#[test]
fn generation_deterministic() {
    let a = generate_world(WorldSize::new(256), 12345);
    let b = generate_world(WorldSize::new(256), 12345);
    for y in 0..64 {
        for x in 0..64 {
            let pa = a.tile(TilePos { x, y });
            let pb = b.tile(TilePos { x, y });
            assert_eq!(pa.kind, pb.kind);
        }
    }
    assert_eq!(count_objects(&a), count_objects(&b));
}

/// Генерация содержит все важные виды тайлов (не вырождается в один тип).
#[test]
fn generation_has_variety() {
    let w = generate_world(WorldSize::new(256), 999);
    let mut has_water = false;
    let mut has_land = false;
    for y in 0..w.size.tiles() as i32 {
        for x in 0..w.size.tiles() as i32 {
            match w.terrain_kind(TilePos { x, y }) {
                TileType::Water => has_water = true,
                TileType::Grass | TileType::ForestGrass | TileType::Dirt | TileType::Stone => {
                    has_land = true;
                }
                _ => {}
            }
        }
    }
    assert!(has_water, "нет воды на карте");
    assert!(has_land, "нет суши на карте");
    assert!(
        count_objects(&w)[ObjectKind::Tree.discriminant() as usize] > 0,
        "нет деревьев на карте"
    );
}

/// Генерация сеет все четыре вида объектов.
#[test]
fn generation_places_all_object_kinds() {
    let w = generate_world(WorldSize::new(256), 999);
    let counts = count_objects(&w);
    assert!(counts[ObjectKind::Tree.discriminant() as usize] > 0, "нет деревьев");
    assert!(counts[ObjectKind::Bush.discriminant() as usize] > 0, "нет кустов");
    assert!(counts[ObjectKind::Rock.discriminant() as usize] > 0, "нет валунов");
    assert!(counts[ObjectKind::AppleTree.discriminant() as usize] > 0, "нет яблонь");
}

/// Генерация содержит все три глубины воды и горы (не вырождается).
#[test]
fn generation_has_water_depths_and_mountains() {
    let w = generate_world(WorldSize::new(256), 999);
    let mut has_deep = false;
    let mut has_ocean = false;
    let mut has_mountain = false;
    for y in 0..w.size.tiles() as i32 {
        for x in 0..w.size.tiles() as i32 {
            match w.terrain_kind(TilePos { x, y }) {
                TileType::DeepWater => has_deep = true,
                TileType::Ocean => has_ocean = true,
                TileType::Mountain => has_mountain = true,
                _ => {}
            }
        }
    }
    assert!(has_deep, "нет глубокой воды на карте");
    assert!(has_ocean, "нет океана на карте");
    assert!(has_mountain, "нет гор на карте");
}

/// «Океан» встречается только в морях: каждый водный регион с тайлом
/// Ocean соединён с краем карты. Внутренние водоёмы (озёра) глубина
/// понижает, поэтому озеро не может иметь океанической краски.
#[test]
fn ocean_tiles_only_touch_border_water() {
    use std::collections::HashSet;

    let w = generate_world(WorldSize::new(256), 999);
    let n = w.size.tiles() as i32;

    // BFS от водных тайлов периметра: помечаем всё море.
    let mut sea: HashSet<u32> = HashSet::new();
    let mut queue: std::collections::VecDeque<(i32, i32)> = std::collections::VecDeque::new();
    for i in 0..n {
        for (x, y) in [(i, 0), (i, n - 1), (0, i), (n - 1, i)] {
            if w.terrain_kind(TilePos { x, y }).is_water() && sea.insert((y * n + x) as u32) {
                queue.push_back((x, y));
            }
        }
    }
    while let Some((x, y)) = queue.pop_front() {
        for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx < 0 || ny < 0 || nx >= n || ny >= n {
                continue;
            }
            if w.terrain_kind(TilePos { x: nx, y: ny }).is_water()
                && sea.insert((ny * n + nx) as u32)
            {
                queue.push_back((nx, ny));
            }
        }
    }
    // Ни один тайл Ocean не остался вне моря.
    for y in 0..n {
        for x in 0..n {
            if w.terrain_kind(TilePos { x, y }) == TileType::Ocean {
                assert!(
                    sea.contains(&((y * n + x) as u32)),
                    "Ocean внутри озера в ({x}, {y})"
                );
            }
        }
    }
}

/// Реки существуют: вода встречается заметно выше уровня моря — такая
/// могла появиться только carve'ом русла через сушу.
#[test]
fn generation_carves_rivers_through_land() {
    use crate::simulation::generation::elevation_at;

    let mut worlds_with_rivers = 0;
    for seed in 1..=8 {
        let w = generate_world(WorldSize::new(256), seed);
        let n = w.size.tiles() as i32;
        let mut has_river = false;
        for y in 0..n {
            for x in 0..n {
                if w.terrain_kind(TilePos { x, y }) == TileType::Water
                    && elevation_at(x, y, n, seed) > 0.52
                {
                    has_river = true;
                    break;
                }
            }
            if has_river {
                break;
            }
        }
        if has_river {
            worlds_with_rivers += 1;
        }
    }
    assert!(
        worlds_with_rivers >= 6,
        "реки не генерируются: {} из 8 миров с реками",
        worlds_with_rivers
    );
}

/// На одном тайле не может быть двух объектов: повторная запись перезаписывает,
/// а очистка убирает объект.
#[test]
fn one_object_per_tile() {
    let mut world = World::new(WorldSize::new(256), 1);
    let pos = TilePos { x: 3, y: 4 };
    world.set_terrain(pos, TileType::Grass);
    world.set_object(pos, Some(WorldObject::tree()));
    world.set_object(pos, Some(WorldObject::bush()));
    assert!(matches!(world.object_at(pos), Some(obj) if obj.kind == ObjectKind::Bush));
    world.set_object(pos, None);
    assert!(world.object_at(pos).is_none());
}

/// Реки всегда заканчиваются водоёмом: любой внутренний водный регион
/// либо соединён с краем карты (море), либо это озеро заметного размера —
/// однотайловых обрывков после заливки котловин не остаётся. Связность
/// восьмисоседняя: русла рек идут и по диагонали.
#[test]
fn rivers_always_end_in_a_water_body() {
    use std::collections::{HashSet, VecDeque};

    for seed in [42_u64, 999, 7] {
        let w = generate_world(WorldSize::new(256), seed);
        let n = w.size.tiles() as i32;

        let mut seen: HashSet<u32> = HashSet::new();
        for start_y in 0..n {
            for start_x in 0..n {
                if !w.terrain_kind(TilePos { x: start_x, y: start_y }).is_water()
                    || seen.contains(&((start_y * n + start_x) as u32))
                {
                    continue;
                }
                // Новый водный регион: размер и выход к краю.
                let mut size = 0_usize;
                let mut touches_edge = false;
                let mut queue: VecDeque<(i32, i32)> = VecDeque::new();
                seen.insert((start_y * n + start_x) as u32);
                queue.push_back((start_x, start_y));
                while let Some((x, y)) = queue.pop_front() {
                    size += 1;
                    if x == 0 || y == 0 || x == n - 1 || y == n - 1 {
                        touches_edge = true;
                    }
                    for dx in -1..=1 {
                        for dy in -1..=1 {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let (nx, ny) = (x + dx, y + dy);
                            if nx < 0 || ny < 0 || nx >= n || ny >= n {
                                continue;
                            }
                            if w.terrain_kind(TilePos { x: nx, y: ny }).is_water()
                                && seen.insert((ny * n + nx) as u32)
                            {
                                queue.push_back((nx, ny));
                            }
                        }
                    }
                }
                assert!(
                    touches_edge || size >= 12,
                    "seed {seed}: внутренний водный регион из {size} тайлов в ({start_x}, {start_y})"
                );
            }
        }
    }
}

/// Генерация заселяет мир волками только на суше.
#[test]
fn generation_spawns_wolves_on_land() {
    use crate::simulation::entities::EntityKind;

    let world = generate_world(WorldSize::new(256), 42);
    let wolves: Vec<_> = world
        .entities
        .iter()
        .filter(|entity| entity.kind == EntityKind::Wolf)
        .collect();
    assert!(!wolves.is_empty(), "на карте нет волков");

    for wolf in wolves {
        let tile = TilePos {
            x: wolf.pos.x.floor() as i32,
            y: wolf.pos.y.floor() as i32,
        };
        assert!(
            !matches!(
                world.terrain_kind(tile),
                TileType::Water | TileType::DeepWater | TileType::Ocean
            ),
            "волк появился в воде: {:?}",
            tile
        );
        assert_eq!(wolf.hp, wolf.kind.max_hp());
    }
}

/// Генерация даёт яблоням запас плодов 4–10.
#[test]
fn generated_apple_trees_have_four_to_ten_fruits() {
    let world = generate_world(WorldSize::new(256), 42);
    let mut found = 0;
    for chunk in &world.chunks {
        for object in chunk.objects.values() {
            if object.kind != ObjectKind::AppleTree {
                continue;
            }
            found += 1;
            assert!(
                (4..=10).contains(&object.charge),
                "запас плодов вне диапазона: {}",
                object.charge
            );
        }
    }
    assert!(found > 0, "на карте нет яблонь");
}
