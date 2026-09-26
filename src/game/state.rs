use crate::game::actor::Actor;
use crate::game::deal::DealMaker;
use crate::game::factuals::Factuals;
use crate::game::market::MarketHistory;
use crate::game::marketorder::MarketOrder;

/// # State
/// 
/// A State is the interface for players in the game.
#[derive(Debug, Clone)]
pub struct State {
    pub id: usize,
    pub name: String,
}

impl State {
    /// End-of-day bookkeeping for this player state.
    /// Only external input is factuals.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("State record keeping")
    }
}

impl DealMaker for State {
    fn actor(&self) -> Actor {
        Actor::State(self.id)
    }

    fn sell_orders(&self, _history: &MarketHistory) -> Vec<MarketOrder> {
        Vec::new()
    }

    fn buy_orders(&self, _history: &MarketHistory) -> Vec<MarketOrder> {
        Vec::new()
    }

    fn free_units(&self, _good: usize) -> f64 {
        0.0
    }
}
