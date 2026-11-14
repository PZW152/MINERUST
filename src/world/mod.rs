pub mod anvil;
pub mod biome;
pub mod explosion;
pub mod features;
pub mod fire;
pub mod gen;
pub mod growth;
pub mod loot;
pub mod populate;
pub mod rail;
pub mod redstone;
pub mod save;
pub mod server;
pub mod structure;
pub mod ticks;
pub mod tripwire;

pub use gen::chunk_provider::{ChunkData, ChunkProvider};
pub use server::ServerWorld;

/// Read-only block access shared by the server world and the client's
/// snapshot of it (collision, picking).
pub trait BlockView {
    /// Block id, air where nothing is loaded.
    fn peek_block(&self, x: i32, y: i32, z: i32) -> u8;
    fn peek_meta(&self, x: i32, y: i32, z: i32) -> u8;
    /// Whether the chunk containing column (x, z) is available.
    fn loaded(&self, x: i32, z: i32) -> bool;
    /// `World.canLightningStrikeAt`: rain falls on this block.
    fn lightning_at(&self, _x: i32, _y: i32, _z: i32) -> bool {
        false
    }
    /// The tile entity of a `piston_extension` block.
    fn moving(&self, _x: i32, _y: i32, _z: i32) -> Option<redstone::MovingView> {
        None
    }
    /// Entities whose `getBoundingBox` is solid to others (boats): id, box.
    fn solid_entities(&self) -> &[(u32, crate::entity::player::Aabb)] {
        &[]
    }
}

impl BlockView for ServerWorld {
    fn peek_block(&self, x: i32, y: i32, z: i32) -> u8 {
        ServerWorld::peek_block(self, x, y, z)
    }
    fn peek_meta(&self, x: i32, y: i32, z: i32) -> u8 {
        ServerWorld::peek_meta(self, x, y, z)
    }
    fn loaded(&self, x: i32, z: i32) -> bool {
        ServerWorld::loaded(self, x, z)
    }
    fn lightning_at(&self, x: i32, y: i32, z: i32) -> bool {
        ServerWorld::lightning_at(self, x, y, z)
    }
    fn moving(&self, x: i32, y: i32, z: i32) -> Option<redstone::MovingView> {
        self.moving_at([x, y, z]).map(|m| m.view())
    }
    fn solid_entities(&self) -> &[(u32, crate::entity::player::Aabb)] {
        &self.solid_entities
    }
}
