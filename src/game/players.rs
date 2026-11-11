use std::collections::HashMap;

use crate::game::factuals::Factuals;
use crate::game::market::Market;
use crate::game::nation::Nation;
use crate::game::pop::SellOffer;

/// # Players
///
/// The players of the game. Players are represented by nations.
#[derive(Debug)]
pub struct Players {
    pub players: HashMap<usize, Nation>,
}

impl Players {
    /// # Nation
    ///
    /// The nation stored under `id`.
    ///
    /// Panics if that nation is missing.
    pub fn nation(&self, id: usize) -> &Nation {
        self.players
            .get(&id)
            .unwrap_or_else(|| panic!("nation {id} is not in Players"))
    }

    /// # Nation Mut
    ///
    /// Mutable access to the nation stored under `id`.
    ///
    /// Panics if that nation is missing.
    pub fn nation_mut(&mut self, id: usize) -> &mut Nation {
        self.players
            .get_mut(&id)
            .unwrap_or_else(|| panic!("nation {id} is not in Players"))
    }

    /// # Early Market Day
    ///
    /// The buy-first pass for this market.
    ///
    /// `market` is the market. `factuals` supplies the world. `offers` are
    /// the sell offers already posted. Every nation is called, in id order.
    /// Which nations belong to the market is not written yet.
    pub fn early_market_day(&self, market: &Market, factuals: &Factuals, offers: &[SellOffer]) {
        self.each_nation(|nation| nation.early_market_day(market, factuals, offers));
    }

    /// # Late Market Day
    ///
    /// The buy-last pass for this market.
    ///
    /// `market` is the market. `factuals` supplies the world. `offers` are
    /// the sell offers already posted. Every nation is called, in id order.
    /// Which nations belong to the market is not written yet.
    pub fn late_market_day(&self, market: &Market, factuals: &Factuals, offers: &[SellOffer]) {
        self.each_nation(|nation| nation.late_market_day(market, factuals, offers));
    }

    /// # Each Nation
    ///
    /// Calls `step` for every nation, in id order.
    ///
    /// `step` is the pass. Nothing is returned.
    fn each_nation(&self, mut step: impl FnMut(&Nation)) {
        let mut ids: Vec<usize> = self.players.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            step(self.nation(id));
        }
    }
}
