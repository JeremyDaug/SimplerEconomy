use std::collections::HashSet;

use crate::game::actor::Actor;
use crate::game::factuals::Factuals;
use crate::game::market::MarketHistory;
use crate::game::marketorder::MarketOrder;
use crate::game::util::whole_units;

use super::Pop;

impl Pop {
    /// # Create Orders
    /// 
    /// Creates request and offer orders for a pop. 
    /// 
    /// For now, this only puts out it's desires, not its, possible offers.
    /// 
    /// When Offer Orders can be put out, it should only be when the pop is 
    /// despirate enough to offer goods in exchange for other things.
    /// 
    /// When creating oredrs for Luxury needs, it will only do one pass, even if the 
    /// budget has excess at the end.
    ///
    /// Posted request amounts are whole units. A shortfall below 1 is skipped.
    pub fn create_orders(
        &self,
        market_history: &MarketHistory,
        factuals: &Factuals,
        unavailable: &std::collections::HashSet<usize>,
    ) -> Vec<MarketOrder> {
        let mut orders: Vec<MarketOrder> = Vec::new();
        let pop_start = factuals.config.market_priority.pop_start;

        let mut remaining_budget = self.current_excess_value(market_history);
        let mut seen = HashSet::new();

        // go through desires, creating orders only for those goods which have targets
        for tier in self.desires.iter() {
            for desire in tier.iter() {
                for &target in desire.ordered_targets().iter() {
                    if remaining_budget <= 0.0 {
                        // if we've overdrawn by this point, break out early.
                        break;
                    }
                    if !self.property.contains_key(&target.good) ||
                    self.property.get(&target.good).unwrap().shop_target == 0.0 {
                        // skip if we don't have a record of the good
                        // or if the good has no target.
                        continue;
                    }
                    if  seen.contains(&target.good) {
                        continue;
                    }
                    if unavailable.contains(&target.good) {
                        continue;
                    }
                    seen.insert(target.good);

                    let good_price = market_history.prices.get(&target.good)
                        .unwrap_or(&1.0);
                    let purchase_target = whole_units(
                        self.property.get(&target.good).unwrap().shop_target
                            - self.property.get(&target.good).unwrap().quantity,
                    );
                    if purchase_target <= 0.0 {
                        continue;
                    }
                    let cost = purchase_target * good_price;

                    // create order for full amount
                    orders.push(MarketOrder::request_order(
                        Actor::Pop(self.id), target.good, purchase_target,
                        pop_start));
                    remaining_budget -= cost;
                }
            }
        }

        // Planned shop shortfalls that are not desire targets (parked savings).
        // Part of the shop plan, so it is filled before opportunistic extra buys.
        if remaining_budget > 0.0 {
            let mut extras: Vec<(usize, f64, f64)> = Vec::new();
            for (&good_id, row) in &self.property {
                if seen.contains(&good_id) || row.shop_target <= 0.0 {
                    continue;
                }
                if unavailable.contains(&good_id) {
                    continue;
                }
                if !factuals.find_good(good_id).is_buyable() {
                    continue;
                }
                let purchase = whole_units(row.shop_target - row.quantity);
                if purchase <= 0.0 {
                    continue;
                }
                extras.push((good_id, purchase, market_history.price(good_id)));
            }
            extras.sort_by_key(|(id, _, _)| *id);
            for (good_id, purchase, price) in extras {
                if remaining_budget <= 0.0 {
                    break;
                }
                orders.push(MarketOrder::request_order(
                    Actor::Pop(self.id), good_id, purchase,
                    pop_start));
                remaining_budget -= purchase * price;
                seen.insert(good_id);
            }
        }

        // if we have no budget left, return our current orders.
        if remaining_budget <= 0.0 {
            return orders;
        }

        // if we still have budget, repeat, adding all desires to possibly reach our goals until we do
        if remaining_budget > 0.0 {
            for tier in self.desires.iter() {
                for desire in tier.iter() {
                    for &target in desire.ordered_targets().iter() {
                        if remaining_budget <= 0.0 {
                            // if we've overdrawn by this point, break out early.
                            break;
                        }
                        if self.property.contains_key(&target.good) &&
                        self.property.get(&target.good).unwrap().shop_target > 0.0 {
                            // If we have a record of it, nad that record has a target, 
                            // we've already added it, so skip.
                            continue;
                        }
                        if !factuals.goods[&target.good].is_buyable() {
                            // if the good is not buyable, skip it.
                            continue;
                        }
                        if  seen.contains(&target.good) {
                            continue;
                        }
                        if unavailable.contains(&target.good) {
                            continue;
                        }
                        seen.insert(target.good);

                        let good_price = market_history.prices.get(&target.good).unwrap_or(&0.0);
                        let purchase_target =
                            whole_units(desire.amount * target.cap / target.efficiency);
                        if purchase_target <= 0.0 {
                            continue;
                        }
                        let cost = purchase_target * good_price;

                        // create order for full amount
                        orders.push(MarketOrder::request_order(
                            Actor::Pop(self.id), target.good, purchase_target,
                            pop_start));
                        remaining_budget -= cost;
                    }
                }
            }
        }

        // There is no third, but if there was, we'd just loop the last tier until we did run out of budget.

        orders
    }
}
