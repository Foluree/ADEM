//! Симуляция мира. Без зависимости от Bevy.
//! Правила и данные мира — здесь; рендер и UI живут в других модулях.

pub mod actions;
pub mod ai;
pub mod chunk;
pub mod clothes;
pub mod climate;
pub mod combat;
pub mod commands;
pub mod containers;
pub mod crafts;
pub mod crops;
pub mod dirty;
pub mod entities;
pub mod generation;
pub mod hunger;
pub mod item_entity;
pub mod items;
pub mod movement;
pub mod needs;
pub mod objects;
pub mod save;
pub mod scheduler;
pub mod tile;
pub mod time;
pub mod vegetation;
pub mod world;
pub mod armor;

#[allow(unused_imports)]
pub use armor::{ARMOR_RATING_MAX, protection_of, rating as armor_rating, reduce_damage};
#[allow(unused_imports)]
pub use chunk::{Chunk, TerrainTiles, CHUNK_SIZE, CHUNK_TILES};
#[allow(unused_imports)]
pub use clothes::{WARMTH_MAX, warmth, warmth_of};
#[allow(unused_imports)]
pub use climate::ambient_temperature_c;
#[allow(unused_imports)]
pub use combat::{Projectile, ProjectileKind, StuckSpear};
#[allow(unused_imports)]
pub use commands::{Command, CommandQueue};
#[allow(unused_imports)]
pub use containers::{
    Container, BARREL_SLOTS, STORAGE_SLOTS, capacity as container_capacity,
    can_put_into, container_at, container_at_mut, wake_station,
    FURNACE_COOK_TICKS, BLAST_COOK_TICKS, CAMPFIRE_COOK_TICKS,
    FURNACE_INPUT, FURNACE_FUEL, FURNACE_OUTPUT, FURNACE_WASTE,
    BLAST_INPUT, BLAST_FUEL, BLAST_OUTPUT, BLAST_WASTE,
    CAMPFIRE_INPUT, CAMPFIRE_OUTPUT, SLOT_AREA_PLAYER_MISC,
};
#[allow(unused_imports)]
pub use crafts::{
    CraftBook, CraftBookLoad, CraftRecipe, CrafterKind, RecipeBody, RecipeShape,
    ShapedPattern, GRID_SLOTS, MANUAL_CATEGORY,
};
#[allow(unused_imports)]
pub use crops::{Crop, CropKind, CROP_KIND_COUNT, MAX_CROP_STAGES};
#[allow(unused_imports)]
pub use dirty::DirtyTiles;
#[allow(unused_imports)]
pub use entities::{EntityKind, EntityStore, EntityState, Facing, HumanGoal, HumanMind, SimEntity};
#[allow(unused_imports)]
pub use hunger::{Activity, Hunger, EAT_COOLDOWN_TICKS, HUNGER_MAX};
#[allow(unused_imports)]
pub use item_entity::{ItemEntities, ItemEntity};
#[allow(unused_imports)]
pub use items::{Inventory, ItemKind, ItemStack, ITEM_KIND_COUNT, MISC_SLOTS, ToolClass};
#[allow(unused_imports)]
pub use needs::{Need, NeedKind};
#[allow(unused_imports)]
pub use objects::{BuildingKind, ObjectKind, ObjectStage, WorldObject};
#[allow(unused_imports)]
pub use actions::{INTERACT_REACH};
pub use world::WorldPos;
#[allow(unused_imports)]
pub use scheduler::{Scheduler, Task, TaskId, TaskKind};
#[allow(unused_imports)]
pub use tile::{ResourceKind, Tile, TileType};
#[allow(unused_imports)]
pub use time::{GameClock, Season};
#[allow(unused_imports)]
pub use vegetation::{after_tile_changed, apply_task, seed_all};
#[allow(unused_imports)]
pub use world::{ChunkPos, TilePos, World, WorldSize, TILE_SIZE, split_tile, tile_pos};
