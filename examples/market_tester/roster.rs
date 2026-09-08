use std::collections::HashMap;
use std::path::PathBuf;

use hexx::Hex;
use simpler_economy::game::actor::Actor;
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

/// Bulk scale for the living roster. Households, line targets, hours, and
/// starting stocks are multiplied. AMV, salability, and per-household desire
/// amounts are not.
pub(crate) const ROSTER_SCALE: f64 = 100.0;

fn qty(n: f64) -> f64 {
    n * ROSTER_SCALE
}

pub(crate) fn world_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world")
}

pub(crate) fn build_world() -> (Vec<Pop>, Vec<Firm>, Factuals, MarketHistory) {
    let factuals = Factuals::load_from_path(world_data_path())
        .unwrap_or_else(|err| panic!("load {}: {err}", world_data_path().display()));

    let mut history = MarketHistory::default();
    history.default_salability = factuals.config.market.salability_default;
    // AMV spread: staples cheap, metals dear, jewelry dearest.
    // Coins are money (sal 1.0); jewelry is liquid-ish (0.8); rest stay below
    // the 0.6 exchange floor unless noted (gold 0.7 can be tender).
    set_quote(&mut history, TIME, 1.0, 0.40);
    set_quote(&mut history, GRAIN, 1.0, 0.50);
    set_quote(&mut history, WATER, 0.50, 0.35);
    set_quote(&mut history, BREAD, 2.5, 0.45);
    set_quote(&mut history, GOLD, 4.0, 0.70);
    set_quote(&mut history, COIN, COIN_AMV, 1.00);
    set_quote(&mut history, JEWELRY, 60.0, 0.80);

    let pops = vec![
        make_farmers_pop(&factuals.config.pop),
        make_laborers_pop(&factuals.config.pop),
        make_townsfolk_pop(&factuals.config.pop),
        make_lord_pop(&factuals.config.pop),
        make_jewelers_pop(&factuals.config.pop),
        make_wellhands_pop(&factuals.config.pop),
    ];
    let lord = Actor::Pop(4);
    // Hours are Time units: day-1 recipe Time, plus one meeting's transport
    // for firms that buy inputs (buyer pays `transaction_cost`).
    let haul = factuals.config.market.transaction_cost;
    let firms = vec![
        with_worker(make_farm().with_owner(lord), 1, qty(15.0) + haul),
        with_worker(make_bakery().with_owner(lord), 3, qty(28.0) + haul),
        with_worker(make_mine().with_owner(lord), 2, qty(32.0)),
        with_worker(make_jeweler().with_owner(lord), 5, qty(5.0) + haul),
        with_worker(make_well().with_owner(lord), 6, qty(30.0)),
    ];
    (pops, firms, factuals, history)
}

pub(crate) fn set_quote(history: &mut MarketHistory, good: usize, amv: f64, salability: f64) {
    history.prices.insert(good, amv);
    history.salability.insert(good, salability);
}

pub(crate) fn consume_target(good: usize) -> DesireTarget {
    DesireTarget::new(good, DesireTargetType::Consume, 1.0)
}

pub(crate) fn consume_target_eff(good: usize, eff: f64) -> DesireTarget {
    DesireTarget::new(good, DesireTargetType::Consume, eff)
}

pub(crate) fn make_desire(id: usize, good: usize, amount: f64) -> Desire {
    Desire {
        source: DesireSource::Species(0, id),
        priority: id as isize,
        target: vec![consume_target(good)],
        amount,
        satisfaction: 0.0,
        category: None,
        effect: vec![],
        scalar: ScalingFactor::Household(1.0),
        decay: 0.0,
    }
}

/// Basic food: grain at 1.0 or bread at 1.5 so bread is the cheaper sat.
pub(crate) fn make_food_desire(id: usize, amount: f64) -> Desire {
    Desire {
        source: DesireSource::Species(0, id),
        priority: id as isize,
        target: vec![
            consume_target_eff(GRAIN, 1.0),
            consume_target_eff(BREAD, 1.5),
        ],
        amount,
        satisfaction: 0.0,
        category: Some("food".into()),
        effect: vec![],
        scalar: ScalingFactor::Household(1.0),
        decay: 0.0,
    }
}

/// Staple spread: food (grain/bread) + water basic, bread common.
pub(crate) fn with_need_spread(mut pop: Pop) -> Pop {
    pop.desires[0].push(make_food_desire(0, 8.0));
    pop.desires[0].push(make_desire(1, WATER, 6.0));
    pop.desires[1].push(make_desire(2, BREAD, 4.0));
    pop.desires[1].push(make_desire(3, GOLD, 1.0));
    pop
}

pub(crate) fn empty_pop(id: usize, pop_cfg: &PopConfig) -> Pop {
    Pop {
        id,
        job: 0,
        property: HashMap::new(),
        desires: vec![vec![]; 3],
        working_desires: vec![],
        demographics: DemoRow {
            household: Household::with_count(qty(10.0)),
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

pub(crate) fn make_farmers_pop(pop_cfg: &PopConfig) -> Pop {
    let mut pop = with_need_spread(empty_pop(1, pop_cfg));
    // Grain surplus funds water/bread requests. No grain shop shortfall.
    pop.property.insert(GRAIN, PopPRow::new(qty(24.0)).with_target(qty(4.0)));
    pop.property.insert(WATER, PopPRow::new(qty(1.0)).with_target(qty(6.0)));
    pop.property.insert(BREAD, PopPRow::new(0.0).with_target(qty(5.0)));
    pop.property.insert(GOLD, PopPRow::new(0.0).with_target(qty(1.0)));
    pop.property.insert(COIN, PopPRow::new(qty(80.0)));
    pop
}

pub(crate) fn make_laborers_pop(pop_cfg: &PopConfig) -> Pop {
    let mut pop = with_need_spread(empty_pop(2, pop_cfg));
    pop.property.insert(GRAIN, PopPRow::new(qty(1.0)).with_target(qty(8.0)));
    pop.property.insert(WATER, PopPRow::new(0.0).with_target(qty(6.0)));
    pop.property.insert(BREAD, PopPRow::new(0.0).with_target(qty(4.0)));
    pop.property.insert(GOLD, PopPRow::new(0.0).with_target(qty(1.0)));
    pop.property.insert(COIN, PopPRow::new(qty(160.0)));
    pop
}

pub(crate) fn make_townsfolk_pop(pop_cfg: &PopConfig) -> Pop {
    let mut pop = with_need_spread(empty_pop(3, pop_cfg));
    pop.property.insert(GRAIN, PopPRow::new(qty(4.0)).with_target(qty(6.0)));
    pop.property.insert(WATER, PopPRow::new(qty(2.0)).with_target(qty(4.0)));
    pop.property.insert(BREAD, PopPRow::new(qty(1.0)).with_target(qty(6.0)));
    pop.property.insert(GOLD, PopPRow::new(0.0).with_target(qty(1.0)));
    pop.property.insert(COIN, PopPRow::new(qty(400.0)));
    pop
}

/// One-household owner. Staples stay small; jewelry is the luxury sink.
/// Starting AMV is about 20x townsfolk wealth per household (~4.7 -> ~93).
pub(crate) fn make_lord_pop(pop_cfg: &PopConfig) -> Pop {
    let mut pop = empty_pop(4, pop_cfg);
    pop.demographics.household = Household::with_count(qty(10.0));
    pop.desires[0].push(make_food_desire(0, 1.0));
    pop.desires[0].push(make_desire(1, WATER, 1.0));
    pop.desires[1].push(make_desire(2, BREAD, 1.0));
    pop.desires[1].push(make_desire(4, GOLD, 1.0));
    pop.desires[2].push(make_desire(3, JEWELRY, 2.0));
    pop.property.insert(GRAIN, PopPRow::new(qty(1.0)).with_target(qty(1.0)));
    pop.property.insert(WATER, PopPRow::new(qty(1.0)).with_target(qty(1.0)));
    pop.property.insert(BREAD, PopPRow::new(qty(1.0)).with_target(qty(1.0)));
    pop.property.insert(GOLD, PopPRow::new(0.0).with_target(qty(1.0)));
    pop.property.insert(JEWELRY, PopPRow::new(0.0).with_target(qty(2.0)));
    pop.property.insert(COIN, PopPRow::new(qty(900.0)));
    pop
}

/// One-household craft workers for the jeweler. Staples only.
pub(crate) fn make_jewelers_pop(pop_cfg: &PopConfig) -> Pop {
    small_worker_pop(5, pop_cfg, 80.0)
}

/// One-household well crew. Staples only.
pub(crate) fn make_wellhands_pop(pop_cfg: &PopConfig) -> Pop {
    small_worker_pop(6, pop_cfg, 40.0)
}

fn small_worker_pop(id: usize, pop_cfg: &PopConfig, coin: f64) -> Pop {
    let mut pop = empty_pop(id, pop_cfg);
    pop.demographics.household = Household::with_count(qty(1.0));
    pop.desires[0].push(make_food_desire(0, 1.0));
    pop.desires[0].push(make_desire(1, WATER, 1.0));
    pop.desires[1].push(make_desire(2, BREAD, 1.0));
    pop.desires[1].push(make_desire(3, GOLD, 1.0));
    pop.property.insert(GRAIN, PopPRow::new(qty(1.0)).with_target(qty(1.0)));
    pop.property.insert(WATER, PopPRow::new(qty(1.0)).with_target(qty(1.0)));
    pop.property.insert(BREAD, PopPRow::new(0.0).with_target(qty(1.0)));
    pop.property.insert(GOLD, PopPRow::new(0.0).with_target(qty(1.0)));
    pop.property.insert(COIN, PopPRow::new(qty(coin)));
    pop
}

/// Roster row: wage contract, 1 coin per Time unit, no worker profit share.
/// Lord is the owner-operator: remainder after wages, with sell piles and a
/// wage-float retained as growth (plan does not write growth_target yet).
pub(crate) fn with_worker(mut firm: Firm, pop_id: usize, hours: f64) -> Firm {
    firm = firm.with_owner_remainder();
    for row in firm.property.values_mut() {
        if row.sell_target > 0.0 {
            row.growth_target = row.growth_target.max(row.sell_target);
        }
    }
    let coin = firm.property.entry(COIN).or_insert_with(FirmPRow::new);
    coin.growth_target = coin.growth_target.max(hours);
    firm.with_workforce(
        Workforce::new(pop_id)
            .with_workers(qty(10.0), qty(10.0))
            .with_hours(hours)
            .with_payment(PaymentTerm::new(COIN, 1.0)),
    )
}

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
    firm.property.insert(COIN, FirmPRow::new().with_quantity(qty(120.0)));
    firm
}

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
        COIN,
        FirmPRow::new()
            .with_quantity(qty(400.0))
            .with_growth_target(qty(90.0)),
    );
    firm
}

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
        COIN,
        FirmPRow::new()
            .with_quantity(qty(120.0))
            .with_growth_target(qty(96.0)),
    );
    firm
}

pub(crate) fn make_jeweler() -> Firm {
    let mut firm = Firm::new(5, "jeweler".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(5, qty(1.0), vec![TIME, GOLD]));
    firm.production_line.push(dummy_line(4, qty(1.0), vec![TIME, GOLD]));
    // Reverse mint stays idle so it does not eat the till at opening prices.
    firm.production_line.push(dummy_line(7, 0.0, vec![TIME, COIN]));
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
        COIN,
        FirmPRow::new()
            .with_quantity(qty(400.0))
            .with_sell_target(qty(150.0))
            .with_growth_target(qty(150.0))
            .with_amv_target(COIN_AMV)
            .with_amv_bound(FirmAmvBound::Minimum(0.168)),
    );
    firm
}

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
    firm.property.insert(COIN, FirmPRow::new().with_quantity(qty(40.0)));
    firm
}

