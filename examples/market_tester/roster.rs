use std::collections::HashMap;
use std::path::PathBuf;

use hexx::Hex;
use simpler_economy::game::actor::Actor;
use simpler_economy::game::config::PopConfig;
use simpler_economy::game::desire::{Desire, DesireSource, DesireTarget, DesireTargetType};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::firm::{Firm, FirmPRow, ProductionLine};
use simpler_economy::game::init::InitData;
use simpler_economy::game::good::TIME;
use simpler_economy::game::household::Household;
use simpler_economy::game::market::MarketHistory;
use simpler_economy::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
use simpler_economy::game::sentiment::Sentiment;
use simpler_economy::game::workforce::Workforce;

use super::*;

/// Opening AMV for every world good. No price spread at start.
pub(crate) const OPENING_AMV: f64 = 10.0;
/// Opening salability for every world good. Below the exchange floor, so
/// nothing starts as money.
pub(crate) const OPENING_SALABILITY: f64 = 0.3;

/// Morning grant of every non-Time good.
pub(crate) const DAILY_ENDOWMENT: f64 = 0.0;
/// Morning pop specialty grant. 0: firms produce the day's output.
pub(crate) const DAILY_OUTPUT: f64 = 0.0;
/// Time units the owner-operator works. 10 Time * 15 output = 150 units.
pub(crate) const FIRM_HOURS: f64 = 10.0;

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

pub(crate) fn init_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/init")
}

/// Builds the living roster: one pop and one remainder-owner firm per world
/// good, each pop one default household (5 members).
pub(crate) fn build_world() -> (Vec<Pop>, Vec<Firm>, Factuals, MarketHistory) {
    let factuals = Factuals::load_from_path(world_data_path())
        .unwrap_or_else(|err| panic!("load {}: {err}", world_data_path().display()));

    let mut history = MarketHistory::default();
    // Flat start: no money good and no price spread.
    history.default_salability = OPENING_SALABILITY;
    for &id in factuals.goods.keys() {
        set_quote(&mut history, id, OPENING_AMV, OPENING_SALABILITY);
    }

    let mut init = InitData::load_from_path(init_data_path(), &factuals)
        .unwrap_or_else(|err| panic!("load {}: {err}", init_data_path().display()));
    for pop in &mut init.pops {
        pop.record_keeping(&factuals, &history);
    }
    (init.pops, init.firms, factuals, history)
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
#[allow(dead_code)]
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
    pop
}

/// Process whose first output is this good. World data must have exactly one.
pub(crate) fn process_id_for_output(factuals: &Factuals, output: usize) -> usize {
    let mut matches: Vec<usize> = factuals
        .processes
        .iter()
        .filter(|(_, process)| process.outputs.iter().any(|row| row.good == output))
        .map(|(&id, _)| id)
        .collect();
    matches.sort_unstable();
    match matches.as_slice() {
        [id] => *id,
        [] => panic!("no process outputs good {output}"),
        _ => panic!("multiple processes output good {output}"),
    }
}

/// One owner-operator firm for this pop's specialty. Remainder owner, no
/// wage basket; they work `FIRM_HOURS` Time for the 1 Time -> 15 recipe.
#[allow(dead_code)]
pub(crate) fn make_specialty_firm(pop_id: usize, factuals: &Factuals) -> Firm {
    let n_goods = factuals.goods.len();
    let good = produced_good_id(pop_id, n_goods);
    let process_id = process_id_for_output(factuals, good);
    let process = factuals
        .processes
        .get(&process_id)
        .unwrap_or_else(|| panic!("missing process {process_id}"));
    let time_in = process
        .inputs
        .iter()
        .find(|input| input.good == TIME)
        .map(|input| input.amount)
        .unwrap_or(1.0);
    debug_assert!(
        time_in > 0.0,
        "process {process_id} Time input must be > 0.0"
    );
    let target = FIRM_HOURS / time_in;
    let name = factuals
        .goods
        .get(&good)
        .unwrap_or_else(|| panic!("missing good {good}"))
        .name
        .as_str();
    let mut firm = Firm::new(
        pop_id,
        format!("firm{pop_id}-{name}"),
        1,
        Hex::new(0, 0),
    )
    .with_owner(Actor::Pop(pop_id))
    .with_owner_remainder()
    .with_workforce(
        Workforce::new(pop_id)
            .with_workers(1.0, 1.0)
            .with_hours(FIRM_HOURS),
    );
    firm.production_line
        .push(dummy_line(process_id, target, vec![TIME]));
    let output_amt = process
        .outputs
        .first()
        .map(|row| row.amount)
        .unwrap_or(0.0);
    let opening = target * output_amt;
    if good != TIME && opening > 0.0 {
        firm.property.insert(
            good,
            FirmPRow::new()
                .with_quantity(opening)
                .with_sell_target(opening),
        );
    }
    firm
}

fn dummy_line(process: usize, target: f64, inputs: Vec<usize>) -> ProductionLine {
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

