use std::collections::HashMap;

use crate::game::{
    actors::Actors, factuals::Factuals, map::Map, market::Market, players::Players,
};

/// # Map Data
///
/// Contains and manages the data for map and environmental data.
#[derive(Debug)]
pub struct MapData {
    pub map: Map,                        // Core hexgrid, tiles, navigation
    pub markets: HashMap<usize, Market>, // Tile-linked or global markets
    // Secondary environmental: weather, pollution, resources regen, etc.
    pub environment: EnvironmentData,
}

impl MapData {
    /// # Market
    ///
    /// The market stored under `id`.
    ///
    /// Panics if that market is missing.
    pub fn market(&self, id: usize) -> &Market {
        self.markets
            .get(&id)
            .unwrap_or_else(|| panic!("market {id} is not in MapData"))
    }

    /// # Market Mut
    ///
    /// Mutable access to the market stored under `id`.
    ///
    /// Panics if that market is missing.
    pub fn market_mut(&mut self, id: usize) -> &mut Market {
        self.markets
            .get_mut(&id)
            .unwrap_or_else(|| panic!("market {id} is not in MapData"))
    }

    /// # Decay Goods
    ///
    /// Decays goods held in markets and on tiles outside a market.
    ///
    /// `factuals` supplies the goods. The pass is not written yet.
    pub fn decay_goods(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Decay goods in Markets and any tiles not contained in a market.")
    }

    /// # Process Organized Migration
    ///
    /// Moves migratory firms, institutions, and player efforts across markets.
    ///
    /// `actors` are the movers, `players` are the efforts, and `factuals`
    /// supplies the world. The pass is not written yet. Cross-market work
    /// must run one market after another.
    pub fn process_organized_migration(
        &mut self,
        actors: &mut Actors,
        players: &Players,
        factuals: &Factuals,
    ) {
        let _ = (self, actors, players, factuals);
        todo!("Process organized migration efforts across markets")
    }

    /// # Process Inter Market Migration
    ///
    /// Moves people from the migrant pool between markets.
    ///
    /// `actors` hold the pops and `factuals` supplies the world. The pass is
    /// not written yet. Cross-market work must run one market after another.
    pub fn process_inter_market_migration(&mut self, actors: &mut Actors, factuals: &Factuals) {
        let _ = (self, actors, factuals);
        todo!("Process inter-market personal migration")
    }
}

/// # Environmental Data
///
/// Stores overarching environmental data, like
#[derive(Debug)]
pub struct EnvironmentData {/* weather, resources, etc. */}
