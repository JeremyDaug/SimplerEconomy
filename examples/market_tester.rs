//! CLI box for probing a market day. Currently only [`Market::match_orders`];
//! meant to grow into a full intramarket-day loop.
//!
//! No factuals and no settlement. Prefab names are labels on ids so we can
//! talk about the same goods and actors; they are not a goods catalog.
//!
//! ```text
//! cargo run --example market_tester
//! ```
//!
//! ```text
//!   request farmers grain 3
//!   offer mill grain 4
//!   list
//!   match
//! ```
//!
//! Actor and good still accept `kind id` / raw numbers (`pop 1`, `1`).

use std::io::{self, IsTerminal, Write};

use rand::rngs::StdRng;
use rand::SeedableRng;
use simpler_economy::game::actor::Actor;
use simpler_economy::game::config::market_priority;
use simpler_economy::game::market::Market;
use simpler_economy::game::marketorder::{compose_sell_priority, MarketOrder};

/// Label on a good id. Not a factual.
struct NamedGood {
    id: usize,
    name: &'static str,
}

/// Label on an actor id. Not a living actor.
struct NamedActor {
    actor: Actor,
    name: &'static str,
}

const PREFAB_GOODS: &[NamedGood] = &[
    NamedGood {
        id: 1,
        name: "grain",
    },
    NamedGood {
        id: 2,
        name: "bread",
    },
    NamedGood {
        id: 3,
        name: "timber",
    },
    NamedGood {
        id: 4,
        name: "tools",
    },
    NamedGood {
        id: 5,
        name: "cloth",
    },
    NamedGood {
        id: 6,
        name: "iron",
    },
    NamedGood {
        id: 7,
        name: "fish",
    },
    NamedGood {
        id: 8,
        name: "pottery",
    },
    NamedGood {
        id: 9,
        name: "meat",
    },
    NamedGood {
        id: 10,
        name: "coin",
    },
];

/// 1 state, 2 institutions, 3 firms, 4 pops.
const PREFAB_ACTORS: &[NamedActor] = &[
    NamedActor {
        actor: Actor::State(1),
        name: "crown",
    },
    NamedActor {
        actor: Actor::Institution(1),
        name: "guild",
    },
    NamedActor {
        actor: Actor::Institution(2),
        name: "temple",
    },
    NamedActor {
        actor: Actor::Firm(1),
        name: "farm",
    },
    NamedActor {
        actor: Actor::Firm(2),
        name: "mill",
    },
    NamedActor {
        actor: Actor::Firm(3),
        name: "trader",
    },
    NamedActor {
        actor: Actor::Pop(1),
        name: "farmers",
    },
    NamedActor {
        actor: Actor::Pop(2),
        name: "millers",
    },
    NamedActor {
        actor: Actor::Pop(3),
        name: "laborers",
    },
    NamedActor {
        actor: Actor::Pop(4),
        name: "townsfolk",
    },
];

struct Session {
    buys: Vec<MarketOrder>,
    sells: Vec<MarketOrder>,
    rng: StdRng,
    seed: Option<u64>,
    log: String,
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
    let mut session = Session {
        buys: Vec::new(),
        sells: Vec::new(),
        rng: StdRng::from_os_rng(),
        seed: None,
        log: "Type help for commands. Matcher is read-only (books stay put).".into(),
    };

    let tty = io::stdout().is_terminal();
    if tty {
        draw_ui(&session);
    } else {
        println!("=== market tester ===");
        println!("Prefab names are labels on ids. Type help for commands.");
        println!("Matcher is read-only (books stay put).\n");
        print_legend();
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
    print_legend();
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
    let msg = match cmd.as_str() {
        "help" | "?" | "h" => help_text(),
        "legend" | "ids" | "prefabs" | "cls" | "list" | "ls" | "l" => {
            "header already shows prefabs and books.".into()
        }
        "quit" | "exit" | "q" => return CmdResult::Quit,
        "clear" => {
            session.buys.clear();
            session.sells.clear();
            "books cleared.".into()
        }
        "seed" => match parse_seed(rest) {
            Ok(seed) => {
                session.rng = StdRng::seed_from_u64(seed);
                session.seed = Some(seed);
                format!("rng seeded to {seed} (next match starts from here).")
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

fn print_legend() {
    println!("prefab goods");
    for (i, good) in PREFAB_GOODS.iter().enumerate() {
        print!("  {:>2} {:<8}", good.id, good.name);
        if i % 5 == 4 {
            println!();
        }
    }
    if PREFAB_GOODS.len() % 5 != 0 {
        println!();
    }
    println!("prefab actors  (1 state, 2 inst, 3 firms, 4 pops)");
    for named in PREFAB_ACTORS {
        println!("  {:<10}  {}", fmt_actor_kind_id(named.actor), named.name);
    }
    println!();
    println!("Type a prefab name, or kind+id / raw good id.");
    println!("  request farmers grain 3");
    println!("  request pop 1 1 3");
    println!();
    println!("Buy order priority: lower goes first. Defaults:");
    println!("  state 0   inst 1   firm 2 (merchant)   pop 4");
    println!("Sell/offer priority: higher is more likely. Default is compose_sell_priority");
    println!("  (1 / actor-band + sqrt(amount) + 0 fills).");
}

fn help_text() -> String {
    "\
commands
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
actor: prefab name (farmers, mill, crown, ...) or kind id (pop 1, firm 2)
good:  prefab name (grain, coin, ...) or id (1, 10)
amounts: type positives. request/buy store +amount, offer/sell store -amount.
buy pay-amount is what the buyer tenders (stored negative). sell want-amount
is what the seller asks for in return (stored positive).

examples
  request farmers grain 3
  offer mill grain 4
  request townsfolk tools 1 4.5
  buy trader grain 5 1.0 coin 5
  sell farm grain 5 1.0 coin 5
  match"
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
        Actor::Firm(_) => market_priority::FIRM_MERCHANT,
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
    format!("buy [{i}] {}", fmt_order(&session.buys[i]))
}

fn add_sell(session: &mut Session, order: MarketOrder) -> String {
    let i = session.sells.partition_point(|o| o.target <= order.target);
    session.sells.insert(i, order);
    format!("sell [{i}] {}", fmt_order(&session.sells[i]))
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
            Ok(format!("dropped buy [{idx}] {}", fmt_order(&removed)))
        }
        "sell" | "s" => {
            if idx >= session.sells.len() {
                return Err(format!("no sell [{idx}]"));
            }
            let removed = session.sells.remove(idx);
            Ok(format!("dropped sell [{idx}] {}", fmt_order(&removed)))
        }
        other => Err(format!("drop side must be buy or sell, got '{other}'")),
    }
}

fn list_books(session: &Session) {
    println!(
        "rng: {}",
        match session.seed {
            Some(s) => format!("seed {s}"),
            None => "os".into(),
        }
    );
    println!();
    print_order_table("buys  (priority, lowest first)", &session.buys);
    println!();
    print_order_table("sells  (target good id)", &session.sells);
}

fn print_order_table(title: &str, orders: &[MarketOrder]) {
    println!("{title}");
    println!("{}", order_header());
    println!("{}", order_rule());
    if orders.is_empty() {
        println!("  (empty)");
    } else {
        for (i, order) in orders.iter().enumerate() {
            println!("{}", order_row(i, order));
        }
    }
}

fn run_match(session: &mut Session) -> String {
    let batch = Market::match_orders(&session.buys, &session.sells, &mut session.rng);
    if batch.is_empty() {
        return "empty batch (no buys, or nothing to deal / restamp).\nbooks unchanged.".into();
    }
    let mut out = String::new();
    match batch.matched {
        Some(pair) => {
            let buy = &session.buys[pair.buy_index];
            let sell = &session.sells[pair.sell_index];
            out.push_str(&format!(
                "matched  buy[{}]  <->  sell[{}]\n",
                pair.buy_index, pair.sell_index
            ));
            out.push_str(&format!("  buy  {}\n", fmt_order(buy)));
            out.push_str(&format!("  sell {}\n", fmt_order(sell)));
            if coincidence(buy, sell) {
                out.push_str(
                    "  coincidence: matching counter-offer goods (sell weight x2 this pick).\n",
                );
            }
        }
        None => out.push_str("no match this pass.\n"),
    }
    if batch.unmatched_buys.is_empty() {
        out.push_str("unmatched buys: (none)\n");
    } else {
        out.push_str("unmatched buys (no other-origin seller of that good):\n");
        for &i in &batch.unmatched_buys {
            out.push_str(&format!("  [{i}] {}\n", fmt_order(&session.buys[i])));
        }
    }
    out.push_str("books unchanged (matcher does not remove or restamp).");
    out
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

fn order_header() -> String {
    format!(
        "{:>2}  {:<7}  {:<10}  {:<8}  {:>8}  {:>8}  {:>6}  {}",
        "#", "kind", "actor", "good", "amt", "prio", "amv", "counter"
    )
}

fn order_rule() -> String {
    format!(
        "{:-<2}  {:-<7}  {:-<10}  {:-<8}  {:-<8}  {:-<8}  {:-<6}  {:-<16}",
        "", "", "", "", "", "", "", ""
    )
}

fn order_row(idx: usize, order: &MarketOrder) -> String {
    format!(
        "{:>2}  {:<7}  {:<10}  {:<8}  {:>8}  {:>8}  {:>6}  {}",
        idx,
        order_kind(order),
        fmt_actor(order.origin),
        fmt_good(order.target),
        fmt_num(order.target_amount),
        fmt_num(order.priority),
        amv_cell(order),
        counter_cell(order)
    )
}

fn fmt_order(order: &MarketOrder) -> String {
    format!(
        "{} {} {} amt {}  prio {}  amv {}  counter {}",
        order_kind(order),
        fmt_actor(order.origin),
        fmt_good(order.target),
        fmt_num(order.target_amount),
        fmt_num(order.priority),
        amv_cell(order),
        counter_cell(order)
    )
}
