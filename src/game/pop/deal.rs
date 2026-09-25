use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::config::deal_constants;
use crate::game::deal::{
    collect_tenders, deal_goods_tradeable, evaluate_keep_ratio, form_buy_proposal,
    transport_cover_on_hand, transport_spend_plan, with_transport_budget, DealMaker,
    DealResponse, DealRole, ProposedDeal,
};
use crate::game::factuals::Factuals;
use crate::game::firm::Firm;
use crate::game::market::MarketHistory;
use crate::game::marketorder::MarketOrder;
use crate::game::pop_property::PopPRow;

use super::Pop;

impl DealMaker for Pop {
    /// # Buy
    ///
    /// Returns a proposed basket as buyer, or `None` if no tender can be named.
    /// Uses free stock above `shop_target.max(reserved)`, minus units listed
    /// on offer orders in `current_orders`. Seller's named counter first (any
    /// salability), then other excess by salability. Highly salable goods
    /// (and that counter) cover the fill first; lower salability only if
    /// those cannot. Shrinks the fill if still short. Does not move stock.
    fn buy(
        &self,
        own_order: &MarketOrder,
        other_order: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> Option<ProposedDeal> {
        self.buy_with_firm(own_order, other_order, history, factuals, None)
    }

    /// # Evaluate
    ///
    /// Returns Accept or Reject for this deal as this pop.
    /// Incoming: best received category sets the haircut for the whole bag
    /// (consume shortfall, else save shortfall, else extra-desired, else
    /// unused). Outgoing: given units peel extra → save → consume at
    /// 0 / 25 / 50 / 100 salability penalty. The 0.50 keep floor always
    /// applies. Buyers accept windfalls. Does not move stock.
    fn evaluate(
        &self,
        deal: &ProposedDeal,
        own_order: &MarketOrder,
        other_order: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> DealResponse {
        let _ = other_order;
        debug_assert_eq!(own_order.origin, Actor::Pop(self.id));
        let Some(role) = deal.role_of(Actor::Pop(self.id)) else {
            debug_assert!(false, "pop must be a party to the deal");
            return DealResponse::Reject;
        };
        if !deal_goods_tradeable(deal, factuals) {
            return DealResponse::Reject;
        }
        let keep = pop_amv_percent_keep(self, deal, role, history);
        evaluate_keep_ratio(role, keep, factuals.config.deal.pop_amv_unused_keep)
    }

    /// # Finalize
    ///
    /// Applies `deal` to this pop's on-hand `quantity`. Seller adds the map,
    /// buyer subtracts it. Creates a row at 0 if the good is new. Does not
    /// edit orders, reserved, or shop/save targets.
    fn finalize(&mut self, deal: &ProposedDeal, history: &MarketHistory) {
        self.finalize_with_firm(deal, history, None);
    }

    fn pay_transport(&mut self, amount: f64, factuals: &Factuals) {
        let on_hand: Vec<(usize, f64)> = self
            .property
            .iter()
            .map(|(id, row)| (*id, row.quantity))
            .collect();
        for (id, sub) in transport_spend_plan(amount, factuals, on_hand) {
            debug_assert!(sub >= 0.0 && sub.is_finite(), "transport spend must be >= 0.0");
            if let Some(row) = self.property.get_mut(&id) {
                row.quantity = (row.quantity - sub).max(0.0);
                row.consumed += sub;
                if row.reserved > row.quantity {
                    row.reserved = row.quantity;
                }
            }
        }
    }
}

impl Pop {
    /// Remainder owners may tender shop stock above dinner. `firm` is that
    /// shop; `None` uses the pop bag only.
    pub(crate) fn buy_with_firm(
        &self,
        own_order: &MarketOrder,
        other_order: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
        firm: Option<&Firm>,
    ) -> Option<ProposedDeal> {
        debug_assert_eq!(own_order.origin, Actor::Pop(self.id));
        let targeted_good = own_order.target;
        let shop_tier = own_order.shop_tier;
        let live = pop_live_tenders_with_firm(self, targeted_good, shop_tier, history, factuals, firm);
        let deal = form_buy_proposal(
            Actor::Pop(self.id),
            own_order,
            other_order,
            history,
            factuals.config.deal.high_salability,
            |good| pop_tenderable_with_firm(self, good, targeted_good, shop_tier, factuals, firm),
            &live,
        )?;
        with_transport_budget(
            deal,
            Actor::Pop(self.id),
            own_order,
            other_order,
            history,
            factuals,
            |good| pop_tenderable_with_firm(self, good, targeted_good, shop_tier, factuals, firm),
            &live,
            remainder_transport_cover(self, firm, factuals),
        )
    }

    /// Applies `deal` to the pop bag, then the remainder shop shelf.
    pub(crate) fn finalize_with_firm(
        &mut self,
        deal: &ProposedDeal,
        history: &MarketHistory,
        mut firm: Option<&mut Firm>,
    ) {
        let _ = history;
        let Some(role) = deal.role_of(Actor::Pop(self.id)) else {
            debug_assert!(false, "pop must be a party to the deal");
            return;
        };
        for (&good, _) in &deal.goods {
            let delta = deal.signed_qty(role, good);
            if delta == 0.0 {
                continue;
            }
            if delta > 0.0 {
                let row = self
                    .property
                    .entry(good)
                    .or_insert_with(|| PopPRow::new(0.0));
                row.quantity += delta;
                debug_assert!(row.quantity >= 0.0, "quantity must be >= 0.0");
                continue;
            }
            let mut need = -delta;
            let row = self
                .property
                .entry(good)
                .or_insert_with(|| PopPRow::new(0.0));
            let from_pop = need.min(row.quantity.max(0.0));
            row.quantity -= from_pop;
            need -= from_pop;
            if need > 0.0 {
                let taken = firm
                    .as_mut()
                    .map(|shop| shop.take_from_shelf(good, need))
                    .unwrap_or(0.0);
                debug_assert!(
                    taken + 1e-12 >= need,
                    "remainder shelf must cover leftover tender"
                );
            }
        }
    }
}

/// True if `good` has a shop target or is on any desire list.
fn pop_good_is_desired(pop: &Pop, good: usize) -> bool {
    if pop
        .property
        .get(&good)
        .is_some_and(|row| row.shop_target > 0.0)
    {
        return true;
    }
    pop.desires.iter().flatten().any(|desire| {
        desire.target.iter().any(|target| target.good == good)
    })
}

/// `1 - penalty * (1 - quote(S))`. Penalty 0/0.25/0.50/1 => consume/save/extra/unused.
fn pop_amv_factor(penalty: f64, salability: f64) -> f64 {
    debug_assert!(
        (0.0..=1.0).contains(&penalty),
        "salability penalty must be in 0..=1"
    );
    let quote = crate::game::config::salability_quote_factor(salability);
    1.0 - penalty * (1.0 - quote)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum PopBagKind {
    Unused,
    ExtraDesired,
    Save,
    Consume,
}

fn pop_row_bands(pop: &Pop, good: usize) -> (f64, f64, f64) {
    let Some(row) = pop.property.get(&good) else {
        return (0.0, 0.0, 0.0);
    };
    debug_assert!(row.quantity.is_finite(), "quantity must be finite");
    debug_assert!(row.desire_needs.is_finite(), "desire_needs must be finite");
    debug_assert!(row.shop_target.is_finite(), "shop_target must be finite");
    let have = row.quantity.max(0.0);
    let consume_end = row.desire_needs.max(0.0);
    let extra_start = row.shop_target.max(consume_end);
    let need = have.min(consume_end);
    let save = (have.min(extra_start) - consume_end).max(0.0);
    let extra = (have - extra_start).max(0.0);
    (need, save, extra)
}

fn pop_received_kind(pop: &Pop, good: usize) -> PopBagKind {
    let Some(row) = pop.property.get(&good) else {
        if pop_good_is_desired(pop, good) {
            return PopBagKind::ExtraDesired;
        }
        return PopBagKind::Unused;
    };
    if row.desire_needs > row.quantity {
        return PopBagKind::Consume;
    }
    if row.shop_target > row.quantity {
        return PopBagKind::Save;
    }
    if pop_good_is_desired(pop, good) {
        PopBagKind::ExtraDesired
    } else {
        PopBagKind::Unused
    }
}

/// Best received category across the bag (consume > save > extra-desired > unused).
fn pop_bag_kind(pop: &Pop, deal: &ProposedDeal, role: DealRole) -> PopBagKind {
    let mut best = PopBagKind::Unused;
    for (good, qty) in deal.goods_received(role) {
        debug_assert!(qty >= 0.0 && qty.is_finite(), "received qty must be >= 0.0");
        best = best.max(pop_received_kind(pop, good));
    }
    best
}

fn pop_received_factor(kind: PopBagKind, salability: f64) -> f64 {
    match kind {
        PopBagKind::Consume => 1.0,
        PopBagKind::Save => {
            pop_amv_factor(deal_constants::POP_AMV_SAVE_PENALTY, salability)
        }
        PopBagKind::ExtraDesired => {
            pop_amv_factor(deal_constants::POP_AMV_UNNEEDED_PENALTY, salability)
        }
        PopBagKind::Unused => crate::game::config::salability_quote_factor(salability),
    }
}

/// Given units peel extra → save → consume. Remainder uses extra-desired
/// if the good is desired, else unused.
fn pop_given_amv(pop: &Pop, history: &MarketHistory, good: usize, qty: f64) -> f64 {
    debug_assert!(qty >= 0.0 && qty.is_finite(), "given qty must be >= 0.0");
    let price = history.price(good);
    let salability = history.salability(good);
    let (need, save, extra) = pop_row_bands(pop, good);
    let extra_factor = if pop_good_is_desired(pop, good) {
        pop_amv_factor(deal_constants::POP_AMV_UNNEEDED_PENALTY, salability)
    } else {
        crate::game::config::salability_quote_factor(salability)
    };
    let save_factor = pop_amv_factor(deal_constants::POP_AMV_SAVE_PENALTY, salability);
    let from_extra = qty.min(extra);
    let rest = qty - from_extra;
    let from_save = rest.min(save);
    let rest = rest - from_save;
    let from_need = rest.min(need);
    let from_over = rest - from_need;
    (from_extra + from_over) * price * extra_factor
        + from_save * price * save_factor
        + from_need * price
}

/// Received AMV / given AMV for this pop.
pub(crate) fn pop_amv_percent_keep(
    pop: &Pop,
    deal: &ProposedDeal,
    role: DealRole,
    history: &MarketHistory,
) -> f64 {
    let mut given = 0.0;
    for (good, qty) in deal.goods_given(role) {
        given += pop_given_amv(pop, history, good, qty);
    }
    if given <= 0.0 {
        return f64::INFINITY;
    }
    let kind = pop_bag_kind(pop, deal, role);
    let mut received = 0.0;
    for (good, qty) in deal.goods_received(role) {
        received += qty * history.price(good) * pop_received_factor(kind, history.salability(good));
    }
    received / given
}

/// Returns how many units of `good` this pop can tender (0 if it is `targeted_good`).
/// Same-tier and lower-tier stock stays above `shop_target.max(reserved)`.
/// Stock whose only desires are above `shop_tier` can pay for this buy.
/// Listed offer qty is excluded either way.
fn pop_tenderable(
    pop: &Pop,
    good: usize,
    targeted_good: usize,
    shop_tier: u8,
    factuals: &Factuals,
) -> f64 {
    if good == targeted_good {
        return 0.0;
    }
    if !factuals.find_good(good).is_buyable() {
        return 0.0;
    }
    let Some(row) = pop.property.get(&good) else {
        return 0.0;
    };
    let keep = tender_keep(pop, good, shop_tier);
    let listed: f64 = pop
        .current_orders
        .iter()
        .filter(|order| order.target == good && order.target_amount < 0.0)
        .map(|order| -order.target_amount)
        .sum();
    (row.quantity - keep - listed).max(0.0)
}

/// Units of `good` that stay out of a shop-tier `shop_tier` payment.
fn tender_keep(pop: &Pop, good: usize, shop_tier: u8) -> f64 {
    let Some(row) = pop.property.get(&good) else {
        return 0.0;
    };
    let fenced_all = fenced_for_tiers(pop, good, u8::MAX);
    let manual = (row.reserved - fenced_all).max(0.0);
    if desire_targets_through(pop, good, shop_tier) {
        return row.shop_target.max(row.reserved).max(0.0);
    }
    if desire_targets_above(pop, good, shop_tier) {
        return manual;
    }
    row.shop_target.max(row.reserved).max(0.0)
}

fn desire_targets_through(pop: &Pop, good: usize, max_tier: u8) -> bool {
    pop.desires.iter().take(max_tier as usize + 1).flatten().any(|desire| {
        desire.target.iter().any(|target| target.good == good)
    })
}

fn desire_targets_above(pop: &Pop, good: usize, max_tier: u8) -> bool {
    pop.desires
        .iter()
        .skip(max_tier as usize + 1)
        .flatten()
        .any(|desire| desire.target.iter().any(|target| target.good == good))
}

/// How much of `good` a reservation of tiers `0..=max_tier` would earmark
/// from the quantity on hand.
fn fenced_for_tiers(pop: &Pop, good: usize, max_tier: u8) -> f64 {
    let mut avail: HashMap<usize, f64> = pop
        .property
        .iter()
        .map(|(&id, row)| (id, row.quantity.max(0.0)))
        .collect();
    let mut fenced = 0.0;
    let last = (max_tier as usize).min(pop.desires.len().saturating_sub(1));
    if pop.desires.is_empty() {
        return 0.0;
    }
    for tier in pop.desires.iter().take(last + 1) {
        for desire in tier {
            let amount = desire.amount.max(0.0);
            if amount <= 0.0 {
                continue;
            }
            let mut remaining = amount;
            for target in desire.ordered_targets() {
                if remaining <= 0.0 || target.efficiency <= 0.0 {
                    break;
                }
                let Some(have) = avail.get_mut(&target.good) else {
                    continue;
                };
                if *have <= 0.0 {
                    continue;
                }
                let want_sat = remaining.min(amount * target.cap);
                if want_sat <= 0.0 {
                    continue;
                }
                let take = (want_sat / target.efficiency).min(*have);
                *have -= take;
                if target.good == good {
                    fenced += take;
                }
                remaining -= take * target.efficiency;
            }
        }
    }
    fenced
}

/// Shop shelf above the stock reserve. Remainder owners pay with this pile.
fn remainder_spendable(firm: &Firm, good: usize, targeted_good: usize, factuals: &Factuals) -> f64 {
    if good == targeted_good {
        return 0.0;
    }
    if !factuals.find_good(good).is_buyable() {
        return 0.0;
    }
    firm.property
        .get(&good)
        .map(|row| row.sellable())
        .unwrap_or(0.0)
}

fn pop_tenderable_with_firm(
    pop: &Pop,
    good: usize,
    targeted_good: usize,
    shop_tier: u8,
    factuals: &Factuals,
    firm: Option<&Firm>,
) -> f64 {
    let bag = pop_tenderable(pop, good, targeted_good, shop_tier, factuals);
    let shop = firm
        .map(|firm| remainder_spendable(firm, good, targeted_good, factuals))
        .unwrap_or(0.0);
    bag + shop
}

fn pop_live_tenders_with_firm(
    pop: &Pop,
    targeted_good: usize,
    shop_tier: u8,
    history: &MarketHistory,
    factuals: &Factuals,
    firm: Option<&Firm>,
) -> Vec<(usize, f64)> {
    let mut ids: HashSet<usize> = pop.property.keys().copied().collect();
    if let Some(firm) = firm {
        ids.extend(firm.property.keys().copied());
    }
    collect_tenders(ids, history, |good| {
        pop_tenderable_with_firm(pop, good, targeted_good, shop_tier, factuals, firm)
    })
}

fn remainder_transport_cover(pop: &Pop, firm: Option<&Firm>, factuals: &Factuals) -> f64 {
    let mut pairs: Vec<(usize, f64)> = pop
        .property
        .iter()
        .map(|(id, row)| (*id, row.quantity.max(0.0)))
        .collect();
    if let Some(firm) = firm {
        for (&id, row) in &firm.property {
            let extra = row.sellable();
            if let Some((_, qty)) = pairs.iter_mut().find(|(good, _)| *good == id) {
                *qty += extra;
            } else {
                pairs.push((id, extra));
            }
        }
    }
    transport_cover_on_hand(pairs, factuals)
}
