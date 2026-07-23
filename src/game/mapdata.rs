use std::collections::HashMap;

use crate::game::{
    actors::Actors,
    factuals::Factuals,
    map::Map,
    market::Market,
    players::Players,
};

/// # Map Data
/// 
/// Contains and manages the data for map and environmental data.
#[derive(Debug)]
pub struct MapData {
    pub map: Map,                     // Core hexgrid, tiles, navigation
    pub markets: HashMap<usize, Market>,             // Tile-linked or global markets
    // Secondary environmental: weather, pollution, resources regen, etc.
    pub environment: EnvironmentData,
}
impl MapData {
    pub(crate) fn decay_goods(&self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Decay goods in Markets and any tiles not contained in a market.")
    }

    /// Organized / mass migration (migratory firms, institutions, player efforts).
    /// Cross-market; must run sequentially.
    pub fn process_organized_migration(
        &mut self,
        actors: &mut Actors,
        players: &Players,
        factuals: &Factuals,
    ) {
        let _ = (self, actors, players, factuals);
        todo!("Process organized migration efforts across markets")
    }

    /// Personal long-distance migration between markets from the migrant pool.
    /// Cross-market; must run sequentially.
    pub fn process_inter_market_migration(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
    ) {
        let _ = (self, actors, factuals);
        todo!("Process inter-market personal migration")
    }
}

/// # Environmental Data
/// 
/// Stores overarching environmental data, like 
#[derive(Debug)]
pub struct EnvironmentData { /* weather, resources, etc. */ }
