use std::collections::HashMap;
use std::path::PathBuf;

use simpler_economy::game::config::PopConfig;
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::firm::Firm;
use simpler_economy::game::household::Household;
use simpler_economy::game::init::InitData;
use simpler_economy::game::market::MarketHistory;
use simpler_economy::game::pop::{DemoRow, Pop, PopRecords};
use simpler_economy::game::sentiment::Sentiment;

/// Opening AMV for every world good. No price spread at start.
pub(crate) const OPENING_AMV: f64 = 100.0;
/// Opening salability for every world good. Below the exchange floor, so
/// nothing starts as money.
pub(crate) const OPENING_SALABILITY: f64 = 0.1;

pub(crate) fn world_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world")
}

pub(crate) fn init_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/init")
}

/// Builds the living pop roster from init data. Firms are read to attach
/// each pop's specialty, then dropped. Unused world goods are dropped so
/// CLI and CSV only show the village catalog.
pub(crate) fn build_world() -> (Vec<Pop>, Vec<Firm>, Factuals, MarketHistory) {
    let mut factuals = Factuals::load_from_path(world_data_path())
        .unwrap_or_else(|err| panic!("load {}: {err}", world_data_path().display()));

    let mut init = InitData::load_from_path(init_data_path(), &factuals)
        .unwrap_or_else(|err| panic!("load {}: {err}", init_data_path().display()));
    init.unload_unused_goods(&mut factuals);

    let mut history = MarketHistory::default();
    history.default_salability = OPENING_SALABILITY;
    for id in catalog_good_ids(&factuals) {
        set_quote(&mut history, id, OPENING_AMV, OPENING_SALABILITY);
    }

    for pop in &mut init.pops {
        pop.record_keeping(&factuals, &history);
    }
    (init.pops, Vec::new(), factuals, history)
}

/// Sorted good ids still in `factuals` after unused goods are unloaded.
pub(crate) fn catalog_good_ids(factuals: &Factuals) -> Vec<usize> {
    let mut ids: Vec<usize> = factuals.goods.keys().copied().collect();
    ids.sort_unstable();
    ids
}

pub(crate) fn set_quote(history: &mut MarketHistory, good: usize, amv: f64, salability: f64) {
    history.prices.insert(good, amv);
    history.salability.insert(good, salability);
}

/// Specialty good this pop's matching firm makes. Pop 28 and 56 wrap onto Time (0).
pub(crate) fn produced_good_id(pop_id: usize, n_goods: usize) -> usize {
    debug_assert!(n_goods > 0, "world catalog must not be empty");
    pop_id % n_goods
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
        household_work: Vec::new(),
        records: PopRecords::from_config(pop_cfg),
    }
}
