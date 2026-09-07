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
use simpler_economy::game::workforce::Workforce;

use super::*;


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
    set_quote(&mut history, WATER, 0.20, 0.35);
    set_quote(&mut history, BREAD, 2.2, 0.45);
    set_quote(&mut history, GOLD, 8.0, 0.70);
    set_quote(&mut history, COIN, COIN_AMV, 1.00);
    set_quote(&mut history, JEWELRY, 15.0, 0.80);

    let pops = vec![
        make_farmers_pop(&factuals.config.pop),
        make_laborers_pop(&factuals.config.pop),
        make_townsfolk_pop(&factuals.config.pop),
        make_lord_pop(&factuals.config.pop),
    ];
    let lord = Actor::Pop(4);
    let firms = vec![
        with_worker(make_farm().with_owner(lord), 1),
        with_worker(make_bakery().with_owner(lord), 3),
        with_worker(make_mine().with_owner(lord), 2),
        with_worker(make_jeweler().with_owner(lord), 3),
        with_worker(make_well().with_owner(lord), 1),
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
            household: Household::with_count(10.0),
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
    pop.property.insert(GRAIN, PopPRow::new(24.0).with_target(4.0));
    pop.property.insert(WATER, PopPRow::new(1.0).with_target(6.0));
    pop.property.insert(BREAD, PopPRow::new(0.0).with_target(5.0));
    pop.property.insert(COIN, PopPRow::new(80.0));
    pop
}

pub(crate) fn make_laborers_pop(pop_cfg: &PopConfig) -> Pop {
    let mut pop = with_need_spread(empty_pop(2, pop_cfg));
    pop.property.insert(GRAIN, PopPRow::new(1.0).with_target(8.0));
    pop.property.insert(WATER, PopPRow::new(0.0).with_target(6.0));
    pop.property.insert(BREAD, PopPRow::new(0.0).with_target(4.0));
    pop.property.insert(COIN, PopPRow::new(160.0));
    pop
}

pub(crate) fn make_townsfolk_pop(pop_cfg: &PopConfig) -> Pop {
    let mut pop = with_need_spread(empty_pop(3, pop_cfg));
    pop.property.insert(GRAIN, PopPRow::new(4.0).with_target(6.0));
    pop.property.insert(WATER, PopPRow::new(2.0).with_target(4.0));
    pop.property.insert(BREAD, PopPRow::new(1.0).with_target(6.0));
    pop.property.insert(COIN, PopPRow::new(400.0));
    pop
}

/// One-household owner. Staples stay small; jewelry is the luxury sink.
/// Starting AMV is about 20x townsfolk wealth per household (~4.7 -> ~93).
pub(crate) fn make_lord_pop(pop_cfg: &PopConfig) -> Pop {
    let mut pop = empty_pop(4, pop_cfg);
    pop.demographics.household = Household::with_count(1.0);
    pop.desires[0].push(make_food_desire(0, 1.0));
    pop.desires[0].push(make_desire(1, WATER, 1.0));
    pop.desires[1].push(make_desire(2, BREAD, 1.0));
    pop.desires[2].push(make_desire(3, JEWELRY, 2.0));
    pop.property.insert(GRAIN, PopPRow::new(1.0).with_target(1.0));
    pop.property.insert(WATER, PopPRow::new(1.0).with_target(1.0));
    pop.property.insert(BREAD, PopPRow::new(1.0).with_target(1.0));
    pop.property.insert(JEWELRY, PopPRow::new(0.0).with_target(2.0));
    pop.property.insert(COIN, PopPRow::new(900.0));
    pop
}

pub(crate) fn with_worker(mut firm: Firm, pop_id: usize) -> Firm {
    let mut w = Workforce::empty();
    w.id = pop_id;
    w.workers = (10.0, 10.0);
    w.hours = 1.0;
    firm.workforce.push(w);
    firm
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
    firm.production_line.push(dummy_line(1, 5.0, vec![TIME, WATER]));
    // Stock matches plan's input_cover * use (2 * 5). Start a little short so
    // a water buy posts on day 1. Bid 0.45 clears the well's 0.20 ask.
    firm.property.insert(
        WATER,
        FirmPRow::new()
            .with_quantity(8.0)
            .with_purchase_target(8.0)
            .with_use_target(5.0)
            .with_stock_target(10.0)
            .with_amv_target(0.45)
            .with_amv_bound(FirmAmvBound::Maximum(2.5)),
    );
    firm.property.insert(
        GRAIN,
        FirmPRow::new()
            .with_quantity(45.0)
            .with_sell_target(30.0)
            .with_amv_target(1.2)
            .with_amv_bound(FirmAmvBound::Minimum(1.0)),
    );
    firm.property.insert(COIN, FirmPRow::new().with_quantity(120.0));
    firm
}

pub(crate) fn make_bakery() -> Firm {
    let mut firm = Firm::new(2, "bakery".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(2, 10.0, vec![TIME, GRAIN]));
    firm.property.insert(
        GRAIN,
        FirmPRow::new()
            .with_quantity(4.0)
            .with_purchase_target(12.0)
            .with_use_target(10.0)
            .with_stock_target(16.0)
            .with_amv_target(2.0)
            .with_amv_bound(FirmAmvBound::Maximum(1.5)),
    );
    firm.property.insert(
        BREAD,
        FirmPRow::new()
            .with_quantity(15.0)
            .with_sell_target(12.0)
            .with_amv_target(2.2)
            .with_amv_bound(FirmAmvBound::Minimum(1.8)),
    );
    firm.property.insert(COIN, FirmPRow::new().with_quantity(250.0));
    firm
}

pub(crate) fn make_mine() -> Firm {
    let mut firm = Firm::new(3, "mine".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(3, 8.0, vec![TIME]));
    firm.property.insert(
        GOLD,
        FirmPRow::new()
            .with_quantity(10.0)
            .with_sell_target(8.0)
            .with_amv_target(8.0)
            .with_amv_bound(FirmAmvBound::Minimum(6.0)),
    );
    firm.property.insert(COIN, FirmPRow::new().with_quantity(40.0));
    firm
}

pub(crate) fn make_jeweler() -> Firm {
    let mut firm = Firm::new(5, "jeweler".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(5, 1.0, vec![TIME, GOLD]));
    firm.production_line.push(dummy_line(4, 1.0, vec![TIME, GOLD]));
    // Reverse mint stays idle so it does not eat the till at opening prices.
    firm.production_line.push(dummy_line(7, 0.0, vec![TIME, COIN]));
    // 8 gold covers jewelry (3) plus mint (1) on day 1 with slack to restock.
    firm.property.insert(
        GOLD,
        FirmPRow::new()
            .with_quantity(8.0)
            .with_purchase_target(4.0)
            .with_use_target(4.0)
            .with_stock_target(8.0)
            .with_amv_bound(FirmAmvBound::Maximum(12.0)),
    );
    firm.property.insert(
        JEWELRY,
        FirmPRow::new()
            .with_quantity(6.0)
            .with_sell_target(5.0)
            .with_amv_target(15.0)
            .with_amv_bound(FirmAmvBound::Minimum(12.0)),
    );
    firm.property.insert(
        COIN,
        FirmPRow::new()
            .with_quantity(300.0)
            .with_sell_target(150.0)
            .with_amv_target(COIN_AMV)
            .with_amv_bound(FirmAmvBound::Minimum(0.168)),
    );
    firm
}

pub(crate) fn make_well() -> Firm {
    let mut firm = Firm::new(6, "well".into(), 1, Hex::new(0, 0));
    // 40/day covers pop water shop (~18) plus the farm restock (~12) with slack.
    // Ask at market (0.20); floor 0.15 so leftover-AMV cheapening can still sell.
    firm.production_line.push(dummy_line(6, 40.0, vec![TIME]));
    firm.property.insert(
        WATER,
        FirmPRow::new()
            .with_quantity(80.0)
            .with_sell_target(40.0)
            .with_amv_target(0.20)
            .with_amv_bound(FirmAmvBound::Minimum(0.15)),
    );
    firm.property.insert(COIN, FirmPRow::new().with_quantity(40.0));
    firm
}

