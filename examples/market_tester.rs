//! CLI box for probing a market day.
//!
//! Startup loads goods, processes, and config from `data/world/`, builds a
//! small living roster (4 pops, 5 producer firms), and loads books from
//! [`Pop::create_orders`] / [`Firm::create_orders`]. The home screen is a
//! short summary. `stock`, `orders`, and `processes` open full pages.
//! `day` / `day N` runs the calendar loop, including firm `record_keeping`
//! (rolling average, records, [`Firm::plan`]) after production and pop
//! consume. Each day appends price CSVs under `data/logs/` (market close,
//! firm quotes, settled trades). `csv` shows the files; `csv <name>`
//! changes the stem.
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
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use hexx::Hex;
use rand::rngs::StdRng;
use rand::SeedableRng;
use simpler_economy::game::actor::Actor;
use simpler_economy::game::config::{pop_constants, MarketPriorityConfig, PopConfig};
use simpler_economy::game::good::TIME;
use simpler_economy::game::desire::{Desire, DesireSource, DesireTarget, DesireTargetType};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::firm::{Firm, FirmAmvBound, FirmPRow, ProductionLine, WagePayout};
use simpler_economy::game::workforce::Workforce;

use simpler_economy::game::household::Household;
use simpler_economy::game::market::{
    Market, MarketDayReport, MarketGood, MarketHistory, MarketMeeting, MeetingOutcome, WashReason,
};
use simpler_economy::game::marketorder::{compose_sell_priority_with, MarketOrder};
use simpler_economy::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
use simpler_economy::game::scalingfactor::ScalingFactor;
use simpler_economy::game::sentiment::Sentiment;

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
const COIN: usize = 5;
const JEWELRY: usize = 6;

/// Tester coin is 10x units. Opening AMV is 0.1 * 2.1 so gold 8 / coin
/// sits near the 40-coin mint recipe (8 / 0.21 ~ 38).
const COIN_AMV: f64 = 0.21;
/// Overnight coin save cap (units). Leaves the rest tenderable.
const COIN_SAVE_UNITS: f64 = 1.0;
/// Default CSV stem under `data/logs/` (`prices_market.csv`, …).
const CSV_STEM_DEFAULT: &str = "prices";

const PREFAB_GOODS: &[NamedGood] = &[
    NamedGood { id: TIME, name: "time" },
    NamedGood { id: GRAIN, name: "grain" },
    NamedGood { id: WATER, name: "water" },
    NamedGood { id: BREAD, name: "bread" },
    NamedGood { id: GOLD, name: "gold" },
    NamedGood { id: COIN, name: "coin" },
    NamedGood { id: JEWELRY, name: "jewelry" },
];

const PREFAB_ACTORS: &[NamedActor] = &[
    NamedActor { actor: Actor::Pop(1), name: "farmers" },
    NamedActor { actor: Actor::Pop(2), name: "laborers" },
    NamedActor { actor: Actor::Pop(3), name: "townsfolk" },
    NamedActor { actor: Actor::Pop(4), name: "lord" },
    NamedActor { actor: Actor::Firm(1), name: "farm" },
    NamedActor { actor: Actor::Firm(2), name: "bakery" },
    NamedActor { actor: Actor::Firm(3), name: "mine" },
    NamedActor { actor: Actor::Firm(5), name: "jeweler" },
    NamedActor { actor: Actor::Firm(6), name: "well" },
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
}

struct Tokens<'a> {
    rest: &'a [&'a str],
}

impl<'a> Tokens<'a> {
    fn new(rest: &'a [&'a str]) -> Self {
        Self { rest }
    }

    fn next(&mut self) -> Option<&'a str> {
        let (first, rest) = self.rest.split_first()?;
        self.rest = rest;
        Some(*first)
    }

    fn expect_empty(&self) -> Result<(), String> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(format!("unexpected extra tokens: {}", self.rest.join(" ")))
        }
    }
}

fn main() {
    let (pops, firms, factuals, history) = build_world();
    let market = market_from_world(&pops, &firms, &history);
    let mut session = Session {
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
    };
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
    for pop in &session.pops {
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

fn is_page_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "day"
            | "d"
            | "days"
            | "stock"
            | "inv"
            | "orders"
            | "books"
            | "book"
            | "processes"
            | "process"
            | "recipes"
            | "amv"
            | "prices"
            | "help"
            | "?"
            | "h"
            | "match"
            | "m"
    )
}

fn format_home(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("=== market tester ===\n");
    out.push_str(&rng_line(session));
    out.push('\n');
    out.push_str("goods  (amv  sal)\n");
    out.push_str("  --  --------  ------  ----\n");
    for good in PREFAB_GOODS {
        out.push_str(&format!(
            "  {:>2}  {:<8}  {:>6}  {:>4}\n",
            good.id,
            good.name,
            fmt_num(session.history.price(good.id)),
            fmt_num(session.history.salability(good.id))
        ));
    }
    let pops: Vec<&str> = PREFAB_ACTORS
        .iter()
        .filter(|a| matches!(a.actor, Actor::Pop(_)))
        .map(|a| a.name)
        .collect();
    let firms: Vec<&str> = PREFAB_ACTORS
        .iter()
        .filter(|a| matches!(a.actor, Actor::Firm(_)))
        .map(|a| a.name)
        .collect();
    out.push('\n');
    out.push_str(&format!("pops   {}\n", pops.join("  ")));
    out.push_str(&format!("firms  {}\n", firms.join("  ")));
    out.push_str(&format!(
        "books  {} buys / {} sells\n",
        session.buys.len(),
        session.sells.len()
    ));
    out.push_str(&format!(
        "csv    {}/{{market,firms,trades}}.csv\n",
        csv_stem_dir_display(session)
    ));
    out.push_str("\nstock  orders  processes  day  amv  csv  help\n");
    if !session.log.is_empty() {
        out.push_str("\n---\n");
        out.push_str(session.log.trim_end());
        out.push('\n');
    }
    out
}

fn format_stock_page(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Stock  (live on-hand)\n");
    out.push_str(&format!("  {:<10}", "actor"));
    for good in PREFAB_GOODS {
        out.push_str(&format!("  {:>8}", good.name));
    }
    out.push('\n');
    out.push_str(&format!("  {:-<10}", ""));
    for _ in PREFAB_GOODS {
        out.push_str(&format!("  {:-<8}", ""));
    }
    out.push('\n');
    for pop in &session.pops {
        out.push_str(&format!("  {:<10}", fmt_actor(Actor::Pop(pop.id))));
        for good in PREFAB_GOODS {
            out.push_str(&format!(
                "  {:>8}",
                stock_cell(pop.property.get(&good.id).map(|r| r.quantity))
            ));
        }
        out.push('\n');
    }
    for firm in &session.firms {
        out.push_str(&format!("  {:<10}", fmt_actor(Actor::Firm(firm.id))));
        for good in PREFAB_GOODS {
            out.push_str(&format!(
                "  {:>8}",
                stock_cell(firm.property.get(&good.id).map(|r| r.quantity))
            ));
        }
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&format_firm_bounds(session));
    out.push('\n');
    out.push_str(&format_firm_quotes(session));
    out
}

fn format_firm_bounds(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Firm bounds  (min = sell floor, max = buy cap)\n");
    out.push_str("  planning guidestones; they do not skip or void trades.\n");
    out.push_str(&format!("  {:<10}  {:<8}  {}\n", "actor", "good", "bound"));
    out.push_str(&format!("  {:-<10}  {:-<8}  {:-<16}\n", "", "", ""));
    let mut any = false;
    for firm in &session.firms {
        let mut rows: Vec<_> = firm.property.iter().collect();
        rows.sort_by_key(|(id, _)| *id);
        for (&good, row) in rows {
            if row.amv_bound == FirmAmvBound::None {
                continue;
            }
            any = true;
            out.push_str(&format!(
                "  {:<10}  {:<8}  {}\n",
                fmt_actor(Actor::Firm(firm.id)),
                fmt_good(good),
                fmt_bound(row.amv_bound)
            ));
        }
    }
    if !any {
        out.push_str("  (none)\n");
    }
    out
}

fn format_firm_records(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Firm records  (confidence 0 cautious .. 1 aggressive)\n");
    out.push_str(&format!(
        "  {:<10} {:>6} {:>7} {:>8} {:>8}\n",
        "firm", "conf", "profit", "success", "sold AMV"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<6} {:-<7} {:-<8} {:-<8}\n",
        "", "", "", "", ""
    ));
    if session.firms.is_empty() {
        out.push_str("  (none)\n");
        return out;
    }
    for firm in &session.firms {
        out.push_str(&format!(
            "  {:<10} {:>6} {:>7} {:>8} {:>8}\n",
            fmt_actor(Actor::Firm(firm.id)),
            fmt_num(firm.records.confidence),
            fmt_num(firm.records.profit_ratio),
            fmt_num(firm.records.sell_success),
            fmt_qty(firm.records.sold_amv)
        ));
    }
    out
}

fn format_firm_quotes(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Firm quotes  (next-day sell / own AMV)\n");
    out.push_str(&format!(
        "  {:<10} {:<8} {:>6} {:>7} {:>7}\n",
        "firm", "good", "sell", "quote", "cost"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<8} {:-<6} {:-<7} {:-<7}\n",
        "", "", "", "", ""
    ));
    let mut any = false;
    for firm in &session.firms {
        if let Some((good, row)) = firm_primary_output(session, firm) {
            any = true;
            out.push_str(&format!(
                "  {:<10} {:<8} {:>6} {:>7} {:>7}\n",
                fmt_actor(Actor::Firm(firm.id)),
                fmt_good(good),
                fmt_qty(row.sell_target),
                fmt_num(row.amv_target),
                fmt_num(row.average_cost)
            ));
        }
    }
    if !any {
        out.push_str("  (none)\n");
    }
    out
}

/// First process output row, or the first row with a sell target.
fn firm_primary_output<'a>(session: &Session, firm: &'a Firm) -> Option<(usize, &'a FirmPRow)> {
    for line in &firm.production_line {
        if let Some(process) = session.factuals.processes.get(&line.process) {
            if let Some(output) = process.outputs.first() {
                if let Some(row) = firm.property.get(&output.good) {
                    return Some((output.good, row));
                }
            }
        }
    }
    let mut rows: Vec<_> = firm
        .property
        .iter()
        .filter(|(_, row)| row.sell_target > 0.0)
        .collect();
    rows.sort_by_key(|(id, _)| *id);
    rows.into_iter().next().map(|(&id, row)| (id, row))
}

fn format_orders_page(session: &Session) -> String {
    let mut out = String::new();
    out.push_str(&format_order_table(
        session,
        "Buys  (priority, lowest first)",
        &session.buys,
    ));
    out.push('\n');
    out.push_str(&format_order_table(
        session,
        "Sells  (target good id)",
        &session.sells,
    ));
    out
}

fn format_order_table(session: &Session, title: &str, orders: &[MarketOrder]) -> String {
    let mut out = String::new();
    out.push_str(title);
    out.push('\n');
    out.push_str(&order_header());
    out.push('\n');
    out.push_str(&order_rule());
    out.push('\n');
    if orders.is_empty() {
        out.push_str("  (empty)\n");
    } else {
        for (i, order) in orders.iter().enumerate() {
            out.push_str(&order_row(session, i, order));
            out.push('\n');
        }
    }
    out
}

fn format_processes_page(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Processes  (world recipes)\n");
    out.push_str(&format!("  {:>3}  {:<14}  {}\n", "id", "name", "recipe"));
    out.push_str(&format!("  {:->3}  {:-<14}  {:-<28}\n", "", "", ""));
    let mut ids: Vec<usize> = session.factuals.processes.keys().copied().collect();
    ids.sort_unstable();
    if ids.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for id in ids {
            let process = &session.factuals.processes[&id];
            out.push_str(&format!(
                "  {:>3}  {:<14}  {}\n",
                process.id,
                process.name,
                fmt_recipe(process)
            ));
        }
    }
    out.push('\n');
    out.push_str(&format_firm_records(session));
    out.push('\n');
    out.push_str("Firm lines\n");
    out.push_str(&format!(
        "  {:<10} {:<14} {:>6} {:>6}  {}\n",
        "firm", "process", "did", "want", "missing"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<14} {:-<6} {:-<6}  {:-<16}\n",
        "", "", "", "", ""
    ));
    let mut any = false;
    for firm in &session.firms {
        for line in &firm.production_line {
            any = true;
            let name = session
                .factuals
                .processes
                .get(&line.process)
                .map(|p| p.name.as_str())
                .unwrap_or("?");
            let missing = if line.last_missing_goods.is_empty() {
                "-".into()
            } else {
                line.last_missing_goods
                    .iter()
                    .copied()
                    .map(fmt_good)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            out.push_str(&format!(
                "  {:<10} {:<14} {:>6} {:>6}  {}\n",
                fmt_actor(Actor::Firm(firm.id)),
                name,
                fmt_qty(line.last_iterations),
                fmt_qty(line.target.unwrap_or(0.0)),
                missing
            ));
        }
    }
    if !any {
        out.push_str("  (none)\n");
    }
    out
}

fn fmt_recipe(process: &simpler_economy::game::process::Process) -> String {
    let inputs: Vec<String> = process
        .inputs
        .iter()
        .map(|input| format!("{} {}", fmt_qty(input.amount), fmt_good(input.good)))
        .collect();
    let outputs: Vec<String> = process
        .outputs
        .iter()
        .map(|output| format!("{} {}", fmt_qty(output.amount), fmt_good(output.good)))
        .collect();
    let left = if inputs.is_empty() {
        "-".into()
    } else {
        inputs.join(" + ")
    };
    let right = if outputs.is_empty() {
        "-".into()
    } else {
        outputs.join(" + ")
    };
    format!("{left} -> {right}")
}

fn stock_cell(qty: Option<f64>) -> String {
    match qty {
        Some(x) => fmt_qty(x),
        None => "-".into(),
    }
}

fn help_text() -> String {
    "\
commands
  day [N]               run N calendar days (default 1)
  stock                 live on-hand + firm AMV bounds and quotes
  orders                current buy/sell books
  processes             world recipes + firm records and lines
  amv                   AMV trail (old -> new)
  csv                   show day-end price CSV paths
  csv <name>            write under data/logs/<name>_*.csv
  csv reset             wipe current CSVs and rewrite headers
  shop                  reload books from create_orders
  home                  back to the summary
  request <actor> <good> <amount> [priority]
  offer   <actor> <good> <amount> [priority]
  buy     <actor> <good> <amount> <amv> <pay-good> <pay-amount> [priority]
  sell    <actor> <good> <amount> <amv> <want-good> <want-amount> [priority]
  match                 one match_orders pass; does not remove anything
  drop buy <i>          remove buy at list index
  drop sell <i>
  seed <n>              deterministic rng from n
  unseed                os rng again
  clear                 empty the books
  help
  quit

Home is a short summary. stock / orders / processes / day / amv / help
open a page; home returns. Each `day` appends one-row-per-day CSVs in
data/logs/ (market quotes, firm quotes, trade candles).
Startup runs shop once. `day` grants Time, pays wage shares, runs the
market, runs each firm's process, pops consume, then pop and firm
record keeping (firm plan; coin save capped at 1) / decay.
actor: prefab name (farmers, lord, bakery, ...) or kind id (pop 1, firm 2)
good:  prefab name (time, grain, coin, jewelry) or id (0, 1, 5, 6)

examples
  day
  stock
  orders
  processes
  day 5
  csv
  csv run1
  request laborers grain 3"
        .into()
}

fn parse_day_count(rest: &[&str]) -> Result<u32, String> {
    if rest.is_empty() {
        return Ok(1);
    }
    if rest.len() != 1 {
        return Err("usage: day [count]".into());
    }
    let n: u32 = rest[0]
        .parse()
        .map_err(|_| format!("not a day count: {}", rest[0]))?;
    if n == 0 {
        return Err("day count must be >= 1".into());
    }
    if n > 365 {
        return Err("day count max is 365".into());
    }
    Ok(n)
}

fn parse_seed(rest: &[&str]) -> Result<u64, String> {
    if rest.len() != 1 {
        return Err("usage: seed <u64>".into());
    }
    rest[0]
        .parse::<u64>()
        .map_err(|_| format!("not a u64: {}", rest[0]))
}

/// request / offer: actor good amount [priority]
fn parse_simple_order(
    rest: &[&str],
    is_buy: bool,
    cfg: &MarketPriorityConfig,
) -> Result<MarketOrder, String> {
    let kind = if is_buy { "request" } else { "offer" };
    let usage = format!("usage: {kind} <actor> <good> <amount> [priority]");
    let mut tok = Tokens::new(rest);
    let actor = parse_actor(&mut tok).map_err(|e| format!("{e}  {usage}"))?;
    let good = parse_good(&mut tok).map_err(|e| format!("{e}  {usage}"))?;
    let amount = parse_positive_amount(tok.next().ok_or_else(|| usage.clone())?)?;
    let priority = match tok.next() {
        Some(raw) => parse_f64(raw, "priority")?,
        None => {
            if is_buy {
                default_buy_priority(actor, cfg)
            } else {
                compose_sell_priority_with(
                    default_buy_priority(actor, cfg),
                    amount,
                    0.0,
                    cfg.sell_actor_priority_floor,
                    cfg.successful_sell_bonus,
                )
            }
        }
    };
    tok.expect_empty()?;
    check_priority(actor, priority, is_buy, cfg)?;
    if is_buy {
        Ok(MarketOrder::request_order(actor, good, amount, priority))
    } else {
        Ok(MarketOrder::offer_order(actor, good, -amount, priority))
    }
}

/// buy / sell: actor good amount amv other-good other-amount [priority]
fn parse_exchange_order(
    rest: &[&str],
    is_buy: bool,
    cfg: &MarketPriorityConfig,
) -> Result<MarketOrder, String> {
    let kind = if is_buy { "buy" } else { "sell" };
    let usage = format!(
        "usage: {kind} <actor> <good> <amount> <amv> <other-good> <other-amount> [priority]"
    );
    let mut tok = Tokens::new(rest);
    let actor = parse_actor(&mut tok).map_err(|e| format!("{e}  {usage}"))?;
    let good = parse_good(&mut tok).map_err(|e| format!("{e}  {usage}"))?;
    let amount = parse_positive_amount(tok.next().ok_or_else(|| usage.clone())?)?;
    let amv = parse_f64(tok.next().ok_or_else(|| usage.clone())?, "amv")?;
    let other_good = parse_good(&mut tok).map_err(|e| format!("{e}  {usage}"))?;
    let other_amount = parse_positive_amount(tok.next().ok_or_else(|| usage.clone())?)?;
    let priority = match tok.next() {
        Some(raw) => parse_f64(raw, "priority")?,
        None => {
            if is_buy {
                default_buy_priority(actor, cfg)
            } else {
                compose_sell_priority_with(
                    default_buy_priority(actor, cfg),
                    amount,
                    0.0,
                    cfg.sell_actor_priority_floor,
                    cfg.successful_sell_bonus,
                )
            }
        }
    };
    tok.expect_empty()?;
    check_priority(actor, priority, is_buy, cfg)?;
    if is_buy {
        Ok(MarketOrder::buy_order(
            actor,
            good,
            amount,
            amv,
            other_good,
            -other_amount,
            priority,
        ))
    } else {
        Ok(MarketOrder::sell_order(
            actor,
            good,
            -amount,
            amv,
            other_good,
            other_amount,
            priority,
        ))
    }
}

fn parse_actor(tok: &mut Tokens<'_>) -> Result<Actor, String> {
    let first = tok
        .next()
        .ok_or_else(|| "expected actor (prefab name or kind id)".to_string())?;
    let key = first.to_ascii_lowercase();
    if let Some(named) = PREFAB_ACTORS.iter().find(|a| a.name == key) {
        return Ok(named.actor);
    }
    let id_tok = tok
        .next()
        .ok_or_else(|| format!("unknown actor '{first}' (need a prefab name, or kind plus id)"))?;
    parse_actor_kind_id(&key, id_tok)
}

fn parse_actor_kind_id(kind: &str, id: &str) -> Result<Actor, String> {
    let id = parse_usize(id, "actor id")?;
    match kind {
        "pop" | "p" => Ok(Actor::Pop(id)),
        "firm" | "f" => Ok(Actor::Firm(id)),
        "inst" | "institution" | "i" => Ok(Actor::Institution(id)),
        "state" | "s" => Ok(Actor::State(id)),
        other => Err(format!(
            "unknown actor kind '{other}' (pop/firm/inst/state or p/f/i/s)"
        )),
    }
}

fn parse_good(tok: &mut Tokens<'_>) -> Result<usize, String> {
    let raw = tok
        .next()
        .ok_or_else(|| "expected good (prefab name or id)".to_string())?;
    let key = raw.to_ascii_lowercase();
    if let Some(good) = PREFAB_GOODS.iter().find(|g| g.name == key) {
        return Ok(good.id);
    }
    parse_usize(raw, "good").map_err(|_| format!("unknown good '{raw}' (prefab name or id)"))
}

fn parse_usize(raw: &str, name: &str) -> Result<usize, String> {
    raw.parse::<usize>()
        .map_err(|_| format!("{name} must be a usize, got '{raw}'"))
}

fn parse_f64(raw: &str, name: &str) -> Result<f64, String> {
    let v: f64 = raw
        .parse()
        .map_err(|_| format!("{name} must be a number, got '{raw}'"))?;
    if !v.is_finite() {
        return Err(format!("{name} must be finite"));
    }
    Ok(v)
}

fn parse_positive_amount(raw: &str) -> Result<f64, String> {
    let v = parse_f64(raw, "amount")?.abs();
    if v == 0.0 {
        return Err("amount must be non-zero".into());
    }
    Ok(v)
}

fn default_buy_priority(actor: Actor, cfg: &MarketPriorityConfig) -> f64 {
    match actor {
        Actor::Pop(_) => cfg.pop_start,
        Actor::Firm(_) => cfg.firm_producer(),
        Actor::Institution(_) => cfg.institution_before_firms,
        Actor::State(_) => cfg.state_first,
    }
}

fn check_priority(
    actor: Actor,
    priority: f64,
    is_buy: bool,
    cfg: &MarketPriorityConfig,
) -> Result<(), String> {
    if !priority.is_finite() {
        return Err("priority must be finite".into());
    }
    if !is_buy {
        if priority <= 0.0 {
            return Err("sell/offer priority must be > 0".into());
        }
        return Ok(());
    }
    match actor {
        Actor::Pop(_) => {
            if !(cfg.pop_start..cfg.pop_end).contains(&priority) {
                return Err(format!(
                    "pop buy priority must be in [{}, {})",
                    cfg.pop_start, cfg.pop_end
                ));
            }
        }
        Actor::Firm(_) => {
            if !(cfg.firm_merchant_start..cfg.firm_producer_end).contains(&priority) {
                return Err(format!(
                    "firm buy priority must be in [{}, {})",
                    cfg.firm_merchant_start, cfg.firm_producer_end
                ));
            }
        }
        Actor::Institution(_) | Actor::State(_) => {}
    }
    Ok(())
}

fn add_buy(session: &mut Session, order: MarketOrder) -> String {
    let i = session
        .buys
        .partition_point(|o| o.priority <= order.priority);
    session.buys.insert(i, order);
    format!("buy [{i}] {}", fmt_order(session, &session.buys[i]))
}

fn add_sell(session: &mut Session, order: MarketOrder) -> String {
    let i = session.sells.partition_point(|o| o.target <= order.target);
    session.sells.insert(i, order);
    format!("sell [{i}] {}", fmt_order(session, &session.sells[i]))
}

fn drop_order(session: &mut Session, rest: &[&str]) -> Result<String, String> {
    if rest.len() != 2 {
        return Err("usage: drop buy <i>  or  drop sell <i>".into());
    }
    let side = rest[0].to_ascii_lowercase();
    let idx = parse_usize(rest[1], "index")?;
    match side.as_str() {
        "buy" | "b" => {
            if idx >= session.buys.len() {
                return Err(format!("no buy [{idx}]"));
            }
            let removed = session.buys.remove(idx);
            Ok(format!("dropped buy [{idx}] {}", fmt_order(session, &removed)))
        }
        "sell" | "s" => {
            if idx >= session.sells.len() {
                return Err(format!("no sell [{idx}]"));
            }
            let removed = session.sells.remove(idx);
            Ok(format!("dropped sell [{idx}] {}", fmt_order(session, &removed)))
        }
        other => Err(format!("drop side must be buy or sell, got '{other}'")),
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
    let mut last_wages: Vec<(usize, WagePayout)> = Vec::new();
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

/// Runs one tester calendar day: wages, market, production, consume,
/// pop and firm record keeping (firm plan), decay.
fn run_one_day(session: &mut Session) -> (MarketDayReport, Vec<(usize, WagePayout)>) {
    let mut pops: HashMap<usize, Pop> = session.pops.drain(..).map(|pop| (pop.id, pop)).collect();
    let mut firms: HashMap<usize, Firm> =
        session.firms.drain(..).map(|firm| (firm.id, firm)).collect();

    for pop in pops.values_mut() {
        pop.start_day(&vec![(
            TIME,
            ScalingFactor::Labor(pop_constants::TIME_PER_LABOR),
        )]);
        pop.records.income_amv = 0.0;
        pop.initial_reservations_and_update_satisfaction();
    }
    for firm in firms.values_mut() {
        firm.clear_day_flows();
    }

    let coin_amv = session.history.price(COIN);
    let mut wages = Vec::new();
    let mut firm_ids: Vec<usize> = firms.keys().copied().collect();
    firm_ids.sort_unstable();
    for id in &firm_ids {
        let payout = firms
            .get_mut(id)
            .expect("firm id from keys")
            .pay_wage_shares(&mut pops, COIN, coin_amv, &session.factuals.config);
        wages.push((*id, payout));
    }

    let report = session.market.run_market_day(
        &session.factuals,
        &mut pops,
        &mut firms,
        &mut session.rng,
    );

    for firm in firms.values_mut() {
        let _effects = firm.run_production(&session.factuals, &session.market);
    }

    let closing = session.market.history();
    for pop in pops.values_mut() {
        pop.consume();
        pop.update_sentiments(&closing, &session.factuals.config.pop);
        pop.record_keeping(&session.factuals, &closing);
        cap_coin_save(pop);
    }
    for id in &firm_ids {
        firms
            .get_mut(id)
            .expect("firm id from keys")
            .record_keeping(&session.factuals, &closing);
    }
    for pop in pops.values_mut() {
        pop.decay_goods(&session.factuals);
    }
    for firm in firms.values_mut() {
        firm.decay_goods(&session.factuals);
    }

    session.pops = pops.into_values().collect();
    session.pops.sort_by_key(|pop| pop.id);
    session.firms = firms.into_values().collect();
    session.firms.sort_by_key(|firm| firm.id);
    session.buys.clear();
    session.sells.clear();
    session.history = closing;
    session.day += 1;
    (report, wages)
}

fn csv_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/logs")
}

fn csv_path(stem: &str, kind: &str) -> PathBuf {
    csv_dir().join(format!("{stem}_{kind}.csv"))
}

fn csv_stem_dir_display(session: &Session) -> String {
    format!("data/logs/{}", session.csv_stem)
}

fn csv_status_line(session: &Session) -> String {
    format!(
        "logged  {}_{{market,firms,trades}}.csv",
        csv_stem_dir_display(session)
    )
}

fn csv_status(session: &Session) -> String {
    let stem = &session.csv_stem;
    format!(
        "CSV directory  data/logs/\n  {stem}_market.csv   one row/day; good blocks: amv, salability, average_price\n  {stem}_firms.csv    one row/day; firm conf/profit/success, then good blocks: qty, targets, bid, ask, costs, sold, produced\n  {stem}_trades.csv   one row/day; good candles: open, high, low, close, volume\ncsv <name>  changes the stem.  csv reset  wipes these files. Header mismatch (old layout) needs reset or a new stem."
    )
}

fn handle_csv_command(session: &mut Session, rest: &[&str]) -> String {
    if rest.is_empty() {
        return csv_status(session);
    }
    if rest.len() == 1 && rest[0].eq_ignore_ascii_case("reset") {
        return match reset_price_log(session) {
            Ok(()) => format!("wiped CSVs.\n{}", csv_status(session)),
            Err(err) => format!("csv reset failed: {err}"),
        };
    }
    if rest.len() != 1 {
        return "usage: csv [name]  or  csv reset".into();
    }
    match sanitize_csv_stem(rest[0]) {
        Ok(stem) => {
            session.csv_stem = stem;
            csv_status(session)
        }
        Err(err) => err,
    }
}

fn sanitize_csv_stem(raw: &str) -> Result<String, String> {
    if raw.is_empty() || raw.len() > 40 {
        return Err("csv name must be 1 to 40 characters".into());
    }
    if !raw
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("csv name may use letters, digits, _ and - only".into());
    }
    if raw == "reset" {
        return Err("csv name 'reset' is reserved. Use csv reset to wipe.".into());
    }
    Ok(raw.to_string())
}

fn reset_price_log(session: &Session) -> Result<(), String> {
    fs::create_dir_all(csv_dir()).map_err(|err| format!("create data/logs: {err}"))?;
    write_csv_file(
        &csv_path(&session.csv_stem, "market"),
        &market_csv_header(session),
        &[],
        true,
    )?;
    write_csv_file(
        &csv_path(&session.csv_stem, "firms"),
        &firm_csv_header(session),
        &[],
        true,
    )?;
    write_csv_file(
        &csv_path(&session.csv_stem, "trades"),
        &trade_csv_header(session),
        &[],
        true,
    )?;
    Ok(())
}

const MARKET_CSV_FIELDS: &[&str] = &["amv", "salability", "average_price"];
const FIRM_RECORD_FIELDS: &[&str] = &["confidence", "profit", "sell_success"];
const FIRM_CSV_FIELDS: &[&str] = &[
    "quantity",
    "sell_target",
    "amv_target",
    "bid",
    "ask",
    "average_cost",
    "average_price",
    "sold",
    "produced",
];
const TRADE_CSV_FIELDS: &[&str] = &["open", "high", "low", "close", "volume"];

fn append_price_log(session: &Session, report: &MarketDayReport) -> Result<(), String> {
    fs::create_dir_all(csv_dir()).map_err(|err| format!("create data/logs: {err}"))?;
    write_csv_file(
        &csv_path(&session.csv_stem, "market"),
        &market_csv_header(session),
        &[market_csv_row(session)],
        false,
    )?;
    write_csv_file(
        &csv_path(&session.csv_stem, "firms"),
        &firm_csv_header(session),
        &[firm_csv_row(session)],
        false,
    )?;
    write_csv_file(
        &csv_path(&session.csv_stem, "trades"),
        &trade_csv_header(session),
        &[trade_csv_row(session, report)],
        false,
    )?;
    Ok(())
}

fn write_csv_file(
    path: &Path,
    header: &str,
    rows: &[String],
    reset: bool,
) -> Result<(), String> {
    if reset {
        let mut text = String::from(header);
        text.push('\n');
        for row in rows {
            text.push_str(row);
            text.push('\n');
        }
        return fs::write(path, text).map_err(|err| format!("write {}: {err}", path.display()));
    }
    let need_header = match fs::read_to_string(path) {
        Ok(text) if text.is_empty() => true,
        Ok(text) => {
            let existing = text.lines().next().unwrap_or("");
            if existing != header {
                return Err(format!(
                    "{} header does not match (old layout?). Run `csv reset` or `csv <newname>`.",
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("csv")
                ));
            }
            false
        }
        Err(_) => true,
    };
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| format!("open {}: {err}", path.display()))?;
    if need_header {
        writeln!(file, "{header}").map_err(|err| format!("write {}: {err}", path.display()))?;
    }
    for row in rows {
        writeln!(file, "{row}").map_err(|err| format!("write {}: {err}", path.display()))?;
    }
    Ok(())
}

fn csv_num(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        String::new()
    }
}

fn market_csv_goods(session: &Session) -> Vec<usize> {
    let mut ids: Vec<usize> = PREFAB_GOODS.iter().map(|good| good.id).collect();
    let mut extra: Vec<usize> = session
        .market
        .goods
        .keys()
        .copied()
        .filter(|id| !ids.contains(id))
        .collect();
    extra.sort_unstable();
    ids.extend(extra);
    ids
}

fn csv_good_col(id: usize) -> String {
    fmt_good(id)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn market_csv_header(session: &Session) -> String {
    let mut cols = vec!["day".to_string()];
    for id in market_csv_goods(session) {
        let name = csv_good_col(id);
        for field in MARKET_CSV_FIELDS {
            cols.push(format!("{name}_{field}"));
        }
    }
    cols.join(",")
}

fn market_csv_row(session: &Session) -> String {
    let mut cells = vec![session.day.to_string()];
    for id in market_csv_goods(session) {
        match session.market.goods.get(&id) {
            Some(row) => {
                cells.push(csv_num(row.amv));
                cells.push(csv_num(row.salability));
                if row.purchased > 0.0 {
                    cells.push(csv_num(row.average_price));
                } else {
                    cells.push(String::new());
                }
            }
            None => {
                for _ in MARKET_CSV_FIELDS {
                    cells.push(String::new());
                }
            }
        }
    }
    cells.join(",")
}

fn csv_actor_col(actor: Actor) -> String {
    fmt_actor(actor)
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn firm_csv_header(session: &Session) -> String {
    let mut cols = vec!["day".to_string()];
    let goods = market_csv_goods(session);
    for firm in &session.firms {
        let firm_name = csv_actor_col(Actor::Firm(firm.id));
        for field in FIRM_RECORD_FIELDS {
            cols.push(format!("{firm_name}_{field}"));
        }
        for id in &goods {
            let good_name = csv_good_col(*id);
            for field in FIRM_CSV_FIELDS {
                cols.push(format!("{firm_name}_{good_name}_{field}"));
            }
        }
    }
    cols.join(",")
}

fn firm_csv_row(session: &Session) -> String {
    let mut cells = vec![session.day.to_string()];
    let goods = market_csv_goods(session);
    for firm in &session.firms {
        cells.push(csv_num(firm.records.confidence));
        cells.push(csv_num(firm.records.profit_ratio));
        cells.push(csv_num(firm.records.sell_success));
        for id in &goods {
            match firm.property.get(id) {
                Some(row) => {
                    let market_amv = session.history.price(*id);
                    let mid = row.mid_amv(market_amv);
                    cells.push(csv_num(row.quantity));
                    cells.push(csv_num(row.sell_target));
                    cells.push(csv_num(row.amv_target));
                    cells.push(csv_num(row.bid_amv(mid)));
                    cells.push(csv_num(row.ask_amv(mid)));
                    cells.push(csv_num(row.average_cost));
                    cells.push(csv_num(row.average_price));
                    cells.push(csv_num(row.sold));
                    cells.push(csv_num(row.produced));
                }
                None => {
                    for _ in FIRM_CSV_FIELDS {
                        cells.push(String::new());
                    }
                }
            }
        }
    }
    cells.join(",")
}

fn trade_csv_header(session: &Session) -> String {
    let mut cols = vec!["day".to_string()];
    for id in market_csv_goods(session) {
        let name = csv_good_col(id);
        for field in TRADE_CSV_FIELDS {
            cols.push(format!("{name}_{field}"));
        }
    }
    cols.join(",")
}

struct DayCandle {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    seen: bool,
}

impl DayCandle {
    fn new() -> Self {
        Self {
            open: 0.0,
            high: 0.0,
            low: 0.0,
            close: 0.0,
            volume: 0.0,
            seen: false,
        }
    }

    fn push(&mut self, unit_amv: f64, qty: f64) {
        if !unit_amv.is_finite() || qty <= 0.0 {
            return;
        }
        if !self.seen {
            self.open = unit_amv;
            self.high = unit_amv;
            self.low = unit_amv;
            self.close = unit_amv;
            self.volume = qty;
            self.seen = true;
        } else {
            self.high = self.high.max(unit_amv);
            self.low = self.low.min(unit_amv);
            self.close = unit_amv;
            self.volume += qty;
        }
    }
}

fn trade_csv_row(session: &Session, report: &MarketDayReport) -> String {
    let goods = market_csv_goods(session);
    let mut candles: HashMap<usize, DayCandle> = goods
        .iter()
        .map(|id| (*id, DayCandle::new()))
        .collect();
    for meeting in &report.meetings {
        let MeetingOutcome::Traded { goods: basket, .. } = &meeting.outcome else {
            continue;
        };
        let sought = meeting.buy.target;
        let qty = basket.get(&sought).copied().unwrap_or(0.0).abs();
        let pay_amv: f64 = basket
            .iter()
            .filter(|(id, q)| **id != sought && **q > 0.0)
            .map(|(id, q)| *q * session.history.price(*id))
            .sum();
        if qty <= 0.0 {
            continue;
        }
        let unit_amv = pay_amv / qty;
        if let Some(candle) = candles.get_mut(&sought) {
            candle.push(unit_amv, qty);
        }
    }
    let mut cells = vec![session.day.to_string()];
    for id in goods {
        match candles.get(&id) {
            Some(candle) if candle.seen => {
                cells.push(csv_num(candle.open));
                cells.push(csv_num(candle.high));
                cells.push(csv_num(candle.low));
                cells.push(csv_num(candle.close));
                cells.push(csv_num(candle.volume));
            }
            _ => {
                cells.push(String::new());
                cells.push(String::new());
                cells.push(String::new());
                cells.push(String::new());
                cells.push(csv_num(0.0));
            }
        }
    }
    cells.join(",")
}

/// Caps coin save/shop at [`COIN_SAVE_UNITS`] so leftover coin stays excess.
fn cap_coin_save(pop: &mut Pop) {
    if let Some(row) = pop.property.get_mut(&COIN) {
        row.save_target = COIN_SAVE_UNITS;
        row.shop_target = COIN_SAVE_UNITS;
    }
}

fn day_digest(
    session: &Session,
    report: &MarketDayReport,
    wages: &[(usize, WagePayout)],
) -> String {
    let n_trade = report
        .meetings
        .iter()
        .filter(|m| matches!(m.outcome, MeetingOutcome::Traded { .. }))
        .count();
    let n_wash = report.meetings.len() - n_trade;
    let wage_coin: f64 = wages
        .iter()
        .map(|(_, p)| p.owner_amount + p.worker_amount)
        .sum();
    let sol: f64 = if session.pops.is_empty() {
        0.0
    } else {
        session
            .pops
            .iter()
            .map(|pop| pop.records.living_standard)
            .sum::<f64>()
            / session.pops.len() as f64
    };
    let conf: f64 = if session.firms.is_empty() {
        0.0
    } else {
        session
            .firms
            .iter()
            .map(|firm| firm.records.confidence)
            .sum::<f64>()
            / session.firms.len() as f64
    };
    format!(
        "{} trade{}  {} wash{}  wages {} coin  SOL {}  conf {}",
        n_trade,
        if n_trade == 1 { "" } else { "s" },
        n_wash,
        if n_wash == 1 { "" } else { "es" },
        fmt_qty(wage_coin),
        fmt_num(sol),
        fmt_num(conf)
    )
}

fn format_day_report(
    session: &Session,
    report: &MarketDayReport,
    wages: &[(usize, WagePayout)],
) -> String {
    let n_trade = report
        .meetings
        .iter()
        .filter(|m| matches!(m.outcome, MeetingOutcome::Traded { .. }))
        .count();
    let n_wash = report.meetings.len() - n_trade;
    let haul: f64 = report
        .meetings
        .iter()
        .map(|m| match &m.outcome {
            MeetingOutcome::Traded { transport_needed, .. } => *transport_needed,
            MeetingOutcome::Wash { transport, .. } => *transport,
        })
        .sum();
    let has_cargo = session
        .factuals
        .goods
        .values()
        .any(|good| good.is_transport());

    let mut out = String::new();
    out.push_str(&format!("=== market day {} ===\n", session.day));
    out.push_str(&format!(
        "{} trade{}   {} wash{}   {} unmatched   leftover {} buy / {} sell\n",
        n_trade,
        if n_trade == 1 { "" } else { "s" },
        n_wash,
        if n_wash == 1 { "" } else { "es" },
        report.unmatched_buys.len(),
        report.leftover_buys.len(),
        report.leftover_sells.len(),
    ));
    if has_cargo {
        out.push_str(&format!("haul  {}\n", fmt_qty(haul)));
    } else {
        out.push_str("no transport-tagged goods; haul skipped\n");
    }
    out.push_str(&format!("{:-<64}\n", ""));

    if !report.unmatched_buys.is_empty() {
        out.push_str("\nUnmatched  (no seller)\n");
        out.push_str(&format!("  {:<10}  {:>6} {}\n", "buyer", "qty", "good"));
        out.push_str(&format!("  {:-<10}  {:-<6} {:-<8}\n", "", "", ""));
        for order in &report.unmatched_buys {
            out.push_str(&format!(
                "  {:<10}  {:>6} {}\n",
                fmt_actor(order.origin),
                fmt_qty(order.target_amount.abs()),
                fmt_good(order.target)
            ));
        }
    }

    out.push_str("\nTrades\n");
    let trades: Vec<_> = report
        .meetings
        .iter()
        .filter(|m| matches!(m.outcome, MeetingOutcome::Traded { .. }))
        .collect();
    if trades.is_empty() {
        out.push_str("  (none)\n");
    } else {
        out.push_str(&format!(
            "  {:<10}  {:>6} {:<8} | {:<10}  {}\n",
            "buyer", "qty", "good", "seller", "pays"
        ));
        out.push_str(&format!(
            "  {:-<10}  {:-<6} {:-<8}-+-{:-<10}  {:-<16}\n",
            "", "", "", "", ""
        ));
        for meeting in trades {
            let MeetingOutcome::Traded {
                goods,
                transport_needed,
            } = &meeting.outcome
            else {
                continue;
            };
            let mut line = format!(
                "  {:<10}  {:>6} {:<8} | {:<10}  {}",
                fmt_actor(meeting.buy.origin),
                fmt_qty(bought_qty(goods, meeting.buy.target)),
                fmt_good(meeting.buy.target),
                fmt_actor(meeting.sell.origin),
                fmt_payment(goods, meeting.buy.target)
            );
            if has_cargo && *transport_needed > 0.0 {
                line.push_str(&format!("  haul {}", fmt_qty(*transport_needed)));
            }
            out.push_str(&line);
            out.push('\n');
        }
    }

    out.push_str("\nWashes\n");
    let washes = group_washes(&report.meetings);
    if washes.is_empty() {
        out.push_str("  (none)\n");
    } else {
        out.push_str(&format!(
            "  {:<10}  {:<8} | {:<10}  {:<14}  {}\n",
            "buyer", "good", "seller", "why", "end"
        ));
        out.push_str(&format!(
            "  {:-<10}  {:-<8}-+-{:-<10}  {:-<14}  {:-<10}\n",
            "", "", "", "", ""
        ));
        for group in washes {
            out.push_str(&format!(
                "  {:<10}  {:<8} | {:<10}  {:<14}  x{} {}\n",
                fmt_actor(group.buyer),
                fmt_good(group.good),
                fmt_actor(group.seller),
                fmt_wash_reason(group.reason),
                group.count,
                if group.closed { "closed" } else { "open" }
            ));
        }
    }

    if !report.leftover_buys.is_empty() {
        out.push_str("\nLeftover buys\n");
        out.push_str(&format!("  {:<10}  {:>6} {}\n", "buyer", "qty", "good"));
        out.push_str(&format!("  {:-<10}  {:-<6} {:-<8}\n", "", "", ""));
        fmt_compact_orders(&mut out, &report.leftover_buys);
    }
    if !report.leftover_sells.is_empty() {
        out.push_str("\nLeftover sells\n");
        out.push_str(&format!("  {:<10}  {:>6} {}\n", "seller", "qty", "good"));
        out.push_str(&format!("  {:-<10}  {:-<6} {:-<8}\n", "", "", ""));
        fmt_compact_orders(&mut out, &report.leftover_sells);
    }

    out.push_str("\nOutcomes\n");
    out.push_str(&format!(
        "  {:<8} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}\n",
        "good", "demand", "supply", "bought", "paid", "vol", "amv", "sal"
    ));
    out.push_str(&format!(
        "  {:-<8} {:-<7} {:-<7} {:-<7} {:-<7} {:-<7} {:-<7} {:-<7}\n",
        "", "", "", "", "", "", "", ""
    ));
    let mut ids: Vec<usize> = session.market.goods.keys().copied().collect();
    ids.sort_unstable();
    let mut any_row = false;
    for id in ids {
        let row = &session.market.goods[&id];
        if row.demand == 0.0
            && row.supply == 0.0
            && row.purchased == 0.0
            && row.payment == 0.0
        {
            continue;
        }
        any_row = true;
        out.push_str(&format!(
            "  {:<8} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}\n",
            fmt_good(id),
            fmt_qty(row.demand),
            fmt_qty(row.supply),
            fmt_qty(row.purchased),
            fmt_qty(row.payment),
            fmt_qty(row.volume()),
            fmt_qty(row.amv),
            fmt_qty(row.salability)
        ));
    }
    if !any_row {
        out.push_str("  (none)\n");
    }
    if !session.market.unavailable_goods.is_empty() {
        let mut names: Vec<String> = session
            .market
            .unavailable_goods
            .iter()
            .copied()
            .map(fmt_good)
            .collect();
        names.sort();
        out.push_str(&format!("unavailable  {}\n", names.join(", ")));
    }
    out.push('\n');
    out.push_str(&format_wage_report(session, wages));
    out.push_str(&format_production_report(session));
    out.push_str(&format_plan_report(session));
    out.push_str(&format_pop_report(session));
    out.push_str(&format_amv_trail(session));
    out.push_str("shop reloaded from current stock.\n");
    out.push_str(&csv_status_line(session));
    out
}

fn format_wage_report(session: &Session, wages: &[(usize, WagePayout)]) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Wages  (owners {:.0}% / workers {:.0}%, ceil, owners first)\n",
        session.factuals.config.labor.owner_share * 100.0,
        session.factuals.config.labor.worker_share * 100.0
    ));
    out.push_str(&format!(
        "  {:<10} {:>6} {:>7} {:>8}  {}\n",
        "firm", "till", "owners", "workers", "to"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<6} {:-<7} {:-<8}  {:-<24}\n",
        "", "", "", "", ""
    ));
    if wages.is_empty() {
        out.push_str("  (none)\n\n");
        return out;
    }
    for (firm_id, payout) in wages {
        let mut dest = Vec::new();
        if payout.owner_amount > 0.0 {
            if payout.owner_credited {
                dest.push(format!(
                    "owner {} {}",
                    fmt_actor(payout.owner),
                    fmt_qty(payout.owner_amount)
                ));
            } else if matches!(payout.owner, Actor::Pop(0)) {
                dest.push(format!("unowned {}", fmt_qty(payout.owner_amount)));
            } else {
                dest.push(format!(
                    "hyp {} {}",
                    fmt_actor(payout.owner),
                    fmt_qty(payout.owner_amount)
                ));
            }
        }
        for (pop_id, qty) in &payout.workers {
            dest.push(format!("{} {}", fmt_actor(Actor::Pop(*pop_id)), fmt_qty(*qty)));
        }
        if dest.is_empty() {
            dest.push("-".into());
        }
        out.push_str(&format!(
            "  {:<10} {:>6} {:>7} {:>8}  {}\n",
            fmt_actor(Actor::Firm(*firm_id)),
            fmt_qty(payout.coinage),
            fmt_qty(payout.owner_amount),
            fmt_qty(payout.worker_amount),
            dest.join(", ")
        ));
    }
    out.push('\n');
    out
}

fn format_production_report(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Production  (did today; want is next-day after plan)\n");
    out.push_str(&format!(
        "  {:<10} {:>6} {:>6}  {}\n",
        "firm", "did", "want", "flows / missing"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<6} {:-<6}  {:-<28}\n",
        "", "", "", ""
    ));
    if session.firms.is_empty() {
        out.push_str("  (none)\n\n");
        return out;
    }
    for firm in &session.firms {
        if firm.production_line.is_empty() {
            continue;
        }
        for (idx, line) in firm.production_line.iter().enumerate() {
            let want = line.target.unwrap_or(0.0);
            let mut bits: Vec<String> = Vec::new();
            // Property flows are firm-wide; print them once on the first line.
            if idx == 0 {
                let mut ids: Vec<usize> = firm.property.keys().copied().collect();
                ids.sort_unstable();
                for id in ids {
                    let row = &firm.property[&id];
                    if row.produced > 0.0 {
                        bits.push(format!("+{} {}", fmt_qty(row.produced), fmt_good(id)));
                    }
                    if row.consumed > 0.0 {
                        bits.push(format!("-{} {}", fmt_qty(row.consumed), fmt_good(id)));
                    }
                }
            }
            if !line.last_missing_goods.is_empty() {
                let missing: Vec<String> = line
                    .last_missing_goods
                    .iter()
                    .copied()
                    .map(fmt_good)
                    .collect();
                bits.push(format!("missing {}", missing.join(", ")));
            }
            if bits.is_empty() {
                bits.push("-".into());
            }
            out.push_str(&format!(
                "  {:<10} {:>6} {:>6}  {}\n",
                fmt_actor(Actor::Firm(firm.id)),
                fmt_qty(line.last_iterations),
                fmt_qty(want),
                bits.join("  ")
            ));
        }
    }
    out.push('\n');
    out
}

fn format_plan_report(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Plans  (after firm record keeping)\n");
    out.push_str(&format!(
        "  {:<10} {:>6} {:>7} {:>8} {:>6} {:>6} {:<8} {:>7}\n",
        "firm", "conf", "profit", "success", "want", "sell", "good", "quote"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<6} {:-<7} {:-<8} {:-<6} {:-<6} {:-<8} {:-<7}\n",
        "", "", "", "", "", "", "", ""
    ));
    if session.firms.is_empty() {
        out.push_str("  (none)\n\n");
        return out;
    }
    for firm in &session.firms {
        let want = firm
            .production_line
            .first()
            .and_then(|line| line.target)
            .unwrap_or(0.0);
        let (good_name, sell, quote) = match firm_primary_output(session, firm) {
            Some((good, row)) => (
                fmt_good(good),
                fmt_qty(row.sell_target),
                fmt_num(row.amv_target),
            ),
            None => ("-".into(), "-".into(), "-".into()),
        };
        out.push_str(&format!(
            "  {:<10} {:>6} {:>7} {:>8} {:>6} {:>6} {:<8} {:>7}\n",
            fmt_actor(Actor::Firm(firm.id)),
            fmt_num(firm.records.confidence),
            fmt_num(firm.records.profit_ratio),
            fmt_num(firm.records.sell_success),
            fmt_qty(want),
            sell,
            good_name,
            quote
        ));
    }
    out.push('\n');
    out
}

fn format_pop_report(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("Pops  (after consume)\n");
    out.push_str(&format!(
        "  {:<10} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}\n",
        "actor", "basic", "common", "luxury", "SOL", "shop", "income"
    ));
    out.push_str(&format!(
        "  {:-<10} {:-<6} {:-<6} {:-<6} {:-<6} {:-<6} {:-<6}\n",
        "", "", "", "", "", "", ""
    ));
    if session.pops.is_empty() {
        out.push_str("  (none)\n\n");
        return out;
    }
    for pop in &session.pops {
        out.push_str(&format!(
            "  {:<10} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}\n",
            fmt_actor(Actor::Pop(pop.id)),
            fmt_num(pop.records.tier_sat[0]),
            fmt_num(pop.records.tier_sat[1]),
            fmt_num(pop.records.tier_sat[2]),
            fmt_num(pop.records.living_standard),
            fmt_num(pop.records.shop_fill),
            fmt_qty(pop.records.income_amv)
        ));
    }
    out.push('\n');
    out
}

fn format_amv_trail(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("AMV trail  (old -> new)\n");
    out.push_str(&format!(
        "  {:<8} {:>7} {:>7}  {}\n",
        "good", "now", "diff", "trail"
    ));
    out.push_str(&format!(
        "  {:-<8} {:-<7} {:-<7}  {:-<16}\n",
        "", "", "", ""
    ));
    let mut ids: Vec<usize> = session.market.goods.keys().copied().collect();
    ids.sort_unstable();
    if ids.is_empty() {
        out.push_str("  (none)\n");
        return out;
    }
    for id in ids {
        let row = &session.market.goods[&id];
        out.push_str(&format!(
            "  {:<8} {:>7} {:>7}  {}\n",
            fmt_good(id),
            fmt_num(row.amv),
            fmt_amv_delta(row),
            fmt_amv_history(row)
        ));
    }
    out
}

fn fmt_amv_history(row: &MarketGood) -> String {
    let trail = row.amv_trail();
    if trail.is_empty() {
        return "-".into();
    }
    trail
        .iter()
        .map(|v| fmt_num(*v))
        .collect::<Vec<_>>()
        .join("  ")
}

fn fmt_amv_delta(row: &MarketGood) -> String {
    let trail = row.amv_trail();
    if trail.len() < 2 {
        return "-".into();
    }
    let d = trail[trail.len() - 1] - trail[trail.len() - 2];
    if d.abs() < 1e-12 {
        "0".into()
    } else if d > 0.0 {
        format!("+{}", fmt_num(d))
    } else {
        fmt_num(d)
    }
}

struct WashGroup {
    buyer: Actor,
    good: usize,
    seller: Actor,
    reason: WashReason,
    count: usize,
    closed: bool,
}

fn group_washes(meetings: &[MarketMeeting]) -> Vec<WashGroup> {
    let mut groups: Vec<WashGroup> = Vec::new();
    for meeting in meetings {
        let MeetingOutcome::Wash {
            reason,
            closed,
            ..
        } = meeting.outcome
        else {
            continue;
        };
        if let Some(group) = groups.iter_mut().find(|group| {
            group.buyer == meeting.buy.origin
                && group.good == meeting.buy.target
                && group.seller == meeting.sell.origin
                && group.reason == reason
        }) {
            group.count += 1;
            group.closed = closed;
        } else {
            groups.push(WashGroup {
                buyer: meeting.buy.origin,
                good: meeting.buy.target,
                seller: meeting.sell.origin,
                reason,
                count: 1,
                closed,
            });
        }
    }
    groups
}

fn bought_qty(goods: &HashMap<usize, f64>, target: usize) -> f64 {
    goods.get(&target).copied().unwrap_or(0.0).abs()
}

fn fmt_payment(goods: &HashMap<usize, f64>, target: usize) -> String {
    let mut ids: Vec<usize> = goods
        .iter()
        .filter(|(id, qty)| **id != target && **qty > 0.0)
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    if ids.is_empty() {
        return "-".into();
    }
    ids.into_iter()
        .map(|id| format!("{} {}", fmt_qty(goods[&id]), fmt_good(id)))
        .collect::<Vec<_>>()
        .join(" + ")
}

fn fmt_compact_orders(out: &mut String, orders: &[MarketOrder]) {
    for order in orders {
        out.push_str(&format!(
            "  {:<10}  {:>6} {}\n",
            fmt_actor(order.origin),
            fmt_qty(order.target_amount.abs()),
            fmt_good(order.target)
        ));
    }
}

fn fmt_wash_reason(reason: WashReason) -> &'static str {
    match reason {
        WashReason::NoProposal => "no proposal",
        WashReason::Rejected => "rejected",
        WashReason::EmptyFill => "empty fill",
    }
}

fn fmt_qty(x: f64) -> String {
    if x.is_finite() && (x - x.round()).abs() < 1e-9 && x.abs() < 1e12 {
        format!("{:.0}", x.round())
    } else {
        format!("{x:.2}")
    }
}

fn coincidence(buy: &MarketOrder, sell: &MarketOrder) -> bool {
    match (buy.counter_offer, sell.counter_offer) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

fn actor_label(actor: Actor) -> Option<&'static str> {
    PREFAB_ACTORS
        .iter()
        .find(|a| a.actor == actor)
        .map(|a| a.name)
}

fn good_label(id: usize) -> Option<&'static str> {
    PREFAB_GOODS.iter().find(|g| g.id == id).map(|g| g.name)
}

fn fmt_actor_kind_id(actor: Actor) -> String {
    match actor {
        Actor::Pop(id) => format!("pop {id}"),
        Actor::Firm(id) => format!("firm {id}"),
        Actor::Institution(id) => format!("inst {id}"),
        Actor::State(id) => format!("state {id}"),
    }
}

fn fmt_actor(actor: Actor) -> String {
    match actor_label(actor) {
        Some(name) => name.to_string(),
        None => fmt_actor_kind_id(actor),
    }
}

fn fmt_good(id: usize) -> String {
    match good_label(id) {
        Some(name) => name.to_string(),
        None => format!("#{id}"),
    }
}

fn fmt_order(session: &Session, order: &MarketOrder) -> String {
    format!(
        "{} {} {} amt {} prio {} amv {} bound {} counter {}",
        order_kind(order),
        fmt_actor(order.origin),
        fmt_good(order.target),
        fmt_num(order.target_amount),
        fmt_num(order.priority),
        amv_cell(order),
        order_bound_cell(session, order),
        counter_cell(order)
    )
}

fn order_kind(order: &MarketOrder) -> &'static str {
    if order.is_request_order() {
        "request"
    } else if order.is_buy_order() {
        "buy"
    } else if order.is_offer_order() {
        "offer"
    } else if order.is_sell_order() {
        "sell"
    } else {
        "order"
    }
}

fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e12 {
        format!("{x:.0}")
    } else {
        format!("{x:.4}")
    }
}

fn counter_cell(order: &MarketOrder) -> String {
    match (order.counter_offer, order.counter_offer_amount) {
        (Some(good), Some(amt)) => format!("{} {}", fmt_good(good), fmt_num(amt)),
        _ => "-".into(),
    }
}

fn amv_cell(order: &MarketOrder) -> String {
    match order.amv_target {
        Some(amv) => fmt_num(amv),
        None => "-".into(),
    }
}

fn fmt_bound(bound: FirmAmvBound) -> String {
    match bound {
        FirmAmvBound::None => "-".into(),
        FirmAmvBound::Minimum(v) => format!("min {}", fmt_num(v)),
        FirmAmvBound::Maximum(v) => format!("max {}", fmt_num(v)),
        FirmAmvBound::MinMax(min, max) => {
            format!("min {} max {}", fmt_num(min), fmt_num(max))
        }
    }
}

fn order_bound_cell(session: &Session, order: &MarketOrder) -> String {
    let Actor::Firm(id) = order.origin else {
        return "-".into();
    };
    let Some(firm) = session.firms.iter().find(|f| f.id == id) else {
        return "-".into();
    };
    match firm.property.get(&order.target) {
        Some(row) => fmt_bound(row.amv_bound),
        None => "-".into(),
    }
}

fn order_header() -> String {
    format!(
        "{:>2}  {:<7}  {:<10}  {:<8}  {:>8}  {:>8}  {:>6}  {:<16}  {}",
        "#", "kind", "actor", "good", "amt", "prio", "amv", "bound", "counter"
    )
}

fn order_rule() -> String {
    format!(
        "{:-<2}  {:-<7}  {:-<10}  {:-<8}  {:-<8}  {:-<8}  {:-<6}  {:-<16}  {:-<16}",
        "", "", "", "", "", "", "", "", ""
    )
}

fn order_row(session: &Session, idx: usize, order: &MarketOrder) -> String {
    format!(
        "{:>2}  {:<7}  {:<10}  {:<8}  {:>8}  {:>8}  {:>6}  {:<16}  {}",
        idx,
        order_kind(order),
        fmt_actor(order.origin),
        fmt_good(order.target),
        fmt_num(order.target_amount),
        fmt_num(order.priority),
        amv_cell(order),
        order_bound_cell(session, order),
        counter_cell(order)
    )
}

// --- living roster ----------------------------------------------------------

fn world_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world")
}

fn build_world() -> (Vec<Pop>, Vec<Firm>, Factuals, MarketHistory) {
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

fn set_quote(history: &mut MarketHistory, good: usize, amv: f64, salability: f64) {
    history.prices.insert(good, amv);
    history.salability.insert(good, salability);
}

fn consume_target(good: usize) -> DesireTarget {
    DesireTarget::new(good, DesireTargetType::Consume, 1.0)
}

fn consume_target_eff(good: usize, eff: f64) -> DesireTarget {
    DesireTarget::new(good, DesireTargetType::Consume, eff)
}

fn make_desire(id: usize, good: usize, amount: f64) -> Desire {
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
fn make_food_desire(id: usize, amount: f64) -> Desire {
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
fn with_need_spread(mut pop: Pop) -> Pop {
    pop.desires[0].push(make_food_desire(0, 8.0));
    pop.desires[0].push(make_desire(1, WATER, 6.0));
    pop.desires[1].push(make_desire(2, BREAD, 4.0));
    pop
}

fn empty_pop(id: usize, pop_cfg: &PopConfig) -> Pop {
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

fn make_farmers_pop(pop_cfg: &PopConfig) -> Pop {
    let mut pop = with_need_spread(empty_pop(1, pop_cfg));
    // Grain surplus funds water/bread requests. No grain shop shortfall.
    pop.property.insert(GRAIN, PopPRow::new(24.0).with_target(4.0));
    pop.property.insert(WATER, PopPRow::new(1.0).with_target(6.0));
    pop.property.insert(BREAD, PopPRow::new(0.0).with_target(5.0));
    pop.property.insert(COIN, PopPRow::new(80.0));
    pop
}

fn make_laborers_pop(pop_cfg: &PopConfig) -> Pop {
    let mut pop = with_need_spread(empty_pop(2, pop_cfg));
    pop.property.insert(GRAIN, PopPRow::new(1.0).with_target(8.0));
    pop.property.insert(WATER, PopPRow::new(0.0).with_target(6.0));
    pop.property.insert(BREAD, PopPRow::new(0.0).with_target(4.0));
    pop.property.insert(COIN, PopPRow::new(160.0));
    pop
}

fn make_townsfolk_pop(pop_cfg: &PopConfig) -> Pop {
    let mut pop = with_need_spread(empty_pop(3, pop_cfg));
    pop.property.insert(GRAIN, PopPRow::new(4.0).with_target(6.0));
    pop.property.insert(WATER, PopPRow::new(2.0).with_target(4.0));
    pop.property.insert(BREAD, PopPRow::new(1.0).with_target(6.0));
    pop.property.insert(COIN, PopPRow::new(400.0));
    pop
}

/// One-household owner. Staples stay small; jewelry is the luxury sink.
/// Starting AMV is about 20x townsfolk wealth per household (~4.7 -> ~93).
fn make_lord_pop(pop_cfg: &PopConfig) -> Pop {
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

fn with_worker(mut firm: Firm, pop_id: usize) -> Firm {
    let mut w = Workforce::empty();
    w.id = pop_id;
    w.workers = (10.0, 10.0);
    w.hours = 1.0;
    firm.workforce.push(w);
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

fn make_farm() -> Firm {
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

fn make_bakery() -> Firm {
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

fn make_mine() -> Firm {
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

fn make_jeweler() -> Firm {
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

fn make_well() -> Firm {
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
