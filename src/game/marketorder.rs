use crate::game::actor::Actor;
use crate::game::config::market_priority;
use crate::game::util::lerp;

/// # Market Order
/// 
/// A Market order is a message from an actor (player, firm, institution, pop) to a 
/// market they are in. It covers what they want, and need.
/// 
/// It contains: 
/// - Who sent the order.
/// - What they want.
/// - how much they are seeking.
/// 
/// How much they are looking for can be positive or negative. If positive, they are
/// looking to buy. If negative, they'll sell.
/// 
/// It can also include:
/// - Their AMV target for acceptance.
/// - A good they are seeking in return.
/// - The amount of the other good they are requesting.
/// 
/// These last parts are only allowed if the actor has access to Buy and Sell orders.
/// Pops do not have access to this, but firms, institutions, and states do.
///
/// Orders also carry a purchase **order priority** (lower goes first). Named slots
/// live in [`market_priority`]. Bands, ranking, and what is not wired yet are in
/// `docs/proposals/market-order-priority.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct MarketOrder {
    /// Who is making this order.
    pub origin: Actor,
    /// What they are seeking.
    pub target: usize,
    /// How much they are seeking.
    pub target_amount: f64,

    /// The AMV being targeted for the offer maker.
    pub amv_target: Option<f64>,
    /// A good that would be sought out in return for their offer.
    pub counter_offer: Option<usize>,
    /// The amount of their counter_offer good.
    pub counter_offer_amount: Option<f64>,

    /// Buy/request: FCFS sort key, **lower goes first** (actor band / wealth rank).
    /// Sell/offer: selection **weight**, **higher is more likely**. Compose with
    /// [`compose_sell_priority`]; add [`market_priority::SELL_SUCCESS_BONUS`]
    /// after each successful fill.
    pub priority: f64,
}

/// Predefined state / player insert points along the market-day order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateMarketSlot {
    /// Before everyone (`0.0`).
    First,
    /// After institution-before-firms, before merchants (`1.5`).
    BeforeFirms,
    /// After ranked merchants (`FIRM_MERCHANT_END - STATE_FIRM_SLOT_MARGIN`).
    AfterMerchants,
    /// After ranked producers (`FIRM_PRODUCER_END - STATE_FIRM_SLOT_MARGIN`).
    AfterProducers,
    /// After institution-between, before pops (`3.1`).
    AfterFirms,
    /// After institution-after-pops (`5.1`).
    Last,
}

impl StateMarketSlot {
    /// Numeric order priority for this slot. Lower goes first.
    pub fn priority(self) -> f64 {
        match self {
            Self::First => market_priority::STATE_FIRST,
            Self::BeforeFirms => market_priority::STATE_BEFORE_FIRMS,
            Self::AfterMerchants => market_priority::STATE_AFTER_MERCHANTS,
            Self::AfterProducers => market_priority::STATE_AFTER_PRODUCERS,
            Self::AfterFirms => market_priority::STATE_AFTER_FIRMS,
            Self::Last => market_priority::STATE_LAST,
        }
    }
}

/// Maps a unit rank in `[0.0, 1.0)` into `[start, end)`.
/// Rank `0.0` is first in the band (band start).
/// `unit_rank` must be in `[0.0, 1.0)`.
pub fn priority_in_band(start: f64, end: f64, unit_rank: f64) -> f64 {
    debug_assert!(start < end, "start must be < end");
    debug_assert!(
        (0.0..1.0).contains(&unit_rank),
        "unit_rank must be in [0.0, 1.0)"
    );
    lerp(start, end, unit_rank)
}

/// Unit rank from per-household total AMV relative to the market's richest.
///
/// `unit_rank = 1 - wealth / max_wealth`. Richest is `0.0` (first in band).
/// If `max_wealth <= 0`, everyone is `0.0`. A true zero-wealth result would be
/// `1.0`, which is nudged just below so it stays in `[0.0, 1.0)`.
/// Curve grading (compressing the middle of the wealth spread) is later.
pub fn wealth_unit_rank(wealth: f64, max_wealth: f64) -> f64 {
    debug_assert!(wealth.is_finite(), "wealth must be finite");
    debug_assert!(max_wealth.is_finite(), "max_wealth must be finite");
    if max_wealth <= 0.0 {
        0.0
    } else {
        let relative = (wealth / max_wealth).clamp(0.0, 1.0);
        let rank = 1.0 - relative;
        if rank < 1.0 {
            rank
        } else {
            1.0 - f64::EPSILON
        }
    }
}

/// Pop order priority from a unit wealth rank in `[0.0, 1.0)`.
/// Rank `0.0` (richest / first) lands on [`market_priority::POP_START`].
pub fn pop_priority_from_rank(unit_rank: f64) -> f64 {
    priority_in_band(
        market_priority::POP_START,
        market_priority::POP_END,
        unit_rank,
    )
}

/// Pop order priority from per-household total AMV vs the market's richest.
pub fn pop_priority_from_wealth(wealth: f64, max_wealth: f64) -> f64 {
    pop_priority_from_rank(wealth_unit_rank(wealth, max_wealth))
}

/// Merchant / trader firm priority from a unit rank in `[0.0, 1.0)`.
/// Lerps toward [`market_priority::STATE_AFTER_MERCHANTS`] and never reaches it,
/// so that state slot stays after every ranked merchant.
pub fn firm_merchant_priority_from_rank(unit_rank: f64) -> f64 {
    priority_in_band(
        market_priority::FIRM_MERCHANT_START,
        market_priority::STATE_AFTER_MERCHANTS,
        unit_rank,
    )
}

/// Producer firm priority from a unit rank in `[0.0, 1.0)`.
/// Lerps toward [`market_priority::STATE_AFTER_PRODUCERS`] and never reaches it,
/// so that state slot stays after every ranked producer.
pub fn firm_producer_priority_from_rank(unit_rank: f64) -> f64 {
    priority_in_band(
        market_priority::FIRM_PRODUCER_START,
        market_priority::STATE_AFTER_PRODUCERS,
        unit_rank,
    )
}

/// Sell/offer selection weight from who is selling, how much, and past fills.
///
/// `1 / actor_priority + sqrt(supply) + SELL_SUCCESS_BONUS * successful_sells`.
/// `actor_priority` is the buy-style band value (lower = earlier actor).
/// `supply` is units offered (positive). Marketing and other flat adds come later.
pub fn compose_sell_priority(
    actor_priority: f64,
    supply: f64,
    successful_sells: f64,
) -> f64 {
    debug_assert!(actor_priority.is_finite(), "actor_priority must be finite");
    debug_assert!(supply >= 0.0, "supply must be >= 0.0");
    debug_assert!(successful_sells >= 0.0, "successful_sells must be >= 0.0");
    let actor = 1.0 / actor_priority.max(market_priority::SELL_ACTOR_PRIORITY_FLOOR);
    actor + supply.sqrt() + market_priority::SELL_SUCCESS_BONUS * successful_sells
}

/// Origin band checks. Compiled out of release so the match is not executed.
/// Buys use actor-band ranges. Sells use a positive weight (`> 0.0`).
#[cfg(debug_assertions)]
fn assert_priority_for_origin(origin: Actor, priority: f64, target_amount: f64) {
    debug_assert!(priority.is_finite(), "priority must be finite");
    if target_amount < 0.0 {
        debug_assert!(priority > 0.0, "sell priority must be > 0.0");
        return;
    }
    debug_assert!(
        target_amount > 0.0,
        "target_amount must be > 0.0 or < 0.0"
    );
    match origin {
        Actor::Pop(_) => debug_assert!(
            (market_priority::POP_START..market_priority::POP_END).contains(&priority),
            "pop buy priority must be in [POP_START, POP_END)"
        ),
        Actor::Firm(_) => debug_assert!(
            (market_priority::FIRM_MERCHANT_START..market_priority::FIRM_PRODUCER_END)
                .contains(&priority),
            "firm buy priority must be in [FIRM_MERCHANT_START, FIRM_PRODUCER_END)"
        ),
        Actor::Institution(_) | Actor::State(_) => {}
    }
}

#[cfg(not(debug_assertions))]
#[inline(always)]
fn assert_priority_for_origin(_origin: Actor, _priority: f64, _target_amount: f64) {}

impl MarketOrder {
    pub fn buy_order(buyer: Actor, target: usize, target_amount: f64,
    amv_target: f64, counter_offer: usize, counter_offer_amount: f64,
    priority: f64) -> Self {
        debug_assert!(target_amount > 0.0, "Buy Orders must have positive target amounts.");
        debug_assert!(counter_offer_amount < 0.0, "Counter Offers in buy Orders must be negative.");
        assert_priority_for_origin(buyer, priority, target_amount);

        Self {
            origin: buyer,
            target,
            target_amount,
            amv_target: Some(amv_target),
            counter_offer: Some(counter_offer),
            counter_offer_amount: Some(counter_offer_amount),
            priority,
        }
    }

    pub fn sell_order(seller: Actor, target: usize, target_amount: f64,
    amv_target: f64, counter_offer: usize, counter_offer_amount: f64,
    priority: f64) -> Self {
        debug_assert!(target_amount < 0.0, "Sell Orders must have Negative target amounts.");
        debug_assert!(counter_offer_amount > 0.0, "Counter Offers in Sell Orders must be Positive.");
        assert_priority_for_origin(seller, priority, target_amount);

        Self {
            origin: seller,
            target,
            target_amount,
            amv_target: Some(amv_target),
            counter_offer: Some(counter_offer),
            counter_offer_amount: Some(counter_offer_amount),
            priority,
        }
    }

    pub fn offer_order(seller: Actor, target: usize, target_amount: f64,
    priority: f64) -> Self {
        debug_assert!(target_amount < 0.0, "Offer Orders must have negative target amounts.");
        assert_priority_for_origin(seller, priority, target_amount);

        Self {
            origin: seller,
            target,
            target_amount,
            amv_target: None,
            counter_offer: None,
            counter_offer_amount: None,
            priority,
        }
    }

    pub fn request_order(buyer: Actor, target: usize, target_amount: f64,
    priority: f64) -> Self {
        debug_assert!(target_amount > 0.0, "Request Orders must have positive target amounts.");
        assert_priority_for_origin(buyer, priority, target_amount);

        Self {
            origin: buyer,
            target,
            target_amount,
            amv_target: None,
            counter_offer: None,
            counter_offer_amount: None,
            priority,
        }
    }

    /// Sets order priority.
    /// Buy/request: pops in `[POP_START, POP_END)`, firms in
    /// `[FIRM_MERCHANT_START, FIRM_PRODUCER_END)`. Sell/offer: `priority > 0.0`.
    pub fn set_priority(&mut self, priority: f64) {
        assert_priority_for_origin(self.origin, priority, self.target_amount);
        self.priority = priority;
    }

    /// Sets order priority.
    /// Buy/request: pops in `[POP_START, POP_END)`, firms in
    /// `[FIRM_MERCHANT_START, FIRM_PRODUCER_END)`. Sell/offer: `priority > 0.0`.
    pub fn with_priority(mut self, priority: f64) -> Self {
        self.set_priority(priority);
        self
    }

    /// Adds [`market_priority::SELL_SUCCESS_BONUS`] after a successful fill.
    /// Must be a sell or offer order.
    pub fn add_sell_success_bonus(&mut self) {
        debug_assert!(
            self.target_amount < 0.0,
            "sell success bonus is for sell/offer orders"
        );
        self.priority += market_priority::SELL_SUCCESS_BONUS;
    }

    pub fn is_buy_order(&self) -> bool {
        if self.amv_target.is_some() && self.counter_offer.is_some() && self.counter_offer_amount.is_some() {
            // check that the target amount is positive, and the counter_offer_amount is negative 
            if self.target_amount > 0.0 && self.counter_offer_amount.unwrap() < 0.0 {
                true
            } else {
                false
            }
        } else if self.amv_target.is_none() && self.counter_offer.is_none() && self.counter_offer_amount.is_none() {
            // if they are none, then it can't be a buy order.
            false
        } else {
            unreachable!("Market Orders cannot mix it's optionals.");
        }
    }

    pub fn is_sell_order(&self) -> bool {
        if self.amv_target.is_some() && self.counter_offer.is_some() && self.counter_offer_amount.is_some() {
            // check that the target amount is negative, and the counter_offer_amount is positive 
            if self.target_amount < 0.0 && self.counter_offer_amount.unwrap() > 0.0 {
                true
            } else {
                false
            }
        } else if self.amv_target.is_none() && self.counter_offer.is_none() && self.counter_offer_amount.is_none() {
            // if they are none, then it can't be a buy order.
            false
        } else {
            unreachable!("Market Orders cannot mix it's optionals.");
        }
    }

    pub fn is_offer_order(&self) -> bool {
        if self.amv_target.is_some() && self.counter_offer.is_some() && self.counter_offer_amount.is_some() {
            // if they are some, then it can't be a offer or request order.
            false
        } else if self.amv_target.is_none() && self.counter_offer.is_none() && self.counter_offer_amount.is_none() {
            if self.target_amount < 0.0 {
                true
            } else {
                false
            }
        } else {
            unreachable!("Market Orders cannot mix it's optionals.");
        }
    }

    pub fn is_request_order(&self) -> bool {
        if self.amv_target.is_some() && self.counter_offer.is_some() && self.counter_offer_amount.is_some() {
            // if they are some, then it can't be a offer or request order.
            false
        } else if self.amv_target.is_none() && self.counter_offer.is_none() && self.counter_offer_amount.is_none() {
            if self.target_amount > 0.0 {
                true
            } else {
                false
            }
        } else {
            unreachable!("Market Orders cannot mix it's optionals.");
        }
    }
}

#[cfg(test)]
mod market_order_should {
    use super::*;

    #[test]
    fn request_carries_origin_amount_and_priority() {
        let order = MarketOrder::request_order(
            Actor::Pop(3),
            10,
            2.0,
            market_priority::POP_START,
        );
        assert_eq!(order.origin, Actor::Pop(3));
        assert_eq!(order.target, 10);
        assert_eq!(order.target_amount, 2.0);
        assert_eq!(order.priority, market_priority::POP_START);
        assert!(order.is_request_order());
    }

    #[test]
    fn with_priority_restamps_a_pop_order() {
        let order = MarketOrder::request_order(
            Actor::Pop(1),
            7,
            1.0,
            market_priority::POP_START,
        )
        .with_priority(pop_priority_from_rank(0.25));
        assert!((order.priority - 4.25).abs() < 1e-12);
    }

    #[test]
    fn rank_helpers_stay_inside_their_bands() {
        assert_eq!(pop_priority_from_rank(0.0), market_priority::POP_START);
        let rich_mid = pop_priority_from_rank(0.5);
        assert!((rich_mid - 4.5).abs() < 1e-12);
        assert!(rich_mid < market_priority::POP_END);

        assert_eq!(
            firm_merchant_priority_from_rank(0.0),
            market_priority::FIRM_MERCHANT
        );
        let merchant_slot = market_priority::STATE_AFTER_MERCHANTS;
        let merchant_mid = firm_merchant_priority_from_rank(0.5);
        assert!(
            (merchant_mid
                - lerp(
                    market_priority::FIRM_MERCHANT_START,
                    merchant_slot,
                    0.5
                ))
            .abs()
                < 1e-12
        );
        assert!(merchant_mid < merchant_slot);
        assert!(firm_merchant_priority_from_rank(0.999) < merchant_slot);

        assert_eq!(
            firm_producer_priority_from_rank(0.0),
            market_priority::FIRM_PRODUCER
        );
        let producer_slot = market_priority::STATE_AFTER_PRODUCERS;
        let producer_mid = firm_producer_priority_from_rank(0.5);
        assert!(
            (producer_mid
                - lerp(
                    market_priority::FIRM_PRODUCER_START,
                    producer_slot,
                    0.5
                ))
            .abs()
                < 1e-12
        );
        assert!(producer_mid < producer_slot);
        assert!(firm_producer_priority_from_rank(0.999) < producer_slot);
    }

    #[test]
    fn wealth_unit_rank_is_one_minus_share_of_max() {
        assert_eq!(wealth_unit_rank(10.0, 10.0), 0.0);
        assert!((wealth_unit_rank(5.0, 10.0) - 0.5).abs() < 1e-12);
        assert_eq!(wealth_unit_rank(0.0, 0.0), 0.0);
        let poorest = wealth_unit_rank(0.0, 10.0);
        assert!(poorest > 0.0);
        assert!(poorest < 1.0);
        assert_eq!(
            pop_priority_from_wealth(10.0, 10.0),
            market_priority::POP_START
        );
    }

    #[test]
    fn compose_sell_priority_adds_actor_sqrt_supply_and_success() {
        let floor = market_priority::SELL_ACTOR_PRIORITY_FLOOR;
        let bonus = market_priority::SELL_SUCCESS_BONUS;
        assert!((compose_sell_priority(2.0, 0.0, 0.0) - 0.5).abs() < 1e-12);
        assert!((compose_sell_priority(0.0, 0.0, 0.0) - 1.0 / floor).abs() < 1e-12);
        assert!((compose_sell_priority(2.0, 4.0, 0.0) - 2.5).abs() < 1e-12);
        assert!(
            (compose_sell_priority(2.0, 0.0, 3.0) - (0.5 + 3.0 * bonus)).abs() < 1e-12
        );
    }

    #[test]
    fn add_sell_success_bonus_is_a_flat_add() {
        let mut order = MarketOrder::offer_order(
            Actor::Pop(1),
            10,
            -4.0,
            compose_sell_priority(market_priority::POP_START, 4.0, 0.0),
        );
        let before = order.priority;
        order.add_sell_success_bonus();
        assert!(
            (order.priority - (before + market_priority::SELL_SUCCESS_BONUS)).abs() < 1e-12
        );
    }

    #[test]
    fn state_slots_match_named_constants() {
        assert_eq!(StateMarketSlot::First.priority(), market_priority::STATE_FIRST);
        assert_eq!(
            StateMarketSlot::BeforeFirms.priority(),
            market_priority::STATE_BEFORE_FIRMS
        );
        assert_eq!(
            StateMarketSlot::AfterMerchants.priority(),
            market_priority::STATE_AFTER_MERCHANTS
        );
        assert_eq!(
            StateMarketSlot::AfterProducers.priority(),
            market_priority::STATE_AFTER_PRODUCERS
        );
        assert_eq!(
            StateMarketSlot::AfterFirms.priority(),
            market_priority::STATE_AFTER_FIRMS
        );
        assert_eq!(StateMarketSlot::Last.priority(), market_priority::STATE_LAST);
        assert_eq!(
            market_priority::STATE_AFTER_MERCHANTS,
            market_priority::FIRM_MERCHANT_END - market_priority::STATE_FIRM_SLOT_MARGIN
        );
        assert_eq!(
            market_priority::STATE_AFTER_PRODUCERS,
            market_priority::FIRM_PRODUCER_END - market_priority::STATE_FIRM_SLOT_MARGIN
        );
    }

    #[test]
    fn firm_buy_order_accepts_merchant_default() {
        let order = MarketOrder::buy_order(
            Actor::Firm(8),
            1,
            4.0,
            1.0,
            2,
            -4.0,
            market_priority::FIRM_MERCHANT,
        );
        assert!(order.is_buy_order());
        assert_eq!(order.priority, market_priority::FIRM_MERCHANT);
    }
}
