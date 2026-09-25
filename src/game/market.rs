use std::collections::{HashMap, HashSet};

use circular_buffer::CircularBuffer;
use rand::Rng;

use crate::game::actor::Actor;
use crate::game::config::{market_constants, market_priority, MarketConfig};
use crate::game::firm::Firm;
use crate::game::good::TIME;
use crate::game::marketorder::MarketOrder;
use crate::game::pop::Pop;
use crate::game::util::{lerp, whole_units};
use crate::game::{actors::Actors, factuals::Factuals};

/// One buy/sell pair from [`Market::match_orders`].
///
/// Indices refer to the `buys` / `sells` slices passed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderMatch {
    pub buy_index: usize,
    pub sell_index: usize,
}

/// One matching pass: at most one deal. When none, `unmatched_buys` is every
/// remaining buy (no other-origin seller).
///
/// Indices refer to the slices passed in. The matcher does not remove them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderMatchBatch {
    pub matched: Option<OrderMatch>,
    pub unmatched_buys: Vec<usize>,
}

impl OrderMatchBatch {
    fn empty() -> Self {
        Self {
            matched: None,
            unmatched_buys: Vec::new(),
        }
    }

    /// True when there is nothing for the caller to deal or update.
    pub fn is_empty(&self) -> bool {
        self.matched.is_none() && self.unmatched_buys.is_empty()
    }
}

/// Why a matched pair washed instead of trading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WashReason {
    /// Buyer [`DealMaker::buy`] returned `None`.
    NoProposal,
    /// Seller did not [`DealResponse::Accept`].
    Rejected,
    /// Accepted map did not move the target good.
    EmptyFill,
}

/// Result of one matched buy/sell meeting.
#[derive(Debug, Clone, PartialEq)]
pub enum MeetingOutcome {
    /// Seller accepted. `goods` is the seller's inventory change.
    Traded {
        goods: HashMap<usize, f64>,
        transport_needed: f64,
    },
    /// No trade. Flat meeting fee from on-hand. `closed` means the buy was
    /// not renewed.
    Wash {
        reason: WashReason,
        transport: f64,
        closed: bool,
    },
}

/// One buy/sell pair that reached `settle_pair` (traded or washed).
#[derive(Debug, Clone, PartialEq)]
pub struct MarketMeeting {
    pub buy: MarketOrder,
    pub sell: MarketOrder,
    pub outcome: MeetingOutcome,
}

/// What [`Market::run_market_day`] did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MarketDayReport {
    /// Buys with no other-origin seller. Marked unavailable. Not a meeting.
    pub unmatched_buys: Vec<MarketOrder>,
    /// Each matched pair, in day order, traded or washed.
    pub meetings: Vec<MarketMeeting>,
    /// Buys still in the book when the loop stopped.
    pub leftover_buys: Vec<MarketOrder>,
    /// Sells still in the book when the loop stopped.
    pub leftover_sells: Vec<MarketOrder>,
}

/// Walk `weights` with a `roll` in `[0, sum)`. Last index wins leftover float dust.
fn pick_weighted_index(weights: &[f64], mut roll: f64) -> usize {
    for (i, weight) in weights.iter().enumerate() {
        if *weight <= 0.0 {
            continue;
        }
        if roll < *weight {
            return i;
        }
        roll -= *weight;
    }
    weights.len().saturating_sub(1)
}

/// Sell selection weight for this pick: listed units, times coincidence
/// when both named counters match.
fn sell_match_weight_with(
    buy: &MarketOrder,
    sell: &MarketOrder,
    coincidence_weight: f64,
) -> f64 {
    let mut weight = (-sell.target_amount).max(0.0);
    if matching_counter_offers(buy, sell) {
        weight *= coincidence_weight;
    }
    weight
}

fn matching_counter_offers(buy: &MarketOrder, sell: &MarketOrder) -> bool {
    match (buy.counter_offer, sell.counter_offer) {
        (Some(buy_good), Some(sell_good)) => buy_good == sell_good,
        _ => false,
    }
}

/// Other-origin sells of `buy.target`. `had_other` is true if any exist.
fn classify_sells(
    sells: &[MarketOrder],
    buy: &MarketOrder,
    coincidence_weight: f64,
) -> (Vec<usize>, Vec<f64>, bool) {
    let start = sells.partition_point(|s| s.target < buy.target);
    let end = start + sells[start..].partition_point(|s| s.target == buy.target);
    let mut available = Vec::new();
    let mut weights = Vec::new();
    for i in start..end {
        if sells[i].origin == buy.origin || sells[i].target_amount >= 0.0 {
            continue;
        }
        available.push(i);
        weights.push(sell_match_weight_with(buy, &sells[i], coincidence_weight));
    }
    let had_other = !available.is_empty();
    (available, weights, had_other)
}

fn pick_available_sell<R: Rng + ?Sized>(
    available: &[usize],
    weights: &[f64],
    rng: &mut R,
) -> Option<usize> {
    if available.is_empty() {
        return None;
    }
    let total: f64 = weights.iter().sum();
    let pick = if total > 0.0 && total.is_finite() {
        pick_weighted_index(weights, rng.random_range(0.0..total))
    } else {
        rng.random_range(0..available.len())
    };
    Some(available[pick])
}



impl Market {
}

/// Pushes each order into the buy book (`target_amount` > 0) or the sell book
/// (`target_amount` < 0). Zero-amount orders are dropped.
/// Goods a remainder owner already makes in-shop. They are not leftover
/// market demand and are not posted as buys.
fn remainder_owner_firm(pop_id: usize, firms: &HashMap<usize, Firm>) -> Option<&Firm> {
    firms
        .values()
        .find(|firm| firm.owners.liable && firm.owners.pop_id() == Some(pop_id))
}

fn remainder_pantry_goods(
    pop_id: usize,
    firms: &HashMap<usize, Firm>,
    factuals: &Factuals,
) -> HashSet<usize> {
    let mut goods = HashSet::new();
    if let Some(firm) = remainder_owner_firm(pop_id, firms) {
        goods.extend(firm.produced_goods(factuals));
    }
    goods
}

fn split_into_books(
    orders: Vec<MarketOrder>,
    buys: &mut Vec<MarketOrder>,
    sells: &mut Vec<MarketOrder>,
) {
    for order in orders {
        if order.target_amount > 0.0 {
            buys.push(order);
        } else if order.target_amount < 0.0 {
            sells.push(order);
        }
    }
}

/// Returns `order` with `filled` units removed, or `None` if nothing remains.
/// Scales a named counter amount by the same remaining/original ratio.
/// Leftover target and counter amounts are whole units.
fn leftover_order(mut order: MarketOrder, filled: f64) -> Option<MarketOrder> {
    debug_assert!(filled >= 0.0, "filled must be >= 0.0");
    let original = order.target_amount;
    if original > 0.0 {
        order.target_amount = whole_units(order.target_amount - filled);
        if order.target_amount <= 0.0 {
            return None;
        }
    } else {
        order.target_amount = whole_units(order.target_amount + filled);
        if order.target_amount >= 0.0 {
            return None;
        }
    }
    if let Some(counter) = order.counter_offer_amount.as_mut() {
        if original != 0.0 {
            let scaled = *counter * order.target_amount / original;
            let mut qty = whole_units(scaled);
            if qty == 0.0 && scaled != 0.0 {
                qty = scaled.signum();
            }
            *counter = qty;
        }
    }
    Some(order)
}

/// # Market
/// 
/// The market holds the actors and acts on stuff. It is what abstracts physical details
/// from the region away and consolidates it into the goods that can be bought, sold,
/// and traded.
/// 
/// Things which can't be moved out of a region can't be moved out of a market, and so on.
#[derive(Debug, Clone)]
pub struct Market {
    /// The unique ID of the market, should match the ID of the region it represents.
    pub id: usize,
    /// The pops in the market.
    pub pops: HashSet<usize>,
    /// The firms in the market.
    pub firms: HashSet<usize>,
    /// Institutions present in this market (membership only; actors own the data).
    ///
    /// An institution may appear in multiple markets via its own `markets` list.
    pub institution_ids: HashSet<usize>,
    /// The goods in the market and records of them available to all.
    /// 
    /// If needed, this will have to be culled and cleaned out of old goods periodically.
    /// 
    /// The key is the ID of the good.
    pub goods: HashMap<usize, MarketGood>,
    /// Distance / size multiplier on deal bulk. 0 on a one-hex market.
    /// Transport bill is `TRANSACTION_COST + bulk * friction`.
    pub friction: f64,
    /// Goods with no other-origin seller today. Passed into `create_orders`.
    /// Cleared at market-day start; unmatched buys insert here.
    pub unavailable_goods: HashSet<usize>,
    /// Market days completed. Incremented at the end of each
    /// [`Market::run_market_day`]. AMV rescale uses this against
    /// [`MarketConfig::amv_rescale_period`].
    pub market_days: u32,
    /// Unfilled buy/request units by good at market close (leftover book plus
    /// unmatched). Plan treats this as remaining demand, not a sell miss.
    pub leftover_buy: HashMap<usize, f64>,
}

impl Market {
    /// Empty market with this id. No pops, firms, institutions, or goods.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            pops: HashSet::new(),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods: HashMap::new(),
            friction: 0.0,
            unavailable_goods: HashSet::new(),
            market_days: 0,
            leftover_buy: HashMap::new(),
        }
    }

    /// Sets the market friction factor. Must be `>= 0.0`.
    pub fn with_friction(mut self, friction: f64) -> Self {
        debug_assert!(friction >= 0.0, "friction must be >= 0.0");
        self.friction = friction;
        self
    }

    /// End-of-day market bookkeeping (prices, volume history, clear day locals, …).
    /// Only external input is factuals; does not touch actors.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Market record keeping")
    }

    /// Aggregate pop emigration and firm hiring pressures for this market region.
    /// Reads actor pressures already computed; does not move actors between markets.
    pub fn sum_migratory_pressure(&mut self, actors: &Actors, factuals: &Factuals) {
        let _ = (self, actors, factuals);
        todo!("Market sum migratory pressure (positive / negative / net, migrant pool)")
    }

    pub fn run_market_day<R: Rng + ?Sized>(
        &mut self,
        factuals: &Factuals,
        pops: &mut HashMap<usize, Pop>,
        firms: &mut HashMap<usize, Firm>,
        rng: &mut R,
    ) -> MarketDayReport {
        todo!();
        MarketDayReport::default()
    }

    /// # Rescale AMV To Mean
    ///
    /// Multiplies live AMV and average price so the unweighted mean of one
    /// unit of each tradeable good equals `target`. Time is skipped.
    ///
    /// Returns the scale applied, or `1.0` when this is a no-op. Does not
    /// rewrite the AMV trail: recorded closes already live in that day's
    /// mean units, so scaling them again compounds the unit change into
    /// diff/trend. Does not change salability. Callers scale firm AMV
    /// quotes by the same factor ([`Firm::scale_amv_unit`]).
    pub fn rescale_amv_to_mean(&mut self, target: f64, min_abs: f64) -> f64 {
        if !(target.is_finite() && target > 0.0) {
            return 1.0;
        }
        let n = self.goods.len() as f64;
        if n <= 0.0 {
            return 1.0;
        }
        let mean: f64 = self
            .goods
            .iter()
            .map(|(_, good)| good.amv)
            .sum::<f64>()
            / n;
        if !mean.is_finite() || mean.abs() < min_abs {
            return 1.0;
        }
        let scale = target / mean;
        if !scale.is_finite() {
            return 1.0;
        }
        for (&id, good) in self.goods.iter_mut() {
            if id == TIME {
                continue;
            }
            good.set_amv_min(good.amv * scale, min_abs);
            good.set_average_price_min(good.average_price * scale, min_abs);
        }
        scale
    }

    /// Pushes current AMV into an empty history ring (the opening AMV).
    fn seed_amv_history(&mut self) {
        for good in self.goods.values_mut() {
            if good.amv_history.is_empty() {
                good.record_amv();
            }
        }
    }

    /// Pushes each good's current AMV as today's close.
    fn record_amv_closes(&mut self) {
        for good in self.goods.values_mut() {
            good.record_amv();
        }
    }

    /// Lerps each good's salability toward `payment / tender` when it was
    /// offered as payment today. Goods with no tender are left alone.
    fn update_salability(&mut self, cfg: &crate::game::config::MarketConfig) {
        let blend = cfg.salability_blend;
        for good in self.goods.values_mut() {
            if good.tender <= 0.0 {
                continue;
            }
            let accept = (good.payment / good.tender).clamp(0.0, 1.0);
            if !accept.is_finite() {
                continue;
            }
            let target = accept;
            good.set_salability(lerp(good.salability, target, blend));
        }
    }

    /// # Cap Salability From Decay
    ///
    /// Caps live salability at `1 - decayed / volume` per good.
    ///
    /// `rot` is `(decayed, volume)` from [`Pop::decay_goods`] /
    /// [`Firm::decay_goods`]. Volume is leftover on-hand after `used` is
    /// returned, plus `consumed`. Eaten stock counts as volume, not rot, so a
    /// good that is fully consumed and never leftover-decays is not capped.
    /// Zero or missing volume leaves salability unchanged. Does not raise
    /// salability. Unknown market goods are skipped.
    ///
    /// Call after decay, before record keeping so save ranking sees the cap.
    pub fn cap_salability_from_decay(&mut self, rot: &HashMap<usize, (f64, f64)>) {
        for (&id, &(decayed, volume)) in rot {
            if volume <= 0.0 {
                debug_assert!(decayed <= 0.0, "decayed must be <= 0.0 when volume <= 0.0");
                continue;
            }
            debug_assert!(decayed.is_finite() && volume.is_finite(), "decay rot must be finite");
            debug_assert!(decayed >= 0.0, "decayed must be >= 0.0");
            let rot_frac = (decayed / volume).clamp(0.0, 1.0);
            let cap = (1.0 - rot_frac);
            let Some(good) = self.goods.get_mut(&id) else {
                continue;
            };
            if good.salability > cap {
                good.set_salability(cap);
            }
        }
    }

    /// Pulls AMV of the sold good and its tenders toward the midpoint of the
    /// basket totals. Uses live [`MarketGood::amv`], not the frozen history.
    fn drift_amv_on_accept(
        &mut self,
        target: usize,
        filled: f64,
        goods: &HashMap<usize, f64>,
        cfg: &crate::game::config::MarketConfig,
    ) {
    }

    /// Lowers salability on goods that failed as payment. Does not move AMV.
    /// `blend` is the lerp toward 0 (pop reject uses `salability_blend`;
    /// firm reject uses that times `salability_firm_reject_scale`).
    fn drift_salability_on_reject(
        &mut self,
        target: usize,
        goods: &HashMap<usize, f64>,
        cfg: &crate::game::config::MarketConfig,
        blend: f64,
    ) {
    }

    /// ±`amv_imbalance_kick` on each tradeable good toward heavier opening
    /// demand vs supply. Live kick is **flat ±1 AMV**. Tie (including both 0)
    /// does not move. Time skipped.
    ///
    /// Deferred if flat ±1 is too strong on cheap goods: (1) ±1% of |AMV|
    /// (`old * 0.01`) — scale-invariant, but the bottom sagged in a seed-1
    /// 60-day compare; (2) asymmetric +1 demand / −1% supply — rescue cheap
    /// goods people still want, don't hammer a glut by a flat dollar.
    fn nudge_amv_from_imbalance(&mut self, cfg: &MarketConfig) {
    }

    /// Emits pop and firm orders for this market and splits them into buy and
    /// sell books.
    fn collect_orders(
        &self,
        history: &MarketHistory,
        factuals: &Factuals,
        pops: &mut HashMap<usize, Pop>,
        firms: &HashMap<usize, Firm>,
    ) -> (Vec<MarketOrder>, Vec<MarketOrder>) {
        let mut buys = Vec::new();
        let mut sells = Vec::new();

        (buys, sells)
    }

    /// Zeros today's exchange counters on every recorded good. Leaves AMV,
    /// salability, average price, stock, production, consumption, and imports.
    fn reset_day_exchange_stats(&mut self) {
        for (&id, good) in self.goods.iter_mut() {
            if id == TIME {
                continue;
            }
            good.set_supply(0.0);
            good.set_suppliers(0.0);
            good.set_demand(0.0);
            good.set_buyers(0.0);
            good.set_requests(0.0);
            good.set_purchased(0.0);
            good.set_tender(0.0);
            good.set_payment(0.0);
        }
    }

    /// Writes opening supply, demand, unique buyers, and unique suppliers
    /// from the current books onto [`MarketGood`] rows.
    fn collate_order_books(
        &mut self,
        buys: &[MarketOrder],
        sells: &[MarketOrder],
        cfg: &MarketConfig,
    ) {
        let mut demand: HashMap<usize, f64> = HashMap::new();
        let mut supply: HashMap<usize, f64> = HashMap::new();
        let mut buyers: HashMap<usize, HashSet<Actor>> = HashMap::new();
        let mut suppliers: HashMap<usize, HashSet<Actor>> = HashMap::new();

        for order in buys {
            *demand.entry(order.target).or_insert(0.0) += order.target_amount;
            buyers.entry(order.target).or_default().insert(order.origin);
        }
        for order in sells {
            *supply.entry(order.target).or_insert(0.0) += -order.target_amount;
            suppliers
                .entry(order.target)
                .or_default()
                .insert(order.origin);
        }

        for (good, qty) in demand {
            let n = buyers.get(&good).map(|set| set.len() as f64).unwrap_or(0.0);
            let row = self.market_good_mut(good, cfg);
            row.set_demand(qty);
            row.set_buyers(n);
        }
        for (good, qty) in supply {
            let n = suppliers
                .get(&good)
                .map(|set| set.len() as f64)
                .unwrap_or(0.0);
            let row = self.market_good_mut(good, cfg);
            row.set_supply(qty);
            row.set_suppliers(n);
        }
    }

    /// Returns the row for `good`, inserting a config-default good if it is new.
    fn market_good_mut(&mut self, good: usize, cfg: &MarketConfig) -> &mut MarketGood {
        self.goods
            .entry(good)
            .or_insert_with(|| MarketGood::from_config(cfg))
    }

    /// Adds `qty` to this good's deal-request total.
    fn add_requests(&mut self, good: usize, qty: f64, cfg: &MarketConfig) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        let row = self.market_good_mut(good, cfg);
        row.set_requests(row.requests + qty);
    }

    /// Adds `qty` to this good's offered-as-payment total.
    fn add_tender(&mut self, good: usize, qty: f64, cfg: &MarketConfig) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        let row = self.market_good_mut(good, cfg);
        row.set_tender(row.tender + qty);
    }

    /// Adds `qty` to this good's accepted-as-payment total.
    fn add_payment(&mut self, good: usize, qty: f64, cfg: &MarketConfig) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        let row = self.market_good_mut(good, cfg);
        row.set_payment(row.payment + qty);
    }

    /// Records a successful purchase of `qty` at `unit_price` on the target
    /// good (purchased and rolling average price). Volume is derived.
    fn record_exchange(&mut self, good: usize, qty: f64, unit_price: f64, cfg: &MarketConfig) {
        debug_assert!(qty >= 0.0, "qty must be >= 0.0");
        debug_assert!(unit_price.is_finite(), "unit_price must be finite");
        let row = self.market_good_mut(good, cfg);
        let prev_qty = row.purchased;
        let prev_avg = row.average_price;
        let new_qty = prev_qty + qty;
        row.set_purchased(new_qty);
        if new_qty > 0.0 {
            row.set_average_price_min(
                (prev_avg * prev_qty + unit_price * qty) / new_qty,
                cfg.amv_min_abs,
            );
        }
    }
    /// # History
    ///
    /// Snapshot of current AMVs and salability for pop record keeping and
    /// sentiment wealth. Readers default missing prices to 1.0 and missing
    /// salability to [`market_constants::SALABILITY_DEFAULT`].
    pub fn history(&self) -> MarketHistory {
        self.history_with_salability(market_constants::SALABILITY_DEFAULT)
    }

    /// Snapshot using a loaded missing-salability default.
    pub fn history_with(&self, cfg: &MarketConfig) -> MarketHistory {
        self.history_with_salability(cfg.salability_default)
    }

    fn history_with_salability(&self, default_salability: f64) -> MarketHistory {
        let mut history = MarketHistory::new();
        history.default_salability = default_salability;
        for (&good_id, good) in &self.goods {
            history.prices.insert(good_id, good.amv);
            history.salability.insert(good_id, good.salability);
            history.purchased.insert(good_id, good.purchased);
            history.amv_history.insert(good_id, good.amv_trail());
        }
        history.leftover_demand = self.leftover_buy.clone();
        history.friction = self.friction;
        history
    }
}

/// # Market History
/// 
/// A saved record of minimal data for passing around.
#[derive(Debug, Clone)]
pub struct MarketHistory {
    /// Last known AMV price per good.
    pub prices: HashMap<usize, f64>,
    /// Last known salability per good, typically in 0.0..=1.0.
    pub salability: HashMap<usize, f64>,
    /// Market friction factor copied from [`Market::friction`].
    pub friction: f64,
    /// Used when a good has no recorded salability. Default 0.4.
    pub default_salability: f64,
    /// Units purchased today. Missing = unknown share.
    pub purchased: HashMap<usize, f64>,
    /// Oldest-to-newest AMV closes. Empty = unknown trend and volatility.
    /// TODO: replace Vec with circular buffer for performance.
    pub amv_history: HashMap<usize, Vec<f64>>,
    /// Unfulfilled buy/request units by good at market close. Missing = 0.
    pub leftover_demand: HashMap<usize, f64>,
}

impl Default for MarketHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-market AMV snapshots plus pop-to-market membership.
/// Histories are day-static; rebuild membership after pops move.
#[derive(Debug, Clone, Default)]
pub struct MarketLookups {
    pub histories: HashMap<usize, MarketHistory>,
    pub pop_to_market: HashMap<usize, usize>,
}

impl MarketLookups {
    pub fn new() -> Self {
        Self::default()
    }

    /// One history per market, and each member pop id mapped to that market id.
    pub fn from_markets(markets: &HashMap<usize, Market>) -> Self {
        Self::from_markets_with(markets, market_constants::SALABILITY_DEFAULT)
    }

    /// Same as [`Self::from_markets`], using a loaded missing-salability default.
    pub fn from_markets_with(
        markets: &HashMap<usize, Market>,
        default_salability: f64,
    ) -> Self {
        let mut histories = HashMap::new();
        let mut pop_to_market = HashMap::new();
        for market in markets.values() {
            let mut history = market.history();
            history.default_salability = default_salability;
            histories.insert(market.id, history);
            for &pop_id in &market.pops {
                pop_to_market.insert(pop_id, market.id);
            }
        }
        Self {
            histories,
            pop_to_market,
        }
    }

    /// History for `pop_id`'s market, or `empty` if the pop is in none.
    pub fn history_for_pop<'a>(
        &'a self,
        pop_id: usize,
        empty: &'a MarketHistory,
    ) -> &'a MarketHistory {
        self.pop_to_market
            .get(&pop_id)
            .and_then(|mid| self.histories.get(mid))
            .unwrap_or(empty)
    }
}

impl MarketHistory {
    pub(crate) fn new() -> Self {
        Self { 
            prices: HashMap::new(),
            salability: HashMap::new(),
            friction: 0.0,
            default_salability: market_constants::SALABILITY_DEFAULT,
            purchased: HashMap::new(),
            amv_history: HashMap::new(),
            leftover_demand: HashMap::new(),
        }
    }

    /// Unfilled buy/request units for `good_id` at market close, or 0.
    pub fn leftover_buy(&self, good_id: usize) -> f64 {
        self.leftover_demand.get(&good_id).copied().unwrap_or(0.0)
    }

    /// Price for `good_id`, or 1.0 if missing.
    pub fn price(&self, good_id: usize) -> f64 {
        self.prices.get(&good_id).copied().unwrap_or(1.0)
    }

    /// Salability for `good_id`, or this snapshot's default if missing.
    pub fn salability(&self, good_id: usize) -> f64 {
        self.salability
            .get(&good_id)
            .copied()
            .unwrap_or(self.default_salability)
    }

    /// Highest recorded salability among tradeable goods in this snapshot.
    /// Time is skipped. Returns 0.0 when the snapshot has no other goods.
    pub fn max_salability(&self) -> f64 {
        let mut ids: HashSet<usize> = self.prices.keys().copied().collect();
        ids.extend(self.salability.keys().copied());
        ids.remove(&TIME);
        ids.iter()
            .map(|&id| self.salability(id))
            .fold(0.0, f64::max)
    }
}

/// If `new` is inside the AMV dead zone, land `min_abs` on the other side
/// of 0 from `old`. Otherwise return `new` unchanged.
fn bounce_away_from_zero(old: f64, new: f64, min_abs: f64) -> f64 {
    debug_assert!(new.is_finite(), "new must be finite");
    debug_assert!(min_abs > 0.0, "min_abs must be > 0.0");
    if new.abs() >= min_abs {
        new
    } else if old >= 0.0 {
        -min_abs
    } else {
        min_abs
    }
}

/// # Market Good
/// 
/// Publically available data for a maret good.
/// 
/// Records overall production, consumption, buying/selling (both times traded and 
/// quantity traded), and the current estimated Abstract Market Value (AMV).
/// 
/// Note: Consumption means both consumed by a pop for their needs, and consumed by a 
/// firm for production purposes. It does not currently distinguish between the two.
#[derive(Debug, Clone)]
pub struct MarketGood {
    // Valuation data. Key features of exchange data.
    /// The current Abstract Market Value, an estimation of it's market value.
    pub amv: f64,
    /// The current salability of the good. Must be bound between [0.0 and 1.0].
    /// 
    /// Low Salability means it's hard to sell and generally illiquid.
    /// High Salability means its easy to sell, generally liquid.
    /// 
    /// Salability factors:
    /// - Salability is pushed up or down relative to the history of being accepted or 
    /// rejected.
    /// - The total Volume Moved and Velocity of trades of the good. 
    /// - External effects (Culture, Institutions, State action, etc)
    /// - Safety of Value (how often does it lose value and how much does it lose).
    /// 
    /// Possible Additional Factors:
    /// - Price Impact, 
    pub salability: f64,

    // placeholder for AMV Historical records.
    /// Closing AMVs for the last [`market_constants::AMV_HISTORY_MAX`] market
    /// days, oldest first. The first sample is the opening AMV on the day the
    /// ring was seeded; later samples are end-of-day closes.
    pub amv_history: CircularBuffer<{ market_constants::AMV_HISTORY_MAX }, f64>,

    // Physical data. End-of-Day-Stock = Stock + imported + production - Consumption.
    /// How many were made today.
    pub production: f64,
    /// How many were consumed today.
    pub consumption: f64,
    /// How many were brought in or out by traders. (Negative vaules means exports)
    pub imported: f64,
    /// How many of this good already existed in the market from yesterday.
    pub stock: f64,

    // Market Data. What was actually shown to the market fully.
    /// How many units of the good were offered in sale. The Sum of all Sell and Offer 
    /// orders.
    pub supply: f64,
    /// How many unique sellers there were.
    pub suppliers: f64,
    /// How many units of the good were requested in sale. The Sum of all Buy and 
    /// Request orders.
    pub demand: f64,
    /// How many unique buyers their were.
    pub buyers: f64,

    // Deal Records. When Buyer and Seller are matched, what happened.
    /// How many units of the good were sought out in all deals.
    pub requests: f64,
    /// How many requested goods were successfully purchased.
    pub purchased: f64,
    /// How many units of the good were offered as payment in all deals.
    pub tender: f64,
    /// How many units of the good were actually accepted as payment in all deals.
    pub payment: f64,
    /// The average price the good traded for.
    /// Average Price = (average_price * purchased + deal's price * deals purchase amount) 
    ///     / (purchased + deals purchase amount).
    /// Alternatively may be updated at days end instead.
    pub average_price: f64,
}

impl Default for MarketGood {
    /// AMV defaults to 1.0, average price to 1.0, and salability to
    /// [`market_constants::SALABILITY_DEFAULT`]. All others default to 0.0.
    fn default() -> Self {
        Self {
            amv: 1.0,
            salability: market_constants::SALABILITY_DEFAULT,
            amv_history: CircularBuffer::new(),
            production: 0.0,
            consumption: 0.0,
            imported: 0.0,
            stock: 0.0,
            supply: 0.0,
            suppliers: 0.0,
            demand: 0.0,
            buyers: 0.0,
            requests: 0.0,
            purchased: 0.0,
            tender: 0.0,
            payment: 0.0,
            average_price: 1.0,
        }
    }
}

impl MarketGood {
    /// # New
    ///
    /// Same defaults as [`Default`]: AMV 1.0, salability
    /// [`market_constants::SALABILITY_DEFAULT`], average price 1.0, all others 0.0.
    pub fn new() -> Self {
        Self::default()
    }

    /// New row using loaded salability default. AMV still starts at 1.0.
    pub fn from_config(cfg: &MarketConfig) -> Self {
        let mut good = Self::new();
        good.salability = cfg.salability_default.clamp(0.0, market_constants::SALABILITY_MAX);
        good
    }

    /// Sets the current Abstract Market Value.
    /// Zero and |value| below [`market_constants::AMV_MIN_ABS`] bounce past 0
    /// from the previous sign (positive -> slightly negative, and vice versa).
    pub fn set_amv(&mut self, amv: f64) {
        self.set_amv_min(amv, market_constants::AMV_MIN_ABS);
    }

    /// Sets AMV using a loaded bounce floor.
    pub fn set_amv_min(&mut self, amv: f64, min_abs: f64) {
        self.amv = bounce_away_from_zero(self.amv, amv, min_abs);
    }

    /// Sets the current Abstract Market Value.
    /// Zero and |value| below [`market_constants::AMV_MIN_ABS`] bounce past 0
    /// from the previous sign (positive -> slightly negative, and vice versa).
    pub fn with_amv(mut self, amv: f64) -> Self {
        self.set_amv(amv);
        self
    }

    /// Pushes the current AMV onto `amv_history`.
    pub fn record_amv(&mut self) {
        self.amv_history.push_back(self.amv);
    }

    /// Oldest-to-newest AMV samples currently in the ring.
    pub fn amv_trail(&self) -> Vec<f64> {
        self.amv_history.iter().copied().collect()
    }

    /// Sets salability, clamped to `0.0..=`[`market_constants::SALABILITY_MAX`].
    pub fn set_salability(&mut self, salability: f64) {
        debug_assert!(salability.is_finite(), "salability must be finite");
        self.salability = salability.clamp(0.0, market_constants::SALABILITY_MAX);
    }

    /// Sets salability, clamped to `0.0..=`[`market_constants::SALABILITY_MAX`].
    pub fn with_salability(mut self, salability: f64) -> Self {
        self.set_salability(salability);
        self
    }

    /// Sets how many units were produced today.
    /// Must be `>= 0.0`.
    pub fn set_production(&mut self, production: f64) {
        debug_assert!(production >= 0.0, "production must be >= 0.0");
        self.production = production;
    }

    /// Sets how many units were produced today.
    /// Must be `>= 0.0`.
    pub fn with_production(mut self, production: f64) -> Self {
        self.set_production(production);
        self
    }

    /// Sets how many units were consumed today.
    /// Must be `>= 0.0`.
    pub fn set_consumption(&mut self, consumption: f64) {
        debug_assert!(consumption >= 0.0, "consumption must be >= 0.0");
        self.consumption = consumption;
    }

    /// Sets how many units were consumed today.
    /// Must be `>= 0.0`.
    pub fn with_consumption(mut self, consumption: f64) -> Self {
        self.set_consumption(consumption);
        self
    }

    /// Sets net imports today. Negative values are exports.
    pub fn set_imported(&mut self, imported: f64) {
        self.imported = imported;
    }

    /// Sets net imports today. Negative values are exports.
    pub fn with_imported(mut self, imported: f64) -> Self {
        self.set_imported(imported);
        self
    }

    /// Sets yesterday's leftover stock.
    /// Must be `>= 0.0`.
    pub fn set_stock(&mut self, stock: f64) {
        debug_assert!(stock >= 0.0, "stock must be >= 0.0");
        self.stock = stock;
    }

    /// Sets yesterday's leftover stock.
    /// Must be `>= 0.0`.
    pub fn with_stock(mut self, stock: f64) -> Self {
        self.set_stock(stock);
        self
    }

    /// Sets units offered in sale (sum of sell and offer orders).
    /// Must be `>= 0.0`.
    pub fn set_supply(&mut self, supply: f64) {
        debug_assert!(supply >= 0.0, "supply must be >= 0.0");
        self.supply = supply;
    }

    /// Sets units offered in sale (sum of sell and offer orders).
    /// Must be `>= 0.0`.
    pub fn with_supply(mut self, supply: f64) -> Self {
        self.set_supply(supply);
        self
    }

    /// Sets how many unique sellers there were.
    /// Must be `>= 0.0`.
    pub fn set_suppliers(&mut self, suppliers: f64) {
        debug_assert!(suppliers >= 0.0, "suppliers must be >= 0.0");
        self.suppliers = suppliers;
    }

    /// Sets how many unique sellers there were.
    /// Must be `>= 0.0`.
    pub fn with_suppliers(mut self, suppliers: f64) -> Self {
        self.set_suppliers(suppliers);
        self
    }

    /// Sets units requested (sum of buy and request orders).
    /// Must be `>= 0.0`.
    pub fn set_demand(&mut self, demand: f64) {
        debug_assert!(demand >= 0.0, "demand must be >= 0.0");
        self.demand = demand;
    }

    /// Sets units requested (sum of buy and request orders).
    /// Must be `>= 0.0`.
    pub fn with_demand(mut self, demand: f64) -> Self {
        self.set_demand(demand);
        self
    }

    /// Sets how many unique buyers there were.
    /// Must be `>= 0.0`.
    pub fn set_buyers(&mut self, buyers: f64) {
        debug_assert!(buyers >= 0.0, "buyers must be >= 0.0");
        self.buyers = buyers;
    }

    /// Sets how many unique buyers there were.
    /// Must be `>= 0.0`.
    pub fn with_buyers(mut self, buyers: f64) -> Self {
        self.set_buyers(buyers);
        self
    }

    /// Local units that changed hands today: `purchased + payment`.
    /// Does not include imports and exports. Total volume with trade is
    /// `volume() + imported.abs()`.
    pub fn volume(&self) -> f64 {
        self.purchased + self.payment
    }

    /// Sets units sought out across all deals.
    /// Must be `>= 0.0`.
    pub fn set_requests(&mut self, requests: f64) {
        debug_assert!(requests >= 0.0, "requests must be >= 0.0");
        self.requests = requests;
    }

    /// Sets units sought out across all deals.
    /// Must be `>= 0.0`.
    pub fn with_requests(mut self, requests: f64) -> Self {
        self.set_requests(requests);
        self
    }

    /// Sets units successfully purchased across all deals.
    /// Must be `>= 0.0`.
    pub fn set_purchased(&mut self, purchased: f64) {
        debug_assert!(purchased >= 0.0, "purchased must be >= 0.0");
        self.purchased = purchased;
    }

    /// Sets units successfully purchased across all deals.
    /// Must be `>= 0.0`.
    pub fn with_purchased(mut self, purchased: f64) -> Self {
        self.set_purchased(purchased);
        self
    }

    /// Sets units offered as payment across all deals.
    /// Must be `>= 0.0`.
    pub fn set_tender(&mut self, tender: f64) {
        debug_assert!(tender >= 0.0, "tender must be >= 0.0");
        self.tender = tender;
    }

    /// Sets units offered as payment across all deals.
    /// Must be `>= 0.0`.
    pub fn with_tender(mut self, tender: f64) -> Self {
        self.set_tender(tender);
        self
    }

    /// Sets units accepted as payment across all deals.
    /// Must be `>= 0.0`.
    pub fn set_payment(&mut self, payment: f64) {
        debug_assert!(payment >= 0.0, "payment must be >= 0.0");
        self.payment = payment;
    }

    /// Sets units accepted as payment across all deals.
    /// Must be `>= 0.0`.
    pub fn with_payment(mut self, payment: f64) -> Self {
        self.set_payment(payment);
        self
    }

    /// Sets the average price the good traded for.
    /// Zero and |value| below [`market_constants::AMV_MIN_ABS`] bounce past 0
    /// from the previous sign (positive -> slightly negative, and vice versa).
    pub fn set_average_price(&mut self, average_price: f64) {
        self.set_average_price_min(average_price, market_constants::AMV_MIN_ABS);
    }

    /// Sets average price using a loaded bounce floor.
    pub fn set_average_price_min(&mut self, average_price: f64, min_abs: f64) {
        self.average_price = bounce_away_from_zero(self.average_price, average_price, min_abs);
    }

    /// Sets the average price the good traded for.
    /// Zero and |value| below [`market_constants::AMV_MIN_ABS`] bounce past 0
    /// from the previous sign (positive -> slightly negative, and vice versa).
    pub fn with_average_price(mut self, average_price: f64) -> Self {
        self.set_average_price(average_price);
        self
    }
}

#[cfg(test)]
mod market_lookups_should {
    use super::*;

    #[test]
    fn snapshots_one_history_per_market_and_maps_pops() {
        let mut market = Market {
            id: 7,
            pops: HashSet::from([10, 11]),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods: HashMap::new(),
            friction: 0.0,
            unavailable_goods: HashSet::new(),
            market_days: 0,
            leftover_buy: HashMap::new(),
        };
        market.goods.insert(5, MarketGood::new().with_amv(3.0));
        let mut markets = HashMap::new();
        markets.insert(7, market);

        let lookups = MarketLookups::from_markets(&markets);
        let empty = MarketHistory::new();

        assert_eq!(lookups.histories.len(), 1);
        assert_eq!(lookups.history_for_pop(10, &empty).price(5), 3.0);
        assert_eq!(lookups.history_for_pop(11, &empty).price(5), 3.0);
        assert_eq!(lookups.history_for_pop(99, &empty).price(5), 1.0);
    }
}

#[cfg(test)]
mod market_good_should {
    use super::*;
    use crate::game::config::market_constants;

    #[test]
    fn default_to_unit_amv_default_salability_and_zero_flow() {
        let good = MarketGood::new();
        assert_eq!(good.amv, 1.0);
        assert_eq!(good.salability, market_constants::SALABILITY_DEFAULT);
        assert_eq!(good.average_price, 1.0);
        assert_eq!(good.production, 0.0);
        assert_eq!(good.consumption, 0.0);
        assert_eq!(good.imported, 0.0);
        assert_eq!(good.stock, 0.0);
        assert_eq!(good.supply, 0.0);
        assert_eq!(good.suppliers, 0.0);
        assert_eq!(good.demand, 0.0);
        assert_eq!(good.buyers, 0.0);
        assert_eq!(good.volume(), 0.0);
        assert_eq!(good.requests, 0.0);
        assert_eq!(good.purchased, 0.0);
        assert_eq!(good.tender, 0.0);
        assert_eq!(good.payment, 0.0);
        assert!(good.amv_history.is_empty());
    }

    #[test]
    fn fluent_setters_override_defaults() {
        let good = MarketGood::new()
            .with_amv(2.5)
            .with_salability(0.8)
            .with_production(4.0)
            .with_consumption(1.0)
            .with_imported(-0.5)
            .with_stock(10.0)
            .with_supply(3.0)
            .with_suppliers(2.0)
            .with_demand(5.0)
            .with_buyers(3.0)
            .with_requests(5.0)
            .with_purchased(2.0)
            .with_tender(6.0)
            .with_payment(4.0)
            .with_average_price(1.5);

        assert_eq!(good.amv, 2.5);
        assert_eq!(good.salability, 0.8);
        assert_eq!(good.production, 4.0);
        assert_eq!(good.consumption, 1.0);
        assert_eq!(good.imported, -0.5);
        assert_eq!(good.stock, 10.0);
        assert_eq!(good.supply, 3.0);
        assert_eq!(good.suppliers, 2.0);
        assert_eq!(good.demand, 5.0);
        assert_eq!(good.buyers, 3.0);
        assert_eq!(good.volume(), 6.0);
        assert_eq!(good.requests, 5.0);
        assert_eq!(good.purchased, 2.0);
        assert_eq!(good.tender, 6.0);
        assert_eq!(good.payment, 4.0);
        assert_eq!(good.average_price, 1.5);
        assert_eq!(good.volume(), good.purchased + good.payment);
    }

    #[test]
    fn bounces_positive_amv_past_zero_to_negative() {
        let min = market_constants::AMV_MIN_ABS;
        let good = MarketGood::new().with_amv(0.0);
        assert_eq!(good.amv, -min);

        let good = MarketGood::new().with_amv(min / 10.0);
        assert_eq!(good.amv, -min);
    }

    #[test]
    fn bounces_negative_amv_past_zero_to_positive() {
        let min = market_constants::AMV_MIN_ABS;
        let good = MarketGood::new().with_amv(-1.0).with_amv(0.0);
        assert_eq!(good.amv, min);

        let good = MarketGood::new().with_amv(-1.0).with_amv(-min / 10.0);
        assert_eq!(good.amv, min);
    }

    #[test]
    fn keeps_amv_outside_the_dead_zone() {
        let min = market_constants::AMV_MIN_ABS;
        assert_eq!(MarketGood::new().with_amv(min).amv, min);
        assert_eq!(MarketGood::new().with_amv(-min).amv, -min);
        assert_eq!(MarketGood::new().with_amv(-2.5).amv, -2.5);
    }

    #[test]
    fn average_price_uses_the_same_zero_bounce() {
        let min = market_constants::AMV_MIN_ABS;
        assert_eq!(
            MarketGood::new().with_average_price(0.0).average_price,
            -min
        );
        assert_eq!(
            MarketGood::new()
                .with_average_price(-1.0)
                .with_average_price(0.0)
                .average_price,
            min
        );
    }

    #[test]
    fn clamps_salability_to_max() {
        assert_eq!(MarketGood::new().with_salability(1.5).salability, 1.5);
        assert_eq!(
            MarketGood::new().with_salability(2.5).salability,
            market_constants::SALABILITY_MAX
        );
        assert_eq!(MarketGood::new().with_salability(-0.2).salability, 0.0);
        assert_eq!(MarketGood::new().with_salability(0.4).salability, 0.4);
        assert_eq!(MarketGood::new().with_salability(0.0).salability, 0.0);
        assert_eq!(MarketGood::new().with_salability(1.0).salability, 1.0);
    }

    #[test]
    fn mutating_setters_share_the_same_invariants() {
        let min = market_constants::AMV_MIN_ABS;
        let mut good = MarketGood::new();
        good.set_amv(0.0);
        assert_eq!(good.amv, -min);
        good.set_salability(2.0);
        assert_eq!(good.salability, 2.0);
        good.set_production(3.0);
        assert_eq!(good.production, 3.0);
    }

    #[test]
    fn record_amv_pushes_current_value() {
        let mut good = MarketGood::new().with_amv(2.5);
        assert!(good.amv_history.is_empty());
        good.record_amv();
        good.set_amv(2.75);
        good.record_amv();
        assert_eq!(good.amv_trail(), vec![2.5, 2.75]);
    }
}

#[cfg(test)]
mod cap_salability_from_decay_should {
    use super::*;

    const GRAIN: usize = 1;
    const COIN: usize = 2;

    fn market_with(grain: f64, coin: f64) -> Market {
        let mut market = Market::new(1);
        market.goods.insert(GRAIN, MarketGood::new().with_salability(grain));
        market.goods.insert(COIN, MarketGood::new().with_salability(coin));
        market
    }

    #[test]
    fn full_rot_caps_at_zero() {
        let mut market = market_with(1.0, 1.0);
        let mut rot = HashMap::new();
        rot.insert(GRAIN, (10.0, 10.0));
        market.cap_salability_from_decay(&rot);
        assert_eq!(market.goods[&GRAIN].salability, 0.0);
        assert_eq!(market.goods[&COIN].salability, 1.0);
    }

    #[test]
    fn consumed_only_volume_leaves_salability() {
        let mut market = market_with(0.8, 1.0);
        let mut rot = HashMap::new();
        rot.insert(GRAIN, (0.0, 10.0));
        market.cap_salability_from_decay(&rot);
        assert!((market.goods[&GRAIN].salability - 0.8).abs() < 1e-12);
    }


    #[test]
    fn does_not_raise_salability_already_below_cap() {
        let mut market = market_with(0.2, 1.0);
        let mut rot = HashMap::new();
        rot.insert(GRAIN, (1.0, 10.0));
        market.cap_salability_from_decay(&rot);
        assert!((market.goods[&GRAIN].salability - 0.2).abs() < 1e-12);
    }

    #[test]
    fn skips_unknown_goods_and_zero_volume() {
        let mut market = market_with(0.7, 1.0);
        let mut rot = HashMap::new();
        rot.insert(99, (5.0, 5.0));
        rot.insert(COIN, (0.0, 0.0));
        market.cap_salability_from_decay(&rot);
        assert!((market.goods[&GRAIN].salability - 0.7).abs() < 1e-12);
        assert_eq!(market.goods[&COIN].salability, 1.0);
    }
}

#[cfg(test)]
mod rescale_amv_to_mean_should {
    use super::*;
    use crate::game::config::market_constants;

    const GRAIN: usize = 1;
    const COIN: usize = 2;
    const BREAD: usize = 3;

    #[test]
    fn scales_unweighted_mean_of_one_unit_each_to_target() {
        let mut market = Market::new(1);
        market.goods.insert(GRAIN, MarketGood::new().with_amv(1.0).with_average_price(1.0));
        market.goods.insert(COIN, MarketGood::new().with_amv(2.0).with_average_price(2.0));
        market.goods.insert(BREAD, MarketGood::new().with_amv(3.0).with_average_price(3.0));
        market.rescale_amv_to_mean(10.0, market_constants::AMV_MIN_ABS);
        // Mean was 2; scale 5.
        assert!((market.goods[&GRAIN].amv - 5.0).abs() < 1e-12);
        assert!((market.goods[&COIN].amv - 10.0).abs() < 1e-12);
        assert!((market.goods[&BREAD].amv - 15.0).abs() < 1e-12);
        assert!((market.goods[&GRAIN].average_price - 5.0).abs() < 1e-12);
        let mean: f64 = (5.0 + 10.0 + 15.0) / 3.0;
        assert!((mean - 10.0).abs() < 1e-12);
    }

    #[test]
    fn does_not_rescale_recorded_closes() {
        let mut market = Market::new(1);
        let mut grain = MarketGood::new().with_amv(10.0);
        grain.record_amv();
        grain.set_amv(30.0);
        market.goods.insert(GRAIN, grain);
        market.goods.insert(COIN, MarketGood::new().with_amv(10.0));
        // Mean (30+10)/2 = 20; scale 0.5 to target 10.
        let scale = market.rescale_amv_to_mean(10.0, market_constants::AMV_MIN_ABS);
        assert!((scale - 0.5).abs() < 1e-12);
        assert_eq!(market.goods[&GRAIN].amv_trail(), vec![10.0]);
        assert!((market.goods[&GRAIN].amv - 15.0).abs() < 1e-12);
        market.goods.get_mut(&GRAIN).unwrap().record_amv();
        assert_eq!(market.goods[&GRAIN].amv_trail(), vec![10.0, 15.0]);
    }

    #[test]
    fn skips_when_mean_is_too_close_to_zero() {
        let mut market = Market::new(1);
        market.goods.insert(GRAIN, MarketGood::new().with_amv(1.0));
        market.goods.insert(COIN, MarketGood::new().with_amv(-1.0));
        market.rescale_amv_to_mean(10.0, market_constants::AMV_MIN_ABS);
        assert!((market.goods[&GRAIN].amv - 1.0).abs() < 1e-12);
        assert!((market.goods[&COIN].amv + 1.0).abs() < 1e-12);
    }

}

#[cfg(test)]
mod run_market_day_should {
    use super::*;
    use crate::game::actor::Actor;
    use crate::game::config::market_priority;
    use crate::game::good::TIME;

    const GRAIN: usize = 1;
    const COIN: usize = 2;

    fn priced_market() -> Market {
        let mut market = Market::new(1);
        market.goods.insert(
            GRAIN,
            MarketGood::new().with_amv(1.0).with_salability(0.5),
        );
        market.goods.insert(
            COIN,
            MarketGood::new().with_amv(1.0).with_salability(1.0),
        );
        market
    }


    #[test]
    fn leftover_buy_order_scales_the_counter() {
        let order = MarketOrder::buy_order(
            Actor::Firm(1),
            GRAIN,
            4.0,
            1.0,
            COIN,
            -4.0,
            market_priority::FIRM_PRODUCER,
        );
        let leftover = leftover_order(order, 2.0).expect("remaining");
        assert!((leftover.target_amount - 2.0).abs() < 1e-12);
        assert!((leftover.counter_offer_amount.unwrap() + 2.0).abs() < 1e-12);
        assert!(leftover_order(
            MarketOrder::offer_order(
                Actor::Firm(1),
                GRAIN,
                -2.0,
                1.0,
            ),
            2.0
        )
        .is_none());
    }

    #[test]
    fn leftover_buy_order_snaps_a_fractional_counter() {
        let order = MarketOrder::buy_order(
            Actor::Firm(1),
            GRAIN,
            5.0,
            1.0,
            COIN,
            -9.0,
            market_priority::FIRM_PRODUCER,
        );
        let leftover = leftover_order(order, 2.0).expect("remaining");
        assert_eq!(leftover.target_amount, 3.0);
        // 9 * 3/5 = 5.4, trunc to 5
        assert_eq!(leftover.counter_offer_amount, Some(-5.0));
    }

    #[test]
    fn imbalance_kick_skips_ties_and_time() {
        let mut market = priced_market();
        market.goods.insert(TIME, MarketGood::new().with_amv(5.0));
        market.goods.get_mut(&TIME).unwrap().set_demand(9.0);
        market.goods.get_mut(&TIME).unwrap().set_supply(1.0);
        market.goods.get_mut(&GRAIN).unwrap().set_demand(3.0);
        market.goods.get_mut(&GRAIN).unwrap().set_supply(3.0);
        let cfg = crate::game::config::MarketConfig::default();
        market.nudge_amv_from_imbalance(&cfg);
        assert!((market.goods[&TIME].amv - 5.0).abs() < 1e-12);
        assert!((market.goods[&GRAIN].amv - 1.0).abs() < 1e-12);
    }
}