//! CLI box for probing a market day.
//!
//! Startup loads goods from `data/world/goods.toml`, builds a small living
//! roster (3 pops, 6 producer firms) and a price/salability snapshot, then
//! loads books from
//! [`Pop::create_orders`] / [`Firm::create_orders`]. `day` runs
//! [`Market::run_market_day`] on those actors and prints a summary plus
//! the AMV trail. `amv` reprints that trail. `match` is still a read-only
//! matcher pass on the on-screen books.
//!
//! ```text
//! cargo run --example market_tester
//! ```
//!
//! ```text
//!   shop
//!   day
//!   amv
//!   match
//!   request laborers grain 3
//! ```

use std::collections::{HashMap, HashSet};
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use hexx::Hex;
use rand::rngs::StdRng;
use rand::SeedableRng;
use simpler_economy::game::actor::Actor;
use simpler_economy::game::config::market_priority;
use simpler_economy::game::desire::{Desire, DesireSource, DesireTarget, DesireTargetType};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::firm::{Firm, FirmAmvBound, FirmPRow, ProductionLine};

use simpler_economy::game::household::Household;
use simpler_economy::game::market::{
    Market, MarketDayReport, MarketGood, MarketHistory, MarketMeeting, MeetingOutcome, WashReason,
};
use simpler_economy::game::marketorder::{compose_sell_priority, MarketOrder};
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

const PREFAB_GOODS: &[NamedGood] = &[
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
    NamedActor { actor: Actor::Firm(1), name: "farm" },
    NamedActor { actor: Actor::Firm(2), name: "bakery" },
    NamedActor { actor: Actor::Firm(3), name: "mine" },
    NamedActor { actor: Actor::Firm(4), name: "mint" },
    NamedActor { actor: Actor::Firm(5), name: "jeweler" },
    NamedActor { actor: Actor::Firm(6), name: "well" },
];

/// Intended buy/sell roles for the roster table (not live order amounts).
const ROSTER: &[(&str, &str, &str)] = &[
    ("farmers", "water, bread, jewelry", "-"),
    ("laborers", "grain, water, bread, jewelry", "-"),
    ("townsfolk", "grain, water, bread, jewelry", "-"),
    ("farm", "water", "grain"),
    ("bakery", "grain", "bread"),
    ("mine", "-", "gold"),
    ("mint", "gold", "coin"),
    ("jeweler", "gold", "jewelry"),
    ("well", "-", "water"),
];

struct Session {
    buys: Vec<MarketOrder>,
    sells: Vec<MarketOrder>,
    rng: StdRng,
    seed: Option<u64>,
    log: String,
    /// When true, the screen is the last log (a day report) instead of the roster.
    focus_log: bool,
    pops: Vec<Pop>,
    firms: Vec<Firm>,
    factuals: Factuals,
    history: MarketHistory,
    market: Market,
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
        pops,
        firms,
        factuals,
        history,
        market,
    };
    session.log = shop_from_actors(&mut session);

    let tty = io::stdout().is_terminal();
    if tty {
        draw_ui(&session);
    } else {
        println!("=== market tester ===");
        println!("Living roster loaded via create_orders. Type help for commands.");
        println!("`day` runs a full market day. `match` is read-only (books stay put).\n");
        print_legend(&session);
        println!();
        list_books(&session);
        println!();
        println!("{}", session.log.trim_end());
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
    println!("=== market tester ===");
    if session.focus_log {
        println!("{}", rng_line(session));
        println!("shop / cls  restores the roster.");
        println!();
        println!("{}", session.log.trim_end());
        return;
    }
    print_legend(session);
    println!();
    list_books(session);
    if !session.log.is_empty() {
        println!();
        println!("---");
        println!("{}", session.log.trim_end());
    }
}

enum CmdResult {
    Continue(String),
    Quit,
}

fn handle_line(session: &mut Session, line: &str) -> CmdResult {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let cmd = tokens[0].to_ascii_lowercase();
    let rest = &tokens[1..];
    session.focus_log = cmd == "day" || cmd == "d";
    let msg = match cmd.as_str() {
        "help" | "?" | "h" => help_text(),
        "legend" | "ids" | "prefabs" | "roster" | "cls" | "list" | "ls" | "l" => {
            "header already shows roster, goods, and books.".into()
        }
        "quit" | "exit" | "q" => return CmdResult::Quit,
        "clear" => {
            session.buys.clear();
            session.sells.clear();
            "books cleared.".into()
        }
        "shop" => shop_from_actors(session),
        "day" | "d" => run_day(session),
        "amv" | "prices" => format_amv_trail(session).trim_end().to_string(),
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
        "request" | "req" => match parse_simple_order(rest, true) {
            Ok(order) => add_buy(session, order),
            Err(err) => err,
        },
        "offer" => match parse_simple_order(rest, false) {
            Ok(order) => add_sell(session, order),
            Err(err) => err,
        },
        "buy" => match parse_exchange_order(rest, true) {
            Ok(order) => add_buy(session, order),
            Err(err) => err,
        },
        "sell" => match parse_exchange_order(rest, false) {
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

fn print_legend(session: &Session) {
    println!("goods  (id  name  amv  sal  trail old->new)");
    println!("  --  --------  ------  ----  ----------------");
    for good in PREFAB_GOODS {
        let amv = session.history.price(good.id);
        let sal = session.history.salability(good.id);
        let trail = session
            .market
            .goods
            .get(&good.id)
            .map(fmt_amv_history)
            .unwrap_or_else(|| "-".into());
        println!(
            "  {:>2}  {:<8}  {:>6}  {:>4}  {}",
            good.id,
            good.name,
            fmt_num(amv),
            fmt_num(sal),
            trail
        );
    }
    println!();
    println!("roster  (pops request only; firms buy/sell from create_orders)");
    println!("  {:<10}  {:<32} | {}", "actor", "buying", "selling");
    println!("  {:-<10}  {:-<32}-+-{:-<16}", "", "", "");
    for (name, buying, selling) in ROSTER {
        println!("  {:<10}  {:<32} | {}", name, buying, selling);
    }
    println!();
    print_firm_bounds(session);
    println!();
    println!("Type a name, or kind+id / raw good id.  shop  reloads actor orders.");
    println!("  day  runs a full market day on the living roster.");
    println!("  request laborers grain 3");
    println!();
    println!("Buy order priority: lower goes first. Defaults:");
    println!("  firm 2.5 (producer)   pop 4");
    println!("Sell/offer priority: higher is more likely. Default is compose_sell_priority.");
}

fn print_firm_bounds(session: &Session) {
    println!("firm bounds  (min = sell floor, max = buy cap)");
    println!("  shop skips a buy when market AMV is already above max.");
    println!("  {:<10}  {:<8}  {}", "actor", "good", "bound");
    println!("  {:-<10}  {:-<8}  {:-<16}", "", "", "");
    let mut any = false;
    for firm in &session.firms {
        let mut rows: Vec<_> = firm.property.iter().collect();
        rows.sort_by_key(|(id, _)| *id);
        for (&good, row) in rows {
            if row.amv_bound == FirmAmvBound::None {
                continue;
            }
            any = true;
            println!(
                "  {:<10}  {:<8}  {}",
                fmt_actor(Actor::Firm(firm.id)),
                fmt_good(good),
                fmt_bound(row.amv_bound)
            );
        }
    }
    if !any {
        println!("  (none)");
    }
}

fn help_text() -> String {
    "\
commands
  shop                  reload books from pop and firm create_orders
  day                   run a full market day on the living roster
  amv                   print AMV trail (old -> new)
  request <actor> <good> <amount> [priority]
  offer   <actor> <good> <amount> [priority]
  buy     <actor> <good> <amount> <amv> <pay-good> <pay-amount> [priority]
  sell    <actor> <good> <amount> <amv> <want-good> <want-amount> [priority]
  match                 one match_orders pass; does not remove anything
  drop buy <i>          remove buy at list index
  drop sell <i>
  seed <n>              deterministic rng from n
  unseed                os rng again
  clear                 empty the books (not the screen)
  cls                   redraw
  help
  quit

The screen clears and redraws after each command. Empty enter also redraws.
Startup runs shop once. Pops emit requests; firms emit buy/sell/offer.
`day` collects from the current actors, settles, and prints a summary.
On-screen books are cleared after a day (inventory has moved).
Firm rows may carry an AMV bound (min sell floor / max buy cap). create_orders
clamps order AMV to that bound and skips buys when market AMV is above max.
actor: prefab name (farmers, bakery, ...) or kind id (pop 1, firm 2)
good:  prefab name (grain, coin, jewelry) or id (1, 5, 6)
amounts: type positives. request/buy store +amount, offer/sell store -amount.

examples
  shop
  day
  amv
  match
  request laborers grain 3
  offer farm grain 4
  buy bakery grain 5 1.0 coin 5
  sell farm grain 5 1.0 coin 5"
        .into()
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
fn parse_simple_order(rest: &[&str], is_buy: bool) -> Result<MarketOrder, String> {
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
                default_buy_priority(actor)
            } else {
                compose_sell_priority(default_buy_priority(actor), amount, 0.0)
            }
        }
    };
    tok.expect_empty()?;
    check_priority(actor, priority, is_buy)?;
    if is_buy {
        Ok(MarketOrder::request_order(actor, good, amount, priority))
    } else {
        Ok(MarketOrder::offer_order(actor, good, -amount, priority))
    }
}

/// buy / sell: actor good amount amv other-good other-amount [priority]
fn parse_exchange_order(rest: &[&str], is_buy: bool) -> Result<MarketOrder, String> {
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
                default_buy_priority(actor)
            } else {
                compose_sell_priority(default_buy_priority(actor), amount, 0.0)
            }
        }
    };
    tok.expect_empty()?;
    check_priority(actor, priority, is_buy)?;
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

fn default_buy_priority(actor: Actor) -> f64 {
    match actor {
        Actor::Pop(_) => market_priority::POP_START,
        Actor::Firm(_) => market_priority::FIRM_PRODUCER,
        Actor::Institution(_) => market_priority::INSTITUTION_BEFORE_FIRMS,
        Actor::State(_) => market_priority::STATE_FIRST,
    }
}

fn check_priority(actor: Actor, priority: f64, is_buy: bool) -> Result<(), String> {
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
            if !(market_priority::POP_START..market_priority::POP_END).contains(&priority) {
                return Err(format!(
                    "pop buy priority must be in [{}, {})",
                    market_priority::POP_START,
                    market_priority::POP_END
                ));
            }
        }
        Actor::Firm(_) => {
            if !(market_priority::FIRM_MERCHANT_START..market_priority::FIRM_PRODUCER_END)
                .contains(&priority)
            {
                return Err(format!(
                    "firm buy priority must be in [{}, {})",
                    market_priority::FIRM_MERCHANT_START,
                    market_priority::FIRM_PRODUCER_END
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
    match session.seed {
        Some(s) => format!("rng: seed {s}"),
        None => "rng: os".into(),
    }
}

fn list_books(session: &Session) {
    println!("{}", rng_line(session));
    println!();
    print_order_table(session, "buys  (priority, lowest first)", &session.buys);
    println!();
    print_order_table(session, "sells  (target good id)", &session.sells);
}

fn print_order_table(session: &Session, title: &str, orders: &[MarketOrder]) {
    println!("{title}");
    println!("{}", order_header());
    println!("{}", order_rule());
    if orders.is_empty() {
        println!("  (empty)");
    } else {
        for (i, order) in orders.iter().enumerate() {
            println!("{}", order_row(session, i, order));
        }
    }
}

fn run_match(session: &mut Session) -> String {
    let batch = Market::match_orders(&session.buys, &session.sells, &mut session.rng);
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

fn run_day(session: &mut Session) -> String {
    let mut pops: HashMap<usize, Pop> = session.pops.drain(..).map(|pop| (pop.id, pop)).collect();
    let mut firms: HashMap<usize, Firm> =
        session.firms.drain(..).map(|firm| (firm.id, firm)).collect();
    let report = session.market.run_market_day(
        &session.factuals,
        &mut pops,
        &mut firms,
        &mut session.rng,
    );
    session.pops = pops.into_values().collect();
    session.pops.sort_by_key(|pop| pop.id);
    session.firms = firms.into_values().collect();
    session.firms.sort_by_key(|firm| firm.id);
    session.buys.clear();
    session.sells.clear();
    session.history = session.market.history();
    format_day_report(session, &report)
}

fn format_day_report(session: &Session, report: &MarketDayReport) -> String {
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
    out.push_str("=== market day ===\n");
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
    out.push_str(&format_amv_trail(session));
    out.push_str("shop  reloads books from current stock.");
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

fn world_goods_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world/goods.toml")
}

fn build_world() -> (Vec<Pop>, Vec<Firm>, Factuals, MarketHistory) {
    let factuals = Factuals::load_from_path(world_goods_path())
        .unwrap_or_else(|err| panic!("load {}: {err}", world_goods_path().display()));

    let mut history = MarketHistory::default();
    // AMV spread: staples cheap, metals dear, jewelry dearest.
    // Coins are money (sal 1.0); jewelry is liquid-ish (0.8); rest stay below
    // the 0.6 exchange floor unless noted (gold 0.7 can be tender).
    set_quote(&mut history, GRAIN, 1.0, 0.50);
    set_quote(&mut history, WATER, 0.3, 0.35);
    set_quote(&mut history, BREAD, 2.2, 0.45);
    set_quote(&mut history, GOLD, 8.0, 0.70);
    set_quote(&mut history, COIN, 1.0, 1.00);
    set_quote(&mut history, JEWELRY, 15.0, 0.80);

    let pops = vec![
        make_farmers_pop(),
        make_laborers_pop(),
        make_townsfolk_pop(),
    ];
    let firms = vec![
        make_farm(),
        make_bakery(),
        make_mine(),
        make_mint(),
        make_jeweler(),
        make_well(),
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

/// Same desire spread on every pop: grain+water basic, bread common, jewelry luxury.
fn with_need_spread(mut pop: Pop) -> Pop {
    pop.desires[0].push(make_desire(0, GRAIN, 8.0));
    pop.desires[0].push(make_desire(1, WATER, 6.0));
    pop.desires[1].push(make_desire(2, BREAD, 4.0));
    pop.desires[2].push(make_desire(3, JEWELRY, 1.0));
    pop
}

fn empty_pop(id: usize) -> Pop {
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
        records: PopRecords::default(),
    }
}

fn make_farmers_pop() -> Pop {
    let mut pop = with_need_spread(empty_pop(1));
    // Grain surplus funds water/bread/jewelry requests. No grain shop shortfall.
    pop.property.insert(GRAIN, PopPRow::new(24.0).with_target(4.0));
    pop.property.insert(WATER, PopPRow::new(1.0).with_target(6.0));
    pop.property.insert(BREAD, PopPRow::new(0.0).with_target(5.0));
    pop.property.insert(JEWELRY, PopPRow::new(0.0).with_target(1.0));
    pop.property.insert(COIN, PopPRow::new(8.0));
    pop
}

fn make_laborers_pop() -> Pop {
    let mut pop = with_need_spread(empty_pop(2));
    pop.property.insert(GRAIN, PopPRow::new(1.0).with_target(8.0));
    pop.property.insert(WATER, PopPRow::new(0.0).with_target(6.0));
    pop.property.insert(BREAD, PopPRow::new(0.0).with_target(4.0));
    pop.property.insert(JEWELRY, PopPRow::new(0.0).with_target(1.0));
    pop.property.insert(COIN, PopPRow::new(16.0));
    pop
}

fn make_townsfolk_pop() -> Pop {
    let mut pop = with_need_spread(empty_pop(3));
    pop.property.insert(GRAIN, PopPRow::new(4.0).with_target(6.0));
    pop.property.insert(WATER, PopPRow::new(2.0).with_target(4.0));
    pop.property.insert(BREAD, PopPRow::new(1.0).with_target(6.0));
    pop.property.insert(JEWELRY, PopPRow::new(0.0).with_target(2.0));
    pop.property.insert(COIN, PopPRow::new(40.0));
    pop
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
    firm.production_line.push(dummy_line(1, 20.0, vec![WATER]));
    firm.property.insert(
        WATER,
        FirmPRow::new()
            .with_quantity(2.0)
            .with_purchase_target(8.0)
            .with_use_target(5.0)
            .with_stock_target(10.0)
            .with_amv_bound(FirmAmvBound::Maximum(1.0)),
    );
    firm.property.insert(
        GRAIN,
        FirmPRow::new()
            .with_quantity(30.0)
            .with_sell_target(20.0)
            .with_amv_target(1.0)
            .with_amv_bound(FirmAmvBound::Minimum(1.2)),
    );
    firm.property.insert(COIN, FirmPRow::new().with_quantity(6.0));
    firm
}

fn make_bakery() -> Firm {
    let mut firm = Firm::new(2, "bakery".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(2, 10.0, vec![GRAIN]));
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
    firm.property.insert(COIN, FirmPRow::new().with_quantity(8.0));
    firm
}

fn make_mine() -> Firm {
    let mut firm = Firm::new(3, "mine".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(3, 5.0, vec![]));
    firm.property.insert(
        GOLD,
        FirmPRow::new()
            .with_quantity(10.0)
            .with_sell_target(8.0)
            .with_amv_target(8.0)
            .with_amv_bound(FirmAmvBound::Minimum(6.0)),
    );
    firm.property.insert(COIN, FirmPRow::new().with_quantity(4.0));
    firm
}

fn make_mint() -> Firm {
    let mut firm = Firm::new(4, "mint".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(4, 10.0, vec![GOLD]));
    firm.property.insert(
        GOLD,
        FirmPRow::new()
            .with_quantity(2.0)
            .with_purchase_target(6.0)
            .with_use_target(5.0)
            .with_stock_target(8.0)
            .with_amv_bound(FirmAmvBound::Maximum(12.0)),
    );
    firm.property.insert(
        COIN,
        FirmPRow::new()
            .with_quantity(20.0)
            .with_sell_target(15.0)
            .with_amv_target(1.0)
            .with_amv_bound(FirmAmvBound::Minimum(0.8)),
    );
    firm
}

fn make_jeweler() -> Firm {
    let mut firm = Firm::new(5, "jeweler".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(5, 2.0, vec![GOLD]));
    firm.property.insert(
        GOLD,
        FirmPRow::new()
            .with_quantity(1.0)
            .with_purchase_target(4.0)
            .with_use_target(3.0)
            .with_stock_target(5.0)
            .with_amv_bound(FirmAmvBound::Maximum(7.0)),
    );
    firm.property.insert(
        JEWELRY,
        FirmPRow::new()
            .with_quantity(6.0)
            .with_sell_target(5.0)
            .with_amv_target(15.0)
            .with_amv_bound(FirmAmvBound::Minimum(12.0)),
    );
    firm.property.insert(COIN, FirmPRow::new().with_quantity(10.0));
    firm
}

fn make_well() -> Firm {
    let mut firm = Firm::new(6, "well".into(), 1, Hex::new(0, 0));
    firm.production_line.push(dummy_line(6, 20.0, vec![]));
    firm.property.insert(
        WATER,
        FirmPRow::new()
            .with_quantity(25.0)
            .with_sell_target(20.0)
            .with_amv_target(0.3)
            .with_amv_bound(FirmAmvBound::Minimum(0.4)),
    );
    firm.property.insert(COIN, FirmPRow::new().with_quantity(4.0));
    firm
}
