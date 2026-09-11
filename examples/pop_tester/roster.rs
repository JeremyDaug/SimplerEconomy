use std::collections::HashMap;
use std::path::PathBuf;

use simpler_economy::game::config::PopConfig;
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::firm::Firm;
use simpler_economy::game::household::Household;
use simpler_economy::game::init::InitData;
use simpler_economy::game::market::MarketHistory;
use simpler_economy::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
use simpler_economy::game::sentiment::Sentiment;

/// Opening AMV for every world good. No price spread at start.
pub(crate) const OPENING_AMV: f64 = 100.0;
/// Opening salability for every world good. Below the exchange floor, so
/// nothing starts as money.
pub(crate) const OPENING_SALABILITY: f64 = 0.1;

/// Per pop id, the morning stock cap: process output good and `amount * target`.
pub(crate) type MorningOutputs = HashMap<usize, Vec<(usize, f64)>>;

pub(crate) fn world_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world")
}

pub(crate) fn init_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/init")
}

/// Builds the living pop roster from init data. Firms are read only to
/// size each pop's morning process-output cap, then dropped.
pub(crate) fn build_world() -> (Vec<Pop>, Vec<Firm>, Factuals, MarketHistory, MorningOutputs) {
    let factuals = Factuals::load_from_path(world_data_path())
        .unwrap_or_else(|err| panic!("load {}: {err}", world_data_path().display()));

    let mut history = MarketHistory::default();
    history.default_salability = OPENING_SALABILITY;
    for &id in factuals.goods.keys() {
        set_quote(&mut history, id, OPENING_AMV, OPENING_SALABILITY);
    }

    let mut init = InitData::load_from_path(init_data_path(), &factuals)
        .unwrap_or_else(|err| panic!("load {}: {err}", init_data_path().display()));
    let morning_outputs = morning_outputs_from_firms(&init.firms, &factuals);
    for pop in &mut init.pops {
        pop.record_keeping(&factuals, &history);
    }
    (init.pops, Vec::new(), factuals, history, morning_outputs)
}

pub(crate) fn set_quote(history: &mut MarketHistory, good: usize, amv: f64, salability: f64) {
    history.prices.insert(good, amv);
    history.salability.insert(good, salability);
}

/// Specialty good this pop's matching firm makes. Pop 28 wraps onto Time (0).
pub(crate) fn produced_good_id(pop_id: usize, n_goods: usize) -> usize {
    debug_assert!(n_goods > 0, "world catalog must not be empty");
    pop_id % n_goods
}

fn add_qty(pop: &mut Pop, good: usize, qty: f64) {
    debug_assert!(qty >= 0.0 && qty.is_finite(), "grant qty must be >= 0.0");
    if qty == 0.0 {
        return;
    }
    pop.property
        .entry(good)
        .or_insert_with(|| PopPRow::new(0.0))
        .quantity += qty;
}

/// Tops this pop up to its loaded firm process outputs (`amount * target`).
/// Adds only the shortfall. Stock already at or above the cap is left alone.
pub(crate) fn grant_daily_endowment(pop: &mut Pop, grants: &MorningOutputs) {
    let Some(rows) = grants.get(&pop.id) else {
        return;
    };
    for &(good, cap) in rows {
        debug_assert!(cap >= 0.0 && cap.is_finite(), "grant cap must be >= 0.0");
        let have = pop
            .property
            .get(&good)
            .map(|row| row.quantity)
            .unwrap_or(0.0);
        debug_assert!(have >= 0.0 && have.is_finite(), "quantity must be >= 0.0");
        add_qty(pop, good, (cap - have).max(0.0));
    }
}

/// Morning stock cap for each owner pop: every process output times that
/// line's target. No efficiency, hours, or success scaling.
pub(crate) fn morning_outputs_from_firms(firms: &[Firm], factuals: &Factuals) -> MorningOutputs {
    let mut grants: MorningOutputs = HashMap::new();
    for firm in firms {
        let Some(pop_id) = firm.owners.pop_id() else {
            continue;
        };
        for line in &firm.production_line {
            let target = line.target.unwrap_or(0.0);
            debug_assert!(
                target >= 0.0 && target.is_finite(),
                "line target must be >= 0.0"
            );
            let process = factuals
                .processes
                .get(&line.process)
                .unwrap_or_else(|| panic!("missing process {}", line.process));
            for output in &process.outputs {
                let qty = output.amount * target;
                debug_assert!(
                    qty >= 0.0 && qty.is_finite(),
                    "process output qty must be >= 0.0"
                );
                if qty <= 0.0 {
                    continue;
                }
                grants.entry(pop_id).or_default().push((output.good, qty));
            }
        }
    }
    grants
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
