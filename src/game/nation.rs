
use crate::game::factuals::Factuals;
use crate::game::market::Market;
use crate::game::pop::SellOffer;

/// # Nation
///
/// A Nation is the interface for players in the game.
#[derive(Debug, Clone)]
pub struct Nation {
    pub id: usize,
    pub name: String,
}

impl Nation {
    /// # Early Market Day
    ///
    /// This nation's buy-first pass in `market`.
    ///
    /// `factuals` supplies the world. `offers` are the sell offers already
    /// posted. The pass is not written yet.
    pub fn early_market_day(&self, market: &Market, factuals: &Factuals, offers: &[SellOffer]) {
        let _ = (self, market, factuals, offers);
    }

    /// # Late Market Day
    ///
    /// This nation's buy-last pass in `market`.
    ///
    /// `factuals` supplies the world. `offers` are the sell offers already
    /// posted. The pass is not written yet.
    pub fn late_market_day(&self, market: &Market, factuals: &Factuals, offers: &[SellOffer]) {
        let _ = (self, market, factuals, offers);
    }
}
