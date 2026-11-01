use std::collections::HashMap;

use crate::game::actor::Actor;
use crate::game::factuals::Factuals;
use crate::game::market::MarketHistory;
use crate::game::marketorder::MarketOrder;

/// A buyer's proposed basket.
///
/// `goods` is the buyer's change. Positive units move from the seller to the
/// buyer. Negative units move from the buyer to the seller. A good is only
/// on one side. `freight` is the transport bill for that move, paid by the
/// buyer from transport they hold after the goods have changed hands.
/// `fresh` is the giver's fresh fraction for each good in `goods`.
/// [`ProposedDeal::stamp_fresh_shares`] writes it. Finalize reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedDeal {
    pub buyer: Actor,
    pub seller: Actor,
    /// The good that caused the meeting.
    pub match_good: usize,
    pub goods: HashMap<usize, f64>,
    /// Giver's fresh fraction for that good. `0.0` to `1.0`. Missing is `0.0`.
    pub fresh: HashMap<usize, f64>,
    pub freight: f64,
}

impl ProposedDeal {
    /// # Stamp Fresh Shares
    ///
    /// Records the giver's fresh fraction for each good in `goods`.
    ///
    /// `share` reads one actor's fraction for one good. Positive quantities
    /// use the seller. Negative quantities use the buyer. Each fraction is
    /// clamped to `0..=1`. A zero quantity is skipped.
    pub fn stamp_fresh_shares(&mut self, share: impl Fn(Actor, usize) -> f64) {
        let lines: Vec<(usize, f64)> = self.goods.iter().map(|(&good, &qty)| (good, qty)).collect();
        for (good, qty) in lines {
            if qty == 0.0 {
                continue;
            }
            let giver = if qty > 0.0 { self.seller } else { self.buyer };
            self.fresh.insert(good, share(giver, good).clamp(0.0, 1.0));
        }
    }

    /// # Fresh Share
    ///
    /// Fresh fraction recorded for `good`.
    ///
    /// A missing entry is `0`. The result is clamped to `0..=1`.
    pub fn fresh_share(&self, good: usize) -> f64 {
        self.fresh
            .get(&good)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0)
    }
}

/// An actor the market can ask for a mass order and settle.
///
/// `prepare_mass_buys` and `mass_order` do not move stock.
/// [`DealMaker::finalize`] does. [`DealMaker::reset_day`] runs at the start of
/// [`crate::game::market::Market::market_day`].
pub trait DealMaker {
    fn actor(&self) -> Actor;

    /// # Prepare Mass Buys
    ///
    /// Lays out this actor's mass-market buys before the clear.
    ///
    /// `history` is the card. `currencies` are the tender slots, empty
    /// entries included. `factuals` names goods and processes. The default
    /// has nothing to buy.
    fn prepare_mass_buys(
        &mut self,
        history: &MarketHistory,
        currencies: &[Option<usize>],
        factuals: &Factuals,
    ) {
        let _ = (self, history, currencies, factuals);
    }

    /// # Mass Market Order
    ///
    /// One mass-market order for `good`, or `None` when this actor sits out.
    ///
    /// `history` is the standing AMVs. `currencies` are the market's tender
    /// slots, empty entries included. `factuals` names goods and processes.
    /// A returned order is a buy when its amount is positive and a sell when
    /// negative. Its accept price is the most a buy will pay, or the least a
    /// sell will take. The default sits out.
    fn mass_order(
        &self,
        good: usize,
        history: &MarketHistory,
        currencies: &[Option<usize>],
        factuals: &Factuals,
    ) -> Option<MarketOrder> {
        let _ = (self, good, history, currencies, factuals);
        None
    }

    /// # Mass Card Sell
    ///
    /// A sell of `good` at the card, or `None` when this actor sits out.
    ///
    /// `history` is the standing AMVs. `currencies` are the tender slots.
    /// `factuals` names goods and processes. The default sits out. A pop
    /// posts the units it will sell at that card.
    fn mass_card_sell(
        &self,
        good: usize,
        history: &MarketHistory,
        currencies: &[Option<usize>],
        factuals: &Factuals,
    ) -> Option<MarketOrder> {
        let _ = (self, good, history, currencies, factuals);
        None
    }

    /// # Mass Conversion Buy
    ///
    /// A buy of `good` paid only from `tenders`, or `None`.
    ///
    /// `history` is the card. `factuals` names goods. The default sits out.
    /// A pop bids the whole units those tenders can pay at the card.
    fn mass_conversion_buy(
        &self,
        good: usize,
        history: &MarketHistory,
        tenders: &[usize],
        factuals: &Factuals,
    ) -> Option<MarketOrder> {
        let _ = (self, good, history, tenders, factuals);
        None
    }

    /// Units of `good` this actor can give up without touching a reserve.
    fn free_units(&self, good: usize) -> f64;

    /// # Fresh Share
    ///
    /// Fraction of `good` on hand that is fresh.
    ///
    /// `good` is the good id. Returns `0` when this actor has no fresh pile.
    /// Pops report [`crate::game::pop_property::PopPRow::fresh_share`].
    /// Firms report [`crate::game::firm::FirmPRow::fresh_share`].
    fn fresh_share(&self, good: usize) -> f64 {
        let _ = (self, good);
        0.0
    }

    /// Move this actor's stock for an accepted proposal.
    fn finalize(&mut self, proposal: &ProposedDeal, factuals: &Factuals) {
        let _ = (self, proposal, factuals);
    }

    /// # Pay Transport
    ///
    /// Spends `amount` of transport cover this actor already holds.
    ///
    /// `factuals` names the transport goods. The default spends nothing.
    /// Nothing is returned.
    fn pay_transport(&mut self, amount: f64, factuals: &Factuals) {
        let _ = (self, amount, factuals);
    }

    /// # Reset Day
    ///
    /// Drops yesterday's same-day state before this market day starts.
    ///
    /// `self` is the actor. Nothing is returned. The default leaves the
    /// actor unchanged. Pops clear satisfaction and reserves here.
    fn reset_day(&mut self) {}

    /// Morning reservation. Pops satisfy open desires, apply their craft, then
    /// reserve inputs for their job. Firms reserve stock for production and savings.
    fn reserve(&mut self, factuals: &Factuals, rng: &mut dyn rand::RngCore) {
        let _ = (self, factuals, rng);
    }

    /// Today's productive work. Pops run their job.
    fn produce(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
    }

    /// Pops eat or use goods for desires.
    fn consume(&mut self) {}

    /// # Rest
    ///
    /// Eats leftover goods a species still accepts after its desires.
    ///
    /// `factuals` supplies that rule. The default eats nothing. A pop's
    /// implementation is [`crate::game::pop::Pop::rest`]. Nothing is returned.
    fn rest(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
    }

    /// Units lost to rot and the stock they came from, per good.
    fn decay_goods(&mut self, factuals: &Factuals) -> HashMap<usize, (f64, f64)> {
        let _ = (self, factuals);
        HashMap::new()
    }

    /// End-of-day books. `history` is still today's card.
    fn record_keeping(&mut self, factuals: &Factuals, history: &MarketHistory) {
        let _ = (self, factuals, history);
    }

    /// End-of-day planning.
    fn plan(&mut self, factuals: &Factuals, history: &MarketHistory) {
        let _ = (self, factuals, history);
    }
}

/// Flat meeting fee plus bulk times market friction, when the world has a
/// transport good. Otherwise 0. Negative bulk cannot pull the bill under the
/// flat fee.
pub fn transport_cost(
    factuals: &Factuals,
    market_friction: f64,
    moved: &HashMap<usize, f64>,
) -> f64 {
    if !factuals.goods.values().any(|good| good.is_transport()) {
        return 0.0;
    }
    let mut bulk = 0.0;
    for (&id, qty) in moved {
        if let Some(good) = factuals.goods.get(&id) {
            bulk += good.bulk() * qty.abs();
        }
    }
    let flat = factuals.config.market.transaction_cost;
    (flat + bulk * market_friction.max(0.0)).max(flat)
}

/// The freight bill for `goods`, or 0 when the world has no transport good.
pub fn freight_bill(factuals: &Factuals, market_friction: f64, goods: &HashMap<usize, f64>) -> f64 {
    if !factuals.goods.values().any(|good| good.is_transport()) {
        return 0.0;
    }
    transport_cost(factuals, market_friction, goods)
}

pub fn amv_total(history: &MarketHistory, goods: &[(usize, f64)]) -> f64 {
    goods
        .iter()
        .map(|(id, qty)| history.price(*id).abs() * qty)
        .sum()
}

/// Whole units, or 0 when `qty` is not a positive finite number.
pub fn whole_units(qty: f64) -> f64 {
    if qty.is_finite() && qty > 0.0 {
        qty.ceil()
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::transport_cost;
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag};
    use std::collections::{HashMap, HashSet};

    #[test]
    fn transport_cost_is_the_flat_fee_when_bulk_is_zero() {
        let mut time = Good {
            id: 0,
            name: "time".to_string(),
            class: None,
            decay_rate: 0.0,
            decay_result: Default::default(),
            mass: 0.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: Vec::new(),
        };
        time.tags.insert(GoodTag::transport(1.0));
        let factuals = Factuals::new().with_good(time);
        let bill = transport_cost(&factuals, 0.0, &HashMap::from([(2, 4.0)]));
        assert!((bill - factuals.config.market.transaction_cost).abs() < 1e-9);
    }
}
