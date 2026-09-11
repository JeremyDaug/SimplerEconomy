//! CLI box for probing a pop-only market day.
//!
//! Copy of `market_tester` with firms left out. Startup loads goods,
//! processes, and config from `data/world/`, pops from `data/init/`, and
//! sizes each pop's morning stock cap from the matching init firm's process
//! outputs (`amount * target`, no scaling). Firms are not kept on the
//! session. Books come from [`Pop::create_orders`]. `day` / `day N` grants
//! Time, then tops each pop up to those process outputs, runs the market,
//! then consume / decay / `record_keeping`. `day N` stops early if any good
//! AMV goes negative. No firm production or plan.
//! Each day appends core market CSVs under `data/logs/` (close quotes and
//! trade candles). Pops are logged only when flagged (`csv on <actor>`).
//! `csv` shows the files; `csv <name>` changes the stem.
//!
//! ```text
//! cargo run --example pop_tester
//! ```
//!
//! ```text
//!   day
//!   stock
//!   orders
//!   processes
//!   day 5
//!   csv
//! ```

use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Write};

use rand::rngs::StdRng;
use rand::SeedableRng;
use simpler_economy::game::actor::Actor;
use simpler_economy::game::config::pop_constants;
use simpler_economy::game::good::TIME;
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::firm::Firm;
use simpler_economy::game::workforce::LaborSettlement;
use simpler_economy::game::market::{
    Market, MarketDayReport, MarketGood, MarketHistory, MeetingOutcome,
};
use simpler_economy::game::marketorder::{compose_sell_priority_with, MarketOrder};
use simpler_economy::game::desire::DesireTargetType;
use simpler_economy::game::pop::Pop;
use simpler_economy::game::scalingfactor::ScalingFactor;

mod csv;
mod format;
mod parse;
mod roster;

use csv::*;
use format::*;
use parse::*;
use roster::*;

/// Label on a good id.
struct NamedGood {
    id: usize,
    name: &'static str,
}

/// Label on an actor id.
struct NamedActor {
    actor: Actor,
    name: &'static str,
}

const GRAIN: usize = 1;
const WATER: usize = 2;
const BREAD: usize = 3;
const GOLD: usize = 4;
const GOLD_TOKEN: usize = 5;
const JEWELRY: usize = 6;
const WOOD: usize = 7;
const CABINS: usize = 8;
const WOOD_TOOLS: usize = 9;
const BUCKETS: usize = 10;
const IRON: usize = 11;
const IRON_TOOLS: usize = 12;
const COPPER: usize = 13;
const TIN: usize = 14;
const BRONZE: usize = 15;
const BRONZE_TOOLS: usize = 16;
const BLADES: usize = 17;
const BRONZE_MIRROR: usize = 18;
const BRONZE_TOKEN: usize = 19;
const IRON_TOKEN: usize = 20;
const COPPER_TOKEN: usize = 21;
const TIN_TOKEN: usize = 22;
const COAL: usize = 23;
const CHARCOAL: usize = 24;
const BEER: usize = 25;
const CLAY: usize = 26;
const POTS: usize = 27;

/// Default CSV stem under `data/logs/` (`pop_prices_market.csv`, …).
const CSV_STEM_DEFAULT: &str = "pop_prices";

const PREFAB_GOODS: &[NamedGood] = &[
    NamedGood { id: TIME, name: "time" },
    NamedGood { id: GRAIN, name: "grain" },
    NamedGood { id: WATER, name: "water" },
    NamedGood { id: BREAD, name: "bread" },
    NamedGood { id: GOLD, name: "gold" },
    NamedGood { id: GOLD_TOKEN, name: "gold_token" },
    NamedGood { id: JEWELRY, name: "jewelry" },
    NamedGood { id: WOOD, name: "wood" },
    NamedGood { id: CABINS, name: "cabins" },
    NamedGood { id: WOOD_TOOLS, name: "wood_tools" },
    NamedGood { id: BUCKETS, name: "buckets" },
    NamedGood { id: IRON, name: "iron" },
    NamedGood { id: IRON_TOOLS, name: "iron_tools" },
    NamedGood { id: COPPER, name: "copper" },
    NamedGood { id: TIN, name: "tin" },
    NamedGood { id: BRONZE, name: "bronze" },
    NamedGood { id: BRONZE_TOOLS, name: "bronze_tools" },
    NamedGood { id: BLADES, name: "blades" },
    NamedGood { id: BRONZE_MIRROR, name: "bronze_mirror" },
    NamedGood { id: BRONZE_TOKEN, name: "bronze_token" },
    NamedGood { id: IRON_TOKEN, name: "iron_token" },
    NamedGood { id: COPPER_TOKEN, name: "copper_token" },
    NamedGood { id: TIN_TOKEN, name: "tin_token" },
    NamedGood { id: COAL, name: "coal" },
    NamedGood { id: CHARCOAL, name: "charcoal" },
    NamedGood { id: BEER, name: "beer" },
    NamedGood { id: CLAY, name: "clay" },
    NamedGood { id: POTS, name: "pots" },
];

const PREFAB_ACTORS: &[NamedActor] = &[
    NamedActor { actor: Actor::Pop(1), name: "pop1-grain" },
    NamedActor { actor: Actor::Pop(2), name: "pop2-water" },
    NamedActor { actor: Actor::Pop(3), name: "pop3-bread" },
    NamedActor { actor: Actor::Pop(4), name: "pop4-gold" },
    NamedActor { actor: Actor::Pop(5), name: "pop5-gold_token" },
    NamedActor { actor: Actor::Pop(6), name: "pop6-jewelry" },
    NamedActor { actor: Actor::Pop(7), name: "pop7-wood" },
    NamedActor { actor: Actor::Pop(8), name: "pop8-cabins" },
    NamedActor { actor: Actor::Pop(9), name: "pop9-wood_tools" },
    NamedActor { actor: Actor::Pop(10), name: "pop10-buckets" },
    NamedActor { actor: Actor::Pop(11), name: "pop11-iron" },
    NamedActor { actor: Actor::Pop(12), name: "pop12-iron_tools" },
    NamedActor { actor: Actor::Pop(13), name: "pop13-copper" },
    NamedActor { actor: Actor::Pop(14), name: "pop14-tin" },
    NamedActor { actor: Actor::Pop(15), name: "pop15-bronze" },
    NamedActor { actor: Actor::Pop(16), name: "pop16-bronze_tools" },
    NamedActor { actor: Actor::Pop(17), name: "pop17-blades" },
    NamedActor { actor: Actor::Pop(18), name: "pop18-bronze_mirror" },
    NamedActor { actor: Actor::Pop(19), name: "pop19-bronze_token" },
    NamedActor { actor: Actor::Pop(20), name: "pop20-iron_token" },
    NamedActor { actor: Actor::Pop(21), name: "pop21-copper_token" },
    NamedActor { actor: Actor::Pop(22), name: "pop22-tin_token" },
    NamedActor { actor: Actor::Pop(23), name: "pop23-coal" },
    NamedActor { actor: Actor::Pop(24), name: "pop24-charcoal" },
    NamedActor { actor: Actor::Pop(25), name: "pop25-beer" },
    NamedActor { actor: Actor::Pop(26), name: "pop26-clay" },
    NamedActor { actor: Actor::Pop(27), name: "pop27-pots" },
    NamedActor { actor: Actor::Pop(28), name: "pop28-time" },
];

struct Session {
    buys: Vec<MarketOrder>,
    sells: Vec<MarketOrder>,
    rng: StdRng,
    seed: Option<u64>,
    log: String,
    /// When true, the screen is the last requested page instead of home.
    focus_log: bool,
    /// Completed calendar days in this session.
    day: u32,
    pops: Vec<Pop>,
    firms: Vec<Firm>,
    factuals: Factuals,
    history: MarketHistory,
    market: Market,
    /// Basename for day-end CSVs in `data/logs/` (`{stem}_market.csv`, …).
    csv_stem: String,
    /// Pop ids written to `{stem}_pops.csv`. Empty skips that file.
    csv_pops: HashSet<usize>,
    /// Firm ids written to `{stem}_firms.csv`. Empty skips that file.
    csv_firms: HashSet<usize>,
    /// Morning process-output stock cap per pop, loaded from init firms.
    morning_outputs: MorningOutputs,
}


fn main() {
    let mut session = boot_session();
    session.log = shop_from_actors(&mut session);

    let tty = io::stdout().is_terminal();
    if tty {
        draw_ui(&session);
    } else {
        print!("{}", format_home(&session));
    }

    let stdin = io::stdin();
    loop {
        print!("> ");
        if io::stdout().flush().is_err() {
            break;
        }
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => {
                println!();
                break;
            }
            Ok(_) => {}
            Err(err) => {
                eprintln!("read error: {err}");
                break;
            }
        }
        let line = line.trim();
        if line.is_empty() {
            if tty {
                draw_ui(&session);
            }
            continue;
        }
        match handle_line(&mut session, line) {
            CmdResult::Continue(msg) => {
                session.log = msg;
                if tty {
                    draw_ui(&session);
                } else {
                    println!("{}", session.log.trim_end());
                }
            }
            CmdResult::Quit => break,
        }
    }
}

fn clear_screen() {
    print!("\x1B[2J\x1B[H");
}

fn draw_ui(session: &Session) {
    clear_screen();
    if session.focus_log {
        println!("=== pop tester ===");
        println!("{}", rng_line(session));
        println!("home  back to summary.");
        println!();
        println!("{}", session.log.trim_end());
        return;
    }
    print!("{}", format_home(session));
}

enum CmdResult {
    Continue(String),
    Quit,
}

fn handle_line(session: &mut Session, line: &str) -> CmdResult {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let cmd = tokens[0].to_ascii_lowercase();
    let rest = &tokens[1..];
    session.focus_log = is_page_command(&cmd);
    let msg = match cmd.as_str() {
        "help" | "?" | "h" => help_text(),
        "home" | "cls" => {
            session.focus_log = false;
            String::new()
        }
        "quit" | "exit" | "q" => return CmdResult::Quit,
        "clear" => {
            session.buys.clear();
            session.sells.clear();
            "books cleared.".into()
        }
        "shop" => shop_from_actors(session),
        "stock" | "inv" => format_stock_page(session),
        "orders" | "books" | "book" => format_orders_page(session),
        "processes" | "process" | "recipes" => format_processes_page(session),
        "day" | "d" | "days" => match parse_day_count(rest) {
            Ok(n) => run_days(session, n),
            Err(err) => err,
        },
        "amv" | "prices" => format_amv_trail(session).trim_end().to_string(),
        "csv" | "log" => handle_csv_command(session, rest),
        "seed" => match parse_seed(rest) {
            Ok(seed) => {
                session.rng = StdRng::seed_from_u64(seed);
                session.seed = Some(seed);
                format!("rng seeded to {seed} (next match / day starts from here).")
            }
            Err(err) => err,
        },
        "unseed" => {
            session.rng = StdRng::from_os_rng();
            session.seed = None;
            "rng back to os entropy.".into()
        }
        "request" | "req" => match parse_simple_order(rest, true, &session.factuals.config.market_priority) {
            Ok(order) => add_buy(session, order),
            Err(err) => err,
        },
        "offer" => match parse_simple_order(rest, false, &session.factuals.config.market_priority) {
            Ok(order) => add_sell(session, order),
            Err(err) => err,
        },
        "buy" => match parse_exchange_order(rest, true, &session.factuals.config.market_priority) {
            Ok(order) => add_buy(session, order),
            Err(err) => err,
        },
        "sell" => match parse_exchange_order(rest, false, &session.factuals.config.market_priority) {
            Ok(order) => add_sell(session, order),
            Err(err) => err,
        },
        "drop" => match drop_order(session, rest) {
            Ok(msg) => msg,
            Err(err) => err,
        },
        "match" | "m" => run_match(session),
        other => format!("unknown command '{other}'. Type help."),
    };
    CmdResult::Continue(msg)
}

fn shop_from_actors(session: &mut Session) -> String {
    session.buys.clear();
    session.sells.clear();
    let mut pop_orders = Vec::new();
    for pop in &mut session.pops {
        pop_orders.extend(pop.create_orders(
            &session.history,
            &session.factuals,
            &HashSet::new(),
        ));
    }
    let n_pop = pop_orders.len();
    for order in pop_orders {
        insert_order(session, order);
    }
    format!(
        "shop loaded {} pop orders ({} buys, {} sells).",
        n_pop,
        session.buys.len(),
        session.sells.len()
    )
}

fn insert_order(session: &mut Session, order: MarketOrder) {
    if order.target_amount > 0.0 {
        let _ = add_buy(session, order);
    } else {
        let _ = add_sell(session, order);
    }
}



fn rng_line(session: &Session) -> String {
    let rng = match session.seed {
        Some(s) => format!("rng: seed {s}"),
        None => "rng: os".into(),
    };
    format!("calendar day {}   {rng}", session.day)
}



fn run_match(session: &mut Session) -> String {
    let batch = Market::match_orders_with_coincidence(
        &session.buys,
        &session.sells,
        &mut session.rng,
        session.factuals.config.market_priority.sell_coincidence_weight,
    );
    if batch.is_empty() {
        return "empty batch (no buys, or nothing to deal / update).\nbooks unchanged.".into();
    }
    let mut out = String::new();
    match batch.matched {
        Some(pair) => {
            let buy = &session.buys[pair.buy_index];
            let sell = &session.sells[pair.sell_index];
            out.push_str(&format!(
                "match  {}  {} {}  <-  {}  {} {}\n",
                fmt_actor(buy.origin),
                fmt_qty(buy.target_amount.abs()),
                fmt_good(buy.target),
                fmt_actor(sell.origin),
                fmt_qty(sell.target_amount.abs()),
                fmt_good(sell.target),
            ));
            out.push_str(&format!(
                "  books  buy[{}]  sell[{}]\n",
                pair.buy_index, pair.sell_index
            ));
            if coincidence(buy, sell) {
                out.push_str("  coincidence  matching counters (sell weight x2 this pick)\n");
            }
        }
        None => out.push_str("no match this pass\n"),
    }
    if batch.unmatched_buys.is_empty() {
        out.push_str("unmatched  (none)\n");
    } else {
        out.push_str("unmatched  (no seller)\n");
        for &i in &batch.unmatched_buys {
            let order = &session.buys[i];
            out.push_str(&format!(
                "  [{i}]  {}  {} {}\n",
                fmt_actor(order.origin),
                fmt_qty(order.target_amount.abs()),
                fmt_good(order.target)
            ));
        }
    }
    out.push_str("books unchanged");
    out
}

/// Loads the living pop roster, morning process-output caps, and an empty
/// CSV flag set. Firms are not kept on the session.
pub(crate) fn boot_session() -> Session {
    let (pops, firms, factuals, history, morning_outputs) = build_world();
    debug_assert!(firms.is_empty(), "pop tester does not load firms");
    let mut history = history;
    history.friction = factuals.config.market.friction;
    let market = market_from_world(&pops, &firms, &history)
        .with_friction(history.friction);
    Session {
        buys: Vec::new(),
        sells: Vec::new(),
        rng: StdRng::from_os_rng(),
        seed: None,
        log: String::new(),
        focus_log: false,
        day: 0,
        pops,
        firms,
        factuals,
        history,
        market,
        csv_stem: CSV_STEM_DEFAULT.to_string(),
        csv_pops: HashSet::new(),
        csv_firms: HashSet::new(),
        morning_outputs,
    }
}

fn market_from_world(pops: &[Pop], firms: &[Firm], history: &MarketHistory) -> Market {
    let mut market = Market::new(1);
    for pop in pops {
        market.pops.insert(pop.id);
    }
    for firm in firms {
        market.firms.insert(firm.id);
    }
    for good in PREFAB_GOODS {
        let mut row = MarketGood::new()
            .with_amv(history.price(good.id))
            .with_salability(history.salability(good.id));
        row.record_amv();
        market.goods.insert(good.id, row);
    }
    market
}

fn run_days(session: &mut Session, n: u32) -> String {
    let mut out = String::new();
    let mut last_report = None;
    let mut last_wages: Vec<(usize, LaborSettlement)> = Vec::new();
    if n > 1 {
        out.push_str("=== calendar ===\n");
    }
    for _ in 0..n {
        let (report, wages) = run_one_day(session);
        match append_price_log(session, &report) {
            Ok(()) => {}
            Err(err) => out.push_str(&format!("csv write failed: {err}\n")),
        }
        if n > 1 {
            out.push_str(&format!(
                "day {}  {}\n",
                session.day,
                day_digest(session, &report, &wages)
            ));
        }
        last_report = Some(report);
        last_wages = wages;
        let negatives = negative_amv_goods(session);
        if !negatives.is_empty() {
            out.push_str(&format_negative_amv_stop(session.day, &negatives));
            break;
        }
    }
    let report = last_report.expect("ran at least one day");
    if n > 1 {
        out.push('\n');
    }
    out.push_str(&format_day_report(session, &report, &last_wages));
    let _ = shop_from_actors(session);
    out
}

/// Runs one tester calendar day: morning Time and process-output top-up,
/// labor settle (no-op with no firms), market, consume, sentiments, decay,
/// salability rot cap, then pop record keeping.
fn run_one_day(session: &mut Session) -> (MarketDayReport, Vec<(usize, LaborSettlement)>) {
    let mut pops: HashMap<usize, Pop> = session.pops.drain(..).map(|pop| (pop.id, pop)).collect();
    let mut firms: HashMap<usize, Firm> = HashMap::new();

    for pop in pops.values_mut() {
        pop.start_day(&vec![(
            TIME,
            ScalingFactor::Labor(pop_constants::TIME_PER_LABOR),
        )]);
        grant_daily_endowment(pop, &session.morning_outputs);
        pop.records.income_amv = 0.0;
        pop.initial_reservations_and_update_satisfaction();
    }

    let wages = session
        .market
        .settle_labor(&mut pops, &mut firms, &session.factuals);

    let report = session.market.run_market_day(
        &session.factuals,
        &mut pops,
        &mut firms,
        &mut session.rng,
    );

    let market_close = session.market.history();
    let mut rot: HashMap<usize, (f64, f64)> = HashMap::new();
    for pop in pops.values_mut() {
        pop.consume();
        pop.update_sentiments(&market_close, &session.factuals.config.pop);
        add_decay_rot(&mut rot, pop.decay_goods(&session.factuals));
    }
    session.market.cap_salability_from_decay(&rot);
    let closing = session.market.history();
    for pop in pops.values_mut() {
        pop.record_keeping(&session.factuals, &closing);
    }
    let budget_day = session.day + 1;
    session
        .market
        .budget_labor(&pops, &mut firms, &session.factuals, budget_day);

    session.pops = pops.into_values().collect();
    session.pops.sort_by_key(|pop| pop.id);
    session.firms.clear();
    session.buys.clear();
    session.sells.clear();
    session.history = session.market.history();
    session.day += 1;
    (report, wages)
}

/// Goods whose close AMV is below 0.0, sorted by id.
fn negative_amv_goods(session: &Session) -> Vec<(usize, String, f64)> {
    let mut ids: Vec<usize> = session.market.goods.keys().copied().collect();
    ids.sort_unstable();
    let mut rows = Vec::new();
    for id in ids {
        let amv = session.market.goods[&id].amv;
        if amv < 0.0 {
            rows.push((id, fmt_good(id), amv));
        }
    }
    rows
}

fn format_negative_amv_stop(day: u32, negatives: &[(usize, String, f64)]) -> String {
    debug_assert!(!negatives.is_empty(), "stop line needs a negative good");
    let mut out = format!("stopped: day {day}  negative AMV\n");
    for (id, name, amv) in negatives {
        out.push_str(&format!("  {name} (#{id})  amv {}\n", fmt_num(*amv)));
    }
    out
}

/// Sums `(decayed, volume)` maps from pop [`Pop::decay_goods`].
fn add_decay_rot(into: &mut HashMap<usize, (f64, f64)>, from: HashMap<usize, (f64, f64)>) {
    for (id, (decayed, volume)) in from {
        let entry = into.entry(id).or_insert((0.0, 0.0));
        entry.0 += decayed;
        entry.1 += volume;
    }
}

#[cfg(test)]
mod day_should {
    use super::*;
    use simpler_economy::game::init::InitData;
    use simpler_economy::game::pop::PopPRow;

    #[test]
    fn living_roster_is_one_pop_per_world_good_and_no_firms() {
        let session = boot_session();
        let n_goods = session.factuals.goods.len();
        assert_eq!(session.pops.len(), n_goods);
        assert!(session.firms.is_empty());
        assert!(session.market.firms.is_empty());
        let ids: Vec<usize> = session.pops.iter().map(|pop| pop.id).collect();
        assert_eq!(ids, (1..=n_goods).collect::<Vec<_>>());
    }

    #[test]
    fn negative_amv_stop_names_the_day_and_good() {
        let mut session = boot_session();
        assert!(negative_amv_goods(&session).is_empty());
        session.market.goods.get_mut(&GRAIN).expect("grain").amv = -0.01;
        let rows = negative_amv_goods(&session);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, GRAIN);
        let msg = format_negative_amv_stop(7, &rows);
        assert!(msg.contains("stopped: day 7"), "{msg}");
        assert!(msg.contains("grain"), "{msg}");
        assert!(msg.contains("negative AMV"), "{msg}");
    }

    #[test]
    fn opening_quotes_are_flat_amv_and_salability() {
        let session = boot_session();
        assert!((session.history.friction - 1.0).abs() < 1e-9);
        assert!((session.market.friction - 1.0).abs() < 1e-9);
        assert!((session.history.default_salability - OPENING_SALABILITY).abs() < 1e-9);
        for id in session.factuals.goods.keys() {
            assert!(
                (session.history.price(*id) - OPENING_AMV).abs() < 1e-9,
                "good {id} AMV {}",
                session.history.price(*id)
            );
            assert!(
                (session.history.salability(*id) - OPENING_SALABILITY).abs() < 1e-9,
                "good {id} sal {}",
                session.history.salability(*id)
            );
        }
    }

    #[test]
    fn living_pops_have_no_opening_stock() {
        let session = boot_session();
        for pop in &session.pops {
            for (&id, row) in &pop.property {
                if id == TIME {
                    continue;
                }
                assert!(
                    row.quantity.abs() < 1e-9,
                    "pop {} good {id} qty {}",
                    pop.id,
                    row.quantity
                );
            }
        }
    }

    #[test]
    fn each_pop_has_one_household_and_grouped_consume_desires() {
        let session = boot_session();
        let pop = &session.pops[0];
        let house = &pop.demographics.household;
        assert!((house.count - 1.0).abs() < 1e-9);
        assert!((pop.demographics.total_population() - 5.0).abs() < 1e-9);
        assert_eq!(pop.desires[0].len(), 4);
        assert_eq!(pop.desires[1].len(), 4);
        assert_eq!(pop.desires[2].len(), 2);
        assert_eq!(pop.desires[0][0].category.as_deref(), Some("food"));
        assert_eq!(pop.desires[0][1].category.as_deref(), Some("hydration"));
        assert_eq!(pop.desires[1][0].category.as_deref(), Some("utility items"));
        assert_eq!(pop.desires[2][1].category.as_deref(), Some("libations"));
        for tier in &pop.desires {
            for desire in tier {
                match desire.scalar {
                    ScalingFactor::All(weight) => {
                        assert!((weight - 1.0).abs() < 1e-9)
                    }
                    other => panic!("want All(1.0), got {other:?}"),
                }
                assert!((desire.amount - 5.0).abs() < 1e-9);
                assert!(desire.target.iter().all(|t| {
                    matches!(t.desire_type, DesireTargetType::Consume)
                }));
            }
        }
    }

    #[test]
    fn start_day_grants_labor_time() {
        let mut session = boot_session();
        let pop = &mut session.pops[0];
        pop.property.remove(&TIME);
        pop.start_day(&vec![(
            TIME,
            ScalingFactor::Labor(pop_constants::TIME_PER_LABOR),
        )]);
        let want = pop.demographics.labor() * pop_constants::TIME_PER_LABOR;
        let got = pop.property.get(&TIME).map(|row| row.quantity).unwrap_or(0.0);
        assert!((got - want).abs() < 1e-9, "time {got} want {want}");
        assert!((pop_constants::TIME_PER_LABOR - 64.0).abs() < 1e-9);
    }

    #[test]
    fn morning_outputs_come_from_init_firm_process_times_target() {
        let session = boot_session();
        let init = InitData::load_from_path(init_data_path(), &session.factuals)
            .expect("load init firms for grant check");
        let expected = morning_outputs_from_firms(&init.firms, &session.factuals);
        assert_eq!(session.morning_outputs.len(), expected.len());
        for (pop_id, rows) in &expected {
            assert_eq!(session.morning_outputs.get(pop_id), Some(rows));
        }
        assert!(session.firms.is_empty());

        let grain_firm = init.firms.iter().find(|firm| firm.id == 1).expect("firm 1");
        let grain_line = &grain_firm.production_line[0];
        let grain_proc = &session.factuals.processes[&grain_line.process];
        let grain_qty = grain_proc.outputs[0].amount * grain_line.target.unwrap();
        assert_eq!(grain_proc.outputs[0].good, GRAIN);
        assert!(grain_qty > 0.0);
        assert_eq!(session.morning_outputs[&1], vec![(GRAIN, grain_qty)]);

        let gold_firm = init.firms.iter().find(|firm| firm.id == 4).expect("firm 4");
        let gold_line = &gold_firm.production_line[0];
        let gold_proc = &session.factuals.processes[&gold_line.process];
        let gold_qty = gold_proc.outputs[0].amount * gold_line.target.unwrap();
        assert_eq!(gold_proc.outputs[0].good, GOLD);
        assert!(gold_qty > 0.0);
        assert_eq!(session.morning_outputs[&4], vec![(GOLD, gold_qty)]);

        let time_id = session.pops.len();
        let time_firm = init
            .firms
            .iter()
            .find(|firm| firm.id == time_id)
            .expect("time firm");
        let time_line = &time_firm.production_line[0];
        let time_proc = &session.factuals.processes[&time_line.process];
        let time_qty = time_proc.outputs[0].amount * time_line.target.unwrap();
        assert_eq!(time_proc.outputs[0].good, TIME);
        assert_eq!(session.morning_outputs[&time_id], vec![(TIME, time_qty)]);
    }

    #[test]
    fn morning_grant_tops_up_to_process_output_and_does_not_overfill() {
        let session = boot_session();
        let n_goods = session.factuals.goods.len();
        assert_eq!(produced_good_id(1, n_goods), GRAIN);
        assert_eq!(produced_good_id(n_goods, n_goods), TIME);

        let grain_cap = session.morning_outputs[&1][0].1;
        let gold_cap = session.morning_outputs[&4][0].1;
        assert!(grain_cap > 50.0);
        assert!(gold_cap > 5.0);

        let mut grain_pop = empty_pop(1, &session.factuals.config.pop);
        grant_daily_endowment(&mut grain_pop, &session.morning_outputs);
        assert!((grain_pop.property[&GRAIN].quantity - grain_cap).abs() < 1e-9);
        assert!(
            grain_pop
                .property
                .get(&WATER)
                .map(|row| row.quantity)
                .unwrap_or(0.0)
                .abs()
                < 1e-9
        );

        let mut short = empty_pop(1, &session.factuals.config.pop);
        short.property.insert(GRAIN, PopPRow::new(50.0));
        grant_daily_endowment(&mut short, &session.morning_outputs);
        assert!((short.property[&GRAIN].quantity - grain_cap).abs() < 1e-9);

        let over = grain_cap + 50.0;
        let mut full = empty_pop(1, &session.factuals.config.pop);
        full.property.insert(GRAIN, PopPRow::new(over));
        grant_daily_endowment(&mut full, &session.morning_outputs);
        assert!((full.property[&GRAIN].quantity - over).abs() < 1e-9);

        let mut gold_pop = empty_pop(4, &session.factuals.config.pop);
        gold_pop.property.insert(GOLD, PopPRow::new(5.0));
        grant_daily_endowment(&mut gold_pop, &session.morning_outputs);
        assert!((gold_pop.property[&GOLD].quantity - gold_cap).abs() < 1e-9);

        let mut time_pop = empty_pop(n_goods, &session.factuals.config.pop);
        time_pop.property.insert(TIME, PopPRow::new(64.0));
        grant_daily_endowment(&mut time_pop, &session.morning_outputs);
        let time_qty = session.morning_outputs[&n_goods][0].1;
        assert!((time_pop.property[&TIME].quantity - time_qty).abs() < 1e-9);
    }
}
