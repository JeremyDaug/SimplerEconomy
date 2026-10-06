//! A few pops, one market, a few days.
//!
//! Loads world data and `data/pop_tester/`, then runs
//! [`Market::market_day`]. The scenario folder sets the opening board, each
//! pop's craft and stock, and that morning's work. Desires come from the
//! species, culture, and religion files. This file does not decide prices,
//! baskets, or accept/reject.
//!
//! ```text
//! cargo run --example pop_tester
//! cargo run --example pop_tester -- 5
//! ```

mod load;

use std::collections::HashMap;
use std::path::PathBuf;

use rand::rngs::StdRng;
use rand::SeedableRng;
use simpler_economy::game::actor::Actor;
use simpler_economy::game::actors::Actors;
use simpler_economy::game::deal::{Meeting, MeetingOutcome, ProposedDeal};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::good::Good;
use simpler_economy::game::market::{Market, MarketGood};
use simpler_economy::game::pop::Pop;

fn main() {
    let days = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<u32>().ok())
        .unwrap_or(3);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    let world = root.join("world");
    let scenario_dir = root.join("pop_tester");
    let mut factuals = Factuals::load_from_path(&world).unwrap_or_else(|err| {
        eprintln!("load {}: {err}", world.display());
        std::process::exit(1);
    });
    let scenario = load::load_scenario(&scenario_dir, &mut factuals).unwrap_or_else(|err| {
        eprintln!("{err}");
        std::process::exit(1);
    });
    let mut ids: Vec<usize> = scenario.market.pops.iter().copied().collect();
    ids.sort_unstable();
    let market_id = scenario.market.id;
    let mut markets = HashMap::from([(market_id, scenario.market)]);
    let mut actors = scenario.actors;

    let mut rng = StdRng::seed_from_u64(1);
    println!("pop tester");
    println!("world: {}", world.display());
    println!("scenario: {}", scenario_dir.display());
    println!("days: {days}");
    println!();

    for day in 1..=days {
        println!("=== day {day} ===");
        actors.start_day(&mut markets);
        let market = markets
            .get_mut(&market_id)
            .unwrap_or_else(|| panic!("market {market_id} is not in the day-start set"));
        let meetings = market.market_day(&mut actors, &factuals, &mut rng);
        print_exchanges(&factuals, &meetings);
        print_pops(&factuals, &mut actors, &ids);
        print_market_board(&factuals, market);
    }
}

fn good_name<'a>(factuals: &'a Factuals, id: usize) -> &'a str {
    factuals
        .goods
        .get(&id)
        .map(|good| good.name.as_str())
        .unwrap_or("unknown")
}

/// # Print Exchanges
///
/// Prints one row per meeting.
///
/// Columns are buyer, seller, the good they met on, the outcome, and the
/// basket. An empty day still prints the header.
fn print_exchanges(factuals: &Factuals, meetings: &[Meeting]) {
    let rows = meetings
        .iter()
        .map(|meeting| {
            let basket = match &meeting.proposal {
                Some(proposal) => basket(factuals, proposal),
                None => "none".to_string(),
            };
            vec![
                actor_name(meeting.buyer),
                actor_name(meeting.seller),
                good_name(factuals, meeting.match_good).to_string(),
                outcome_name(meeting.outcome).to_string(),
                basket,
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        "exchanges",
        &["buyer", "seller", "good", "outcome", "basket"],
        &rows,
    );
}

/// # Print Pops
///
/// Prints each pop's craft, the schedule left on the job, what that day
/// made, standard of living, and what they still hold.
///
/// `ids` is the pop order. Standard of living is today's satisfaction,
/// including desire bonuses. Holdings skip goods whose quantity is 0.
fn print_pops(factuals: &Factuals, actors: &mut Actors, ids: &[usize]) {
    let rows = ids
        .iter()
        .map(|id| {
            let pop = actors.pop_mut(*id);
            let sol = pop.calculate_sol(factuals);
            vec![
                id.to_string(),
                craft_name(factuals, pop.job.craft),
                schedule(factuals, pop),
                made(factuals, pop),
                qty_text(sol),
                holdings(factuals, pop),
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        "pops",
        &["pop", "craft", "schedule", "made", "sol", "holdings"],
        &rows,
    );
}

/// # Craft Name
///
/// The craft's display name, or `none` when the pop has no baseline craft.
fn craft_name(factuals: &Factuals, craft: usize) -> String {
    factuals
        .get_craft(craft)
        .map(|craft| craft.name.clone())
        .unwrap_or_else(|| "none".to_string())
}

/// # Schedule
///
/// Each job line as `process target`, in line order.
///
/// A missing process keeps its id. No lines read as "idle".
fn schedule(factuals: &Factuals, pop: &Pop) -> String {
    if pop.job.lines.is_empty() {
        return "idle".to_string();
    }
    pop.job
        .lines
        .iter()
        .map(|line| {
            let name = factuals
                .get_process(line.process)
                .map(|process| process.name.as_str())
                .unwrap_or("unknown");
            let target = line.target.map(qty_text).unwrap_or_else(|| "open".to_string());
            format!("{name} {target}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// # Made
///
/// Goods this pop's job put out today, lowest id first.
///
/// Reads `produced`. Nothing made reads as "nothing".
fn made(factuals: &Factuals, pop: &Pop) -> String {
    let mut rows: Vec<(&Good, f64)> = pop
        .property
        .iter()
        .filter(|(_, row)| row.produced > 0.0)
        .filter_map(|(id, row)| factuals.goods.get(id).map(|good| (good, row.produced)))
        .collect();
    rows.sort_by_key(|(good, _)| good.id);
    if rows.is_empty() {
        "nothing".to_string()
    } else {
        rows.iter()
            .map(|(good, qty)| format!("{} {}", qty_text(*qty), good.name))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// # Print Market Board
///
/// Prints each good's AMV, salability, units lost to rot, and that loss as
/// a percent of the stock it came from.
fn print_market_board(factuals: &Factuals, market: &Market) {
    let mut ids: Vec<usize> = market.goods.keys().copied().collect();
    ids.sort_unstable();
    let rows = ids
        .into_iter()
        .map(|id| {
            let good = &market.goods[&id];
            vec![
                good_name(factuals, id).to_string(),
                qty_text(good.amv),
                qty_text(good.salability),
                qty_text(good.decayed),
                format!("{}%", qty_text(decay_percent(good))),
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        "market board",
        &["good", "amv", "salability", "decay", "decay %"],
        &rows,
    );
}

/// # Decay Percent
///
/// Units lost over the stock they came from, as a percent.
///
/// The base is `volume` when that is positive, otherwise `stock`. That is
/// the same base the night uses. No base yields 0.
fn decay_percent(good: &MarketGood) -> f64 {
    let base = if good.volume > 0.0 {
        good.volume
    } else {
        good.stock
    };
    if base > 0.0 {
        (good.decayed / base).clamp(0.0, 1.0) * 100.0
    } else {
        0.0
    }
}

/// # Holdings
///
/// Goods this pop still has, lowest id first.
///
/// A quantity of 0 is left out. No stock reads as "nothing".
fn holdings(factuals: &Factuals, pop: &Pop) -> String {
    let mut rows: Vec<(&Good, f64)> = pop
        .property
        .iter()
        .filter(|(_, row)| row.quantity != 0.0)
        .filter_map(|(id, row)| factuals.goods.get(id).map(|good| (good, row.quantity)))
        .collect();
    rows.sort_by_key(|(good, _)| good.id);
    if rows.is_empty() {
        "nothing".to_string()
    } else {
        rows.iter()
            .map(|(good, qty)| format!("{} {}", qty_text(*qty), good.name))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// # Print Table
///
/// Prints `title`, then a header and one row per entry in `rows`.
///
/// Columns line up by the widest cell. `rows` may be empty.
fn print_table(title: &str, headers: &[&str], rows: &[Vec<String>]) {
    println!("{title}");
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(column, header)| {
            let widest = rows
                .iter()
                .map(|row| row.get(column).map(String::len).unwrap_or(0))
                .max()
                .unwrap_or(0);
            header.len().max(widest)
        })
        .collect();
    println!("{}", table_line(headers, &widths));
    println!(
        "{}",
        widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join("  ")
    );
    for row in rows {
        println!("{}", table_line(row, &widths));
    }
    println!();
}

fn table_line(cells: &[impl AsRef<str>], widths: &[usize]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(column, cell)| {
            let width = widths.get(column).copied().unwrap_or(0);
            format!("{:width$}", cell.as_ref())
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn basket(factuals: &Factuals, proposal: &ProposedDeal) -> String {
    let mut rows: Vec<(usize, f64)> = proposal.goods.iter().map(|(&id, &qty)| (id, qty)).collect();
    rows.sort_by_key(|(id, _)| *id);
    let mut parts: Vec<String> = rows
        .into_iter()
        .map(|(id, qty)| {
            let sign = if qty > 0.0 { "+" } else { "" };
            format!(
                "{sign}{} {}",
                qty_text(qty),
                good_name(factuals, id)
            )
        })
        .collect();
    parts.push(format!("freight {}", qty_text(proposal.freight)));
    parts.join(", ")
}

fn actor_name(actor: Actor) -> String {
    match actor {
        Actor::Pop(id) => format!("pop {id}"),
        Actor::Firm(id) => format!("firm {id}"),
        Actor::Institution(id) => format!("institution {id}"),
        Actor::State(id) => format!("state {id}"),
    }
}

fn outcome_name(outcome: MeetingOutcome) -> &'static str {
    match outcome {
        MeetingOutcome::Abandoned => "abandoned",
        MeetingOutcome::Rejected => "rejected",
        MeetingOutcome::Accepted => "accepted",
    }
}

fn qty_text(qty: f64) -> String {
    if qty.is_finite() && qty == qty.trunc() && qty.abs() < 1_000_000_000.0 {
        format!("{}", qty as i64)
    } else {
        format!("{qty:.4}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}
