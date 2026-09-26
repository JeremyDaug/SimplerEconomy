use std::collections::HashMap;

use crate::game::actor::Actor;
use crate::game::factuals::Factuals;
use crate::game::market::MarketHistory;
use crate::game::marketorder::MarketOrder;

/// Everything a seller is currently offering and requesting.
///
/// Shown to the buyer after the two are matched on one good. The match good
/// is only why they met. The basket may move any goods in this book.
#[derive(Debug, Clone)]
pub struct SellerBook {
    pub seller: Actor,
    /// Negative `target_amount`.
    pub offers: Vec<MarketOrder>,
    /// Positive `target_amount`.
    pub requests: Vec<MarketOrder>,
}

/// A buyer's proposed basket.
///
/// `goods` is the buyer's change. Positive units move from the seller to the
/// buyer. Negative units move from the buyer to the seller. A good is only
/// on one side. `freight` is the transport bill for that move, paid by the
/// buyer from transport they hold after the goods have changed hands.
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedDeal {
    pub buyer: Actor,
    pub seller: Actor,
    /// The good that caused the meeting.
    pub match_good: usize,
    pub goods: HashMap<usize, f64>,
    pub freight: f64,
}

/// Seller's verdict on a [`ProposedDeal`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DealResponse {
    Accept,
    Reject,
}

/// An actor the market can match, ask for a basket, and settle.
///
/// `sell_orders`, `buy_orders`, `propose`, and `evaluate` do not move stock.
/// [`DealMaker::finalize`] does. [`DealMaker::reevaluate`] runs after every
/// meeting, accepted or not.
pub trait DealMaker {
    fn actor(&self) -> Actor;

    fn sell_orders(&self, history: &MarketHistory) -> Vec<MarketOrder>;

    fn buy_orders(&self, history: &MarketHistory) -> Vec<MarketOrder>;

    /// Units of `good` this actor can give up without touching a reserve.
    fn free_units(&self, good: usize) -> f64;

    /// Build a basket from `book`, or abandon the meeting.
    fn propose(
        &self,
        match_good: usize,
        book: &SellerBook,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> Option<ProposedDeal> {
        let _ = (self, match_good, book, history, factuals);
        None
    }

    /// Accept or reject `proposal`. Default rejects. Sellers that trade
    /// override this.
    fn evaluate(
        &self,
        proposal: &ProposedDeal,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> DealResponse {
        let _ = (self, proposal, history, factuals);
        DealResponse::Reject
    }

    /// Move this actor's stock for an accepted proposal.
    fn finalize(&mut self, proposal: &ProposedDeal, factuals: &Factuals) {
        let _ = (self, proposal, factuals);
    }

    /// Rewrite orders after a meeting. Pops reserve goods they just received.
    fn reevaluate(&mut self, history: &MarketHistory, rng: &mut dyn rand::RngCore) {
        let _ = (self, history, rng);
    }
}

/// The buy is seeking a good the sell is offering. Different actors.
pub fn matched_on(buy: &MarketOrder, sell: &MarketOrder) -> bool {
    buy.origin != sell.origin
        && buy.target_amount > 0.0
        && sell.target_amount < 0.0
        && buy.target == sell.target
}

/// Seller can give every listed good, receives something, and the payment
/// AMV covers the goods they give. The match good is among those goods.
pub fn seller_can_accept(
    seller: &impl DealMaker,
    proposal: &ProposedDeal,
    history: &MarketHistory,
) -> bool {
    let mut received_amv = 0.0;
    let mut paid_amv = 0.0;
    let mut received = false;
    let mut paid = false;
    for (&good, &qty) in &proposal.goods {
        if qty > 0.0 {
            received = true;
            if seller.free_units(good) < qty {
                return false;
            }
            received_amv += history.price(good).abs() * qty;
        } else if qty < 0.0 {
            paid = true;
            paid_amv += history.price(good).abs() * -qty;
        }
    }
    let match_qty = proposal.goods.get(&proposal.match_good).copied().unwrap_or(0.0);
    received && paid && match_qty >= 1.0 && paid_amv >= received_amv
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

/// Freight still unpaid after transport the buyer will receive and transport
/// they can spend from stock. A negative `goods` entry is units the buyer
/// gives away, so that stock is not available to spend.
pub fn freight_shortfall(
    factuals: &Factuals,
    market_friction: f64,
    goods: &HashMap<usize, f64>,
    buyer_free: &HashMap<usize, f64>,
) -> f64 {
    if !factuals.goods.values().any(|good| good.is_transport()) {
        return 0.0;
    }
    let bill = transport_cost(factuals, market_friction, goods);
    let mut cover = 0.0;
    for (&id, &qty) in goods {
        if qty > 0.0 {
            cover += factuals
                .goods
                .get(&id)
                .map(|good| good.transport_cover(qty))
                .unwrap_or(0.0);
        }
    }
    for (&id, &qty) in buyer_free {
        let given = goods.get(&id).copied().unwrap_or(0.0).min(0.0).abs();
        let spare = (qty - given).max(0.0);
        cover += factuals
            .goods
            .get(&id)
            .map(|good| good.transport_cover(spare))
            .unwrap_or(0.0);
    }
    (bill - cover).max(0.0)
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
    use super::{matched_on, seller_can_accept, transport_cost, ProposedDeal};
    use crate::game::actor::Actor;
    use crate::game::deal::DealMaker;
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag};
    use crate::game::market::MarketHistory;
    use crate::game::marketorder::MarketOrder;
    use std::collections::{HashMap, HashSet};

    struct Stub {
        actor: Actor,
        free: Vec<(usize, f64)>,
    }

    impl DealMaker for Stub {
        fn actor(&self) -> Actor {
            self.actor
        }
        fn sell_orders(&self, _: &MarketHistory) -> Vec<MarketOrder> {
            Vec::new()
        }
        fn buy_orders(&self, _: &MarketHistory) -> Vec<MarketOrder> {
            Vec::new()
        }
        fn free_units(&self, good: usize) -> f64 {
            self.free
                .iter()
                .find(|(id, _)| *id == good)
                .map(|(_, qty)| *qty)
                .unwrap_or(0.0)
        }
    }

    #[test]
    fn matched_on_is_one_good_not_a_payment_pair() {
        let buy = MarketOrder::buy(Actor::Pop(1), 2, 4.0);
        let sell = MarketOrder::sell(Actor::Pop(2), 2, 10.0);
        assert!(matched_on(&buy, &sell));
        let other = MarketOrder::sell(Actor::Pop(2), 3, 10.0);
        assert!(!matched_on(&buy, &other));
        assert!(!matched_on(&buy, &MarketOrder::sell(Actor::Pop(1), 2, 4.0)));
    }

    #[test]
    fn seller_rejects_a_basket_they_cannot_cover() {
        let seller = Stub {
            actor: Actor::Pop(2),
            free: vec![(2, 1.0)],
        };
        let proposal = ProposedDeal {
            buyer: Actor::Pop(1),
            seller: Actor::Pop(2),
            match_good: 2,
            goods: HashMap::from([(2, 4.0), (9, -8.0)]),
            freight: 0.0,
        };
        let history = MarketHistory::new();
        assert!(!seller_can_accept(&seller, &proposal, &history));
    }

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
