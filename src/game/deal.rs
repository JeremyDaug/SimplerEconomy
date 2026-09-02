use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::config::{deal_constants, market_constants};
use crate::game::factuals::Factuals;
use crate::game::market::MarketHistory;
use crate::game::marketorder::MarketOrder;
use crate::game::util::{is_whole_unit, whole_units, whole_units_up};

/// # Deal Role
///
/// Which side of a [`ProposedDeal`] an actor is sitting on.
///
/// The goods map is the **seller's inventory change**: add it to the seller,
/// subtract it from the buyer. Positive qty = seller gains that good
/// (payment). Negative qty = seller loses that good (the sale).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DealRole {
    Buyer,
    Seller,
}

impl DealRole {
    /// Returns `+1` for seller, `-1` for buyer (buyer subtracts the same map).
    fn sign(self) -> f64 {
        match self {
            DealRole::Seller => 1.0,
            DealRole::Buyer => -1.0,
        }
    }
}

/// # Deal Response
///
/// Verdict on a complete [`ProposedDeal`]. First-pass impls return `Accept` or
/// `Reject` only. The rest is for later haggling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DealResponse {
    /// Take the basket as written.
    Accept,
    /// Same goods; the actor was given more than enough and would return the
    /// excess ("here is your change"). Unused in the first pass.
    AcceptWithChange,
    /// Not this basket; rewrite it (often a different tender). Unused in the
    /// first pass.
    Counteroffer,
    /// Not this proposal. Another chance is allowed.
    Reject,
    /// Nuclear: do not retry this pairing. Unused in the first pass.
    HardReject,
}

/// # Proposed Deal
///
/// A complete exchange basket between a buyer and a seller.
///
/// `goods` is the seller's inventory change. Apply later with seller `+=`
/// the map and buyer `-=` the map. Positive qty: seller receives (buyer
/// pays). Negative qty: seller gives (buyer receives).
#[derive(Debug, Clone, PartialEq)]
pub struct ProposedDeal {
    pub buyer: Actor,
    pub seller: Actor,
    /// Seller's inventory change, keyed by good id. Zero entries are omitted.
    /// Whole units, including a transport-tagged good being exchanged.
    pub goods: HashMap<usize, f64>,
    /// Transport units the buyer must spend after this deal succeeds.
    /// `TRANSACTION_COST + bulk * market.friction`. 0 if the world has no
    /// transport-tagged goods. May be fractional; paying it may spend a
    /// fraction of a transport good.
    pub transport_needed: f64,
}

impl ProposedDeal {
    /// Creates an empty goods map between `buyer` and `seller`.
    /// Buyer and seller must differ.
    pub fn new(buyer: Actor, seller: Actor) -> Self {
        debug_assert_ne!(buyer, seller, "buyer and seller must differ");
        Self {
            buyer,
            seller,
            goods: HashMap::new(),
            transport_needed: 0.0,
        }
    }

    /// Adds a quantity in seller-inventory terms. Zero is omitted. Same good
    /// accumulates; a zero total is dropped.
    /// `qty` must be finite and a whole unit.
    pub fn with_good(mut self, good: usize, qty: f64) -> Self {
        debug_assert!(qty.is_finite(), "qty must be finite");
        debug_assert!(is_whole_unit(qty), "qty must be a whole unit, got {qty}");
        if qty != 0.0 {
            let entry = self.goods.entry(good).or_insert(0.0);
            *entry += qty;
            if *entry == 0.0 {
                self.goods.remove(&good);
            }
        }
        self
    }

    /// Returns Buyer or Seller if `actor` is a party to this deal, else `None`.
    pub fn role_of(&self, actor: Actor) -> Option<DealRole> {
        if actor == self.buyer {
            Some(DealRole::Buyer)
        } else if actor == self.seller {
            Some(DealRole::Seller)
        } else {
            None
        }
    }

    /// Returns this role's inventory change for `good` (seller adds the map,
    /// buyer subtracts it).
    pub fn signed_qty(&self, role: DealRole, good: usize) -> f64 {
        role.sign() * self.goods.get(&good).copied().unwrap_or(0.0)
    }

    /// Iterates goods this role receives (positive signed qty) as `(good, qty)`.
    pub fn goods_received(&self, role: DealRole) -> impl Iterator<Item = (usize, f64)> + '_ {
        self.goods.iter().filter_map(move |(&good, &qty)| {
            let signed = role.sign() * qty;
            if signed > 0.0 {
                Some((good, signed))
            } else {
                None
            }
        })
    }

    /// Sums given AMV and received AMV for `role` in one pass.
    ///
    /// AMV is `|qty| * price` first, then the role's sign picks the bucket
    /// (seller adds the map, buyer subtracts it). Given goods use full AMV. Received
    /// goods `uses` marks skip the salability haircut; others are AMV *
    /// salability.
    fn amv_sides(
        &self,
        role: DealRole,
        history: &MarketHistory,
        uses: impl Fn(usize) -> bool,
    ) -> (f64, f64) {
        let mut given = 0.0;
        let mut received = 0.0;
        for (&good, &qty) in &self.goods {
            let signed_qty = role.sign() * qty;
            // TODO, consider adding salability here, may be unneeded, but just a thought.
            let mut price = history.price(good);
            if signed_qty > 0.0 && !uses(good) {
                price *= history.salability(good);
            }
            let amv = qty.abs() * price;
            if signed_qty > 0.0 {
                received += amv;
            } else if signed_qty < 0.0 {
                given += amv;
            }
        }
        (given, received)
    }

    /// Returns the AMV this role pays out (full market prices, no salability haircut).
    pub fn given_amv(
        &self,
        role: DealRole,
        history: &MarketHistory,
        uses: impl Fn(usize) -> bool,
    ) -> f64 {
        self.amv_sides(role, history, uses).0
    }

    /// Returns the AMV this role takes in (use-goods at full AMV; others * salability).
    pub fn received_amv(
        &self,
        role: DealRole,
        history: &MarketHistory,
        uses: impl Fn(usize) -> bool,
    ) -> f64 {
        self.amv_sides(role, history, uses).1
    }

    /// Returns received AMV / given AMV for `role`.
    /// `INFINITY` when given AMV is not positive. `uses` skips the salability
    /// haircut on received goods the actor will consume or run as inputs.
    pub fn amv_percent_keep(
        &self,
        role: DealRole,
        history: &MarketHistory,
        uses: impl Fn(usize) -> bool,
    ) -> f64 {
        let (given, received) = self.amv_sides(role, history, uses);
        if given <= 0.0 {
            f64::INFINITY
        } else {
            received / given
        }
    }

    /// Sets transport units the buyer spends on a successful finalize.
    /// Must be `>= 0.0`.
    pub fn with_transport_needed(mut self, transport_needed: f64) -> Self {
        debug_assert!(
            transport_needed >= 0.0 && transport_needed.is_finite(),
            "transport_needed must be >= 0.0"
        );
        self.transport_needed = transport_needed;
        self
    }
}

/// # Deal Maker
///
/// An actor who can sit on either side of a matched pair.
///
/// `buy`, `sell`, and `evaluate` are read-only. They propose or judge a
/// [`ProposedDeal`] and do not move stock. [`DealMaker::finalize`] applies an
/// accepted basket to this actor's inventory. It does not edit market orders.
pub trait DealMaker {
    /// Returns a proposed basket as buyer, or `None` if no tender can be named.
    ///
    /// `own_order` is this actor's buy/request. `other_order` is the matched
    /// sell/offer. Does not move stock.
    fn buy(
        &self,
        own_order: &MarketOrder,
        other_order: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> Option<ProposedDeal>;

    /// Returns Accept or Reject for `deal` (first pass; other verdicts unused).
    fn evaluate(
        &self,
        deal: &ProposedDeal,
        own_order: &MarketOrder,
        other_order: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> DealResponse;

    /// Returns a rewritten `deal` as seller. First pass clones `deal` unchanged.
    fn sell(
        &self,
        deal: &ProposedDeal,
        own_order: &MarketOrder,
        other_order: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> ProposedDeal {
        let _ = (self, own_order, other_order, history, factuals);
        deal.clone()
    }

    /// Applies `deal` to this actor's inventory (seller adds the map, buyer
    /// subtracts it). Does not edit orders. `history` prices are used for
    /// exchange records where the actor keeps them.
    fn finalize(&mut self, deal: &ProposedDeal, history: &MarketHistory);

    /// Spends `amount` friction-cover from on-hand transport-tagged goods
    /// (lowest id first, `qty * efficiency`). Never goes negative; leftover
    /// unpaid is dropped.
    /// `amount` may be fractional. This is the wagon bill, not an exchange.
    fn pay_transport(&mut self, amount: f64, factuals: &Factuals);

    /// Returns a renewed copy of this failed buy/request with `tries`
    /// incremented, or `None` to close it out.
    ///
    /// Default auto-renews the same order until
    /// [`market_constants::BUY_TRY_LIMIT`] retries; a further failure
    /// closes. Recalculating amounts is later.
    fn renew_buy(&self, order: &MarketOrder) -> Option<MarketOrder> {
        let _ = self;
        debug_assert!(
            order.target_amount > 0.0,
            "renew_buy is for buy/request orders"
        );
        if order.tries >= market_constants::BUY_TRY_LIMIT {
            return None;
        }
        let mut renewed = order.clone();
        renewed.tries += 1;
        Some(renewed)
    }
}

/// Returns Accept or Reject from this role's AMV keep (`received / given`).
///
/// Keep is computed by [`ProposedDeal::amv_percent_keep`]. Goods this role
/// **gives** are always full market AMV. Goods they **receive** use full AMV
/// when `uses` is true (consume / process input); otherwise AMV * salability.
///
/// Accepts if any of:
/// 1. Buyer and keep >= 1.0 (received AMV >= given; a windfall, no equity
///    seeking). Currently also covered by (2) because `min_keep` is below 1.0;
///    kept explicit so a later tighter floor does not reject good deals.
/// 2. keep >= `min_keep` (normal band: pop 0.25, firm 0.50).
/// 3. `needs_received` and keep >= `need_keep` (looser band when they need
///    an inbound good; firm 0.25).
///
/// Otherwise Reject.
///
/// # Arguments
///
/// * `deal` — the proposed basket.
/// * `role` — whose keep to judge (buyer or seller). The other party is not
///   consulted.
/// * `history` — market AMV and salability for the goods in the deal.
/// * `min_keep` — minimum `received / given` to Accept in the normal band.
///   0.25 means keep at least a quarter of the AMV you give (up to 75% loss).
///   0.50 means at most 50% loss.
/// * `need_keep` — looser minimum used only when `needs_received` is true.
///   Lets a firm take a worse ratio when the inbound good is a purchase or
///   use target and the deal cannot land in the firm band.
/// * `needs_received` — this role is receiving a good they have a
///   `purchase_target` or `use_target` for. Not the same as `uses`: a
///   merchant restock is a need (looser floor) but not a use (still
///   haircut by salability). Pops pass `false`.
/// * `uses` — per good, true if this actor will consume it or run it as a
///   process input. Those skip the salability haircut on the received side.
pub fn evaluate_amv_floor(
    deal: &ProposedDeal,
    role: DealRole,
    history: &MarketHistory,
    min_keep: f64,
    need_keep: f64,
    needs_received: bool,
    uses: impl Fn(usize) -> bool,
) -> DealResponse {
    debug_assert!(min_keep.is_finite(), "min_keep must be finite");
    debug_assert!(need_keep.is_finite(), "need_keep must be finite");
    debug_assert!(min_keep >= 0.0, "min_keep must be >= 0.0");
    debug_assert!(need_keep >= 0.0, "need_keep must be >= 0.0");

    let keep = deal.amv_percent_keep(role, history, uses);
    if role == DealRole::Buyer && keep >= 1.0 {
        return DealResponse::Accept;
    }
    if keep >= min_keep {
        return DealResponse::Accept;
    }
    if needs_received && keep >= need_keep {
        return DealResponse::Accept;
    }
    DealResponse::Reject
}

/// Returns Accept or Reject using the pop AMV keep floor (0.25).
/// `needs_received` is off; `need_keep` equals `min_keep`. `uses` is
/// typically shop-target / desire goods.
pub fn evaluate_pop_amv(
    deal: &ProposedDeal,
    role: DealRole,
    history: &MarketHistory,
    uses: impl Fn(usize) -> bool,
) -> DealResponse {
    evaluate_amv_floor(
        deal,
        role,
        history,
        deal_constants::POP_AMV_MIN_KEEP,
        deal_constants::POP_AMV_MIN_KEEP,
        false,
        uses,
    )
}

/// Returns Accept or Reject using the firm AMV keep floor (0.50).
/// `needs_received` enables the looser 0.25 catch. `uses` is typically
/// `use_target` (process inputs), not merchant restock.
pub fn evaluate_firm_amv(
    deal: &ProposedDeal,
    role: DealRole,
    history: &MarketHistory,
    needs_received: bool,
    uses: impl Fn(usize) -> bool,
) -> DealResponse {
    evaluate_amv_floor(
        deal,
        role,
        history,
        deal_constants::FIRM_AMV_MIN_KEEP,
        deal_constants::FIRM_AMV_NEED_KEEP,
        needs_received,
        uses,
    )
}

/// Returns a [`ProposedDeal`] for this buy/sell pair, or `None`
/// if no payment can be named.
///
/// `targeted_units` is how many units of `targeted_good` this deal tries
/// to move (`min` of the buy amount and the absolute sell amount).
/// Payment goods are ranked:
/// 1. Seller's named `counter_offer` (what they asked to be paid in),
///    regardless of salability.
/// 2. Other `live_tenders` in the order given (callers sort by salability).
///
/// [`take_tenders`] then covers as many targeted units as it can from
/// preferred goods (seller counter plus salability at or above
/// [`deal_constants::HIGH_SALABILITY`]). Goods below that floor are only
/// offered if preferred goods cannot cover the fill. If every on-hand
/// tender is still short, the targeted units shrink to what was paid.
///
/// Skips a candidate if it is `targeted_good`, already listed, or
/// `tenderable` is 0. Tender qty for the seller's named counter uses that
/// order's counter amount scaled to the remaining units; everything else
/// uses market AMV (`remaining * price(targeted_good) / price(give)`).
///
/// The deal map is the seller's inventory change: targeted qty is
/// negative (seller gives), tender qty is positive (seller receives).
/// Apply later with seller `+=` the map, buyer `-=` the map.
///
/// Returns `None` when `targeted_units` is not positive or no usable
/// tender exists.
/// Different target goods are a matcher bug (`debug_assert`); release still
/// returns `None` so no mixed-good deal is built.
/// Self-trade (`buyer` equals the sell origin) also returns `None`.
/// Buyer `amv_target`, when set, is a unit-AMV ceiling: payment AMV per
/// filled unit above it returns `None`.
///
/// Fill and payment quantities are whole units. A fractional shortfall
/// is dropped; AMV (or a named counter) that is not a whole number of
/// tender units is paid by ceiling the cost of the largest whole fill
/// on-hand can cover.
///
/// # Arguments
///
/// * `buyer` — the buying actor; must be `own_order.origin`.
/// * `own_order` — this actor's buy or request (`target_amount` > 0).
/// * `other_order` — the matched sell or offer (`target_amount` < 0).
/// * `history` — market AMV and salability.
/// * `tenderable` — how many units of a good the buyer can actually pay with
///   (0 if they should not tender that good).
/// * `live_tenders` — other payment goods as `(id, qty)`, already ranked
///   by salability.
pub fn form_buy_proposal(
    buyer: Actor,
    own_order: &MarketOrder,
    other_order: &MarketOrder,
    history: &MarketHistory,
    tenderable: impl Fn(usize) -> f64,
    live_tenders: &[(usize, f64)],
) -> Option<ProposedDeal> {
    debug_assert!(
        own_order.target_amount > 0.0,
        "own_order.target_amount must be > 0.0"
    );
    debug_assert!(
        other_order.target_amount < 0.0,
        "other_order.target_amount must be < 0.0"
    );
    debug_assert_eq!(
        own_order.origin, buyer,
        "own_order.origin must be the buyer"
    );
    debug_assert_eq!(
        own_order.target, other_order.target,
        "matched orders must share a target good"
    );
    if own_order.target != other_order.target || buyer == other_order.origin {
        return None;
    }
    let targeted_good = own_order.target;
    let needed_units = whole_units(own_order.target_amount.min(-other_order.target_amount));
    if needed_units <= 0.0 {
        return None;
    }

    let mut listed = HashSet::new();
    listed.insert(targeted_good);

    let mut preferred: Vec<(usize, f64)> = Vec::new();
    let mut fallback: Vec<(usize, f64)> = Vec::new();

    if let Some(good) = other_order.counter_offer {
        if listed.insert(good) {
            let have = tenderable(good);
            if have > 0.0 {
                preferred.push((good, have));
            }
        }
    }

    for &(good, have) in live_tenders {
        if have <= 0.0 || !listed.insert(good) {
            continue;
        }
        if history.salability(good) >= deal_constants::HIGH_SALABILITY {
            preferred.push((good, have));
        } else {
            fallback.push((good, have));
        }
    }

    let mut tenders = HashMap::new();
    let mut remaining_units = take_tenders(
        needed_units,
        targeted_good,
        &preferred,
        other_order,
        history,
        &mut tenders,
    );
    if remaining_units > 0.0 {
        remaining_units = take_tenders(
            remaining_units,
            targeted_good,
            &fallback,
            other_order,
            history,
            &mut tenders,
        );
    }

    if tenders.is_empty() {
        return None;
    }
    let filled_units = needed_units - remaining_units;
    if filled_units <= 0.0 {
        return None;
    }

    let mut deal = ProposedDeal::new(buyer, other_order.origin)
        .with_good(targeted_good, -filled_units);
    for (good, qty) in tenders {
        deal = deal.with_good(good, qty);
    }
    if let Some(cap) = own_order.amv_target {
        if deal_exceeds_buyer_unit_cap(&deal, targeted_good, cap, history) {
            return None;
        }
    }
    Some(deal)
}

/// Returns whether any good in `factuals` is tagged Transport.
fn world_has_transport(factuals: &Factuals) -> bool {
    factuals.goods.values().any(|good| good.is_transport())
}

/// Sum of `|qty| * bulk` over the deal map.
fn deal_bulk(deal: &ProposedDeal, factuals: &Factuals) -> f64 {
    deal.goods
        .iter()
        .map(|(&id, &qty)| qty.abs() * factuals.find_good(id).bulk())
        .sum()
}

/// Buyer-given and buyer-received transport *cover* in `deal` (qty * efficiency).
fn deal_transport_legs(deal: &ProposedDeal, factuals: &Factuals) -> (f64, f64) {
    let mut given = 0.0;
    let mut received = 0.0;
    for (&id, &qty) in &deal.goods {
        let good = factuals.find_good(id);
        if qty > 0.0 {
            given += good.transport_cover(qty);
        } else {
            received += good.transport_cover(qty.abs());
        }
    }
    (given, received)
}

/// Fill scale `k` in `(0, 1]` so post-exchange transport cover pays
/// `TRANSACTION_COST + bulk * friction`. `None` if the flat fee cannot be met.
fn transport_fill_scale(
    deal: &ProposedDeal,
    factuals: &Factuals,
    friction: f64,
    on_hand: f64,
) -> Option<f64> {
    debug_assert!(friction.is_finite(), "friction must be finite");
    debug_assert!(on_hand.is_finite(), "on_hand must be finite");
    let fee = market_constants::TRANSACTION_COST;
    let factor = friction.max(0.0);
    let bulk = deal_bulk(deal, factuals);
    let (given_t, rec_t) = deal_transport_legs(deal, factuals);
    let coef = given_t - rec_t + bulk * factor;
    let slack = on_hand - fee;
    if coef <= 0.0 {
        if slack >= coef {
            Some(1.0)
        } else {
            None
        }
    } else {
        let k = (slack / coef).min(1.0);
        if k <= 0.0 {
            None
        } else {
            Some(k)
        }
    }
}

fn transport_needed_for(deal: &ProposedDeal, factuals: &Factuals, friction: f64) -> f64 {
    market_constants::TRANSACTION_COST + deal_bulk(deal, factuals) * friction.max(0.0)
}

/// Caps the buy fill to what post-exchange transport can pay, then forms the
/// basket at that size. Does not scale an already-built map.
///
/// `on_hand` is current transport *cover* (`qty * efficiency`). Worlds with no
/// transport-tagged goods skip the bill. Returns `None` if the flat fee cannot
/// be covered even at a vanishing fill.
/// Recap fill is floored to whole units before the basket is formed again.
/// The wagon bill on the result may still be fractional.
pub fn with_transport_budget(
    deal: ProposedDeal,
    buyer: Actor,
    own_order: &MarketOrder,
    other_order: &MarketOrder,
    history: &MarketHistory,
    factuals: &Factuals,
    tenderable: impl Fn(usize) -> f64,
    live_tenders: &[(usize, f64)],
    on_hand: f64,
) -> Option<ProposedDeal> {
    if !world_has_transport(factuals) {
        return Some(deal.with_transport_needed(0.0));
    }
    let k = transport_fill_scale(&deal, factuals, history.friction, on_hand)?;
    let mut out = if k >= 1.0 {
        deal
    } else {
        let filled = deal
            .goods
            .get(&own_order.target)
            .copied()
            .unwrap_or(0.0)
            .abs();
        let mut capped = own_order.clone();
        capped.target_amount = whole_units(filled * k);
        if capped.target_amount <= 0.0 {
            return None;
        }
        form_buy_proposal(
            buyer,
            &capped,
            other_order,
            history,
            tenderable,
            live_tenders,
        )?
    };
    out.transport_needed = transport_needed_for(&out, factuals, history.friction);
    Some(out)
}

/// Returns true if payment AMV per unit of `targeted_good` is above `cap`.
pub(crate) fn deal_exceeds_buyer_unit_cap(
    deal: &ProposedDeal,
    targeted_good: usize,
    cap: f64,
    history: &MarketHistory,
) -> bool {
    if !cap.is_finite() {
        return false;
    }
    let filled = deal.goods.get(&targeted_good).copied().unwrap_or(0.0).abs();
    if filled <= 0.0 {
        return false;
    }
    let payment: f64 = deal
        .goods
        .iter()
        .filter_map(|(&good, &qty)| {
            if good != targeted_good && qty > 0.0 {
                Some(qty * history.price(good))
            } else {
                None
            }
        })
        .sum();
    payment > cap * filled + 1e-12
}

/// Covers as many of `remaining_units` of `targeted_good` as `candidates`
/// can pay for. Appends taken quantities onto `tenders` (seller receives
/// these). Walks `candidates` in order. Each good pays `min(on-hand,
/// intended)`, where intended is the seller's named counter rate if that
/// order names this good, otherwise market AMV. Returns leftover targeted
/// units still unpaid.
fn take_tenders(
    mut remaining_units: f64,
    targeted_good: usize,
    candidates: &[(usize, f64)],
    counter_order: &MarketOrder,
    history: &MarketHistory,
    tenders: &mut HashMap<usize, f64>,
) -> f64 {
    for &(good, have) in candidates {
        if remaining_units <= 0.0 {
            break;
        }
        remaining_units = take_tender(
            remaining_units,
            targeted_good,
            good,
            have,
            counter_order,
            history,
            tenders,
        );
    }
    remaining_units
}

/// Pays as much of `remaining_units` as `have` of `good` can cover.
/// Adds that qty to `tenders` and returns leftover targeted units.
/// Give and fill are whole units of goods (including a transport-tagged
/// good being exchanged). Payment ceils the AMV (or named-counter) cost of
/// the largest whole fill `have` can cover, so 2.5 AMV of value for 1
/// target unit becomes 3 coins paid. AMV itself stays fractional. Sub-unit
/// leftovers stay in inventory. The wagon bill is not this path.
fn take_tender(
    remaining_units: f64,
    targeted_good: usize,
    good: usize,
    have: f64,
    counter_order: &MarketOrder,
    history: &MarketHistory,
    tenders: &mut HashMap<usize, f64>,
) -> f64 {
    if remaining_units <= 0.0 || have <= 0.0 || good == targeted_good {
        return remaining_units;
    }
    let Some(intended) =
        counter_or_amv_qty(counter_order, targeted_good, remaining_units, good, history)
    else {
        return remaining_units;
    };
    if intended <= 0.0 {
        return remaining_units;
    }
    let Some((fill, give)) = whole_fill_and_pay(remaining_units, have, intended) else {
        return remaining_units;
    };
    *tenders.entry(good).or_insert(0.0) += give;
    remaining_units - fill
}

/// Largest whole fill of `remaining` that `have` can pay for at `intended`
/// cost for those remaining units, and the whole payment for that fill.
/// Payment ceils the AMV (or named-counter) cost. `None` if not even 1 unit.
fn whole_fill_and_pay(remaining: f64, have: f64, intended: f64) -> Option<(f64, f64)> {
    let remaining = whole_units(remaining);
    let have = whole_units(have);
    if remaining <= 0.0 || have <= 0.0 || intended <= 0.0 {
        return None;
    }
    let unit_cost = intended / remaining;
    if !unit_cost.is_finite() || unit_cost <= 0.0 {
        return None;
    }
    let fill = whole_units((have / unit_cost).min(remaining));
    if fill <= 0.0 {
        return None;
    }
    let give = have.min(whole_units_up(unit_cost * fill));
    if give <= 0.0 {
        return None;
    }
    Some((fill, give))
}

/// Returns true if every good in the deal is buyable in `factuals`.
pub fn deal_goods_tradeable(deal: &ProposedDeal, factuals: &Factuals) -> bool {
    deal.goods
        .keys()
        .all(|&good| factuals.find_good(good).is_buyable())
}

/// Sorts `(good, salability, qty)` by salability descending, then good id,
/// and returns `(good, qty)`.
pub fn sort_tenders_by_salability(mut rows: Vec<(usize, f64, f64)>) -> Vec<(usize, f64)> {
    rows.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    rows.into_iter().map(|(good, _, qty)| (good, qty)).collect()
}

/// Returns tender qty of `give` for `targeted_units` of `targeted_good`.
/// Uses the order's named counter amount if that good matches, otherwise
/// market AMV.
fn counter_or_amv_qty(
    order: &MarketOrder,
    targeted_good: usize,
    targeted_units: f64,
    give: usize,
    history: &MarketHistory,
) -> Option<f64> {
    if let (Some(good), Some(amount)) = (order.counter_offer, order.counter_offer_amount) {
        if good == give {
            let target_abs = order.target_amount.abs();
            if target_abs > 0.0 && amount.abs() > 0.0 {
                let qty = amount.abs() * targeted_units / target_abs;
                if qty > 0.0 && qty.is_finite() {
                    return Some(qty);
                }
            }
        }
    }
    amv_tender_qty(targeted_units, targeted_good, give, history)
}

/// Returns how many units of `give` match `targeted_units` of `targeted_good`
/// at market AMV.
fn amv_tender_qty(
    targeted_units: f64,
    targeted_good: usize,
    give: usize,
    history: &MarketHistory,
) -> Option<f64> {
    let give_price = history.price(give);
    if give_price <= 0.0 {
        return None;
    }
    let qty = targeted_units * history.price(targeted_good) / give_price;
    if qty > 0.0 && qty.is_finite() {
        Some(qty)
    } else {
        None
    }
}

#[cfg(test)]
mod proposed_deal_should {
    use super::*;

    fn bread_for_coin(bread: f64, coin: f64) -> ProposedDeal {
        ProposedDeal::new(Actor::Pop(1), Actor::Firm(2))
            .with_good(1, -bread)
            .with_good(2, coin)
    }

    fn unit_history() -> MarketHistory {
        let mut history = MarketHistory::new();
        history.prices.insert(1, 1.0);
        history.prices.insert(2, 1.0);
        history
    }

    #[test]
    fn seller_adds_buyer_subtracts() {
        let deal = bread_for_coin(5.0, 10.0);
        assert_eq!(deal.signed_qty(DealRole::Seller, 1), -5.0);
        assert_eq!(deal.signed_qty(DealRole::Seller, 2), 10.0);
        assert_eq!(deal.signed_qty(DealRole::Buyer, 1), 5.0);
        assert_eq!(deal.signed_qty(DealRole::Buyer, 2), -10.0);
    }

    #[test]
    fn with_good_drops_cancelled_totals() {
        let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(2))
            .with_good(1, -4.0)
            .with_good(1, 4.0);
        assert!(deal.goods.is_empty());
    }

    #[test]
    fn amv_keep_is_received_over_given() {
        let deal = bread_for_coin(5.0, 10.0);
        let history = unit_history();
        assert!((deal.amv_percent_keep(DealRole::Buyer, &history, |_| true) - 0.5).abs() < 1e-12);
        assert!((deal.amv_percent_keep(DealRole::Seller, &history, |_| true) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn unused_received_is_haircut_by_salability() {
        let deal = bread_for_coin(5.0, 5.0);
        let mut history = unit_history();
        history.salability.insert(1, 0.4);
        history.salability.insert(2, 1.0);
        // Buyer keeps bread: unused -> 5 * 0.4 / 5 = 0.4
        assert!(
            (deal.amv_percent_keep(DealRole::Buyer, &history, |_| false) - 0.4).abs() < 1e-12
        );
        // Buyer uses bread: full AMV, keep 1.0
        assert!(
            (deal.amv_percent_keep(DealRole::Buyer, &history, |g| g == 1) - 1.0).abs() < 1e-12
        );
        // Seller keeps coin at sal 1.0, unused or used same
        assert!(
            (deal.amv_percent_keep(DealRole::Seller, &history, |_| false) - 1.0).abs() < 1e-12
        );
    }

    #[test]
    fn role_of_maps_parties() {
        let deal = bread_for_coin(1.0, 1.0);
        assert_eq!(deal.role_of(Actor::Pop(1)), Some(DealRole::Buyer));
        assert_eq!(deal.role_of(Actor::Firm(2)), Some(DealRole::Seller));
        assert_eq!(deal.role_of(Actor::Pop(9)), None);
    }
}

#[cfg(test)]
mod amv_verdict_should {
    use super::*;

    fn bread_for_coin(bread: f64, coin: f64) -> ProposedDeal {
        ProposedDeal::new(Actor::Pop(1), Actor::Firm(2))
            .with_good(1, -bread)
            .with_good(2, coin)
    }

    fn unit_history() -> MarketHistory {
        let mut history = MarketHistory::new();
        history.prices.insert(1, 1.0);
        history.prices.insert(2, 1.0);
        history
    }

    #[test]
    fn pop_accepts_at_twenty_five_percent_keep() {
        let deal = bread_for_coin(1.0, 4.0);
        let history = unit_history();
        assert_eq!(
            evaluate_pop_amv(&deal, DealRole::Buyer, &history, |_| true),
            DealResponse::Accept
        );
    }

    #[test]
    fn pop_rejects_below_twenty_five_percent_keep() {
        let deal = bread_for_coin(1.0, 5.0);
        let history = unit_history();
        assert_eq!(
            evaluate_pop_amv(&deal, DealRole::Buyer, &history, |_| true),
            DealResponse::Reject
        );
    }

    #[test]
    fn firm_rejects_forty_percent_keep_without_need() {
        let deal = bread_for_coin(4.0, 10.0);
        let history = unit_history();
        assert_eq!(
            evaluate_firm_amv(&deal, DealRole::Buyer, &history, false, |_| true),
            DealResponse::Reject
        );
    }

    #[test]
    fn firm_need_catch_accepts_forty_percent_keep() {
        let deal = bread_for_coin(4.0, 10.0);
        let history = unit_history();
        assert_eq!(
            evaluate_firm_amv(&deal, DealRole::Buyer, &history, true, |_| true),
            DealResponse::Accept
        );
    }

    #[test]
    fn firm_accepts_sixty_percent_keep() {
        let deal = bread_for_coin(6.0, 10.0);
        let history = unit_history();
        assert_eq!(
            evaluate_firm_amv(&deal, DealRole::Buyer, &history, false, |_| true),
            DealResponse::Accept
        );
    }

    #[test]
    fn buyer_accepts_an_exceptional_windfall() {
        let deal = bread_for_coin(10.0, 2.0);
        let history = unit_history();
        assert_eq!(
            evaluate_pop_amv(&deal, DealRole::Buyer, &history, |_| true),
            DealResponse::Accept
        );
        assert_eq!(
            evaluate_firm_amv(&deal, DealRole::Buyer, &history, false, |_| true),
            DealResponse::Accept
        );
    }

    #[test]
    fn seller_may_reject_the_same_windfall_for_the_buyer() {
        let deal = bread_for_coin(10.0, 2.0);
        let history = unit_history();
        assert_eq!(
            evaluate_firm_amv(&deal, DealRole::Seller, &history, false, |_| true),
            DealResponse::Reject
        );
    }

    #[test]
    fn unused_low_salability_tender_can_fail_the_firm_floor() {
        let deal = bread_for_coin(10.0, 10.0);
        let mut history = unit_history();
        history.salability.insert(1, 0.3);
        history.salability.insert(2, 1.0);
        // Seller keeps coin at full; buyer keeps bread unused at 0.3
        assert_eq!(
            evaluate_firm_amv(&deal, DealRole::Buyer, &history, false, |_| false),
            DealResponse::Reject
        );
        assert_eq!(
            evaluate_firm_amv(&deal, DealRole::Buyer, &history, false, |g| g == 1),
            DealResponse::Accept
        );
    }
}

#[cfg(test)]
mod form_buy_proposal_should {
    use super::*;
    use crate::game::config::market_priority;

    fn request(buyer: Actor, good: usize, qty: f64) -> MarketOrder {
        MarketOrder::request_order(buyer, good, qty, market_priority::POP_START)
    }

    fn offer(seller: Actor, good: usize, qty: f64) -> MarketOrder {
        MarketOrder::offer_order(
            seller,
            good,
            -qty,
            market_priority::FIRM_PRODUCER,
        )
    }

    fn unit_history() -> MarketHistory {
        let mut history = MarketHistory::new();
        history.prices.insert(1, 2.0);
        history.prices.insert(2, 1.0);
        history.prices.insert(3, 1.0);
        history.prices.insert(4, 1.0);
        history
    }

    fn sal_history() -> MarketHistory {
        let mut history = unit_history();
        history.salability.insert(2, 1.0);
        history.salability.insert(3, 0.9);
        history.salability.insert(4, 0.3);
        history
    }

    #[test]
    fn uses_market_amv_when_orders_have_no_counter() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = offer(Actor::Firm(2), 1, 4.0);
        let history = unit_history();
        let deal = form_buy_proposal(buyer, &own, &other, &history, |_| 10.0, &[(2, 10.0)])
            .expect("proposal");
        assert_eq!(deal.buyer, buyer);
        assert_eq!(deal.seller, Actor::Firm(2));
        assert!((deal.goods[&1] + 4.0).abs() < 1e-12);
        assert!((deal.goods[&2] - 8.0).abs() < 1e-12);
    }

    #[test]
    fn prefers_seller_named_counter() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = MarketOrder::sell_order(
            Actor::Firm(2),
            1,
            -4.0,
            2.0,
            3,
            4.0,
            market_priority::FIRM_PRODUCER,
        );
        let history = unit_history();
        let deal = form_buy_proposal(
            buyer,
            &own,
            &other,
            &history,
            |g| if g == 3 { 10.0 } else { 0.0 },
            &[(2, 10.0)],
        )
        .expect("proposal");
        assert!(deal.goods.contains_key(&3));
        assert!(!deal.goods.contains_key(&2));
        assert!((deal.goods[&3] - 4.0).abs() < 1e-12);
    }

    #[test]
    fn scales_targeted_units_when_tender_is_short() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = offer(Actor::Firm(2), 1, 4.0);
        let history = unit_history();
        let deal = form_buy_proposal(buyer, &own, &other, &history, |_| 4.0, &[(2, 4.0)])
            .expect("proposal");
        // intended give is 8 (4 * 2 / 1); have 4 -> half fill
        assert!((deal.goods[&1] + 2.0).abs() < 1e-12);
        assert!((deal.goods[&2] - 4.0).abs() < 1e-12);
    }

    #[test]
    fn returns_none_without_a_tender() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = offer(Actor::Firm(2), 1, 4.0);
        let history = unit_history();
        assert!(form_buy_proposal(buyer, &own, &other, &history, |_| 0.0, &[]).is_none());
    }

    #[test]
    fn combines_high_salability_tenders_instead_of_shrinking() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = offer(Actor::Firm(2), 1, 4.0);
        let history = sal_history();
        // 4 bread * 2 AMV = 8. 3 coins buy 1 bread (2 coins, leftover 1);
        // gold covers the remaining 3 bread with 6.
        let deal = form_buy_proposal(
            buyer,
            &own,
            &other,
            &history,
            |_| 10.0,
            &[(2, 3.0), (3, 10.0)],
        )
        .expect("proposal");
        assert!((deal.goods[&1] + 4.0).abs() < 1e-12);
        assert!((deal.goods[&2] - 2.0).abs() < 1e-12);
        assert!((deal.goods[&3] - 6.0).abs() < 1e-12);
    }

    #[test]
    fn uses_seller_counter_then_high_sal_tenders() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = MarketOrder::sell_order(
            Actor::Firm(2),
            1,
            -4.0,
            2.0,
            4,
            4.0,
            market_priority::FIRM_PRODUCER,
        );
        let history = sal_history();
        // Seller wants 4 of good 4 for 4 bread. Buyer has 1 of it (covers 1
        // bread at the named rate) and coins for the rest at AMV (3 * 2 = 6).
        let deal = form_buy_proposal(
            buyer,
            &own,
            &other,
            &history,
            |g| match g {
                4 => 1.0,
                2 => 10.0,
                _ => 0.0,
            },
            &[(2, 10.0), (4, 1.0)],
        )
        .expect("proposal");
        assert!((deal.goods[&1] + 4.0).abs() < 1e-12);
        assert!((deal.goods[&4] - 1.0).abs() < 1e-12);
        assert!((deal.goods[&2] - 6.0).abs() < 1e-12);
    }

    #[test]
    fn does_not_tender_low_salability_when_high_sal_covers() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = offer(Actor::Firm(2), 1, 4.0);
        let history = sal_history();
        let deal = form_buy_proposal(
            buyer,
            &own,
            &other,
            &history,
            |_| 10.0,
            &[(2, 10.0), (4, 10.0)],
        )
        .expect("proposal");
        assert!((deal.goods[&1] + 4.0).abs() < 1e-12);
        assert!((deal.goods[&2] - 8.0).abs() < 1e-12);
        assert!(!deal.goods.contains_key(&4));
    }

    #[test]
    fn tenders_low_salability_only_when_high_sal_is_short() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = offer(Actor::Firm(2), 1, 4.0);
        let history = sal_history();
        // 3 coins buy 1 bread (2 coins); barter good 4 covers the remaining 3 with 6.
        let deal = form_buy_proposal(
            buyer,
            &own,
            &other,
            &history,
            |_| 10.0,
            &[(2, 3.0), (4, 10.0)],
        )
        .expect("proposal");
        assert!((deal.goods[&1] + 4.0).abs() < 1e-12);
        assert!((deal.goods[&2] - 2.0).abs() < 1e-12);
        assert!((deal.goods[&4] - 6.0).abs() < 1e-12);
    }

    #[test]
    fn still_uses_seller_counter_when_it_is_not_highly_salable() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = MarketOrder::sell_order(
            Actor::Firm(2),
            1,
            -4.0,
            2.0,
            4,
            4.0,
            market_priority::FIRM_PRODUCER,
        );
        let history = sal_history();
        let deal = form_buy_proposal(
            buyer,
            &own,
            &other,
            &history,
            |g| if g == 4 { 10.0 } else { 0.0 },
            &[(2, 10.0)],
        )
        .expect("proposal");
        assert!(deal.goods.contains_key(&4));
        assert!(!deal.goods.contains_key(&2));
        assert!((deal.goods[&4] - 4.0).abs() < 1e-12);
    }

    #[test]
    fn returns_none_when_buyer_is_the_seller() {
        let actor = Actor::Pop(1);
        let own = request(actor, 1, 4.0);
        let other = offer(actor, 1, 4.0);
        let history = unit_history();
        assert!(form_buy_proposal(actor, &own, &other, &history, |_| 10.0, &[(2, 10.0)]).is_none());
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn returns_none_when_target_goods_differ() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = offer(Actor::Firm(2), 3, 4.0);
        let history = unit_history();
        assert!(form_buy_proposal(buyer, &own, &other, &history, |_| 10.0, &[(2, 10.0)]).is_none());
    }

    fn priced_buy(
        buyer: Actor,
        good: usize,
        qty: f64,
        amv: f64,
        pay: usize,
        pay_qty: f64,
    ) -> MarketOrder {
        MarketOrder::buy_order(
            buyer,
            good,
            qty,
            amv,
            pay,
            -pay_qty,
            market_priority::FIRM_PRODUCER,
        )
    }

    #[test]
    fn returns_none_when_payment_unit_amv_exceeds_buyer_cap() {
        let buyer = Actor::Firm(1);
        // Cap 1.5; seller named rate is 8 coin for 4 bread = 2.0 AMV/unit.
        let own = priced_buy(buyer, 1, 4.0, 1.5, 2, 6.0);
        let other = MarketOrder::sell_order(
            Actor::Firm(2),
            1,
            -4.0,
            2.0,
            2,
            8.0,
            market_priority::FIRM_PRODUCER,
        );
        let history = unit_history();
        assert!(form_buy_proposal(buyer, &own, &other, &history, |_| 10.0, &[(2, 10.0)]).is_none());
    }

    #[test]
    fn still_proposes_when_payment_unit_amv_is_at_buyer_cap() {
        let buyer = Actor::Firm(1);
        let own = priced_buy(buyer, 1, 4.0, 1.5, 2, 6.0);
        let other = MarketOrder::sell_order(
            Actor::Firm(2),
            1,
            -4.0,
            1.5,
            2,
            6.0,
            market_priority::FIRM_PRODUCER,
        );
        let history = unit_history();
        let deal = form_buy_proposal(buyer, &own, &other, &history, |_| 10.0, &[(2, 10.0)])
            .expect("proposal at cap");
        assert!((deal.goods[&1] + 4.0).abs() < 1e-12);
        assert!((deal.goods[&2] - 6.0).abs() < 1e-12);
    }

    #[test]
    fn ceils_payment_when_amv_is_not_a_whole_unit() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 1.0);
        let other = offer(Actor::Firm(2), 1, 1.0);
        let mut history = MarketHistory::new();
        history.prices.insert(1, 2.5);
        history.prices.insert(2, 1.0);
        let deal = form_buy_proposal(buyer, &own, &other, &history, |_| 10.0, &[(2, 10.0)])
            .expect("proposal");
        // 2.5 AMV of coin for 1 bread becomes 3 coins paid.
        assert!((deal.goods[&1] + 1.0).abs() < 1e-12);
        assert!((deal.goods[&2] - 3.0).abs() < 1e-12);
    }

    #[test]
    fn does_not_tender_a_fractional_remainder() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 4.0);
        let other = offer(Actor::Firm(2), 1, 4.0);
        let history = unit_history();
        let deal = form_buy_proposal(buyer, &own, &other, &history, |_| 2.7, &[(2, 2.7)])
            .expect("proposal");
        // 2.7 coins floors to 2; bread is 2 AMV so that buys 1 whole unit.
        assert!((deal.goods[&1] + 1.0).abs() < 1e-12);
        assert!((deal.goods[&2] - 2.0).abs() < 1e-12);
    }

    #[test]
    fn returns_none_when_a_whole_unit_cannot_be_bought() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 1, 1.0);
        let other = offer(Actor::Firm(2), 1, 1.0);
        let mut history = MarketHistory::new();
        history.prices.insert(1, 2.5);
        history.prices.insert(2, 1.0);
        // 2 coins cannot cover ceil(2.5) for one bread.
        assert!(form_buy_proposal(buyer, &own, &other, &history, |_| 2.0, &[(2, 2.0)]).is_none());
    }

    #[test]
    fn exchanging_a_transport_good_is_still_whole_units() {
        let buyer = Actor::Pop(1);
        let own = request(buyer, 9, 4.0);
        let other = offer(Actor::Firm(2), 9, 4.0);
        let mut history = MarketHistory::new();
        history.prices.insert(9, 2.0);
        history.prices.insert(2, 1.0);
        let deal = form_buy_proposal(buyer, &own, &other, &history, |_| 10.0, &[(2, 10.0)])
            .expect("proposal");
        assert!((deal.goods[&9] + 4.0).abs() < 1e-12);
        assert!((deal.goods[&2] - 8.0).abs() < 1e-12);
        assert!(deal.goods.values().all(|&qty| qty == qty.trunc()));
    }
}

#[cfg(test)]
mod with_transport_budget_should {
    use super::*;
    use crate::game::config::{market_constants, market_priority};
    use crate::game::good::Good;
    use std::collections::HashSet;

    fn plain_good(id: usize) -> Good {
        Good {
            id,
            name: format!("g{id}"),
            class: None,
            decay_rate: 0.0,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: vec![],
        }
    }

    fn cargo_good(id: usize, efficiency: f64) -> Good {
        let mut good = plain_good(id);
        good.mass = 0.0;
        good = good.with_transport_efficiency(efficiency);
        good
    }

    fn request(qty: f64) -> MarketOrder {
        MarketOrder::request_order(Actor::Pop(1), 1, qty, market_priority::POP_START)
    }

    fn offer(qty: f64) -> MarketOrder {
        MarketOrder::offer_order(Actor::Firm(2), 1, -qty, market_priority::FIRM_PRODUCER)
    }

    fn history(friction: f64) -> MarketHistory {
        let mut history = MarketHistory::new();
        history.prices.insert(1, 2.0);
        history.prices.insert(2, 1.0);
        history.prices.insert(9, 1.0);
        history.friction = friction;
        history
    }

    fn budget(
        deal: ProposedDeal,
        factuals: &Factuals,
        friction: f64,
        on_hand: f64,
        own: &MarketOrder,
        other: &MarketOrder,
    ) -> Option<ProposedDeal> {
        let history = history(friction);
        with_transport_budget(
            deal,
            Actor::Pop(1),
            own,
            other,
            &history,
            factuals,
            |_| 10.0,
            &[(2, 10.0)],
            on_hand,
        )
    }

    #[test]
    fn skips_when_world_has_no_transport_tag() {
        let factuals = Factuals::new()
            .with_good(plain_good(1))
            .with_good(plain_good(2));
        let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(2))
            .with_good(1, -4.0)
            .with_good(2, 8.0);
        let own = request(4.0);
        let other = offer(4.0);
        let out = budget(deal, &factuals, 1.0, 0.0, &own, &other).expect("no cargo world");
        assert_eq!(out.transport_needed, 0.0);
        assert!((out.goods[&1] + 4.0).abs() < 1e-12);
    }

    #[test]
    fn charges_the_flat_fee_when_friction_is_zero() {
        let factuals = Factuals::new()
            .with_good(plain_good(1))
            .with_good(plain_good(2))
            .with_good(cargo_good(9, 1.0));
        let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(2))
            .with_good(1, -4.0)
            .with_good(2, 8.0);
        let own = request(4.0);
        let other = offer(4.0);
        let out = budget(deal, &factuals, 0.0, 20.0, &own, &other).expect("cover");
        assert!((out.transport_needed - market_constants::TRANSACTION_COST).abs() < 1e-12);
        assert!((out.goods[&1] + 4.0).abs() < 1e-12);
    }

    #[test]
    fn returns_none_when_on_hand_cannot_cover_the_fee() {
        let factuals = Factuals::new()
            .with_good(plain_good(1))
            .with_good(plain_good(2))
            .with_good(cargo_good(9, 1.0));
        let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(2))
            .with_good(1, -4.0)
            .with_good(2, 8.0);
        let own = request(4.0);
        let other = offer(4.0);
        assert!(budget(deal, &factuals, 0.0, 5.0, &own, &other).is_none());
    }

    #[test]
    fn reforms_at_a_smaller_fill_when_bulk_friction_overdraws() {
        let factuals = Factuals::new()
            .with_good(plain_good(1))
            .with_good(plain_good(2))
            .with_good(cargo_good(9, 1.0));
        let own = request(4.0);
        let other = offer(4.0);
        let history = history(1.0);
        let deal = form_buy_proposal(
            Actor::Pop(1),
            &own,
            &other,
            &history,
            |_| 10.0,
            &[(2, 10.0)],
        )
        .expect("probe");
        // bulk = 4*1 + 8*1 = 12, F = 1, T = 10, coef = 12, on_hand = 16, k = 0.5
        let out = with_transport_budget(
            deal,
            Actor::Pop(1),
            &own,
            &other,
            &history,
            &factuals,
            |_| 10.0,
            &[(2, 10.0)],
            16.0,
        )
        .expect("reformed");
        assert!((out.goods[&1] + 2.0).abs() < 1e-12);
        assert!((out.goods[&2] - 4.0).abs() < 1e-12);
        assert!((out.transport_needed - 16.0).abs() < 1e-12);
    }

    #[test]
    fn received_transport_can_cover_the_flat_fee() {
        let factuals = Factuals::new()
            .with_good(plain_good(2))
            .with_good(cargo_good(9, 1.0));
        let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(2))
            .with_good(9, -20.0)
            .with_good(2, 20.0);
        let own = MarketOrder::request_order(
            Actor::Pop(1),
            9,
            20.0,
            market_priority::POP_START,
        );
        let other = MarketOrder::offer_order(
            Actor::Firm(2),
            9,
            -20.0,
            market_priority::FIRM_PRODUCER,
        );
        let out = budget(deal, &factuals, 0.0, 0.0, &own, &other).expect("buy cargo");
        assert!((out.transport_needed - market_constants::TRANSACTION_COST).abs() < 1e-12);
        assert!((out.goods[&9] + 20.0).abs() < 1e-12);
    }

    #[test]
    fn efficiency_above_one_covers_more_per_unit() {
        let factuals = Factuals::new()
            .with_good(plain_good(1))
            .with_good(plain_good(2))
            .with_good(cargo_good(9, 2.0));
        let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(2))
            .with_good(1, -4.0)
            .with_good(2, 8.0);
        let own = request(4.0);
        let other = offer(4.0);
        // 5 units at efficiency 2.0 = 10 cover, exactly the flat fee.
        let out = budget(deal, &factuals, 0.0, 10.0, &own, &other).expect("efficient cargo");
        assert!((out.transport_needed - market_constants::TRANSACTION_COST).abs() < 1e-12);
        assert!((out.goods[&1] + 4.0).abs() < 1e-12);
    }

    #[test]
    fn transport_bill_may_be_fractional() {
        let factuals = Factuals::new()
            .with_good(plain_good(1))
            .with_good(plain_good(2))
            .with_good(cargo_good(9, 1.0));
        let deal = ProposedDeal::new(Actor::Pop(1), Actor::Firm(2))
            .with_good(1, -4.0)
            .with_good(2, 8.0);
        let own = request(4.0);
        let other = offer(4.0);
        // bulk 12 * 0.125 + TRANSACTION_COST 10 = 11.5
        let out = budget(deal, &factuals, 0.125, 20.0, &own, &other).expect("cover");
        assert!((out.transport_needed - 11.5).abs() < 1e-12);
        assert!((out.goods[&1] + 4.0).abs() < 1e-12);
        assert!((out.goods[&2] - 8.0).abs() < 1e-12);
    }
}
