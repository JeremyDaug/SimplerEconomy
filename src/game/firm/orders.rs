use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::config::MarketConfig;
use crate::game::factuals::Factuals;
use crate::game::market::MarketHistory;
use crate::game::marketorder::{compose_sell_priority_with, MarketOrder};
use crate::game::util::{lerp, round_units, whole_units, whole_units_up};

use super::{Firm, FirmPRow};

impl Firm {
    /// # Create Orders
    ///
    /// Turns current [`FirmPRow`] targets and on-hand stock into market orders.
    /// Read-only: does not edit the firm.
    ///
    /// 1. Classify each tradeable row's free on-hand pile as **sell**, **exchange**,
    ///    and/or **liquidate**. Production-fenced stock is not in that pile.
    /// 2. Emit sell/offer orders, then buy/request orders funded by exchange AMV
    ///    plus expected sell and liquidate AMV (optimistic: assumes outgoing fills).
    ///
    /// Exchange if salability >= [`crate::game::config::market_constants::EXCHANGE_SALABILITY_MIN`].
    /// Dedicated sell if posted sell > 0 (`min(sell_target, max market
    /// salability * daily output)` for goods this firm makes). When both
    /// apply, salability lerps the free pile from 90% sell / 10% exchange at
    /// the exchange floor to 10% sell / 90% exchange at salability 1.0.
    /// Exchange units are rounded to nearest; sell is the remainder, then
    /// capped at posted sell (overflow stays exchange).
    ///
    /// Liquidate if the row has free stock and no purchase, sell, or use target,
    /// and it is not exchange-eligible. Those units are leftover barter and go
    /// out as offer orders, never priced sell orders.
    ///
    /// Dual buy+sell: producer inputs (`use_target` > 0) buy only the stock-target
    /// shortfall and sell only free excess. Merchants (no `use_target`) emit the
    /// full `purchase_target` even above stock target. Buy is incoming stock, not
    /// an on-hand role, so a row may still buy and sell the same good.
    ///
    /// Buys stop when spendable AMV is exhausted; the last buy may overdraw.
    /// AMV on priced orders is recorded (`amv_target`) for later settlement.
    /// Matching does not use it yet. [`FirmPRow::amv_bound`] is a planning
    /// guidestone (residual WTP / input-cost rollup), not a trade gate:
    /// `create_orders` still posts the row's own bid/ask and does not skip a
    /// buy when market AMV is above the cap. Deal formation does not void a
    /// basket against the bound either. This does not compute residual WTP;
    /// planning writes the bound.
    /// Buy order priority is the merchant band if any row is merchant-like
    /// (purchase and sell, no use), otherwise the producer band. Sells use
    /// [`compose_sell_priority`].
    ///
    /// Posted buy/sell/offer amounts are whole units. Named counters ceil to
    /// the next whole payment unit so a 2.5 AMV cost is posted as 3 coins.
    /// Bid and ask AMV stay fractional.
    /// Sell orders name a barter shortcut: the most valuable process input
    /// the firm still needs, else the market's most salable money good (at
    /// or above the exchange floor), even if not on-hand. Tied money
    /// salability prefers the lower id. Buy orders still name an on-hand
    /// exchange good.
    /// 
    /// TODO: Ideally, a firm should have counteroffer goods that it wants
    /// to recieve for it's inputs. It should prioritize getting up to 1 day's 
    /// worth of inputs for each produciton line, focusing on the most valuable
    /// first. Once itt has 1 day for all of it's needs, it defaults to highest
    /// salability good instead.
    pub fn create_orders(
        &self,
        history: &MarketHistory,
        factuals: &Factuals,
        unavailable: &HashSet<usize>,
    ) -> Vec<MarketOrder> {
        let mut line_rank: HashMap<usize, usize> = HashMap::new();
        for (idx, line) in self.production_line.iter().enumerate() {
            for &good_id in &line.inputs {
                line_rank.entry(good_id).or_insert(idx);
            }
        }

        let mut plans: Vec<RowPlan> = Vec::new();
        let mut merchant_like = false;

        for (&good, row) in &self.property {
            if !factuals.find_good(good).is_buyable() {
                continue;
            }

            if row.purchase_target > 0.0 && row.sell_target > 0.0 && row.use_target == 0.0 {
                merchant_like = true;
            }

            let salability = history.salability(good);
            let market_amv = history.price(good);
            let mid = row.mid_amv(market_amv);
            let sell_plan = self.posted_sell_qty(good, history, factuals);
            let split = classify_on_hand(row, salability, &factuals.config.market, sell_plan);
            let buy_qty = whole_units(if unavailable.contains(&good) {
                0.0
            } else {
                row.purchase_qty()
            });
            let sell_qty = whole_units(split.sell);
            let exchange_qty = whole_units(split.exchange);
            let liquidate_qty = whole_units(split.liquidate);

            debug_assert!(sell_qty >= 0.0, "sell_qty must be >= 0.0");
            debug_assert!(exchange_qty >= 0.0, "exchange_qty must be >= 0.0");
            debug_assert!(liquidate_qty >= 0.0, "liquidate_qty must be >= 0.0");
            debug_assert!(buy_qty >= 0.0, "buy_qty must be >= 0.0");
            debug_assert!(
                liquidate_qty == 0.0 || (sell_qty == 0.0 && exchange_qty == 0.0),
                "liquidate stock cannot also be sell or exchange"
            );

            if buy_qty == 0.0
                && sell_qty == 0.0
                && exchange_qty == 0.0
                && liquidate_qty == 0.0
            {
                continue;
            }

            plans.push(RowPlan {
                good,
                buy_qty,
                sell_qty,
                exchange_qty,
                liquidate_qty,
                use_target: row.use_target,
                bid: row.bid_amv(mid),
                ask: row.ask_amv(mid),
                salability,
                line_rank: line_rank.get(&good).copied().unwrap_or(usize::MAX),
            });
        }

        let prio = &factuals.config.market_priority;
        let buy_band = if merchant_like {
            prio.firm_merchant()
        } else {
            prio.firm_producer()
        };

        let mut exchange_goods: Vec<(usize, f64, f64)> = plans
            .iter()
            .filter(|plan| plan.exchange_qty > 0.0)
            .map(|plan| (plan.good, plan.salability, history.price(plan.good)))
            .collect();
        exchange_goods.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });

        let mut spendable = 0.0;
        for plan in &plans {
            let price = history.price(plan.good);
            if price > 0.0 {
                spendable += plan.exchange_qty * price;
                spendable += plan.liquidate_qty * price;
            }
            if plan.ask > 0.0 {
                spendable += plan.sell_qty * plan.ask;
            }
        }

        let mut orders: Vec<MarketOrder> = Vec::new();
        let mut outgoing: Vec<&RowPlan> = plans
            .iter()
            .filter(|plan| plan.sell_qty > 0.0 || plan.liquidate_qty > 0.0)
            .collect();
        outgoing.sort_by_key(|plan| plan.good);
        for plan in outgoing {
            let (qty, liquidate) = if plan.liquidate_qty > 0.0 {
                (plan.liquidate_qty, true)
            } else {
                (plan.sell_qty, false)
            };
            let weight = compose_sell_priority_with(
                buy_band,
                qty,
                0.0,
                prio.sell_actor_priority_floor,
                prio.successful_sell_bonus,
            );
            if liquidate {
                orders.push(MarketOrder::offer_order(
                    Actor::Firm(self.id),
                    plan.good,
                    -qty,
                    weight,
                ));
            } else if let Some((pay_good, pay_price)) =
                needed_input_counter(self, history, plan.good)
                    .or_else(|| {
                        sell_pay_good(
                            history,
                            plan.good,
                            factuals.config.market.exchange_salability_min,
                        )
                    })
                    .or_else(|| counter_good(&exchange_goods, plan.good))
            {
                let pay = whole_units_up(qty * plan.ask / pay_price);
                if pay > 0.0 {
                    orders.push(MarketOrder::sell_order(
                        Actor::Firm(self.id),
                        plan.good,
                        -qty,
                        plan.ask,
                        pay_good,
                        pay,
                        weight,
                    ));
                } else {
                    orders.push(MarketOrder::offer_order(
                        Actor::Firm(self.id),
                        plan.good,
                        -qty,
                        weight,
                    ));
                }
            } else {
                orders.push(MarketOrder::offer_order(
                    Actor::Firm(self.id),
                    plan.good,
                    -qty,
                    weight,
                ));
            }
        }

        let mut buys: Vec<&RowPlan> = plans.iter().filter(|plan| plan.buy_qty > 0.0).collect();
        buys.sort_by(|a, b| {
            let a_prod = if a.use_target > 0.0 { 0 } else { 1 };
            let b_prod = if b.use_target > 0.0 { 0 } else { 1 };
            a_prod
                .cmp(&b_prod)
                .then(a.line_rank.cmp(&b.line_rank))
                .then(a.good.cmp(&b.good))
        });

        let mut remaining = spendable;
        for plan in buys {
            if remaining <= 0.0 {
                break;
            }
            let cost = plan.buy_qty * plan.bid;
            if let Some((pay_good, pay_price)) = counter_good(&exchange_goods, plan.good) {
                let pay = whole_units_up(plan.buy_qty * plan.bid / pay_price);
                if pay > 0.0 {
                    orders.push(MarketOrder::buy_order(
                        Actor::Firm(self.id),
                        plan.good,
                        plan.buy_qty,
                        plan.bid,
                        pay_good,
                        -pay,
                        buy_band,
                    ));
                } else {
                    orders.push(MarketOrder::request_order(
                        Actor::Firm(self.id),
                        plan.good,
                        plan.buy_qty,
                        buy_band,
                    ));
                }
            } else {
                orders.push(MarketOrder::request_order(
                    Actor::Firm(self.id),
                    plan.good,
                    plan.buy_qty,
                    buy_band,
                ));
            }
            remaining -= cost;
        }

        orders
    }
}

/// Per-row shopping plan built by [`Firm::create_orders`].
struct RowPlan {
    good: usize,
    buy_qty: f64,
    sell_qty: f64,
    exchange_qty: f64,
    liquidate_qty: f64,
    use_target: f64,
    bid: f64,
    ask: f64,
    salability: f64,
    line_rank: usize,
}

/// Split of free on-hand stock for [`classify_on_hand`].
pub(super) struct OnHandSplit {
    pub(super) sell: f64,
    exchange: f64,
    pub(super) liquidate: f64,
}

impl OnHandSplit {
    fn empty() -> Self {
        Self {
            sell: 0.0,
            exchange: 0.0,
            liquidate: 0.0,
        }
    }
}

/// Split free on-hand stock into sell, exchange, and/or liquidate.
/// Production-fenced units are already excluded by [`FirmPRow::free_for_market`].
/// `sell_plan` is posted sell ([`Firm::posted_sell_qty`]), not unconstrained
/// `sell_target`.
pub(super) fn classify_on_hand(
    row: &FirmPRow,
    salability: f64,
    market: &MarketConfig,
    sell_plan: f64,
) -> OnHandSplit {
    let free = row.free_for_market();
    if free <= 0.0 {
        return OnHandSplit::empty();
    }

    let trading = row.purchase_target > 0.0
        || row.sell_target > 0.0
        || row.use_target > 0.0;
    let can_exchange = salability >= market.exchange_salability_min;

    if !trading {
        if can_exchange {
            return OnHandSplit {
                sell: 0.0,
                exchange: free,
                liquidate: 0.0,
            };
        }
        return OnHandSplit {
            sell: 0.0,
            exchange: 0.0,
            liquidate: free,
        };
    }

    let can_sell = sell_plan > 0.0;
    if can_sell && can_exchange {
        let span = 1.0 - market.exchange_salability_min;
        let t = if span > 0.0 {
            ((salability - market.exchange_salability_min) / span).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let edge = market.sell_exchange_edge;
        let exchange_frac = lerp(edge, 1.0 - edge, t);
        let mut exchange_qty = round_units(free * exchange_frac).clamp(0.0, free);
        let mut sell_qty = free - exchange_qty;
        if sell_qty > sell_plan {
            exchange_qty += sell_qty - sell_plan;
            sell_qty = sell_plan;
        }
        OnHandSplit {
            sell: sell_qty,
            exchange: exchange_qty,
            liquidate: 0.0,
        }
    } else if can_sell {
        OnHandSplit {
            sell: sell_plan.min(free),
            exchange: 0.0,
            liquidate: 0.0,
        }
    } else if can_exchange {
        OnHandSplit {
            sell: 0.0,
            exchange: free,
            liquidate: 0.0,
        }
    } else {
        OnHandSplit::empty()
    }
}

/// Returns the most valuable process input this firm still needs, as
/// (good id, unit price). Value is `max(purchase shortfall, use_target)`
/// times market AMV. Skips `exclude` and non-positive prices.
fn needed_input_counter(
    firm: &Firm,
    history: &MarketHistory,
    exclude: usize,
) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (&good, row) in &firm.property {
        if good == exclude || row.use_target <= 0.0 {
            continue;
        }
        let price = history.price(good);
        if price <= 0.0 {
            continue;
        }
        let need = row.purchase_qty().max(row.use_target);
        let value = need * price;
        let take = match best {
            None => true,
            Some((id, best_val, _)) => value > best_val || (value == best_val && good < id),
        };
        if take {
            best = Some((good, value, price));
        }
    }
    best.map(|(good, _, price)| (good, price))
}

/// Returns the market's most salable money good to ask as payment on a sell.
/// Skips `exclude`, non-positive prices, and goods below `min_sal`. Tied
/// salability prefers the lower good id. Does not require the firm to hold
/// the good.
fn sell_pay_good(
    history: &MarketHistory,
    exclude: usize,
    min_sal: f64,
) -> Option<(usize, f64)> {
    let mut goods: HashSet<usize> = history.prices.keys().copied().collect();
    goods.extend(history.salability.keys().copied());
    let mut best: Option<(usize, f64, f64)> = None;
    for good in goods {
        if good == exclude {
            continue;
        }
        let price = history.price(good);
        if price <= 0.0 {
            continue;
        }
        let sal = history.salability(good);
        if sal < min_sal {
            continue;
        }
        let take = match best {
            None => true,
            Some((id, best_sal, _)) => sal > best_sal || (sal == best_sal && good < id),
        };
        if take {
            best = Some((good, sal, price));
        }
    }
    best.map(|(good, _, price)| (good, price))
}

/// First on-hand exchange tender that is not `exclude`, as (good id, unit price).
/// Skips non-positive AMV so counter amounts keep the buy/sell sign.
fn counter_good(exchange_goods: &[(usize, f64, f64)], exclude: usize) -> Option<(usize, f64)> {
    exchange_goods.iter().find_map(|&(good, _, price)| {
        if good != exclude && price > 0.0 {
            debug_assert!(price.is_finite(), "tender AMV must be finite");
            Some((good, price))
        } else {
            None
        }
    })
}
