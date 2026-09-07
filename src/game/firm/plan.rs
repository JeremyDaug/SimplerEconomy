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

    /// Writes firm-wide sold/bought AMV, realized profit, sell success, and
    /// lerps `confidence` toward today's evidence. Also lerps each selling
    /// row's `average_price` toward today's unit sale AMV.
    fn update_records(&mut self, cfg: &FirmConfig) {
        let mut sold_amv = 0.0;
        let mut bought_amv = 0.0;
        let mut sold_cost = 0.0;
        let mut sold_units = 0.0;
        let mut sell_plan = 0.0;
        for row in self.property.values_mut() {
            sold_amv += row.sold_amv;
            bought_amv += row.bought_amv;
            if row.sold > 0.0 {
                sold_cost += row.sold * row.average_cost.max(0.0);
                let today = row.sold_unit_amv();
                if row.average_price == 0.0 {
                    row.average_price = today;
                } else {
                    row.average_price =
                        lerp(row.average_price, today, cfg.rolling_avg_weight);
                }
            }
            if row.sell_target > 0.0 {
                sold_units += row.sold.max(0.0);
                sell_plan += row.sell_target;
            }
        }
        let missing = self
            .production_line
            .iter()
            .any(|line| !line.last_missing_goods.is_empty());

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

        let mut evidence: f64 = 0.5;
        if success >= cfg.sell_success_grow {
            evidence += 0.25;
        } else if success < cfg.sell_success_shrink {
            evidence -= 0.25;
        }
        if profit > firm_constants::PROFIT_HIGH {
            evidence += 0.25;
        } else if profit < firm_constants::PROFIT_LOW {
            evidence -= 0.25;
        }
        if missing {
            evidence -= 0.25;
        }
        self.records.confidence = lerp(
            self.records.confidence,
            evidence.clamp(0.0, 1.0),
            cfg.planning_lerp_rate,
        )
        .clamp(0.0, 1.0);
    }

    /// # Plan
    ///
    /// Gathers line and market facts, then writes production-line `target`s,
    /// output `sell_target` / `amv_target`, and input use/stock/purchase
    /// fields. Does not run production or emit orders.
    ///
    /// 1. [`Self::gather_plan_info`]: profitability, sell success, turnover,
    ///    stockpile, decay loss, market AMV, and optional share / volume /
    ///    volatility / trend. Competitor quotes are `None` until other firms
    ///    are passed in.
    /// 2. [`Self::apply_plan_adjustments`]: from a quiet baseline, nudge sell
    ///    plan, own quote, and line targets. Then align production to the sell
    ///    plan, weighted by profitability. A line at 0 that is starting snaps
    ///    to at least 1 iteration (a full recipe) so the day's output can be
    ///    sold under whole-unit exchange.
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
    /// their current target. Missing inputs do not shrink.
    fn apply_plan_adjustments(
        &mut self,
        info: &PlanGather,
        history: &MarketHistory,
        cfg: &FirmConfig,
    ) {
        let n = self.production_line.len();
        let confidence = self.records.confidence;
        let pace = plan_pace(confidence, cfg);
        let mut desired_sell: HashMap<usize, f64> = HashMap::new();
        let mut desired_amv: HashMap<usize, f64> = HashMap::new();
        let mut desired_line: Vec<Option<f64>> = vec![None; n];

        for line in &info.lines {
            desired_line[line.index] = line.target;
        }

        for good in info.goods.values() {
            if good.maker_lines.is_empty() {
                continue;
            }
            let all_cold = good.maker_lines.iter().all(|&i| {
                info.lines.get(i).map(|l| l.cold).unwrap_or(true)
            });
            if all_cold {
                let sell = if good.sell_target > 0.0 {
                    good.sell_target
                } else {
                    good.planned_output.max(good.sold)
                };
                desired_sell.insert(good.good, sell.max(0.0));
                desired_amv.insert(good.good, good.own_amv);
                continue;
            }
            let (sell, amv) = good_plan_nudge(good, cfg, confidence);
            desired_sell.insert(good.good, sell.max(0.0));
            desired_amv.insert(good.good, amv);

            let planned = good.planned_output.max(0.0);
            let gap = sell - planned;
            let measured = good.sell_target > 0.0 || good.produced > 0.0 || good.sold > 0.0;
            let strong = measured && good.sell_success >= cfg.sell_success_grow;
            equalize_line_peers(&mut desired_line, &info.lines, &good.maker_lines);
            let increase = gap > 0.0;
            if increase && !strong {
                continue;
            }
            if gap.abs() > planned * firm_constants::TURNOVER_BAND
                && gap.abs() > sell * firm_constants::TURNOVER_BAND
            {
                align_lines_to_sell(
                    &mut desired_line,
                    &info.lines,
                    &good.maker_lines,
                    sell,
                    increase,
                    cfg,
                    confidence,
                );
            }
        }

        for line in &info.lines {
            if line.target.is_none() {
                continue;
            }
            if line.cold {
                desired_line[line.index] = line.target;
                continue;
            }
            if line.missing_inputs {
                if let (Some(cur), Some(want)) = (line.target, desired_line[line.index]) {
                    desired_line[line.index] = Some(want.max(cur));
                }
            }
        }

        for (line, want) in self.production_line.iter_mut().zip(desired_line) {
            if let (Some(current), Some(target)) = (line.target, want) {
                line.target = Some(next_line_target(current, target, pace));
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

        for (good_id, sell) in desired_sell {
            let row = self.property.entry(good_id).or_insert_with(FirmPRow::new);
            if row.sell_target <= 0.0 {
                row.sell_target = sell;
            } else {
                row.sell_target = lerp(row.sell_target, sell, pace).max(0.0);
            }
            if let Some(&amv) = desired_amv.get(&good_id) {
                let market = history.price(good_id);
                let current = if row.amv_target != 0.0 {
                    row.amv_target
                } else {
                    market
                };
                let bound = row.amv_bound;
                let mut target = lerp(current, amv, pace);
                if let Some(cap) = bound.maximum() {
                    target = target.min(cap);
                }
                if let Some(floor) = bound.minimum() {
                    target = target.max(floor);
                }
                row.amv_target = target;
            }
        }
    }

    /// Sets `use_target`, `stock_target`, `purchase_target`, `sell_target`,
    /// `reserve_target`, `amv_bound`, `amv_target`, and `margin` on property
    /// rows from current production-line targets and today's bought / sold.
    fn rewrite_property_targets(
        &mut self,
        factuals: &Factuals,
        history: &MarketHistory,
        cfg: &FirmConfig,
    ) {
        let pace = plan_pace(self.records.confidence, cfg);
        let (use_qty, make_qty) = recipe_flows(&self.production_line, factuals);
        let bounds = recipe_bounds(&self.production_line, factuals, history);

        let mut goods: HashSet<usize> = self.property.keys().copied().collect();
        goods.extend(use_qty.keys().copied());
        goods.extend(make_qty.keys().copied());

        for good_id in goods {
            let used = use_qty.get(&good_id).copied().unwrap_or(0.0);
            let made = make_qty.get(&good_id).copied().unwrap_or(0.0);
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

            if used <= 0.0 && made <= 0.0 {
                continue;
            }

            row.use_target = used;
            if used > 0.0 {
                row.stock_target = used * cfg.input_cover;
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
            } else {
                row.stock_target = 0.0;
                row.purchase_target = 0.0;
            }

            if made <= 0.0 && used > 0.0 {
                row.sell_target = row.free_for_market();
            }

            row.amv_bound = bound;
            if made > 0.0 {
                let current = row.amv_target;
                if let Some(cap) = bound.maximum() {
                    row.amv_target = current.min(cap);
                }
                if let Some(floor) = bound.minimum() {
                    row.amv_target = row.amv_target.max(floor);
                }
            } else if used > 0.0 && old_purchase > 0.0 && bought < old_purchase {
                let market = history.price(good_id);
                let current = if row.amv_target != 0.0 {
                    row.amv_target
                } else {
                    market
                };
                row.amv_target = lerp(
                    current,
                    current * (1.0 + cfg.amv_nudge),
                    pace,
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
    produced: f64,
    planned_output: f64,
    #[allow(dead_code)]
    post_shop: f64,
    sell_success: f64,
    /// Sold unit AMV / average cost. 0 if we meant to sell and didn't.
    realized_profit: f64,
    #[allow(dead_code)]
    turnover: f64,
    stockpile: f64,
    decay_rate: f64,
    #[allow(dead_code)]
    decay_loss_amv: f64,
    own_amv: f64,
    market_amv: f64,
    market_share: Option<f64>,
    #[allow(dead_code)]
    market_volume: Option<f64>,
    market_volatility: Option<f64>,
    market_trend: Option<f64>,
    /// Mean rival quote. `None` until other firms are passed into plan.
    competitor_amv: Option<f64>,
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
        produced,
        planned_output: 0.0,
        post_shop,
        sell_success: sell_success_of(sold, sell_target, produced),
        realized_profit: realized_profit_of(row),
        turnover: if produced > 0.0 { sold / produced } else { 1.0 },
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
        maker_lines: Vec::new(),
    }
}

/// Returns `(desired_sell, desired_amv)` from baseline-relative nudges.
/// Pressures add, then clamp to one step, so several loud signals cannot
/// stack past `growth_rate` / `shrink_rate` (scaled by confidence).
fn good_plan_nudge(good: &GoodFacts, cfg: &FirmConfig, confidence: f64) -> (f64, f64) {
    let mut sell = if good.sell_target > 0.0 {
        good.sell_target
    } else {
        good.planned_output.max(good.sold)
    };
    let mut amv = good.own_amv;
    let profit = good.realized_profit;
    let prefer_price = price_cut_share(profit, good.decay_rate, good.own_amv, good.market_amv);

    // Firm strategy (later): scale prefer_price (aggressive up, defensive down).

    let vol_boost = good
        .market_volatility
        .filter(|v| *v > firm_constants::VOLATILITY_LOW)
        .map(|v| 1.0 + v)
        .unwrap_or(1.0);

    let mut volume = 0.0;
    let mut price = 0.0;

    if let Some(trend) = good.market_trend {
        if trend > firm_constants::TREND_DEADBAND {
            add_pressures(&mut volume, &mut price, 1.0, prefer_price);
        } else if trend < -firm_constants::TREND_DEADBAND {
            add_pressures(&mut volume, &mut price, -1.0, prefer_price);
        }
    }

    let measured = good.sell_target > 0.0 || good.produced > 0.0 || good.sold > 0.0;
    let strong = measured && good.sell_success >= cfg.sell_success_grow;
    let miss = measured && good.sell_success < cfg.sell_success_shrink;

    if profit < firm_constants::PROFIT_LOW {
        volume -= 1.0;
    } else if profit > firm_constants::PROFIT_HIGH && strong {
        volume += 1.0;
    }

    if strong {
        add_pressures(&mut volume, &mut price, 1.0, prefer_price);
    } else if miss {
        add_pressures(&mut volume, &mut price, -1.0, prefer_price);
    }

    let baseline_stock = good.planned_output.max(good.produced)
        * cfg.output_cover
        * (1.0 - good.decay_rate).max(0.0)
        * vol_boost;
    if baseline_stock > 0.0 {
        let ratio = good.stockpile / baseline_stock;
        if (ratio - 1.0).abs() > firm_constants::STOCKPILE_BAND {
            if ratio > 1.0 {
                volume += 1.0 - prefer_price.clamp(0.0, 1.0);
                price -= prefer_price.clamp(0.0, 1.0);
            } else {
                volume -= 1.0 - prefer_price.clamp(0.0, 1.0);
                price += prefer_price.clamp(0.0, 1.0);
            }
        }
    }

    if let Some(share) = good.market_share {
        if share < firm_constants::SHARE_LOW {
            add_pressures(&mut volume, &mut price, -1.0, prefer_price);
        } else if share > firm_constants::SHARE_HIGH {
            price += 1.0;
        }
    }

    if good.market_amv > 0.0 {
        let rel = good.own_amv / good.market_amv;
        if rel > 1.0 + firm_constants::PRICE_BAND {
            add_pressures(&mut volume, &mut price, -1.0, prefer_price);
        } else if rel < 1.0 - firm_constants::PRICE_BAND {
            add_pressures(&mut volume, &mut price, 1.0, prefer_price);
        }
    }

    // Competitor quotes (later): if rival mean is set and outside +/- 10%,
    // nudge toward competing or holding margin.
    if let Some(rival) = good.competitor_amv {
        if rival > 0.0 {
            let rel = good.own_amv / rival;
            if rel > 1.0 + firm_constants::PRICE_BAND {
                add_pressures(&mut volume, &mut price, -1.0, prefer_price);
            } else if rel < 1.0 - firm_constants::PRICE_BAND {
                add_pressures(&mut volume, &mut price, 1.0, prefer_price);
            }
        }
    }

    volume = volume.clamp(-1.0, 1.0);
    price = price.clamp(-1.0, 1.0);
    let vol_base = if volume < 0.0 {
        cfg.shrink_rate
    } else {
        cfg.growth_rate
    };
    sell = (sell * (1.0 + volume * plan_step(confidence, vol_base, cfg))).max(0.0);
    amv *= 1.0 + price * plan_step(confidence, cfg.growth_rate, cfg);
    (sell, amv)
}

/// Adds a signed volume/price split into the day's pressure totals.
fn add_pressures(volume: &mut f64, price: &mut f64, sign: f64, prefer_price: f64) {
    let p = prefer_price.clamp(0.0, 1.0);
    *volume += sign * (1.0 - p);
    *price += sign * p;
}

/// Returns the lerp/step scale for this confidence: slower when cautious,
/// faster when confident. Mid confidence keeps `planning_lerp_rate`.
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

pub(super) fn plan_pace(confidence: f64, cfg: &FirmConfig) -> f64 {
    let t = confidence.clamp(0.0, 1.0);
    let mul = lerp(cfg.confidence_pace_min, cfg.confidence_pace_max, t);
    (cfg.planning_lerp_rate * mul).clamp(0.0, 1.0)
}

/// Returns `base` scaled by the same confidence multiplier as [`plan_pace`].
fn plan_step(confidence: f64, base: f64, cfg: &FirmConfig) -> f64 {
    let t = confidence.clamp(0.0, 1.0);
    let mul = lerp(cfg.confidence_pace_min, cfg.confidence_pace_max, t);
    (base * mul).clamp(0.0, 1.0)
}

/// Sets maker line targets so total output walks toward `sell`.
/// Increase: more-profitable lines get a larger raise. Decrease: less-profitable
/// lines get a larger cut.
fn align_lines_to_sell(
    desired: &mut [Option<f64>],
    lines: &[LineFacts],
    makers: &[usize],
    sell: f64,
    increase: bool,
    cfg: &FirmConfig,
    confidence: f64,
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
    let step = plan_pace(confidence, cfg).max(plan_step(confidence, base, cfg));
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
    if row.sold > 0.0 {
        let unit_price = row.sold_unit_amv();
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

/// Returns how much of undersell pressure goes to cutting price (`0..=1`).
/// The rest goes to cutting volume.
///
/// High profit raises this (more willing to slip the quote). Profit at or
/// below 1.0 floors it. Own quote above market AMV raises it; at or below
/// market, failed sales look like a volume problem. Decay raises it (move
/// goods before they rot).
///
/// Market share (later): high share lowers this -- cut volume, protect the
/// quote, approaching monopoly.
/// Previous market AMV (later): a falling market raises this.
/// Competitor quotes (later): cheaper rivals raise this if profit allows.
/// Firm strategy (later): aggressive adds (keep-out pricing even with high
/// share); defensive subtracts.
fn price_cut_share(profit: f64, decay: f64, own_amv: f64, market_amv: f64) -> f64 {
    let mut share = if profit > 1.0 {
        ((profit - 1.0) / 1.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if own_amv > 0.0 && market_amv > 0.0 {
        if own_amv > market_amv {
            share = (share + 0.25).min(1.0);
        } else {
            share *= 0.5;
        }
    }
    (share + decay.clamp(0.0, 1.0) * 0.5).clamp(0.0, 1.0)
}

/// Returns `line.target` when it is `Some`, otherwise `line.last_iterations`.
/// Floors at 0.0.
fn planned_iterations(line: &ProductionLine) -> f64 {
    match line.target {
        Some(target) => target.max(0.0),
        None => line.last_iterations.max(0.0),
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

/// Returns `(use_qty, make_qty)`: per-good input units and output units for
/// `lines` at [`planned_iterations`].
fn recipe_flows(
    lines: &[ProductionLine],
    factuals: &Factuals,
) -> (HashMap<usize, f64>, HashMap<usize, f64>) {
    let mut use_qty: HashMap<usize, f64> = HashMap::new();
    let mut make_qty: HashMap<usize, f64> = HashMap::new();
    for line in lines {
        let process = factuals
            .processes
            .get(&line.process)
            .expect("Process not found!");
        let iters = planned_iterations(line);
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
