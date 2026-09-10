//! CLI box for probing a market day.
//!
//! Startup loads goods, processes, and config from `data/world/`, builds a
//! living roster (one household pop and one remainder-owner firm per world good,
//! grouped consume desires), and loads books from
//! [`Pop::create_orders`]. The home screen is a short summary. `stock`,
//! `orders`, and `processes` open full pages. `day` / `day N` runs the
//! calendar loop, including pop decay, salability rot cap, then
//! `record_keeping`. Firms are remainder owner-operators: 10 Time for
//! 150 specialty output, no wage basket.
//! Each day appends core market CSVs under `data/logs/` (close quotes and
//! trade candles). Pops are logged only when flagged (`csv on <actor>`).
//! `csv` shows the files; `csv <name>` changes the stem.
//!
//! ```text
//! cargo run --example market_tester
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

/// Default CSV stem under `data/logs/` (`prices_market.csv`, …).
const CSV_STEM_DEFAULT: &str = "prices";

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
        println!("=== market tester ===");
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
        "keep_alive" | "keepalive" => match rest.first().map(|s| s.to_ascii_lowercase()) {
            None => format!(
                "keep_alive {} (keep_alive on|off).",
                if session.factuals.config.firm.keep_alive {
                    "on"
                } else {
                    "off"
                }
            ),
            Some(ref s) if s == "on" || s == "true" || s == "1" => {
                session.factuals.config.firm.keep_alive = true;
                "keep_alive on. Collapsed firms get coin, inputs, and a 1-iteration floor.".into()
            }
            Some(ref s) if s == "off" || s == "false" || s == "0" => {
                session.factuals.config.firm.keep_alive = false;
                "keep_alive off.".into()
            }
            Some(s) => format!("keep_alive on|off (got {s})."),
        },
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
    let mut firm_orders = Vec::new();
    for firm in &session.firms {
        firm_orders.extend(firm.create_orders(
            &session.history,
            &session.factuals,
            &HashSet::new(),
        ));
    }
    let n_firm = firm_orders.len();
    for order in pop_orders.into_iter().chain(firm_orders) {
        insert_order(session, order);
    }
    format!(
        "shop loaded {} pop + {} firm orders ({} buys, {} sells).",
        n_pop,
        n_firm,
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

/// Loads the living roster and an empty CSV flag set.
pub(crate) fn boot_session() -> Session {
    let (pops, firms, factuals, history) = build_world();
    let market = market_from_world(&pops, &firms, &history);
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
    }
    let report = last_report.expect("ran at least one day");
    if n > 1 {
        out.push('\n');
    }
    out.push_str(&format_day_report(session, &report, &last_wages));
    let _ = shop_from_actors(session);
    out
}

/// Runs one tester calendar day: labor settle, market, production, consume,
/// sentiments, decay, salability rot cap, then pop/firm record keeping
/// (plan from what survived, using post-cap quotes).
fn run_one_day(session: &mut Session) -> (MarketDayReport, Vec<(usize, LaborSettlement)>) {
    let mut pops: HashMap<usize, Pop> = session.pops.drain(..).map(|pop| (pop.id, pop)).collect();
    let mut firms: HashMap<usize, Firm> =
        session.firms.drain(..).map(|firm| (firm.id, firm)).collect();

    let good_ids: Vec<usize> = session.factuals.goods.keys().copied().collect();
    for pop in pops.values_mut() {
        pop.start_day(&vec![(
            TIME,
            ScalingFactor::Labor(pop_constants::TIME_PER_LABOR),
        )]);
        grant_daily_endowment(pop, &good_ids);
        pop.records.income_amv = 0.0;
        pop.initial_reservations_and_update_satisfaction();
    }
    for firm in firms.values_mut() {
        firm.clear_day_flows();
    }

    let wages = session.market.settle_labor(&mut pops, &mut firms, &session.factuals);

    let report = session.market.run_market_day(
        &session.factuals,
        &mut pops,
        &mut firms,
        &mut session.rng,
    );

    for firm in firms.values_mut() {
        let _effects = firm.run_production(&session.factuals, &session.market);
    }

    let market_close = session.market.history();
    let mut rot: HashMap<usize, (f64, f64)> = HashMap::new();
    for pop in pops.values_mut() {
        pop.consume();
        pop.update_sentiments(&market_close, &session.factuals.config.pop);
        add_decay_rot(&mut rot, pop.decay_goods(&session.factuals));
    }
    let mut firm_ids: Vec<usize> = firms.keys().copied().collect();
    firm_ids.sort_unstable();
    for id in &firm_ids {
        add_decay_rot(
            &mut rot,
            firms
                .get_mut(id)
                .expect("firm id from keys")
                .decay_goods(&session.factuals),
        );
    }
    session.market.cap_salability_from_decay(&rot);
    let closing = session.market.history();
    for pop in pops.values_mut() {
        pop.record_keeping(&session.factuals, &closing);
    }
    for id in &firm_ids {
        firms
            .get_mut(id)
            .expect("firm id from keys")
            .record_keeping(&session.factuals, &closing);
    }
    let budget_day = session.day + 1;
    session
        .market
        .budget_labor(&pops, &mut firms, &session.factuals, budget_day);

    session.pops = pops.into_values().collect();
    session.pops.sort_by_key(|pop| pop.id);
    session.firms = firms.into_values().collect();
    session.firms.sort_by_key(|firm| firm.id);
    session.buys.clear();
    session.sells.clear();
    session.history = session.market.history();
    session.day += 1;
    (report, wages)
}

/// Sums `(decayed, volume)` maps from pop/firm [`Pop::decay_goods`] /
/// [`Firm::decay_goods`].
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

    #[test]
    fn living_roster_is_one_pop_per_world_good() {
        let session = boot_session();
        let n_goods = session.factuals.goods.len();
        assert_eq!(session.pops.len(), n_goods);
        assert_eq!(session.firms.len(), n_goods);
        let ids: Vec<usize> = session.pops.iter().map(|pop| pop.id).collect();
        assert_eq!(ids, (1..=n_goods).collect::<Vec<_>>());
        let firm_ids: Vec<usize> = session.firms.iter().map(|firm| firm.id).collect();
        assert_eq!(firm_ids, ids);
    }

    #[test]
    fn each_firm_is_a_remainder_owner_operator() {
        let session = boot_session();
        let n_goods = session.factuals.goods.len();
        for firm in &session.firms {
            let good = produced_good_id(firm.id, n_goods);
            let process_id = process_id_for_output(&session.factuals, good);
            assert_eq!(firm.production_line.len(), 1);
            let line = &firm.production_line[0];
            assert_eq!(line.process, process_id);
            assert_eq!(line.inputs, vec![TIME]);
            let target = line.target.unwrap();
            let time_in = session.factuals.processes[&process_id]
                .inputs
                .iter()
                .find(|input| input.good == TIME)
                .map(|input| input.amount)
                .unwrap_or(1.0);
            if target > 0.0 {
                assert!((line.last_iterations - target).abs() < 1e-9);
                assert!((line.last_success_rate - 1.0).abs() < 1e-9);
            }
            let output_amt = session.factuals.processes[&process_id].outputs[0].amount;
            let opening = target * output_amt;
            if good == TIME || opening <= 0.0 {
                assert!(
                    firm.property
                        .get(&good)
                        .map(|row| row.quantity)
                        .unwrap_or(0.0)
                        .abs()
                        < 1e-9
                );
            } else {
                let row = firm.property.get(&good).expect("opening output stock");
                assert!((row.quantity - opening).abs() < 1e-9);
                assert!((row.sell_target - opening).abs() < 1e-9);
            }
            assert_eq!(firm.workforce.len(), 1);
            assert_eq!(firm.workforce[0].id, firm.id);
            assert!((firm.workforce[0].hours - target * time_in).abs() < 1e-9);
            assert!(firm.workforce[0].payment.is_empty());
            assert!((firm.workforce[0].profit_share - 0.0).abs() < 1e-9);
            assert_eq!(firm.owners.owner, Actor::Pop(firm.id));
            assert!(firm.owners.remainder);
            assert!(session.market.firms.contains(&firm.id));
        }
        let grain = session.factuals.processes.get(&1).expect("make grain");
        assert_eq!(grain.name, "make grain");
        assert_eq!(grain.outputs[0].good, GRAIN);
        assert!((grain.outputs[0].amount - 15.0).abs() < 1e-9);
        let time = session
            .factuals
            .processes
            .get(&process_id_for_output(&session.factuals, TIME))
            .expect("make time");
        assert_eq!(time.outputs[0].good, TIME);
    }

    #[test]
    fn opening_quotes_are_flat_amv_and_salability() {
        let session = boot_session();
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
    fn morning_endowment_is_one_of_each_and_thirty_of_specialty() {
        let session = boot_session();
        let good_ids: Vec<usize> = session.factuals.goods.keys().copied().collect();
        let n_goods = good_ids.len();
        assert_eq!(produced_good_id(1, n_goods), GRAIN);
        assert_eq!(produced_good_id(n_goods, n_goods), TIME);

        let mut grain_pop = empty_pop(1, &session.factuals.config.pop);
        grant_daily_endowment(&mut grain_pop, &good_ids);
        assert!((grain_pop.property[&GRAIN].quantity - DAILY_OUTPUT).abs() < 1e-9);
        assert!((grain_pop.property[&WATER].quantity - DAILY_ENDOWMENT).abs() < 1e-9);
        assert!(
            grain_pop
                .property
                .get(&TIME)
                .map(|row| row.quantity)
                .unwrap_or(0.0)
                .abs()
                < 1e-9
        );

        let mut time_pop = empty_pop(n_goods, &session.factuals.config.pop);
        grant_daily_endowment(&mut time_pop, &good_ids);
        assert!((time_pop.property[&TIME].quantity - DAILY_OUTPUT).abs() < 1e-9);
        assert!((time_pop.property[&GRAIN].quantity - DAILY_ENDOWMENT).abs() < 1e-9);
    }
}
