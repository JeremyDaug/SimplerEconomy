use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::factuals::Factuals;
use crate::game::market::MarketHistory;
use crate::game::marketorder::{compose_sell_priority_with, MarketOrder};
use crate::game::pop_property::PopPRow;
use crate::game::util::{whole_units, whole_units_up};

use super::Pop;

struct FreeGood {
    good: usize,
    units: f64,
    salability: f64,
    importance: i32,
    price: f64,
}

impl Pop {
    /// # Create Orders
    ///
    /// Morning market book: planned shop **requests**, then **offers** of leftover
    /// surplus. Does not replan shop/save. Extra desire buys belong on
    /// [`Self::next_shopping_trip`].
    ///
    /// 1. Basic desire-shop, then parked save, then common, then luxury.
    ///    A higher consume tier is posted only when remaining budget covers
    ///    **all** of that tier's shortfalls; otherwise that tier is posted in
    ///    walk order until overdraw and the next tier is skipped.
    ///    Request size is `whole_units_up(shop_target - quantity)` (overshoot).
    ///    Skip missing rows, `shop_target == 0`, `unavailable`, already-seen
    ///    goods, and shortfalls that ceil to 0.
    /// 2. Freeze enough leftover whole units (salability first, then lowest
    ///    desire importance) to cover posted request AMV. Those units stay
    ///    tenderable.
    /// 3. Offer every remaining whole unit of free stock (`quantity` above
    ///    `shop_target.max(reserved)`). Sell size is floored. Sell weight is
    ///    [`compose_sell_priority_with`].
    ///
    /// Writes [`Self::current_orders`] to the returned book so listed offer
    /// units are excluded from tenders. Offers name the first remaining
    /// request as `counter_offer` (good only). Requests name the most
    /// salable free good as payment coincidence.
    pub fn create_orders(
        &mut self,
        market_history: &MarketHistory,
        factuals: &Factuals,
        unavailable: &HashSet<usize>,
    ) -> Vec<MarketOrder> {
        let mut orders: Vec<MarketOrder> = Vec::new();
        let pop_start = factuals.config.market_priority.pop_start;
        let mut remaining_budget = self.current_excess_value(market_history);
        let mut seen = HashSet::new();
        let mut request_amv = 0.0;

        // 1. Basic restock, parked save, then common/luxury if the wallet covers them.
        self.push_shop_requests(
            &mut orders,
            &mut seen,
            &mut remaining_budget,
            &mut request_amv,
            market_history,
            factuals,
            unavailable,
            pop_start,
            Some(0),
            usize::MAX,
        );
        if remaining_budget > 0.0 {
            self.push_shop_requests(
                &mut orders,
                &mut seen,
                &mut remaining_budget,
                &mut request_amv,
                market_history,
                factuals,
                unavailable,
                pop_start,
                None,
                usize::MAX,
            );
        }
        if remaining_budget > 0.0 {
            self.push_higher_tier_requests(
                &mut orders,
                &mut seen,
                &mut remaining_budget,
                &mut request_amv,
                market_history,
                factuals,
                unavailable,
                pop_start,
                1,
            );
        }
        if remaining_budget > 0.0 {
            self.push_higher_tier_requests(
                &mut orders,
                &mut seen,
                &mut remaining_budget,
                &mut request_amv,
                market_history,
                factuals,
                unavailable,
                pop_start,
                2,
            );
        }

        self.push_cover_offers(
            &mut orders,
            request_amv,
            market_history,
            factuals,
            pop_start,
            false,
        );
        self.stamp_named_counters(&mut orders, market_history);
        self.current_orders = orders.clone();
        orders
    }

    /// # Next Shopping Trip
    ///
    /// Intra-day follow-up after this pop's buy orders are gone. Solidifies
    /// on-hand shop/desire stock into `reserved`, then posts **at most one
    /// request and one offer**, each for the full remaining want / surplus of
    /// that one good (ceil request, floor offer) so later fills can scale the
    /// same order down instead of re-emitting.
    ///
    /// Request pick: remaining desire-shop shortfall, then parked shop, then
    /// one extra desire load that was not on the shop plan. Offer pick: after
    /// covering that request's AMV, the lowest-importance leftover free good.
    ///
    /// Caller should sync [`Self::current_orders`] to this pop's live book
    /// first. If a request is still open (including a parked miss), only an
    /// offer is considered. Otherwise one new request is posted, then one
    /// offer. Cover AMV includes still-open requests so the trip does not
    /// list the last tender.
    pub fn next_shopping_trip(
        &mut self,
        market_history: &MarketHistory,
        factuals: &Factuals,
        unavailable: &HashSet<usize>,
    ) -> Vec<MarketOrder> {
        self.solidify_purchases();

        let pop_start = factuals.config.market_priority.pop_start;
        let mut orders: Vec<MarketOrder> = Vec::new();
        let mut seen = HashSet::new();
        let mut remaining_budget = f64::MAX;
        let mut request_amv = request_amv_of(self.current_orders.iter(), market_history);
        let has_request = self.current_orders.iter().any(|order| order.target_amount > 0.0);

        if !has_request {
            self.push_shop_requests(
                &mut orders,
                &mut seen,
                &mut remaining_budget,
                &mut request_amv,
                market_history,
                factuals,
                unavailable,
                pop_start,
                Some(0),
                1,
            );
            if orders.is_empty() {
                self.push_shop_requests(
                    &mut orders,
                    &mut seen,
                    &mut remaining_budget,
                    &mut request_amv,
                    market_history,
                    factuals,
                    unavailable,
                    pop_start,
                    None,
                    1,
                );
            }
            if orders.is_empty() && remaining_budget > 0.0 {
                self.push_shop_requests(
                    &mut orders,
                    &mut seen,
                    &mut remaining_budget,
                    &mut request_amv,
                    market_history,
                    factuals,
                    unavailable,
                    pop_start,
                    Some(1),
                    1,
                );
            }
            if orders.is_empty() && remaining_budget > 0.0 {
                self.push_shop_requests(
                    &mut orders,
                    &mut seen,
                    &mut remaining_budget,
                    &mut request_amv,
                    market_history,
                    factuals,
                    unavailable,
                    pop_start,
                    Some(2),
                    1,
                );
            }
            if orders.is_empty() {
                self.push_one_extra_desire_request(
                    &mut orders,
                    &mut request_amv,
                    market_history,
                    factuals,
                    unavailable,
                    pop_start,
                );
            }
        }

        self.push_cover_offers(
            &mut orders,
            request_amv,
            market_history,
            factuals,
            pop_start,
            true,
        );
        self.stamp_named_counters(&mut orders, market_history);
        self.current_orders.extend(orders.iter().cloned());
        orders
    }

    /// Raises `reserved` toward on-hand shop / desire keep so just-bought
    /// stock is not listed or tendered on the next trip.
    fn solidify_purchases(&mut self) {
        let mut keep: HashMap<usize, f64> = HashMap::new();
        for (&id, row) in &self.property {
            keep.insert(id, row.shop_target.max(row.desire_needs));
        }
        for tier in &self.desires {
            for desire in tier {
                for target in &desire.target {
                    debug_assert!(
                        target.efficiency > 0.0,
                        "Desire target efficiency must be positive"
                    );
                    let want = desire.amount * target.cap / target.efficiency;
                    let entry = keep.entry(target.good).or_insert(0.0);
                    *entry = (*entry).max(want);
                }
            }
        }
        for (id, want) in keep {
            if let Some(row) = self.property.get_mut(&id) {
                row.reserved = row.quantity.min(row.reserved.max(want)).max(0.0);
            }
        }
    }

    /// Posts a higher consume tier: all of it when remaining budget covers
    /// every shortfall, otherwise walk order until overdraw.
    fn push_higher_tier_requests(
        &self,
        orders: &mut Vec<MarketOrder>,
        seen: &mut HashSet<usize>,
        remaining_budget: &mut f64,
        request_amv: &mut f64,
        market_history: &MarketHistory,
        factuals: &Factuals,
        unavailable: &HashSet<usize>,
        pop_start: f64,
        tier: usize,
    ) {
        let cost = self.tier_shop_cost(tier, seen, market_history, unavailable);
        if *remaining_budget + 1e-12 < cost {
            self.push_shop_requests(
                orders,
                seen,
                remaining_budget,
                request_amv,
                market_history,
                factuals,
                unavailable,
                pop_start,
                Some(tier),
                usize::MAX,
            );
            return;
        }
        self.push_shop_requests(
            orders,
            seen,
            remaining_budget,
            request_amv,
            market_history,
            factuals,
            unavailable,
            pop_start,
            Some(tier),
            usize::MAX,
        );
    }

    fn tier_shop_cost(
        &self,
        tier: usize,
        seen: &HashSet<usize>,
        market_history: &MarketHistory,
        unavailable: &HashSet<usize>,
    ) -> f64 {
        if tier >= self.desires.len() {
            return 0.0;
        }
        let mut cost = 0.0;
        let mut local = seen.clone();
        for desire in &self.desires[tier] {
            for target in desire.ordered_targets() {
                if local.contains(&target.good) || unavailable.contains(&target.good) {
                    continue;
                }
                let Some(row) = self.property.get(&target.good) else {
                    continue;
                };
                if row.shop_target == 0.0 {
                    continue;
                }
                let purchase = shop_purchase_units(row);
                if purchase < 1.0 {
                    continue;
                }
                local.insert(target.good);
                cost += purchase * market_history.price(target.good);
            }
        }
        cost
    }

    /// `only_tier` Some walks that desire tier; None walks parked non-desire
    /// `shop_target` rows, sorted by good id.
    fn push_shop_requests(
        &self,
        orders: &mut Vec<MarketOrder>,
        seen: &mut HashSet<usize>,
        remaining_budget: &mut f64,
        request_amv: &mut f64,
        market_history: &MarketHistory,
        factuals: &Factuals,
        unavailable: &HashSet<usize>,
        pop_start: f64,
        only_tier: Option<usize>,
        max_new: usize,
    ) {
        let start_len = orders.len();
        if let Some(tier) = only_tier {
            let Some(desires) = self.desires.get(tier) else {
                return;
            };
            for desire in desires.iter() {
                for &target in desire.ordered_targets().iter() {
                    if *remaining_budget <= 0.0 || orders.len() - start_len >= max_new {
                        return;
                    }
                    self.try_push_shop_request(
                        target.good,
                        orders,
                        seen,
                        remaining_budget,
                        request_amv,
                        market_history,
                        factuals,
                        unavailable,
                        pop_start,
                        true,
                    );
                }
            }
            return;
        }

        let mut extras: Vec<usize> = self
            .property
            .iter()
            .filter(|(id, row)| {
                !seen.contains(id) && row.shop_target > 0.0
            })
            .map(|(&id, _)| id)
            .collect();
        extras.sort_unstable();
        for good_id in extras {
            if *remaining_budget <= 0.0 || orders.len() - start_len >= max_new {
                return;
            }
            self.try_push_shop_request(
                good_id,
                orders,
                seen,
                remaining_budget,
                request_amv,
                market_history,
                factuals,
                unavailable,
                pop_start,
                false,
            );
        }
    }

    fn has_shop_shortfall(&self, tier: usize) -> bool {
        let Some(desires) = self.desires.get(tier) else {
            return false;
        };
        for desire in desires {
            for target in desire.ordered_targets() {
                if let Some(row) = self.property.get(&target.good) {
                    if shop_purchase_units(row) >= 1.0 {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn try_push_shop_request(
        &self,
        good: usize,
        orders: &mut Vec<MarketOrder>,
        seen: &mut HashSet<usize>,
        remaining_budget: &mut f64,
        request_amv: &mut f64,
        market_history: &MarketHistory,
        factuals: &Factuals,
        unavailable: &HashSet<usize>,
        pop_start: f64,
        require_row: bool,
    ) {
        if seen.contains(&good) || unavailable.contains(&good) {
            return;
        }
        let Some(row) = self.property.get(&good) else {
            return;
        };
        if row.shop_target == 0.0 {
            return;
        }
        if !require_row && !factuals.find_good(good).is_buyable() {
            return;
        }
        let purchase = shop_purchase_units(row);
        if purchase < 1.0 {
            return;
        }
        seen.insert(good);
        let cost = purchase * market_history.price(good);
        orders.push(MarketOrder::request_order(
            Actor::Pop(self.id),
            good,
            purchase,
            pop_start,
        ));
        *remaining_budget -= cost;
        *request_amv += cost;
    }

    fn push_one_extra_desire_request(
        &self,
        orders: &mut Vec<MarketOrder>,
        request_amv: &mut f64,
        market_history: &MarketHistory,
        factuals: &Factuals,
        unavailable: &HashSet<usize>,
        pop_start: f64,
    ) {
        let listed: HashSet<usize> = self
            .current_orders
            .iter()
            .chain(orders.iter())
            .map(|order| order.target)
            .collect();
        let basic_short = self.has_shop_shortfall(0);
        let common_short = self.has_shop_shortfall(1);
        for (tier_idx, tier) in self.desires.iter().enumerate() {
            if tier_idx >= 1 && basic_short {
                break;
            }
            if tier_idx >= 2 && common_short {
                break;
            }
            for desire in tier.iter() {
                for &target in desire.ordered_targets().iter() {
                    if listed.contains(&target.good) || unavailable.contains(&target.good) {
                        continue;
                    }
                    if !factuals.find_good(target.good).is_buyable() {
                        continue;
                    }
                    if self
                        .property
                        .get(&target.good)
                        .is_some_and(|row| row.shop_target > 0.0)
                    {
                        continue;
                    }
                    debug_assert!(
                        target.efficiency > 0.0,
                        "Desire target efficiency must be positive"
                    );
                    let held = self
                        .property
                        .get(&target.good)
                        .map(|row| row.quantity)
                        .unwrap_or(0.0);
                    let want = desire.amount * target.cap / target.efficiency;
                    let purchase = whole_units_up((want - held).max(0.0));
                    if purchase < 1.0 {
                        continue;
                    }
                    let cost = purchase * market_history.price(target.good);
                    orders.push(MarketOrder::request_order(
                        Actor::Pop(self.id),
                        target.good,
                        purchase,
                        pop_start,
                    ));
                    *request_amv += cost;
                    return;
                }
            }
        }
    }

    /// Freeze tender cover, then post leftover free stock as offers.
    /// `one_offer` posts only the lowest-importance leftover good.
    fn push_cover_offers(
        &self,
        orders: &mut Vec<MarketOrder>,
        request_amv: f64,
        market_history: &MarketHistory,
        factuals: &Factuals,
        pop_start: f64,
        one_offer: bool,
    ) {
        let listed: HashMap<usize, f64> = listed_offer_qty_by_good(
            self.current_orders.iter().chain(orders.iter()),
        );
        let importance = self.desire_importance();
        let mut pool: Vec<FreeGood> = Vec::new();
        for (&good, row) in &self.property {
            if !factuals.find_good(good).is_buyable() {
                continue;
            }
            let already = listed.get(&good).copied().unwrap_or(0.0);
            let units = (free_units(row) - already).max(0.0);
            if units < 1.0 {
                continue;
            }
            pool.push(FreeGood {
                good,
                units,
                salability: market_history.salability(good),
                importance: importance.get(&good).copied().unwrap_or(0),
                price: market_history.price(good),
            });
        }
        assign_tender_cover(&mut pool, request_amv);

        let mut leftovers: Vec<&FreeGood> = pool.iter().filter(|item| item.units >= 1.0).collect();
        if one_offer {
            leftovers.sort_by(|a, b| {
                a.importance
                    .cmp(&b.importance)
                    .then_with(|| a.good.cmp(&b.good))
            });
            leftovers.truncate(1);
        } else {
            leftovers.sort_by_key(|item| item.good);
        }

        let prio = &factuals.config.market_priority;
        for item in leftovers {
            let weight = compose_sell_priority_with(
                pop_start,
                item.units,
                0.0,
                prio.sell_actor_priority_floor,
                prio.successful_sell_bonus,
            );
            orders.push(MarketOrder::offer_order(
                Actor::Pop(self.id),
                item.good,
                -item.units,
                weight,
            ));
        }
    }

    /// Offers name the first remaining request (not the offered good).
    /// Requests name the most salable free good (not the requested good).
    fn stamp_named_counters(&self, orders: &mut [MarketOrder], market_history: &MarketHistory) {
        let wants: Vec<usize> = self
            .current_orders
            .iter()
            .chain(orders.iter())
            .filter(|order| order.target_amount > 0.0)
            .map(|order| order.target)
            .collect();
        let pay = self.preferred_payment_good(market_history, &wants);
        for order in orders.iter_mut() {
            if order.target_amount < 0.0 {
                if let Some(want) = wants.iter().copied().find(|good| *good != order.target) {
                    *order = order.clone().with_counter_offer(want);
                }
            } else if order.target_amount > 0.0 {
                if let Some(pay_good) = pay {
                    if pay_good != order.target {
                        *order = order.clone().with_counter_offer(pay_good);
                    }
                }
            }
        }
    }

    fn preferred_payment_good(
        &self,
        market_history: &MarketHistory,
        exclude: &[usize],
    ) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for (&good, row) in &self.property {
            if exclude.contains(&good) || free_units(row) < 1.0 {
                continue;
            }
            let sal = market_history.salability(good);
            match best {
                Some((best_sal, best_id)) if sal < best_sal || (sal == best_sal && good >= best_id) => {}
                _ => best = Some((sal, good)),
            }
        }
        best.map(|(_, good)| good)
    }

    /// Higher is more important (earlier in desire walk). Missing goods are 0.
    fn desire_importance(&self) -> HashMap<usize, i32> {
        let mut map = HashMap::new();
        let mut score = i32::MAX / 4;
        for tier in &self.desires {
            for desire in tier {
                for target in desire.ordered_targets() {
                    map.entry(target.good).or_insert(score);
                    score -= 1;
                }
            }
        }
        map
    }
}

fn shop_purchase_units(row: &PopPRow) -> f64 {
    whole_units_up((row.shop_target - row.quantity).max(0.0))
}

fn free_units(row: &PopPRow) -> f64 {
    whole_units((row.quantity - row.shop_target.max(row.reserved)).max(0.0))
}

fn request_amv_of<'a, I>(orders: I, market_history: &MarketHistory) -> f64
where
    I: Iterator<Item = &'a MarketOrder>,
{
    orders
        .filter(|order| order.target_amount > 0.0)
        .map(|order| order.target_amount * market_history.price(order.target))
        .sum()
}

fn listed_offer_qty_by_good<'a, I>(orders: I) -> HashMap<usize, f64>
where
    I: Iterator<Item = &'a MarketOrder>,
{
    let mut listed = HashMap::new();
    for order in orders {
        if order.target_amount < 0.0 {
            *listed.entry(order.target).or_insert(0.0) += -order.target_amount;
        }
    }
    listed
}

/// Spends whole units from `pool` (already sorted in place) until `need_amv`
/// is covered. Leftover `units` are offerable.
fn assign_tender_cover(pool: &mut [FreeGood], mut need_amv: f64) {
    pool.sort_by(|a, b| {
        b.salability
            .partial_cmp(&a.salability)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.importance.cmp(&b.importance))
            .then_with(|| a.good.cmp(&b.good))
    });
    for item in pool.iter_mut() {
        if need_amv <= 0.0 {
            break;
        }
        if item.units < 1.0 || item.price <= 0.0 {
            continue;
        }
        let take = whole_units_up(need_amv / item.price).min(item.units);
        item.units -= take;
        need_amv -= take * item.price;
    }
}
