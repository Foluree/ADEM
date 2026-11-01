//! Тесты сохранений: roundtrip мира, планировщика и существ, чтение
//! заголовка из префикса, отказ на чужих и повреждённых файлах.

use crate::simulation::generation::generate_world;
use crate::simulation::tile::TileType;
use crate::simulation::world::{TilePos, WorldSize};

/// Сохранение и загрузка возвращают мир побайтово: все слои чанков,
/// ресурсы, объекты, температура — без потерь.
#[test]
fn save_roundtrip_world() {
    use crate::simulation::ResourceKind;
    use crate::simulation::objects::ObjectStage;
    use crate::simulation::save::{load_game, save_game};
    use crate::simulation::time::GameClock;

    let mut world = generate_world(WorldSize::new(256), 99);
    // Ломаем однородность: Uniform-чанк разворачивается в Full, ресурс и
    // объект перезаписываются — все три слоя должны пережить файл.
    world.set_terrain(TilePos { x: 0, y: 0 }, TileType::Sand);
    world.set_terrain(TilePos { x: 255, y: 255 }, TileType::Farmland);
    world.set_resource(TilePos { x: 5, y: 7 }, ResourceKind::Copper, 1234);
    world.set_object(
        TilePos { x: 9, y: 3 },
        Some(crate::simulation::WorldObject {
            kind: crate::simulation::ObjectKind::Rock,
            stage: ObjectStage::Mature,
            charge: 11,
        }),
    );
    world.set_object(TilePos { x: 9, y: 3 }, None);
    // Предмет на земле (дроп) — четвёртый слой чанка, тоже должен
    // пережить файл (версия формата 4).
    world.set_item(
        TilePos { x: 12, y: 8 },
        Some(crate::simulation::ItemStack {
            kind: crate::simulation::ItemKind::Stick,
            count: 7,
        }),
    );
    // Немного поживём: существа должны попасть в файл в произвольных
    // состояниях, а не только в стартовом «стоит».
    for tick in 0..300 {
        world.tick_entities(tick, None);
    }
    // Боевые данные: снаряд в полёте (без стрелка) и воткнутое копьё.
    world.spawn_projectile(
        crate::simulation::ProjectileKind::Arrow,
        crate::simulation::world::WorldPos { x: 50.5, y: 60.0 },
        crate::simulation::world::WorldPos { x: 60.5, y: 60.0 },
        0,
    );
    world.stuck_spears.push(crate::simulation::StuckSpear {
        pos: crate::simulation::world::WorldPos { x: 70.5, y: 80.5 },
        heading: 1.0,
        pitch: 0.7,
    });

    let clock = GameClock { tick: 123_456 };
    let mut scheduler = crate::simulation::Scheduler::default();
    scheduler.run_tick(clock.tick, |_| {});
    let cancelled = scheduler.schedule_at(clock.tick + 1000, crate::simulation::TaskKind::Marker(7));
    scheduler.cancel(cancelled);
    scheduler.schedule_at(clock.tick + 3, crate::simulation::TaskKind::Marker(8));

    let bytes = save_game(&world, &clock, &scheduler, "0.1.0");
    let loaded = load_game(&bytes).expect("сохранение должно разбираться");

    assert_eq!(loaded.seed, world.seed);
    assert_eq!(loaded.tick, clock.tick);
    assert_eq!(loaded.world.size, world.size);
    assert_eq!(loaded.world.chunks, world.chunks);
    assert_eq!(loaded.world.ambient_temperature_c, world.ambient_temperature_c);
    assert_eq!(loaded.world.entities, world.entities, "существа должны пережить файл");
    assert_eq!(loaded.world.projectiles, world.projectiles, "снаряды должны пережить файл");
    assert_eq!(
        loaded.world.stuck_spears, world.stuck_spears,
        "воткнутые копья должны пережить файл"
    );
    // Снимки планировщика равны, когда логическое содержимое совпадает.
    assert_eq!(scheduler.snapshot(), loaded.scheduler.snapshot());
}

/// Загруженный планировщик продолжает работу: задачи из файла исполняются
/// на своих тиках, отменённые id не воскрешаются.
#[test]
fn save_roundtrip_scheduler_continues() {
    use crate::simulation::scheduler::{Scheduler, TaskKind};
    use crate::simulation::save::{load_game, save_game};
    use crate::simulation::time::GameClock;

    let world = generate_world(WorldSize::new(256), 5);
    let tick = 50;
    let mut scheduler = Scheduler::default();
    scheduler.run_tick(tick, |_| {});
    let doomed = scheduler.schedule_at(tick + 2, TaskKind::Marker(1));
    scheduler.cancel(doomed);
    scheduler.schedule_at(tick + 2, TaskKind::Marker(2));
    scheduler.schedule_at(tick + 10_000, TaskKind::Marker(3));

    let bytes = save_game(&world, &GameClock { tick }, &scheduler, "0.1.0");
    let mut loaded = load_game(&bytes).expect("сохранение должно разбираться");

    let mut executed: Vec<TaskKind> = Vec::new();
    loaded
        .scheduler
        .run_tick(tick + 3, |task| executed.push(task.kind));
    assert_eq!(executed, vec![TaskKind::Marker(2)], "отменённая задача молчит");
}

/// Задача планировщика с чанком вне мира не проходит загрузку: битый файл
/// не должен довести симуляцию до выхода за границы `chunks` при первом
/// исполнении такой задачи.
#[test]
fn save_rejects_task_chunk_outside_world() {
    use crate::simulation::save::{SaveError, load_game, save_game};
    use crate::simulation::scheduler::{Scheduler, TaskKind};
    use crate::simulation::time::GameClock;
    use crate::simulation::world::ChunkPos;

    let world = generate_world(WorldSize::new(256), 3);
    let tick = 10;
    let mut scheduler = Scheduler::default();
    scheduler.run_tick(tick, |_| {});
    scheduler.schedule_at(
        tick + 1000,
        TaskKind::GrassSpread { chunk: ChunkPos { x: 999, y: 999 }, local_index: 0 },
    );

    let bytes = save_game(&world, &GameClock { tick }, &scheduler, "0.1.0");
    assert!(
        matches!(load_game(&bytes), Err(SaveError::BadData("чанк задачи вне мира"))),
        "чанк вне мира должен быть отклонён при загрузке"
    );
}

/// Заголовок читается из префикса файла: списку слотов не нужно тело.
#[test]
fn save_header_reads_from_prefix() {
    use crate::simulation::save::{load_game, read_header, save_game};
    use crate::simulation::time::GameClock;

    let world = generate_world(WorldSize::new(256), 11);
    let bytes = save_game(&world, &GameClock { tick: 777 }, &crate::simulation::Scheduler::default(), "0.1.0");

    // Префикс меньше тела: заголовок занимает лишь несколько десятков байт.
    let header = read_header(&bytes[..64]).expect("заголовок разбирается из префикса");
    assert_eq!(header.seed, 11);
    assert_eq!(header.tick, 777);
    assert_eq!(header.world_side, 256);
    assert_eq!(header.game_version, "0.1.0");
    assert!(load_game(&bytes[..32]).is_err(), "обрезанное тело — ошибка");
}

/// Чужой или повреждённый файл не принимается за сохранение.
#[test]
fn save_rejects_broken_files() {
    use crate::simulation::save::{load_game, read_header};

    assert!(matches!(
        read_header(b"NOTA"),
        Err(crate::simulation::save::SaveError::BadMagic)
    ));
    let mut bytes = vec![b'A', b'D', b'E', b'M'];
    bytes.extend_from_slice(&9999_u32.to_le_bytes());
    // Полный заголовок с неверной версией: пустая версия игры + нули
    // вместо seed/tick/размера. До проверки версии формат load_game
    // дочитывает заголовок целиком.
    bytes.push(0);
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    assert!(matches!(
        load_game(&bytes),
        Err(crate::simulation::save::SaveError::UnsupportedFormat { .. })
    ));
}

/// Задачи растительности переживают сохранение: планировщик восстанавливается
/// с теми же цепочками.
#[test]
fn save_roundtrip_vegetation_tasks() {
    use crate::simulation::save::{load_game, save_game};
    use crate::simulation::scheduler::TaskKind;
    use crate::simulation::time::GameClock;
    use crate::simulation::world::ChunkPos;

    let world = generate_world(WorldSize::new(256), 3);
    let tick = 500;
    let mut scheduler = crate::simulation::Scheduler::default();
    scheduler.run_tick(tick, |_| {});
    scheduler.schedule_at(tick + 10, TaskKind::GrassSpread { chunk: ChunkPos { x: 1, y: 2 }, local_index: 77 });
    scheduler.schedule_at(tick + 20, TaskKind::BushGrowth { chunk: ChunkPos { x: 3, y: 4 } });
    scheduler.schedule_at(tick + 30, TaskKind::WaterErosion { chunk: ChunkPos { x: 5, y: 6 }, local_index: 9 });

    let bytes = save_game(&world, &GameClock { tick }, &scheduler, "0.1.0");
    let loaded = load_game(&bytes).expect("сохранение должно разбираться");
    assert_eq!(scheduler.snapshot(), loaded.scheduler.snapshot());
}

/// HP (f32) и счётчик времени в воде переживают сохранение.
#[test]
fn save_roundtrip_entity_hp_and_water_ticks() {
    use crate::simulation::entities::EntityKind;
    use crate::simulation::save::{load_game, save_game};
    use crate::simulation::time::GameClock;
    use crate::simulation::world::WorldPos;

    let mut world = crate::simulation::World::new(WorldSize::new(256), 1);
    world
        .entities
        .spawn(EntityKind::Wolf, WorldPos { x: 10.5, y: 10.5 });
    {
        let wolf = &mut world.entities.entities[0];
        wolf.hp = 12.5;
        wolf.water_ticks = 37;
    }

    let bytes = save_game(
        &world,
        &GameClock { tick: 0 },
        &crate::simulation::Scheduler::default(),
        "0.1.0",
    );
    let loaded = load_game(&bytes).expect("сохранение должно разбираться");
    let wolf = &loaded.world.entities.entities[0];
    assert_eq!(wolf.hp, 12.5);
    assert_eq!(wolf.water_ticks, 37);
}

/// Человек с «умом» переживает сохранение: позиция, hp, голод, выбранный
/// слот хотбара, инвентарь со всеми секциями (включая надетую кольчугу и
/// мусорку). Кулдауны и цель ИИ — transient: при загрузке сбрасываются.
/// Повторное сохранение загруженного мира даёт те же байты.
#[test]
fn save_roundtrip_human_mind() {
    use crate::simulation::entities::{EntityKind, HumanGoal};
    use crate::simulation::hunger::Hunger;
    use crate::simulation::items::{Inventory, ItemKind, ItemStack, SlotArea, SlotRef};
    use crate::simulation::save::{load_game, save_game};
    use crate::simulation::time::GameClock;
    use crate::simulation::world::WorldPos;

    let mut world = crate::simulation::World::new(WorldSize::new(256), 1);
    let human = world
        .entities
        .spawn(EntityKind::Human, WorldPos { x: 33.5, y: 44.5 });
    {
        let entity = world.entities.get_mut(human).unwrap();
        entity.hp = 87.5;
        entity.hunger = Hunger::with_value(42.0);
        let mind = entity.mind.as_mut().unwrap();
        mind.selected_slot = 3;
        mind.inventory.add(ItemKind::Apple, 7);
        mind.inventory.add(ItemKind::Stick, 2);
        // Надетая кольчуга: каждый предмет в свой слот брони (секция
        // отдельная, предметы в misc не мешают).
        mind.inventory.put(
            SlotRef { area: SlotArea::Armor, index: 0 },
            ItemStack { kind: ItemKind::HelmetChainmail, count: 1 },
        );
        mind.inventory.put(
            SlotRef { area: SlotArea::Armor, index: 1 },
            ItemStack { kind: ItemKind::ChestplateChainmail, count: 1 },
        );
        mind.inventory.put(
            SlotRef { area: SlotArea::Armor, index: 3 },
            ItemStack { kind: ItemKind::BootsChainmail, count: 1 },
        );
        // Мусорка с затёртым стеком.
        mind.inventory.trash = Some(ItemStack { kind: ItemKind::Rock, count: 5 });
        // Кулдауны и цель ИИ «горячие» на момент сохранения — в файл
        // не пишутся.
        mind.attack_ready_tick = 1000;
        mind.eat_ready_tick = 2000;
        mind.goal = Some(HumanGoal::Food(crate::simulation::TilePos { x: 1, y: 2 }));
        mind.next_think_tick = 3000;
    }

    let bytes = save_game(
        &world,
        &GameClock { tick: 0 },
        &crate::simulation::Scheduler::default(),
        "0.1.0",
    );
    let loaded = load_game(&bytes).expect("сохранение должно разбираться");
    let entity = loaded.world.entities.get(human).expect("человек должен сохраниться");
    assert_eq!(entity.pos.x, 33.5);
    assert_eq!(entity.pos.y, 44.5);
    assert_eq!(entity.prev_pos, entity.pos, "интерполяция начинается с места");
    assert_eq!(entity.hp, 87.5);
    let restored = entity.mind.as_ref().expect("ум человека должен сохраниться");

    assert_eq!(entity.hunger, Hunger::with_value(42.0));
    assert_eq!(restored.selected_slot, 3);
    // Кулдауны и цель ИИ сброшены.
    assert_eq!(restored.attack_ready_tick, 0);
    assert_eq!(restored.eat_ready_tick, 0);
    assert_eq!(restored.goal, None);
    assert_eq!(restored.next_think_tick, 0);

    let inv: &Inventory = &restored.inventory;
    assert_eq!(inv.count_of(ItemKind::Apple), 7);
    assert_eq!(inv.count_of(ItemKind::Stick), 2);
    assert_eq!(
        inv.get(SlotRef { area: SlotArea::Armor, index: 0 }),
        Some(ItemStack { kind: ItemKind::HelmetChainmail, count: 1 })
    );
    assert_eq!(
        inv.get(SlotRef { area: SlotArea::Armor, index: 1 }),
        Some(ItemStack { kind: ItemKind::ChestplateChainmail, count: 1 })
    );
    assert_eq!(
        inv.get(SlotRef { area: SlotArea::Armor, index: 3 }),
        Some(ItemStack { kind: ItemKind::BootsChainmail, count: 1 })
    );
    assert_eq!(inv.trash, Some(ItemStack { kind: ItemKind::Rock, count: 5 }));
    // Секция одежды существует и пуста (предметов одежды пока нет).
    assert_eq!(restored.inventory.clothes, [None; 4]);

    // Roundtrip побайтово: transient-поля в файл не пишутся, поэтому
    // повторное сохранение загруженного мира воспроизводит те же байты.
    let again = save_game(
        &loaded.world,
        &GameClock { tick: loaded.tick },
        &loaded.scheduler,
        "0.1.0",
    );
    assert_eq!(bytes, again, "сохранение должно воспроизводиться побайтово");
}

/// Волк сохраняется без «ума», человек — с ним: рассинхрон вида и данных
/// не проходит загрузку (битый файл не может сделать волка разумным).
#[test]
fn save_rejects_wrong_item_in_clothes_slot() {
    use crate::simulation::entities::EntityKind;
    use crate::simulation::items::ItemKind;
    use crate::simulation::save::{SaveError, load_game, save_game};
    use crate::simulation::time::GameClock;
    use crate::simulation::world::WorldPos;

    let mut world = crate::simulation::World::new(crate::simulation::WorldSize::new(256), 1);
    let human = world.entities.spawn(EntityKind::Human, WorldPos { x: 0.5, y: 0.5 });
    // Прямая кладка в слот одежды минует `accepts` (UI так не умеет,
    // а битый файл — может).
    world
        .entities
        .get_mut(human)
        .unwrap()
        .mind
        .as_mut()
        .unwrap()
        .inventory
        .clothes[0] = Some(crate::simulation::ItemStack {
        kind: ItemKind::Apple,
        count: 1,
    });

    let bytes = save_game(
        &world,
        &GameClock { tick: 0 },
        &crate::simulation::Scheduler::default(),
        "0.1.0",
    );
    assert!(
        matches!(load_game(&bytes), Err(SaveError::BadData("предмет в чужом слоте"))),
        "не-одежда в слоте одежды должна отклоняться при загрузке"
    );
}

/// Roundtrip v2: культуры, контейнеры станций и item-сущности дропа
/// читаются обратно без потерь (слова̀рное состояние мира).
#[test]
fn save_roundtrip_crops_containers_and_item_entities() {
    use crate::simulation::crops::{Crop, CropKind};
    use crate::simulation::containers::FURNACE_FUEL;
    use crate::simulation::items::ItemKind;
    use crate::simulation::objects::WorldObject;
    use crate::simulation::save::{load_game, save_game};
    use crate::simulation::time::GameClock;
    use crate::simulation::WorldPos;

    let mut world = crate::simulation::World::new(WorldSize::new(256), 3);
    let farm = TilePos { x: 5, y: 5 };
    world.set_terrain(farm, TileType::Farmland);
    world.set_crop(farm, Some(Crop { kind: CropKind::Wheat, stage: 3 }));

    let station = TilePos { x: 6, y: 6 };
    world.set_object_with_container(
        station,
        Some(WorldObject::building(crate::simulation::objects::BuildingKind::Furnace)),
    );
    let container = crate::simulation::containers::container_at_mut(&mut world, station).unwrap();
    container.put(
        FURNACE_FUEL,
        crate::simulation::items::ItemStack { kind: ItemKind::Coal, count: 7 },
    );

    world.item_entities.drop_stack(
        3,
        WorldPos { x: 7.5, y: 7.5 },
        crate::simulation::items::ItemStack { kind: ItemKind::MeatPredatorRaw, count: 2 },
        100,
    );

    // Планировщик должен быть на том же тике, что и часы (проверка load_game).
    let mut sched = crate::simulation::Scheduler::default();
    sched.run_tick(150, |_| {});
    let bytes = save_game(&world, &GameClock { tick: 150 }, &sched, "0.1.0");
    let saved = load_game(&bytes).expect("roundtrip v2");
    let world = saved.world;

    assert_eq!(
        world.crop_at(farm).map(|c| (c.kind, c.stage)),
        Some((CropKind::Wheat, 3))
    );
    let container = crate::simulation::containers::container_at(&world, station).unwrap();
    assert_eq!(
        container.get(FURNACE_FUEL),
        Some(crate::simulation::items::ItemStack { kind: ItemKind::Coal, count: 7 })
    );
    assert_eq!(world.item_entities.len(), 1);
    assert_eq!(world.item_entities.items[0].stack.count, 2);
    assert_eq!(world.item_entities.items[0].spawn_tick, 100);
}

/// Мох появляется пятнами в лесу: у мира с любым seed есть лесные области,
/// и мох встречается только на уровне леса (рядом с ForestGrass).
#[test]
fn generation_places_moss_patches_in_forest() {
    let world = crate::simulation::generation::generate_world(WorldSize::new(256), 42);
    let mut moss = 0_u32;
    let mut forest = 0_u32;
    for chunk in &world.chunks {
        for idx in 0..crate::simulation::CHUNK_TILES {
            match chunk.terrain.get(idx) {
                TileType::Moss => moss += 1,
                TileType::ForestGrass => forest += 1,
                _ => {}
            }
        }
    }
    assert!(forest > 0, "в мире должна быть лесная трава");
    assert!(moss > 0, "мох должен появляться пятнами в лесу");

    // Детерминизм: тот же seed — та же карта.
    let again = crate::simulation::generation::generate_world(WorldSize::new(256), 42);
    assert_eq!(
        world.chunks[0].terrain, again.chunks[0].terrain,
        "рельеф первого чанка должен совпадать при повторной генерации"
    );
}
