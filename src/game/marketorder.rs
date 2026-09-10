use crate::game::actor::Actor;
use crate::game::config::market_priority;
use crate::game::util::{is_whole_unit, lerp};

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
/// Buy and sell orders (firms, institutions, states) set all three. Pop
/// request and offer orders may name a `counter_offer` **good** without an
/// AMV target or counter amount: a coincidence hint, not a price.
///
/// Orders also carry a purchase **order priority** (lower goes first). Named slots
/// live in [`market_priority`]. Bands, ranking, and what is not wired yet are in
/// `docs/proposals/market-order-priority.md`.
///
/// `target_amount` and `counter_offer_amount` are whole units of goods. A pop
/// or firm may still own a fraction; those crumbs cannot be posted.
/// `amv_target` is a price, not a good, and may be fractional. A
/// transport-tagged good in the order is still a whole-unit exchange.
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
    /// [`compose_sell_priority`]; add [`market_priority::SUCCESSFUL_SELL_BONUS`]
    /// after each successful fill.
    pub priority: f64,

    /// How many failed deals this buy/request has already retried.
    /// Fresh orders are 0. After
    /// [`crate::game::config::market_constants::BUY_TRY_LIMIT`] retries,
    /// a further failure closes the order out.
    pub tries: u32,
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
        self.priority_with(&crate::game::config::MarketPriorityConfig::default())
    }

    /// Numeric order priority using loaded slots.
    pub fn priority_with(self, cfg: &crate::game::config::MarketPriorityConfig) -> f64 {
        match self {
            Self::First => cfg.state_first,
            Self::BeforeFirms => cfg.state_before_firms,
            Self::AfterMerchants => cfg.state_after_merchants(),
            Self::AfterProducers => cfg.state_after_producers(),
            Self::AfterFirms => cfg.state_after_firms,
            Self::Last => cfg.state_last,
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
    pop_priority_from_rank_with(
        unit_rank,
        &crate::game::config::MarketPriorityConfig::default(),
    )
}

/// Pop order priority from a unit wealth rank using loaded pop-band edges.
pub fn pop_priority_from_rank_with(
    unit_rank: f64,
    cfg: &crate::game::config::MarketPriorityConfig,
) -> f64 {
    priority_in_band(cfg.pop_start, cfg.pop_end, unit_rank)
}

/// Pop order priority from per-household total AMV vs the market's richest.
pub fn pop_priority_from_wealth(wealth: f64, max_wealth: f64) -> f64 {
    pop_priority_from_rank(wealth_unit_rank(wealth, max_wealth))
}

/// Merchant / trader firm priority from a unit rank in `[0.0, 1.0)`.
/// Lerps toward [`market_priority::STATE_AFTER_MERCHANTS`] and never reaches it,
/// so that state slot stays after every ranked merchant.
pub fn firm_merchant_priority_from_rank(unit_rank: f64) -> f64 {
    firm_merchant_priority_from_rank_with(
        unit_rank,
        &crate::game::config::MarketPriorityConfig::default(),
    )
}

/// Merchant firm priority from a unit rank using loaded band edges.
pub fn firm_merchant_priority_from_rank_with(
    unit_rank: f64,
    cfg: &crate::game::config::MarketPriorityConfig,
) -> f64 {
    priority_in_band(cfg.firm_merchant_start, cfg.state_after_merchants(), unit_rank)
}

/// Producer firm priority from a unit rank in `[0.0, 1.0)`.
/// Lerps toward [`market_priority::STATE_AFTER_PRODUCERS`] and never reaches it,
/// so that state slot stays after every ranked producer.
pub fn firm_producer_priority_from_rank(unit_rank: f64) -> f64 {
    firm_producer_priority_from_rank_with(
        unit_rank,
        &crate::game::config::MarketPriorityConfig::default(),
    )
}

/// Producer firm priority from a unit rank using loaded band edges.
pub fn firm_producer_priority_from_rank_with(
    unit_rank: f64,
    cfg: &crate::game::config::MarketPriorityConfig,
) -> f64 {
    priority_in_band(cfg.firm_producer_start, cfg.state_after_producers(), unit_rank)
}

/// Sell/offer selection weight from who is selling, how much, and past fills.
///
/// `1 / actor_priority + sqrt(supply) + SUCCESSFUL_SELL_BONUS * successful_sells`.
/// `actor_priority` is the buy-style band value (lower = earlier actor).
/// `supply` is units offered (positive). Marketing and other flat adds come later.
pub fn compose_sell_priority(
    actor_priority: f64,
    supply: f64,
    successful_sells: f64,
) -> f64 {
    compose_sell_priority_with(
        actor_priority,
        supply,
        successful_sells,
        market_priority::SELL_ACTOR_PRIORITY_FLOOR,
        market_priority::SUCCESSFUL_SELL_BONUS,
    )
}

/// Returns sell/offer selection weight from actor band, supply, fills, floor, and bonus.
pub fn compose_sell_priority_with(
    actor_priority: f64,
    supply: f64,
    successful_sells: f64,
    actor_floor: f64,
    success_bonus: f64,
) -> f64 {
    debug_assert!(actor_priority.is_finite(), "actor_priority must be finite");
    debug_assert!(supply >= 0.0, "supply must be >= 0.0");
    debug_assert!(successful_sells >= 0.0, "successful_sells must be >= 0.0");
    debug_assert!(actor_floor > 0.0, "actor_floor must be > 0.0");
    let actor = 1.0 / actor_priority.max(actor_floor);
    actor + supply.sqrt() + success_bonus * successful_sells
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
    let _ = origin;
}

#[cfg(not(debug_assertions))]
#[inline(always)]
fn assert_priority_for_origin(_origin: Actor, _priority: f64, _target_amount: f64) {}

fn assert_whole_amount(amount: f64, what: &str) {
    debug_assert!(
        is_whole_unit(amount),
        "{what} must be a whole unit, got {amount}"
    );
}

impl MarketOrder {
    pub fn buy_order(buyer: Actor, target: usize, target_amount: f64,
    amv_target: f64, counter_offer: usize, counter_offer_amount: f64,
    priority: f64) -> Self {
        debug_assert!(target_amount > 0.0, "Buy Orders must have positive target amounts.");
        debug_assert!(counter_offer_amount < 0.0, "Counter Offers in buy Orders must be negative.");
        assert_whole_amount(target_amount, "buy target_amount");
        assert_whole_amount(counter_offer_amount, "buy counter_offer_amount");
        assert_priority_for_origin(buyer, priority, target_amount);

        Self {
            origin: buyer,
            target,
            target_amount,
            amv_target: Some(amv_target),
            counter_offer: Some(counter_offer),
            counter_offer_amount: Some(counter_offer_amount),
            priority,
            tries: 0,
        }
    }

    pub fn sell_order(seller: Actor, target: usize, target_amount: f64,
    amv_target: f64, counter_offer: usize, counter_offer_amount: f64,
    priority: f64) -> Self {
        debug_assert!(target_amount < 0.0, "Sell Orders must have Negative target amounts.");
        debug_assert!(counter_offer_amount > 0.0, "Counter Offers in Sell Orders must be Positive.");
        assert_whole_amount(target_amount, "sell target_amount");
        assert_whole_amount(counter_offer_amount, "sell counter_offer_amount");
        assert_priority_for_origin(seller, priority, target_amount);

        Self {
            origin: seller,
            target,
            target_amount,
            amv_target: Some(amv_target),
            counter_offer: Some(counter_offer),
            counter_offer_amount: Some(counter_offer_amount),
            priority,
            tries: 0,
        }
    }

    pub fn offer_order(seller: Actor, target: usize, target_amount: f64,
    priority: f64) -> Self {
        debug_assert!(target_amount < 0.0, "Offer Orders must have negative target amounts.");
        assert_whole_amount(target_amount, "offer target_amount");
        assert_priority_for_origin(seller, priority, target_amount);

        Self {
            origin: seller,
            target,
            target_amount,
            amv_target: None,
            counter_offer: None,
            counter_offer_amount: None,
            priority,
            tries: 0,
        }
    }

    pub fn request_order(buyer: Actor, target: usize, target_amount: f64,
    priority: f64) -> Self {
        debug_assert!(target_amount > 0.0, "Request Orders must have positive target amounts.");
        assert_whole_amount(target_amount, "request target_amount");
        assert_priority_for_origin(buyer, priority, target_amount);

        Self {
            origin: buyer,
            target,
            target_amount,
            amv_target: None,
            counter_offer: None,
            counter_offer_amount: None,
            priority,
            tries: 0,
        }
    }

    /// Sets how many failed deals this buy/request has already retried.
    pub fn with_tries(mut self, tries: u32) -> Self {
        self.tries = tries;
        self
    }

    /// Names a preferred counter good without an AMV target or counter amount.
    ///
    /// Request and offer only. `good` must differ from `target`.
    pub fn with_counter_offer(mut self, good: usize) -> Self {
        debug_assert!(
            self.amv_target.is_none() && self.counter_offer_amount.is_none(),
            "priced buy/sell orders set counter amount at construction"
        );
        debug_assert!(good != self.target, "counter_offer must differ from target");
        self.counter_offer = Some(good);
        self
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

    /// Adds [`market_priority::SUCCESSFUL_SELL_BONUS`] after a successful fill.
    /// Must be a sell or offer order.
    pub fn add_successful_sell_bonus(&mut self) {
        self.add_successful_sell_bonus_amount(market_priority::SUCCESSFUL_SELL_BONUS);
    }

    /// Adds `bonus` after a successful fill. Must be a sell or offer order.
    pub fn add_successful_sell_bonus_amount(&mut self, bonus: f64) {
        debug_assert!(
            self.target_amount < 0.0,
            "successful sell bonus is for sell/offer orders"
        );
        self.priority += bonus;
    }

    /// Cuts this sell/offer weight by `fraction` after the seller rejects
    /// (0.10 keeps 90%). Same-day book only. Floors at `floor` (`> 0`).
    pub fn apply_reject_weight_penalty(&mut self, fraction: f64, floor: f64) {
        debug_assert!(
            self.target_amount < 0.0,
            "reject weight penalty is for sell/offer orders"
        );
        debug_assert!(fraction.is_finite() && fraction >= 0.0 && fraction <= 1.0);
        debug_assert!(floor.is_finite() && floor > 0.0, "floor must be finite and > 0.0");
        if fraction <= 0.0 {
            return;
        }
        let next = (self.priority * (1.0 - fraction)).max(floor);
        self.set_priority(next);
    }

    pub fn is_buy_order(&self) -> bool {
        if self.is_priced() {
            self.target_amount > 0.0 && self.counter_offer_amount.unwrap() < 0.0
        } else if self.is_unpriced() {
            false
        } else {
            unreachable!("Market Orders cannot mix their optionals.");
        }
    }

    pub fn is_sell_order(&self) -> bool {
        if self.is_priced() {
            self.target_amount < 0.0 && self.counter_offer_amount.unwrap() > 0.0
        } else if self.is_unpriced() {
            false
        } else {
            unreachable!("Market Orders cannot mix their optionals.");
        }
    }

    pub fn is_offer_order(&self) -> bool {
        if self.is_priced() {
            false
        } else if self.is_unpriced() {
            self.target_amount < 0.0
        } else {
            unreachable!("Market Orders cannot mix their optionals.");
        }
    }

    pub fn is_request_order(&self) -> bool {
        if self.is_priced() {
            false
        } else if self.is_unpriced() {
            self.target_amount > 0.0
        } else {
            unreachable!("Market Orders cannot mix their optionals.");
        }
    }

    /// Buy/sell: AMV target, named counter, and counter amount are all set.
    fn is_priced(&self) -> bool {
        self.amv_target.is_some()
            && self.counter_offer.is_some()
            && self.counter_offer_amount.is_some()
    }

    /// Request/offer: no AMV target and no counter amount. A counter **good**
    /// is allowed as a coincidence hint.
    fn is_unpriced(&self) -> bool {
        self.amv_target.is_none() && self.counter_offer_amount.is_none()
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
        assert_eq!(order.tries, 0);
        assert!(order.is_request_order());
    }

    #[test]
    fn named_counter_good_keeps_request_and_offer_unpriced() {
        let request = MarketOrder::request_order(
            Actor::Pop(1),
            10,
            2.0,
            market_priority::POP_START,
        )
        .with_counter_offer(7);
        assert!(request.is_request_order());
        assert!(!request.is_buy_order());
        assert_eq!(request.counter_offer, Some(7));
        assert!(request.amv_target.is_none());
        assert!(request.counter_offer_amount.is_none());

        let offer = MarketOrder::offer_order(
            Actor::Pop(1),
            8,
            -3.0,
            1.5,
        )
        .with_counter_offer(10);
        assert!(offer.is_offer_order());
        assert!(!offer.is_sell_order());
        assert_eq!(offer.counter_offer, Some(10));
        assert!(offer.amv_target.is_none());
        assert!(offer.counter_offer_amount.is_none());
    }

    #[test]
    fn with_priority_updates_a_pop_order() {
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
        let bonus = market_priority::SUCCESSFUL_SELL_BONUS;
        assert!((compose_sell_priority(2.0, 0.0, 0.0) - 0.5).abs() < 1e-12);
        assert!((compose_sell_priority(0.0, 0.0, 0.0) - 1.0 / floor).abs() < 1e-12);
        assert!((compose_sell_priority(2.0, 4.0, 0.0) - 2.5).abs() < 1e-12);
        assert!(
            (compose_sell_priority(2.0, 0.0, 3.0) - (0.5 + 3.0 * bonus)).abs() < 1e-12
        );
    }

    #[test]
    fn add_successful_sell_bonus_is_a_flat_add() {
        let mut order = MarketOrder::offer_order(
            Actor::Pop(1),
            10,
            -4.0,
            compose_sell_priority(market_priority::POP_START, 4.0, 0.0),
        );
        let before = order.priority;
        order.add_successful_sell_bonus();
        assert!(
            (order.priority - (before + market_priority::SUCCESSFUL_SELL_BONUS)).abs() < 1e-12
        );
    }

    #[test]
    fn apply_reject_weight_penalty_cuts_ten_percent() {
        let mut order = MarketOrder::offer_order(
            Actor::Pop(1),
            10,
            -4.0,
            2.0,
        );
        order.apply_reject_weight_penalty(
            market_priority::SELL_REJECT_WEIGHT,
            market_priority::SELL_ACTOR_PRIORITY_FLOOR,
        );
        assert!((order.priority - 1.8).abs() < 1e-12);
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
