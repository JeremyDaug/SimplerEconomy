

use crate::game::actors::Actors;
use crate::game::factuals::Factuals;
use crate::game::mapdata::MapData;
use crate::game::market::MarketLookups;
use crate::game::players::Players;

/// # Play State
///
/// This is the highest level of our game, overseeing a the state of the current game.
///
/// It stores all active data for one game.
#[derive(Debug)]
pub struct PlayState {
    pub factuals: Factuals, // Highly static, Arc or & for sharing/multithreading
    pub map_data: MapData,  // Environmental + spatial
    pub actors: Actors,     // Active pops, firms, institutions, …
    pub players: Players,   // Human/AI players, resources, etc.

    // Optional: turn metadata, game clock, etc.
    pub turn: u64,
    pub is_paused: bool,
    /// Per-market AMV snapshots and pop-to-market ids for this turn.
    /// Rebuild after prices change or pops move; borrow during pop phases.
    pub market_lookups: MarketLookups,
    // ... any other global flags
}
