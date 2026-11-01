//! Симуляция мира. Без зависимости от Bevy.
//! Правила и данные мира — здесь; рендер и UI живут в других модулях.

pub mod chunk;
pub mod clothes;
pub mod climate;
pub mod combat;
pub mod commands;
pub mod crafts;
pub mod dirty;
pub mod entities;
pub mod generation;
pub mod hunger;
pub mod items;
pub mod movement;
pub mod needs;
pub mod objects;
pub mod player;
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
pub use dirty::DirtyTiles;
#[allow(unused_imports)]
pub use crafts::{CraftBook, CraftRecipe, ShapedPattern, GRID_SLOTS, MANUAL_CATEGORY};
#[allow(unused_imports)]
pub use entities::{EntityKind, EntityState, EntityStore, Facing, SimEntity};
#[allow(unused_imports)]
pub use hunger::{Activity, Hunger, APPLE_FOOD_VALUE, EAT_COOLDOWN_TICKS, HUNGER_MAX};
#[allow(unused_imports)]
pub use items::{Inventory, ItemKind, ItemStack, ITEM_KIND_COUNT, MISC_SLOTS};
#[allow(unused_imports)]
pub use needs::{Need, NeedKind};
#[allow(unused_imports)]
pub use objects::{ObjectKind, ObjectStage, WorldObject};
#[allow(unused_imports)]
pub use player::{Player, WorldPos};
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
