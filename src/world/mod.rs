pub mod biome;
pub mod features;
pub mod gen;
pub mod loot;
pub mod populate;
pub mod rail;
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
}
