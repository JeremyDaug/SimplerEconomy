use std::collections::HashMap;
use std::path::PathBuf;

use hexx::Hex;
use simpler_economy::game::config::PopConfig;
use simpler_economy::game::desire::{Desire, DesireSource, DesireTarget, DesireTargetType};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::firm::{Firm, FirmAmvBound, FirmPRow, ProductionLine};
use simpler_economy::game::good::TIME;
use simpler_economy::game::household::Household;
use simpler_economy::game::market::MarketHistory;
use simpler_economy::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
use simpler_economy::game::sentiment::Sentiment;
use simpler_economy::game::workforce::{PaymentTerm, Workforce};

use super::*;

/// Opening AMV for every world good. No price spread at start.
pub(crate) const OPENING_AMV: f64 = 10.0;
/// Opening salability for every world good. Below the exchange floor, so
/// nothing starts as money.
pub(crate) const OPENING_SALABILITY: f64 = 0.3;

/// Morning grant of every non-Time good.
pub(crate) const DAILY_ENDOWMENT: f64 = 0.0;
/// Morning output of this pop's specialty good (`pop.id % n_goods`).
pub(crate) const DAILY_OUTPUT: f64 = 150.0;

/// Bulk scale for unused firm helpers. Households, line targets, hours, and
/// starting stocks are multiplied. AMV, salability, and per-household desire
/// amounts are not.
#[allow(dead_code)]
pub(crate) const ROSTER_SCALE: f64 = 100.0;

#[allow(dead_code)]
fn qty(n: f64) -> f64 {
    n * ROSTER_SCALE
}

pub(crate) fn world_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world")
}

/// Builds the living roster: one pop per world good, each one default
/// household (5 members).
pub(crate) fn build_world() -> (Vec<Pop>, Vec<Firm>, Factuals, MarketHistory) {
    let factuals = Factuals::load_from_path(world_data_path())
        .unwrap_or_else(|err| panic!("load {}: {err}", world_data_path().display()));

    let mut history = MarketHistory::default();
    // Flat start: no money good and no price spread.
    history.default_salability = OPENING_SALABILITY;
    for &id in factuals.goods.keys() {
        set_quote(&mut history, id, OPENING_AMV, OPENING_SALABILITY);
    }

    let pop_cfg = &factuals.config.pop;
    let n_pops = factuals.goods.len();
    let mut pops: Vec<Pop> = (1..=n_pops)
        .map(|id| make_basic_pop(id, pop_cfg))
        .collect();
    for pop in &mut pops {
        pop.record_keeping(&factuals, &history);
    }
    (pops, Vec::new(), factuals, history)
}

pub(crate) fn set_quote(history: &mut MarketHistory, good: usize, amv: f64, salability: f64) {
    history.prices.insert(good, amv);
    history.salability.insert(good, salability);
}

/// Specialty good this pop produces each morning. Pop 28 wraps onto Time (0).
pub(crate) fn produced_good_id(pop_id: usize, n_goods: usize) -> usize {
    debug_assert!(n_goods > 0, "world catalog must not be empty");
    pop_id % n_goods
}

fn add_qty(pop: &mut Pop, good: usize, qty: f64) {
    pop.property
        .entry(good)
        .or_insert_with(|| PopPRow::new(0.0))
        .quantity += qty;
}

/// Adds 1 of every non-Time good and enough extra of the specialty good to
/// reach 30 units of output. Time output is the full 30; labor Time still
/// comes from `Pop::start_day`.
pub(crate) fn grant_daily_endowment(pop: &mut Pop, good_ids: &[usize]) {
    let n_goods = good_ids.len();
    let specialty = produced_good_id(pop.id, n_goods);
    for &id in good_ids {
        if id == TIME {
            continue;
        }
        add_qty(pop, id, DAILY_ENDOWMENT);
    }
    let extra = if specialty == TIME {
        DAILY_OUTPUT
    } else {
        DAILY_OUTPUT - DAILY_ENDOWMENT
    };
    add_qty(pop, specialty, extra);
}

#[allow(dead_code)]
pub(crate) fn consume_target(good: usize) -> DesireTarget {
    DesireTarget::new(good, DesireTargetType::Consume, 1.0)
}

pub(crate) fn consume_target_eff(good: usize, eff: f64) -> DesireTarget {
    DesireTarget::new(good, DesireTargetType::Consume, eff)
}

/// Builds a consume-only desire. `targets` are (good, efficiency).
pub(crate) fn make_consume_desire(
    id: usize,
    name: &str,
    amount: f64,
    targets: &[(usize, f64)],
) -> Desire {
    Desire {
        source: DesireSource::Species(0, id),
        priority: id as isize,
        target: targets
            .iter()
            .map(|&(good, eff)| consume_target_eff(good, eff))
            .collect(),
        amount,
        satisfaction: 0.0,
        category: Some(name.into()),
        effect: vec![],
        scalar: ScalingFactor::All(1.0),
        decay: 0.0,
    }
}

pub(crate) fn empty_pop(id: usize, pop_cfg: &PopConfig) -> Pop {
    Pop {
        id,
        job: 0,
        property: HashMap::new(),
        desires: vec![vec![]; 3],
        working_desires: vec![],
        demographics: DemoRow {
            household: Household::new(),
            species: 0,
            culture: 0,
            class: 0,
            religion: 0,
        },
        current_orders: vec![],
        stored_effects: vec![],
        sentiment: Sentiment::new(),
        records: PopRecords::from_config(pop_cfg),
    }
}

/// Builds one living-roster pop: one household, grouped consume desires at
/// 1 unit per member (5 units with the default 5-person household).
pub(crate) fn make_basic_pop(id: usize, pop_cfg: &PopConfig) -> Pop {
    let mut pop = empty_pop(id, pop_cfg);
    let amount = pop.get_scaling_factor(ScalingFactor::All(1.0));
    pop.desires[0].push(make_consume_desire(
        0,
        "food",
        amount,
        &[(GRAIN, 1.0), (BREAD, 1.5)],
    ));
    pop.desires[0].push(make_consume_desire(1, "hydration", amount, &[(WATER, 1.0)]));
    pop.desires[0].push(make_consume_desire(
        2,
        "heating",
        amount,
        &[(WOOD, 1.0), (CHARCOAL, 1.25), (COAL, 1.5)],
    ));
    pop.desires[0].push(make_consume_desire(3, "housing", amount, &[(CABINS, 1.0)]));
    pop.desires[1].push(make_consume_desire(
        4,
        "utility items",
        amount,
        &[
            (WOOD_TOOLS, 0.2),
            (BUCKETS, 0.5),
            (IRON_TOOLS, 0.9),
            (BRONZE_TOOLS, 0.5),
            (POTS, 0.8),
            (BLADES, 1.0),
        ],
    ));
    pop.desires[1].push(make_consume_desire(
        5,
        "improved food",
        amount,
        &[(BREAD, 1.0), (BEER, 1.5)],
    ));
    pop.desires[1].push(make_consume_desire(
        6,
        "materials",
        amount,
        &[
            (WOOD, 0.5),
            (IRON, 0.5),
            (COPPER, 0.5),
            (TIN, 0.5),
            (BRONZE, 0.5),
            (GOLD, 0.5),
            (CLAY, 0.5),
        ],
    ));
    pop.desires[1].push(make_consume_desire(
        7,
        "health",
        amount,
        &[(BRONZE_MIRROR, 2.5), (TIME, 1.0)],
    ));
    pop.desires[2].push(make_consume_desire(
        8,
        "shiny tokens",
        amount,
        &[
            (GOLD_TOKEN, 1.0),
            (BRONZE_TOKEN, 1.0),
            (IRON_TOKEN, 1.0),
            (COPPER_TOKEN, 1.0),
            (TIN_TOKEN, 1.0),
            (JEWELRY, 1.0),
        ],
    ));
    pop.desires[2].push(make_consume_desire(
        9,
        "libations",
        amount,
        &[(BEER, 1.5), (TIME, 1.0)],
    ));
    for &good in &[
        GRAIN,
        WATER,
        BREAD,
        GOLD,
        GOLD_TOKEN,
        JEWELRY,
        WOOD,
        CABINS,
        WOOD_TOOLS,
        BUCKETS,
        IRON,
        IRON_TOOLS,
        COPPER,
        TIN,
        BRONZE,
        BRONZE_TOOLS,
        BLADES,
        BRONZE_MIRROR,
        BRONZE_TOKEN,
        IRON_TOKEN,
        COPPER_TOKEN,
        TIN_TOKEN,
        COAL,
        CHARCOAL,
        BEER,
        CLAY,
        POTS,
    ] {
        pop.property
            .insert(good, PopPRow::new(1.0).with_target(2.0));
    }
    pop
}

/// Roster row: wage contract, 1 coin per Time unit, no worker profit share.
/// Lord is the owner-operator: remainder after wages, with sell piles and a
/// wage-float retained as growth (plan does not write growth_target yet).
#[allow(dead_code)]
pub(crate) fn with_worker(mut firm: Firm, pop_id: usize, hours: f64) -> Firm {
    firm = firm.with_owner_remainder();
    for row in firm.property.values_mut() {
        if row.sell_target > 0.0 {
            row.growth_target = row.growth_target.max(row.sell_target);
        }
    }
    let coin = firm.property.entry(GOLD_TOKEN).or_insert_with(FirmPRow::new);
    coin.growth_target = coin.growth_target.max(hours);
    firm.with_workforce(
        Workforce::new(pop_id)
            .with_workers(qty(10.0), qty(10.0))
            .with_hours(hours)
            .with_payment(PaymentTerm::new(GOLD_TOKEN, 1.0)),
    )
}

#[allow(dead_code)]
pub(crate) fn dummy_line(process: usize, target: f64, inputs: Vec<usize>) -> ProductionLine {
    ProductionLine {
        process,
        target: Some(target),
        inputs,
        historical_productivity: 0.0,
        last_success_rate: 0.0,
        last_iterations: 0.0,
        last_effects: vec![],
        last_missing_goods: vec![],
        last_amv_consumed: 0.0,
        last_amv_produced: 0.0,
    }
}

#[allow(dead_code)]
pub(crate) fn make_farm() -> Firm {
    let mut firm = Firm::new(1, "farm".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(1, qty(5.0), vec![TIME, WATER]));
    // Stock matches plan's input_cover * use (2 * 5). Start a little short so
    // a water buy posts on day 1. Bid 0.45 clears the well's 0.20 ask.
    firm.property.insert(
        WATER,
        FirmPRow::new()
            .with_quantity(qty(8.0))
            .with_purchase_target(qty(8.0))
            .with_use_target(qty(5.0))
            .with_stock_target(qty(10.0))
            .with_amv_target(0.60)
            .with_amv_bound(FirmAmvBound::Maximum(2.5)),
    );
    firm.property.insert(
        GRAIN,
        FirmPRow::new()
            .with_quantity(qty(45.0))
            .with_sell_target(qty(30.0))
            .with_amv_target(1.2)
            .with_amv_bound(FirmAmvBound::Minimum(1.0)),
    );
    firm.property.insert(GOLD_TOKEN, FirmPRow::new().with_quantity(qty(120.0)));
    firm
}

#[allow(dead_code)]
pub(crate) fn make_bakery() -> Firm {
    let mut firm = Firm::new(2, "bakery".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(2, qty(14.0), vec![TIME, GRAIN]));
    // Two days of grain on the stock fence so day 1 need not buy to bake.
    firm.property.insert(
        GRAIN,
        FirmPRow::new()
            .with_quantity(qty(28.0))
            .with_purchase_target(qty(16.0))
            .with_use_target(qty(14.0))
            .with_stock_target(qty(28.0))
            .with_amv_target(2.0)
            .with_amv_bound(FirmAmvBound::Maximum(1.5)),
    );
    firm.property.insert(
        BREAD,
        FirmPRow::new()
            .with_quantity(qty(24.0))
            .with_sell_target(qty(16.0))
            .with_amv_target(2.5)
            .with_amv_bound(FirmAmvBound::Minimum(1.8)),
    );
    // Coin growth is a wage float remainder cannot take (wages may still raid).
    firm.property.insert(
        GOLD_TOKEN,
        FirmPRow::new()
            .with_quantity(qty(400.0))
            .with_growth_target(qty(90.0)),
    );
    firm
}

#[allow(dead_code)]
pub(crate) fn make_mine() -> Firm {
    let mut firm = Firm::new(3, "mine".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(3, qty(8.0), vec![TIME]));
    firm.property.insert(
        GOLD,
        FirmPRow::new()
            .with_quantity(qty(24.0))
            .with_sell_target(qty(16.0))
            .with_amv_target(4.0)
            .with_amv_bound(FirmAmvBound::Minimum(3.0)),
    );
    firm.property.insert(
        GOLD_TOKEN,
        FirmPRow::new()
            .with_quantity(qty(120.0))
            .with_growth_target(qty(96.0)),
    );
    firm
}

#[allow(dead_code)]
pub(crate) fn make_jeweler() -> Firm {
    let mut firm = Firm::new(5, "jeweler".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(5, qty(1.0), vec![TIME, GOLD]));
    firm.production_line.push(dummy_line(4, qty(1.0), vec![TIME, GOLD]));
    // Reverse mint stays idle so it does not eat the till at opening prices.
    firm.production_line.push(dummy_line(7, 0.0, vec![TIME, GOLD_TOKEN]));
    // 16 gold covers jewelry (3) plus mint (1) for several days without a buy.
    firm.property.insert(
        GOLD,
        FirmPRow::new()
            .with_quantity(qty(16.0))
            .with_purchase_target(qty(4.0))
            .with_use_target(qty(4.0))
            .with_stock_target(qty(16.0))
            .with_amv_bound(FirmAmvBound::Maximum(6.0)),
    );
    firm.property.insert(
        JEWELRY,
        FirmPRow::new()
            .with_quantity(qty(12.0))
            .with_sell_target(qty(8.0))
            .with_amv_target(60.0)
            .with_amv_bound(FirmAmvBound::Minimum(48.0)),
    );
    firm.property.insert(
        GOLD_TOKEN,
        FirmPRow::new()
            .with_quantity(qty(400.0))
            .with_sell_target(qty(150.0))
            .with_growth_target(qty(150.0))
            .with_amv_target(COIN_AMV)
            .with_amv_bound(FirmAmvBound::Minimum(0.168)),
    );
    firm
}

#[allow(dead_code)]
pub(crate) fn make_well() -> Firm {
    let mut firm = Firm::new(6, "well".into(), 1, Hex::new(0, 0));
    // 40/day covers pop water shop (~18) plus the farm restock (~12) with slack.
    // Ask at market (0.20); floor 0.15 so leftover-AMV cheapening can still sell.
    firm.production_line.push(dummy_line(6, qty(30.0), vec![TIME]));
    firm.property.insert(
        WATER,
        FirmPRow::new()
            .with_quantity(qty(60.0))
            .with_sell_target(qty(30.0))
            .with_amv_target(0.50)
            .with_amv_bound(FirmAmvBound::Minimum(0.40)),
    );
    firm.property.insert(GOLD_TOKEN, FirmPRow::new().with_quantity(qty(40.0)));
    firm
}

