use std::collections::{HashMap, HashSet};

use crate::game::config::{firm_constants, FirmConfig};
use crate::game::factuals::Factuals;
use crate::game::market::MarketHistory;
use crate::game::process::{InputType, ProcessInput};
use crate::game::util::lerp;

use super::{Firm, FirmAmvBound, FirmPRow, ProductionLine};

impl Firm {
    /// # Record Keeping
    ///
    /// Writes each row's `rolling_average`, snapshots [`FirmRecords`],
    /// then calls [`Firm::plan`].
    /// Do not also call [`Firm::plan`] on the same day until
    /// snapshot work is split out.
    pub fn record_keeping(&mut self, factuals: &Factuals, history: &MarketHistory) {
        self.update_rolling_averages(&factuals.config.firm);
        self.update_records(&factuals.config.firm);
        self.plan(factuals, history);
    }

    /// Sets `rolling_average` on every property row to a lerp toward `quantity`.
    fn update_rolling_averages(&mut self, cfg: &FirmConfig) {
        for row in self.property.values_mut() {
            row.rolling_average = lerp(
                row.rolling_average,
                row.quantity.max(0.0),
                cfg.rolling_avg_weight,
            );
        }
    }

    /// Writes firm-wide sold/bought AMV, realized profit, and sell success.
    /// Also lerps each selling row's `average_price` toward today's unit sale AMV.
    fn update_records(&mut self, cfg: &FirmConfig) {
        let mut sold_amv = 0.0;
        let mut bought_amv = 0.0;
        let mut sold_cost = 0.0;
        let mut sold_units = 0.0;
        let mut sell_plan = 0.0;
        for row in self.property.values_mut() {
            let credited = row.placed_credited();
            let credited_amv = row.placed_credited_amv();
            sold_amv += row.sold_amv + credited_amv;
            bought_amv += row.bought_amv;
            if row.sold > 0.0 || credited > 0.0 {
                sold_cost += (row.sold + credited) * row.average_cost.max(0.0);
                let today = if row.sold + credited > 0.0 {
                    (row.sold_amv + credited_amv) / (row.sold + credited)
                } else {
                    0.0
                };
                if row.average_price == 0.0 {
                    row.average_price = today;
                } else {
                    row.average_price =
                        lerp(row.average_price, today, cfg.rolling_avg_weight);
                }
            }
            if row.sell_target > 0.0 {
                sold_units += row.sold.max(0.0) + credited;
                sell_plan += row.sell_target;
            }
            let w = cfg.rolling_avg_weight;
            row.sold_avg = blend_day_flow(row.sold_avg, row.sold, w);
            row.sell_fills_avg = blend_day_flow(row.sell_fills_avg, row.sell_fills, w);
            row.sell_rejects_avg = blend_day_flow(row.sell_rejects_avg, row.sell_rejects, w);
            row.sell_no_proposal_avg =
                blend_day_flow(row.sell_no_proposal_avg, row.sell_no_proposal, w);
        }

        let profit = if sold_cost > 0.0 {
            (sold_amv / sold_cost).max(0.0)
        } else if sold_amv > 0.0 {
            2.0
        } else if sell_plan > 0.0 {
            0.0
        } else {
            1.0
        };
        let success = if sell_plan > 0.0 {
            (sold_units / sell_plan).max(0.0)
        } else {
            1.0
        };

        self.records.sold_amv = sold_amv;
        self.records.bought_amv = bought_amv;
        self.records.sold_cost_amv = sold_cost;
        self.records.profit_ratio = profit;
        self.records.sell_success = success;
        self.records.profit_avg = lerp(self.records.profit_avg, profit, cfg.rolling_avg_weight);
        self.records.sell_success_avg =
            lerp(self.records.sell_success_avg, success, cfg.rolling_avg_weight);
    }

    /// # Plan
    ///
    /// Gathers line and market facts, then writes production-line `target`s,
    /// output `sell_target` / `amv_target`, and input use/stock/purchase
    /// fields. Does not run production or emit orders.
    ///
    /// 1. [`Self::gather_plan_info`]: profitability, sell success, turnover,
    ///    stockpile, decay loss, market AMV, and sell-meeting counts.
    ///    Competitor quotes are `None` until other firms are passed in.
    /// 2. [`Self::apply_plan_adjustments`]: walk one step on quote or quota
    ///    (or stay) by predicted profit. Quiet days lerp quota toward `aim`.
    ///    A line at 0 that is starting snaps to at least 1 iteration.
    /// 3. [`Self::rewrite_property_targets`]: input use/stock/purchase/reserve,
    ///    AMV bounds, merchant restock.
    pub fn plan(&mut self, factuals: &Factuals, history: &MarketHistory) {
        let cfg = factuals.config.firm;
        let info = self.gather_plan_info(factuals, history);
        self.apply_plan_adjustments(&info, history, &cfg);
        self.rewrite_property_targets(factuals, history, &cfg);
    }

    /// Builds a read-only snapshot of line and output-good facts for planning.
    fn gather_plan_info(&self, factuals: &Factuals, history: &MarketHistory) -> PlanGather {
        let mut lines = Vec::with_capacity(self.production_line.len());
        let mut goods: HashMap<usize, GoodFacts> = HashMap::new();

        for (i, line) in self.production_line.iter().enumerate() {
            let process = factuals
                .processes
                .get(&line.process)
                .expect("Process not found!");
            let mut outputs = Vec::new();
            for output in &process.outputs {
                outputs.push((output.good, output.amount));
                let facts = goods.entry(output.good).or_insert_with(|| {
                    good_facts_from_row(output.good, self.property.get(&output.good), factuals, history)
                });
                facts.maker_lines.push(i);
                facts.planned_output += planned_iterations(line) * output.amount;
            }
            lines.push(LineFacts {
                index: i,
                target: line.target,
                last_iterations: line.last_iterations,
                last_amv_consumed: line.last_amv_consumed,
                last_success_rate: line.last_success_rate,
                profitability: line_profit_ratio(line),
                missing_inputs: !line.last_missing_goods.is_empty(),
                cold: line.last_iterations == 0.0
                    && line.last_success_rate == 0.0
                    && line.last_missing_goods.is_empty(),
                outputs,
            });
        }

        PlanGather { lines, goods }
    }

    /// Writes line `target`s and output `sell_target` / `amv_target` from
    /// gathered facts. Leaves `target: None` unchanged. Cold-start lines keep
    /// their current target. A run miss walks quota toward last iterations.
    ///
    /// Other lines pick one walk step (raise/cut quote, raise/cut quota, or
    /// stay) by predicted profit. Quiet / stay days lerp quota toward `aim`.
    /// `growth_target` is the expansion gap (quota above aim) on a grow
    /// decision, else 0.
    fn apply_plan_adjustments(
        &mut self,
        info: &PlanGather,
        history: &MarketHistory,
        cfg: &FirmConfig,
    ) {
        let n = self.production_line.len();
        let pace = cfg.planning_lerp_rate;
        let mut desired_amv: HashMap<usize, f64> = HashMap::new();
        let mut desired_line: Vec<Option<f64>> = vec![None; n];
        let mut decided: Vec<bool> = vec![false; n];

        for facts in &info.lines {
            let line = &mut self.production_line[facts.index];
            if let Some(t) = facts.target {
                if line.aim == 0.0 {
                    line.aim = t;
                }
            }
            if facts.cold {
                desired_line[facts.index] = facts.target;
                continue;
            }
            let miss = line_sell_miss(facts, &info.goods, cfg);
            let evidence = line_aim_evidence(facts, &info.goods, miss);
            line.aim = lerp(line.aim, evidence, pace).max(0.0);
            let Some(current) = facts.target else {
                continue;
            };
            if line_run_miss(facts) {
                decided[facts.index] = true;
                desired_line[facts.index] =
                    Some(step_toward_actual(current, facts.last_iterations, cfg.shrink_rate));
            }
        }

        for good in info.goods.values() {
            if good.maker_lines.is_empty() {
                continue;
            }
            let all_cold = good.maker_lines.iter().all(|&i| {
                info.lines.get(i).map(|l| l.cold).unwrap_or(true)
            });
            if all_cold {
                desired_amv.insert(good.good, good.own_amv);
                continue;
            }
            let choice = plan_walk(good, &info.lines, cfg);
            desired_amv.insert(good.good, choice.quote);
            match choice.step {
                WalkStep::RaiseQuota | WalkStep::CutQuota => {
                    let grow = choice.step == WalkStep::RaiseQuota;
                    for &i in &good.maker_lines {
                        if decided[i] || info.lines[i].cold || info.lines[i].target.is_none() {
                            continue;
                        }
                        let current = info.lines[i].target.unwrap();
                        let factor = if grow {
                            1.0 + cfg.growth_rate
                        } else {
                            1.0 - cfg.shrink_rate
                        };
                        desired_line[i] = Some(step_quota(current, factor));
                        decided[i] = true;
                    }
                }
                WalkStep::Stay | WalkStep::RaiseQuote | WalkStep::CutQuote => {}
            }
            equalize_line_peers(&mut desired_line, &info.lines, &good.maker_lines);
        }

        for facts in &info.lines {
            if facts.target.is_none() {
                continue;
            }
            if facts.cold {
                desired_line[facts.index] = facts.target;
                continue;
            }
            if desired_line[facts.index].is_some() {
                continue;
            }
            let aim = self.production_line[facts.index].aim;
            desired_line[facts.index] =
                Some(next_line_target(facts.target.unwrap(), aim, pace));
        }

        for (line, want) in self.production_line.iter_mut().zip(desired_line) {
            if let (Some(_current), Some(target)) = (line.target, want) {
                line.target = Some(target.max(0.0));
            }
            if line.last_amv_consumed > 0.0 {
                let now = line.last_amv_produced / line.last_amv_consumed;
                line.historical_productivity = lerp(
                    line.historical_productivity,
                    now,
                    pace,
                );
            }
        }

        for good in info.goods.values() {
            if good.maker_lines.is_empty() {
                continue;
            }
            let quota_sell: f64 = good
                .maker_lines
                .iter()
                .map(|&i| {
                    let t = self.production_line[i].target.unwrap_or(0.0);
                    let amt = info.lines[i]
                        .outputs
                        .iter()
                        .find(|(g, _)| *g == good.good)
                        .map(|(_, a)| *a)
                        .unwrap_or(0.0);
                    t * amt
                })
                .sum();
            let any_decision = good.maker_lines.iter().any(|&i| decided[i]);
            let row = self.property.entry(good.good).or_insert_with(FirmPRow::new);
            if good.maker_lines.iter().all(|&i| info.lines[i].cold) {
                if row.sell_target <= 0.0 {
                    row.sell_target = good.planned_output.max(good.sold).max(0.0);
                }
            } else if any_decision || row.sell_target <= 0.0 {
                row.sell_target = quota_sell.max(0.0);
            } else {
                row.sell_target = lerp(row.sell_target, quota_sell, pace).max(0.0);
            }
            let mut growth = 0.0;
            for &i in &good.maker_lines {
                let line = &self.production_line[i];
                let Some(quota) = line.target else {
                    continue;
                };
                if !decided[i] || info.lines[i].missing_inputs || quota <= line.aim {
                    continue;
                }
                let amt = info.lines[i]
                    .outputs
                    .iter()
                    .find(|(g, _)| *g == good.good)
                    .map(|(_, a)| *a)
                    .unwrap_or(0.0);
                growth += (quota - line.aim) * amt;
            }
            row.growth_target = growth.max(0.0);
            if let Some(&amv) = desired_amv.get(&good.good) {
                let market = history.price(good.good);
                let current = if row.amv_target != 0.0 {
                    row.amv_target
                } else {
                    market
                };
                row.amv_target = clamp_quote_orbit(
                    lerp(current, amv, pace),
                    market,
                    cfg.quote_orbit,
                );
            }
        }
    }

    /// Sets `use_target`, `stock_target`, `purchase_target`, `sell_target`,
    /// `reserve_target`, `amv_bound`, `amv_target`, and `margin` on property
    /// rows from current production-line targets and today's bought / sold.
    ///
    /// `stock_target` is decay-adjusted [`FirmConfig::operations_cover`] days
    /// of operations (shrink the hold when rot would eat more than one day's
    /// output; overshoot when it would not). Output on hand counts as days
    /// already converted and reduces input days. Wages may raid this buffer;
    /// remainder still respects it.
    fn rewrite_property_targets(
        &mut self,
        factuals: &Factuals,
        history: &MarketHistory,
        cfg: &FirmConfig,
    ) {
        let pace = cfg.planning_lerp_rate;
        let (use_qty, make_qty) = recipe_flows(&self.production_line, factuals);
        let (use_ops, make_ops) =
            recipe_flows_at(&self.production_line, factuals, line_operation_iters);
        let bounds = recipe_bounds(&self.production_line, factuals, history);
        let cover = cfg.operations_cover;
        let output_days = operation_output_days(&self.property, &make_ops);

        let mut goods: HashSet<usize> = self.property.keys().copied().collect();
        goods.extend(use_qty.keys().copied());
        goods.extend(make_qty.keys().copied());
        goods.extend(use_ops.keys().copied());
        goods.extend(make_ops.keys().copied());

        for good_id in goods {
            let used = use_qty.get(&good_id).copied().unwrap_or(0.0);
            let made = make_qty.get(&good_id).copied().unwrap_or(0.0);
            let used_ops = use_ops.get(&good_id).copied().unwrap_or(0.0);
            let made_ops = make_ops.get(&good_id).copied().unwrap_or(0.0);
            let bound = bounds.get(&good_id).copied().unwrap_or(FirmAmvBound::None);
            let tradeable = factuals
                .goods
                .get(&good_id)
                .map(|g| g.is_buyable())
                .unwrap_or(true);

            let row = self.property.entry(good_id).or_insert_with(FirmPRow::new);
            let qty = row.quantity.max(0.0);
            let old_purchase = row.purchase_target;
            let old_sell = row.sell_target;
            let bought = row.bought;
            let sold = row.sold;
            let merchant = used <= 0.0
                && made <= 0.0
                && old_purchase > 0.0
                && old_sell > 0.0;

            if merchant {
                if sold > 0.0 {
                    row.purchase_target =
                        lerp(old_purchase, sold, pace).max(0.0);
                }
                row.sell_target = lerp(old_sell, qty, pace).max(0.0);
                let market = history.price(good_id);
                let current = if row.amv_target != 0.0 {
                    row.amv_target
                } else {
                    market
                };
                // Own quote. Do not lerp onto live market AMV.
                // Later: inventory pressure, rival quotes, strategy.
                row.amv_target = current;
                if row.margin == 0.0 {
                    row.margin = cfg.default_margin;
                }
                row.amv_bound = FirmAmvBound::None;
                row.sync_reserve();
                continue;
            }

            if used <= 0.0 && made <= 0.0 && used_ops <= 0.0 && made_ops <= 0.0 {
                continue;
            }

            row.use_target = used;
            let decay = factuals
                .goods
                .get(&good_id)
                .map(|g| g.decay_rate)
                .unwrap_or(1.0);
            let hold_days = FirmPRow::operations_hold_days(cover, decay);
            if made_ops > 0.0 {
                row.stock_target = made_ops * hold_days;
            }
            if used > 0.0 || used_ops > 0.0 {
                let input_days = (hold_days - output_days).max(0.0);
                let input_stock = used_ops * input_days;
                row.stock_target = if made_ops > 0.0 {
                    row.stock_target.max(input_stock)
                } else {
                    input_stock
                };
                row.purchase_target = if tradeable {
                    (row.stock_target - qty).max(0.0)
                } else {
                    0.0
                };
                let miss = if old_purchase > 0.0 {
                    (1.0 - (bought / old_purchase).clamp(0.0, 1.0)).max(0.0)
                } else {
                    0.0
                };
                let desired_reserve =
                    used * cfg.reserve_cover * (1.0 + cfg.miss_reserve_bonus * miss);
                row.reserve_target =
                    lerp(row.reserve_target, desired_reserve, pace).max(0.0);
            } else if made_ops <= 0.0 {
                row.stock_target = 0.0;
                row.purchase_target = 0.0;
            } else {
                row.purchase_target = 0.0;
            }

            if made <= 0.0 && used > 0.0 {
                row.sell_target = row.free_for_market();
            }

            row.amv_bound = bound;
            let market = history.price(good_id);
            if row.amv_target != 0.0 {
                row.amv_target = clamp_quote_orbit(row.amv_target, market, cfg.quote_orbit);
            }
            if used > 0.0 && old_purchase > 0.0 && bought < old_purchase && made <= 0.0 {
                let market = history.price(good_id);
                let current = if row.amv_target != 0.0 {
                    row.amv_target
                } else {
                    market
                };
                row.amv_target = clamp_quote_orbit(
                    lerp(current, current * (1.0 + cfg.amv_nudge), pace),
                    market,
                    cfg.quote_orbit,
                );
            }
            if row.purchase_target > 0.0 && row.sell_target > 0.0 && row.margin == 0.0 {
                row.margin = cfg.default_margin;
            }
            row.sync_reserve();
        }
    }
}

struct PlanGather {
    lines: Vec<LineFacts>,
    goods: HashMap<usize, GoodFacts>,
}

struct LineFacts {
    index: usize,
    target: Option<f64>,
    #[allow(dead_code)]
    last_iterations: f64,
    last_amv_consumed: f64,
    last_success_rate: f64,
    profitability: f64,
    missing_inputs: bool,
    cold: bool,
    outputs: Vec<(usize, f64)>,
}

struct GoodFacts {
    good: usize,
    sell_target: f64,
    sold: f64,
    placed: f64,
    produced: f64,
    planned_output: f64,
    #[allow(dead_code)]
    post_shop: f64,
    #[allow(dead_code)]
    sell_success: f64,
    /// Sold unit AMV / average cost. 0 if we meant to sell and didn't.
    #[allow(dead_code)]
    realized_profit: f64,
    #[allow(dead_code)]
    turnover: f64,
    #[allow(dead_code)]
    stockpile: f64,
    #[allow(dead_code)]
    decay_rate: f64,
    #[allow(dead_code)]
    decay_loss_amv: f64,
    own_amv: f64,
    market_amv: f64,
    #[allow(dead_code)]
    market_share: Option<f64>,
    #[allow(dead_code)]
    market_volume: Option<f64>,
    #[allow(dead_code)]
    market_volatility: Option<f64>,
    #[allow(dead_code)]
    market_trend: Option<f64>,
    /// Mean rival quote. `None` until other firms are passed into plan.
    #[allow(dead_code)]
    competitor_amv: Option<f64>,
    sell_fills: f64,
    sell_rejects: f64,
    sell_no_proposal: f64,
    sold_avg: f64,
    sell_fills_avg: f64,
    sell_rejects_avg: f64,
    sell_no_proposal_avg: f64,
    average_cost: f64,
    maker_lines: Vec<usize>,
}

/// Fills per-good facts from the property row and market snapshot.
fn good_facts_from_row(
    good: usize,
    row: Option<&FirmPRow>,
    factuals: &Factuals,
    history: &MarketHistory,
) -> GoodFacts {
    let sold = row.map(|r| r.sold).unwrap_or(0.0);
    let placed = row.map(|r| r.placed_credited()).unwrap_or(0.0);
    let produced = row.map(|r| r.produced).unwrap_or(0.0);
    let sell_target = row.map(|r| r.sell_target).unwrap_or(0.0);
    let post_shop = row.map(post_shop_stock).unwrap_or(0.0);
    let market_amv = history.price(good);
    let own_amv = row.map(|r| r.amv_target).filter(|v| *v != 0.0).unwrap_or(market_amv);
    let decay_rate = factuals.goods.get(&good).map(|g| g.decay_rate).unwrap_or(0.0);
    let purchased = history.purchased.get(&good).copied();
    let trail = history.amv_trails.get(&good);
    GoodFacts {
        good,
        sell_target,
        sold,
        placed,
        produced,
        planned_output: 0.0,
        post_shop,
        sell_success: sell_success_of(sold + placed, sell_target, produced),
        realized_profit: realized_profit_of(row),
        turnover: if produced > 0.0 { (sold + placed) / produced } else { 1.0 },
        stockpile: post_shop,
        decay_rate,
        decay_loss_amv: post_shop * decay_rate * market_amv,
        own_amv,
        market_amv,
        market_share: purchased.filter(|p| *p > 0.0).map(|p| (sold / p).clamp(0.0, 1.0)),
        market_volume: purchased,
        market_volatility: trail.and_then(|t| trail_volatility(t)),
        market_trend: trail.and_then(|t| trail_trend(t)),
        competitor_amv: None,
        sell_fills: row.map(|r| r.sell_fills).unwrap_or(0.0),
        sell_rejects: row.map(|r| r.sell_rejects).unwrap_or(0.0),
        sell_no_proposal: row.map(|r| r.sell_no_proposal).unwrap_or(0.0),
        sold_avg: row.map(|r| r.sold_avg).unwrap_or(0.0),
        sell_fills_avg: row.map(|r| r.sell_fills_avg).unwrap_or(0.0),
        sell_rejects_avg: row.map(|r| r.sell_rejects_avg).unwrap_or(0.0),
        sell_no_proposal_avg: row.map(|r| r.sell_no_proposal_avg).unwrap_or(0.0),
        average_cost: row.map(|r| r.average_cost.max(0.0)).unwrap_or(0.0),
        maker_lines: Vec::new(),
    }
}

/// Relative score gap treated as a tie. Closer sell-through wins the tie.
const WALK_SCORE_TIE: f64 = 0.05;

/// Snaps the first observation, then lerps. `clear_day_flows` does not touch this.
fn blend_day_flow(avg: f64, today: f64, weight: f64) -> f64 {
    debug_assert!(today.is_finite() && today >= 0.0);
    if avg <= 0.0 {
        today.max(0.0)
    } else {
        lerp(avg, today.max(0.0), weight)
    }
}

fn walk_sold(good: &GoodFacts) -> f64 {
    if good.sold_avg > 0.0 {
        good.sold_avg
    } else {
        good.sold.max(0.0)
    }
}

fn walk_meets(good: &GoodFacts) -> (f64, f64) {
    let used_avg = good.sell_fills_avg > 0.0
        || good.sell_rejects_avg > 0.0
        || good.sell_no_proposal_avg > 0.0;
    let fills = if used_avg {
        good.sell_fills_avg
    } else {
        good.sell_fills
    };
    let rejects = if used_avg {
        good.sell_rejects_avg
    } else {
        good.sell_rejects
    };
    let none = if used_avg {
        good.sell_no_proposal_avg
    } else {
        good.sell_no_proposal
    };
    let meets = fills.max(0.0) + rejects.max(0.0) + none.max(0.0);
    (meets, rejects.max(0.0) + none.max(0.0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WalkStep {
    Stay,
    RaiseQuote,
    CutQuote,
    RaiseQuota,
    CutQuota,
}

struct WalkChoice {
    step: WalkStep,
    quote: f64,
}

/// Picks one quote or quota step (or stay) by predicted contribution.
/// Scores market `sold` only; remainder placement is not demand.
fn plan_walk(good: &GoodFacts, lines: &[LineFacts], cfg: &FirmConfig) -> WalkChoice {
    let quote = good.own_amv.max(0.0);
    let market = good.market_amv;
    let band = cfg.quote_orbit;
    let cost = output_unit_cost(good, lines);
    let current_qty = implied_output(good, lines, None, cfg);
    let sold = walk_sold(good);
    let (meets, failed) = walk_meets(good);
    let failed_share = if meets > 0.0 { (failed / meets).clamp(0.0, 1.0) } else { 0.0 };
    let plan = if good.sell_target > 0.0 {
        good.sell_target
    } else {
        current_qty.max(good.produced).max(sold)
    };
    let strong = plan > 0.0 && sold / plan >= cfg.sell_success_grow;
    let miss = plan > 0.0 && sold / plan < cfg.sell_success_shrink;
    let underwater = plan > 0.0 && sold * quote + 1e-12 < current_qty.max(plan) * cost;

    let raise_q = clamp_quote_orbit(quote * (1.0 + plan_step(cfg.growth_rate)), market, band);
    let cut_q = clamp_quote_orbit(quote * (1.0 - plan_step(cfg.growth_rate)), market, band);
    let raise_qty = implied_output(good, lines, Some(WalkStep::RaiseQuota), cfg);
    let cut_qty = implied_output(good, lines, Some(WalkStep::CutQuota), cfg);

    let mut candidates = vec![walk_candidate(
        WalkStep::Stay,
        quote,
        current_qty,
        sold,
        failed,
        failed_share,
        meets,
        strong,
        cost,
    )];
    if strong {
        candidates.push(walk_candidate(
            WalkStep::RaiseQuote,
            raise_q,
            current_qty,
            sold,
            failed,
            failed_share,
            meets,
            strong,
            cost,
        ));
        let extra_sells = walk_expected_sold(
            WalkStep::RaiseQuota,
            sold,
            raise_qty,
            failed,
            failed_share,
            meets,
            strong,
        ) > sold + 1e-12;
        if !underwater || extra_sells {
            candidates.push(walk_candidate(
                WalkStep::RaiseQuota,
                quote,
                raise_qty,
                sold,
                failed,
                failed_share,
                meets,
                strong,
                cost,
            ));
        }
    }
    if miss {
        candidates.push(walk_candidate(
            WalkStep::CutQuota,
            quote,
            cut_qty,
            sold,
            failed,
            failed_share,
            meets,
            strong,
            cost,
        ));
        if failed > 0.0 {
            candidates.push(walk_candidate(
                WalkStep::CutQuote,
                cut_q,
                current_qty,
                sold,
                failed,
                failed_share,
                meets,
                strong,
                cost,
            ));
        }
    }

    let best = candidates
        .iter()
        .map(|c| c.score)
        .fold(f64::NEG_INFINITY, f64::max);
    let scale = best.abs().max(1.0);
    let mut tied: Vec<&WalkCandidate> = candidates
        .iter()
        .filter(|c| (best - c.score).abs() <= WALK_SCORE_TIE * scale)
        .collect();
    debug_assert!(!tied.is_empty(), "walk must keep at least stay");
    tied.sort_by(|a, b| {
        a.clearance
            .partial_cmp(&b.clearance)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let pick = tied[0];
    WalkChoice {
        step: pick.step,
        quote: pick.quote,
    }
}

struct WalkCandidate {
    step: WalkStep,
    quote: f64,
    score: f64,
    /// |expected_sold / plan - 1|, 0 is a full clear.
    clearance: f64,
}

fn walk_candidate(
    step: WalkStep,
    quote: f64,
    qty: f64,
    sold: f64,
    failed: f64,
    failed_share: f64,
    meets: f64,
    strong: bool,
    cost: f64,
) -> WalkCandidate {
    let expected = walk_expected_sold(step, sold, qty, failed, failed_share, meets, strong);
    let qty = qty.max(0.0);
    let quote = quote.max(0.0);
    debug_assert!(expected.is_finite() && expected >= 0.0);
    let score = expected * quote - qty * cost.max(0.0);
    let clearance = if qty > 0.0 {
        (expected / qty - 1.0).abs()
    } else if expected > 0.0 {
        1.0
    } else {
        0.0
    };
    WalkCandidate {
        step,
        quote,
        score,
        clearance,
    }
}

/// Predicted market units sold after `step`. Remainder placement is ignored.
fn walk_expected_sold(
    step: WalkStep,
    sold: f64,
    new_qty: f64,
    failed: f64,
    failed_share: f64,
    meets: f64,
    strong: bool,
) -> f64 {
    let sold = sold.max(0.0);
    let new_qty = new_qty.max(0.0);
    let extra_ok = strong && failed_share <= 0.0 && (meets > 0.0 || sold > 0.0);
    let expected = match step {
        WalkStep::Stay | WalkStep::RaiseQuote => sold,
        WalkStep::CutQuote => {
            if meets > 0.0 && failed > 0.0 {
                sold + failed
            } else {
                sold
            }
        }
        WalkStep::RaiseQuota => {
            if extra_ok {
                new_qty
            } else {
                sold
            }
        }
        WalkStep::CutQuota => sold,
    };
    expected.min(new_qty).max(0.0)
}

/// Output units implied by maker-line quotas, optionally after a qty step.
fn implied_output(
    good: &GoodFacts,
    lines: &[LineFacts],
    step: Option<WalkStep>,
    cfg: &FirmConfig,
) -> f64 {
    let mut total = 0.0;
    for &i in &good.maker_lines {
        let line = &lines[i];
        let Some(current) = line.target else {
            continue;
        };
        let next = match step {
            Some(WalkStep::RaiseQuota) => step_quota(current, 1.0 + cfg.growth_rate),
            Some(WalkStep::CutQuota) => step_quota(current, 1.0 - cfg.shrink_rate),
            _ => current.max(0.0),
        };
        let amt = line
            .outputs
            .iter()
            .find(|(g, _)| *g == good.good)
            .map(|(_, a)| *a)
            .unwrap_or(0.0);
        total += next.max(0.0) * amt.max(0.0);
    }
    total
}

/// Unit recipe cost: row `average_cost`, else last AMV-in per output unit.
fn output_unit_cost(good: &GoodFacts, lines: &[LineFacts]) -> f64 {
    if good.average_cost > 0.0 {
        return good.average_cost;
    }
    let mut amv_in = 0.0;
    let mut out = 0.0;
    for &i in &good.maker_lines {
        let line = &lines[i];
        if line.last_iterations <= 0.0 || line.last_amv_consumed <= 0.0 {
            continue;
        }
        let amt = line
            .outputs
            .iter()
            .find(|(g, _)| *g == good.good)
            .map(|(_, a)| *a)
            .unwrap_or(0.0);
        if amt <= 0.0 {
            continue;
        }
        amv_in += line.last_amv_consumed;
        out += line.last_iterations * amt;
    }
    if out > 0.0 {
        amv_in / out
    } else {
        0.0
    }
}

/// Discrete quota step. A line at 0 that is growing snaps to 1 iteration.
/// A 10/20% move smaller than 1 iteration becomes a 1-iteration step.
fn step_quota(current: f64, factor: f64) -> f64 {
    if current <= 0.0 {
        return if factor > 1.0 { 1.0 } else { 0.0 };
    }
    let next = (current * factor).max(0.0);
    let delta = next - current;
    if delta.abs() > 0.0 && delta.abs() < 1.0 {
        if delta > 0.0 {
            current + 1.0
        } else {
            (current - 1.0).max(0.0)
        }
    } else {
        next
    }
}

fn line_run_miss(facts: &LineFacts) -> bool {
    let Some(target) = facts.target else {
        return false;
    };
    if target <= 0.0 {
        return false;
    }
    facts.missing_inputs
        || facts.last_iterations + 1e-9 < target * (1.0 - firm_constants::TURNOVER_BAND)
            && facts.last_iterations + 1e-9 < target
}

fn step_toward_actual(current: f64, actual: f64, rate: f64) -> f64 {
    let next = current + (actual.max(0.0) - current) * rate.clamp(0.0, 1.0);
    if current > 0.0 && next < 1.0 {
        1.0
    } else {
        next.max(0.0)
    }
}

fn line_sell_measured(facts: &LineFacts, goods: &HashMap<usize, GoodFacts>) -> bool {
    facts.outputs.iter().any(|(good, _)| {
        goods.get(good).is_some_and(|g| {
            g.sell_target > 0.0 || g.produced > 0.0 || g.sold > 0.0
        })
    })
}

fn line_sell_miss(facts: &LineFacts, goods: &HashMap<usize, GoodFacts>, cfg: &FirmConfig) -> bool {
    line_sell_measured(facts, goods)
        && facts.outputs.iter().any(|(good, _)| {
            goods.get(good).is_some_and(|g| {
                let plan = if g.sell_target > 0.0 {
                    g.sell_target
                } else {
                    g.produced
                };
                plan > 0.0 && walk_sold(g) / plan < cfg.sell_success_shrink
            })
        })
}

/// Throughput the aim lerps toward. A miss uses sold iterations when known;
/// otherwise last completed iterations (quiet / strong keep operating scale).
fn line_aim_evidence(facts: &LineFacts, goods: &HashMap<usize, GoodFacts>, miss: bool) -> f64 {
    let actual = facts.last_iterations.max(0.0);
    if !miss {
        return actual;
    }
    let mut sold_iters: Option<f64> = None;
    for &(good, amount) in &facts.outputs {
        if amount <= 0.0 {
            continue;
        }
        let Some(g) = goods.get(&good) else {
            continue;
        };
        let iters = (walk_sold(g) + g.placed.max(0.0)) / amount;
        sold_iters = Some(match sold_iters {
            None => iters,
            Some(prev) => prev.min(iters),
        });
    }
    match sold_iters {
        Some(sold) => actual.min(sold),
        None => actual,
    }
}

/// Walks `current` toward `target` by `pace`. A line at 0 that is starting
/// snaps to at least 1 iteration so the day's output is a whole recipe.
fn next_line_target(current: f64, target: f64, pace: f64) -> f64 {
    let next = lerp(current, target, pace).max(0.0);
    if current <= 0.0 && next > 0.0 {
        next.max(1.0)
    } else {
        next
    }
}

fn plan_step(base: f64) -> f64 {
    base.clamp(0.0, 1.0)
}

/// Sets maker line targets so total output walks toward `sell`.
/// Increase: more-profitable lines get a larger raise. Decrease: less-profitable
/// lines get a larger cut.
#[allow(dead_code)]
fn align_lines_to_sell(
    desired: &mut [Option<f64>],
    lines: &[LineFacts],
    makers: &[usize],
    sell: f64,
    increase: bool,
    cfg: &FirmConfig,
) {
    let mut total = 0.0;
    let mut weights = Vec::new();
    for &i in makers {
        let line = &lines[i];
        let Some(t) = desired[i].or(line.target) else {
            weights.push(0.0);
            continue;
        };
        let out: f64 = line.outputs.iter().map(|(_, a)| t * *a).sum();
        total += out;
        let p = line.profitability.max(0.0);
        let w = if increase { p } else { 1.0 / (p + 0.1) };
        weights.push(w);
    }
    if total <= 0.0 && sell <= 0.0 {
        return;
    }
    let gap = sell - total;
    let sum_w: f64 = weights.iter().sum();
    if sum_w <= 0.0 {
        return;
    }
    let base = if increase {
        cfg.growth_rate
    } else {
        cfg.shrink_rate
    };
    let step = cfg.planning_lerp_rate.max(plan_step(base));
    let move_out = gap * step;
    for (k, &i) in makers.iter().enumerate() {
        let line = &lines[i];
        let Some(t) = desired[i].or(line.target) else {
            continue;
        };
        let per = line.outputs.iter().map(|(_, a)| *a).sum::<f64>().max(1e-9);
        let add_units = move_out * (weights[k] / sum_w);
        desired[i] = Some((t + add_units / per).max(0.0));
    }
}

/// Shifts output toward more profitable (and more successful) lines without
/// changing total output much.
fn equalize_line_peers(desired: &mut [Option<f64>], lines: &[LineFacts], makers: &[usize]) {
    if makers.len() < 2 {
        return;
    }
    let mean_p: f64 = makers.iter().map(|&i| lines[i].profitability).sum::<f64>()
        / makers.len() as f64;
    let mean_s: f64 = makers.iter().map(|&i| lines[i].last_success_rate).sum::<f64>()
        / makers.len() as f64;
    if mean_p <= 0.0 {
        return;
    }
    let band = firm_constants::PROFIT_PEER_BAND;
    let mut total = 0.0;
    let mut next = Vec::new();
    for &i in makers {
        let line = &lines[i];
        let t = desired[i].or(line.target).unwrap_or(0.0);
        let mut scale = 1.0;
        if (line.profitability - mean_p).abs() > band * mean_p {
            scale += ((line.profitability - mean_p) / mean_p).clamp(-0.5, 0.5);
        }
        if mean_s > 0.0 && (line.last_success_rate - mean_s).abs() > band {
            scale += (line.last_success_rate - mean_s).clamp(-0.5, 0.5);
        }
        let out = t * scale;
        total += out;
        next.push((i, out, t));
    }
    let old_total: f64 = makers
        .iter()
        .map(|&i| desired[i].or(lines[i].target).unwrap_or(0.0))
        .sum();
    if total <= 0.0 || old_total <= 0.0 {
        return;
    }
    let restore = old_total / total;
    for (i, out, t) in next {
        if t <= 0.0 {
            desired[i] = Some(0.0);
            continue;
        }
        desired[i] = Some((out * restore).max(0.0));
    }
}

/// Returns relative stdev of consecutive AMV changes, or `None` if too short.
fn trail_volatility(trail: &[f64]) -> Option<f64> {
    if trail.len() < 3 {
        return None;
    }
    let mut rets = Vec::new();
    for w in trail.windows(2) {
        if w[0].abs() < 1e-12 {
            continue;
        }
        rets.push((w[1] - w[0]) / w[0].abs());
    }
    if rets.len() < 2 {
        return None;
    }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|r| (r - mean) * (r - mean)).sum::<f64>() / rets.len() as f64;
    Some(var.sqrt())
}

/// Returns (last - first) / first from an AMV trail, or `None` if unusable.
fn trail_trend(trail: &[f64]) -> Option<f64> {
    let first = trail.first().copied().filter(|v| v.abs() > 1e-12)?;
    let last = trail.last().copied()?;
    if trail.len() < 2 {
        return None;
    }
    Some((last - first) / first.abs())
}

/// Returns stock after shopping, before this afternoon's production:
/// `quantity - produced`, floored at 0. Selling runs before production, so
/// evening `quantity` is high even after a good sales day.
fn post_shop_stock(row: &FirmPRow) -> f64 {
    (row.quantity - row.produced).max(0.0)
}

/// Returns `sold / sell_target`, or `sold / produced` when `sell_target` is 0.
/// Returns 1.0 when there is no plan and nothing was produced.
fn sell_success_of(sold: f64, sell_target: f64, produced: f64) -> f64 {
    if sell_target > 0.0 {
        (sold / sell_target).max(0.0)
    } else if produced > 0.0 {
        (sold / produced).max(0.0)
    } else {
        1.0
    }
}

/// Returns sold unit AMV / average cost. Returns 0.0 when the row produced
/// or planned to sell and sold nothing. Returns 1.0 when there is no signal
/// (no row, or sold with no price and no cost recorded).
fn realized_profit_of(row: Option<&FirmPRow>) -> f64 {
    let Some(row) = row else {
        return 1.0;
    };
    let credited = row.placed_credited();
    let qty = row.sold.max(0.0) + credited;
    if qty > 0.0 {
        let unit_price = (row.sold_amv.max(0.0) + row.placed_credited_amv()) / qty;
        if row.average_cost.abs() > 1e-12 {
            if unit_price == 0.0 {
                return 1.0;
            }
            return (unit_price / row.average_cost).max(0.0);
        }
        if unit_price > 0.0 {
            return 2.0;
        }
        return 1.0;
    }
    if row.produced > 0.0 || row.sell_target > 0.0 {
        0.0
    } else {
        1.0
    }
}

/// Returns AMV-out / AMV-in for the last run. No-input lines return 2.0.
/// Unknown (no AMV recorded) returns 1.0.
fn line_profit_ratio(line: &ProductionLine) -> f64 {
    if line.last_amv_consumed > 0.0 {
        line.last_amv_produced / line.last_amv_consumed
    } else if line.last_amv_produced > 0.0 {
        2.0
    } else {
        1.0
    }
}

/// Clamps a quote to `market * (1 ± band)`. Recipe cost floors do not apply.
fn clamp_quote_orbit(quote: f64, market: f64, band: f64) -> f64 {
    if !quote.is_finite() {
        return 0.0;
    }
    if !market.is_finite() || market <= 0.0 {
        return quote.max(0.0);
    }
    let band = band.clamp(0.0, 1.0);
    quote.clamp(market * (1.0 - band), market * (1.0 + band))
}

fn planned_iterations(line: &ProductionLine) -> f64 {
    match line.target {
        Some(target) => target.max(0.0),
        None => line.last_iterations.max(0.0),
    }
}

/// Expected recurring iterations for the operations fence: `aim` when set,
/// otherwise [`planned_iterations`].
fn line_operation_iters(line: &ProductionLine) -> f64 {
    if line.aim > 0.0 {
        line.aim.max(0.0)
    } else {
        planned_iterations(line)
    }
}

/// Returns true if `input` counts toward this line's use: required non-factor
/// inputs, plus optional inputs listed in `line.inputs`. Returns false for
/// factors and for optionals not selected.
fn line_counts_input(line: &ProductionLine, input: &ProcessInput) -> bool {
    if matches!(input.input_type, InputType::Factor) {
        return false;
    }
    if input.is_optional() {
        return line.inputs.contains(&input.good);
    }
    true
}

/// Returns true if `input` is Destroyed or Consumed (included in recipe AMV cost).
fn is_cost_input(input: &ProcessInput) -> bool {
    matches!(
        input.input_type,
        InputType::Destroyed | InputType::Consumed
    )
}

/// Days of expected output already on hand: min over made goods of
/// `quantity / daily make`. 0 when the firm makes nothing.
fn operation_output_days(
    property: &HashMap<usize, FirmPRow>,
    make_qty: &HashMap<usize, f64>,
) -> f64 {
    let mut days: Option<f64> = None;
    for (&good, &made) in make_qty {
        if made <= 0.0 {
            continue;
        }
        let qty = property
            .get(&good)
            .map(|row| row.quantity.max(0.0))
            .unwrap_or(0.0);
        let d = qty / made;
        days = Some(match days {
            None => d,
            Some(prev) => prev.min(d),
        });
    }
    days.unwrap_or(0.0)
}

/// Returns `(use_qty, make_qty)`: per-good input units and output units for
/// `lines` at [`planned_iterations`].
fn recipe_flows(
    lines: &[ProductionLine],
    factuals: &Factuals,
) -> (HashMap<usize, f64>, HashMap<usize, f64>) {
    recipe_flows_at(lines, factuals, planned_iterations)
}

fn recipe_flows_at(
    lines: &[ProductionLine],
    factuals: &Factuals,
    iters_of: fn(&ProductionLine) -> f64,
) -> (HashMap<usize, f64>, HashMap<usize, f64>) {
    let mut use_qty: HashMap<usize, f64> = HashMap::new();
    let mut make_qty: HashMap<usize, f64> = HashMap::new();
    for line in lines {
        let process = factuals
            .processes
            .get(&line.process)
            .expect("Process not found!");
        let iters = iters_of(line);
        if iters <= 0.0 {
            continue;
        }
        for input in &process.inputs {
            if !line_counts_input(line, input) {
                continue;
            }
            *use_qty.entry(input.good).or_insert(0.0) += input.amount * iters;
        }
        for output in &process.outputs {
            *make_qty.entry(output.good).or_insert(0.0) += output.amount * iters;
        }
    }
    (use_qty, make_qty)
}

/// Returns a [`FirmAmvBound`] per good used or made by `lines`.
/// Output goods get [`FirmAmvBound::Minimum`] (consumed-input AMV allocated by
/// output AMV share). Input goods get [`FirmAmvBound::Maximum`] (residual WTP:
/// output AMV minus other cost inputs). Goods that are both get [`FirmAmvBound::MinMax`].
fn recipe_bounds(
    lines: &[ProductionLine],
    factuals: &Factuals,
    history: &MarketHistory,
) -> HashMap<usize, FirmAmvBound> {
    let mut floors: HashMap<usize, f64> = HashMap::new();
    let mut caps: HashMap<usize, f64> = HashMap::new();
    for line in lines {
        // Idle backup lines (target 0) do not write floors or caps.
        if planned_iterations(line) <= 0.0 {
            continue;
        }
        let process = factuals
            .processes
            .get(&line.process)
            .expect("Process not found!");
        let mut input_amv = 0.0;
        for input in &process.inputs {
            if !line_counts_input(line, input) || !is_cost_input(input) {
                continue;
            }
            input_amv += input.amount * history.price(input.good);
        }
        let mut output_amv = 0.0;
        for output in &process.outputs {
            output_amv += output.amount * history.price(output.good);
        }
        if input_amv > 0.0 {
            for output in &process.outputs {
                if output.amount <= 0.0 {
                    continue;
                }
                let cost_per = if output_amv > 0.0 {
                    input_amv * history.price(output.good) / output_amv
                } else {
                    input_amv / output.amount
                };
                floors
                    .entry(output.good)
                    .and_modify(|floor| *floor = floor.max(cost_per))
                    .or_insert(cost_per);
            }
        }
        for input in &process.inputs {
            if !line_counts_input(line, input) || !is_cost_input(input) {
                continue;
            }
            if input.amount <= 0.0 {
                continue;
            }
            let others = input_amv - input.amount * history.price(input.good);
            let wtp = ((output_amv - others) / input.amount).max(0.0);
            caps.entry(input.good)
                .and_modify(|cap| *cap = cap.max(wtp))
                .or_insert(wtp);
        }
    }
    let mut bounds = HashMap::new();
    let mut goods: HashSet<usize> = floors.keys().copied().collect();
    goods.extend(caps.keys().copied());
    for good in goods {
        bounds.insert(
            good,
            FirmAmvBound::from_parts(floors.get(&good).copied(), caps.get(&good).copied()),
        );
    }
    bounds
}
