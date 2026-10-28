//! A few firms, the pops they sell to, one market, a few days.
//!
//! Loads world data and `data/firm_tester/`, then runs
//! [`Market::market_day`]. The market, pops, and desires load through the
//! pop tester's loader. `firms.toml` adds each firm's owner, stock, and
//! production lines. This file does not decide prices, baskets, or
//! accept/reject.
//!
//! ```text
//! cargo run --example firm_tester
//! cargo run --example firm_tester -- 5
//! ```

#[path = "../pop_tester/load.rs"]
mod pop_load;

mod load;

use std::collections::HashMap;
use std::path::PathBuf;

use rand::SeedableRng;
use rand::rngs::StdRng;
use simpler_economy::game::actor::Actor;
use simpler_economy::game::actors::Actors;
use simpler_economy::game::deal::{Meeting, MeetingOutcome, ProposedDeal};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::firm::Firm;
use simpler_economy::game::market::Market;

fn main() {
    let days = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<u32>().ok())
        .unwrap_or(3);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    let world = root.join("world");
    let scenario_dir = root.join("firm_tester");
    let mut factuals = Factuals::load_from_path(&world).unwrap_or_else(|err| {
        eprintln!("load {}: {err}", world.display());
        std::process::exit(1);
    });
    let mut scenario =
        pop_load::load_scenario(&scenario_dir, &mut factuals).unwrap_or_else(|err| {
            eprintln!("{err}");
            std::process::exit(1);
        });
    load::load_firms(&scenario_dir, &factuals, &mut scenario.market, &mut scenario.actors)
        .unwrap_or_else(|err| {
            eprintln!("{err}");
            std::process::exit(1);
        });
    let mut firm_ids: Vec<usize> = scenario.market.firms.iter().copied().collect();
    firm_ids.sort_unstable();
    let market_id = scenario.market.id;
    let mut markets = HashMap::from([(market_id, scenario.market)]);
    let mut actors = scenario.actors;

    let mut rng = StdRng::seed_from_u64(1);
    println!("firm tester");
    println!("world: {}", world.display());
    println!("scenario: {}", scenario_dir.display());
    println!("days: {days}");
    println!();
    print_firms(&factuals, &actors, &firm_ids, "opening firms");

    for day in 1..=days {
        println!("=== day {day} ===");
        actors.start_day(&mut markets, &factuals);
        let market = markets
            .get_mut(&market_id)
            .unwrap_or_else(|| panic!("market {market_id} is not in the day-start set"));
        let meetings = market.market_day(&mut actors, &factuals, &mut rng);
        print_exchanges(&factuals, &meetings);
        print_firms(&factuals, &actors, &firm_ids, "firms");
        print_pops(&factuals, &mut actors);
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
/// Prints one row per meeting: buyer, seller, the good they met on, the
/// outcome, and the basket.
///
/// No proposal prints `none`. An empty day still prints the header.
fn print_exchanges(factuals: &Factuals, meetings: &[Meeting]) {
    let rows = meetings
        .iter()
        .map(|meeting| {
            vec![
                actor_name(meeting.buyer),
                actor_name(meeting.seller),
                good_name(factuals, meeting.match_good).to_string(),
                outcome_name(meeting.outcome).to_string(),
                meeting
                    .proposal
                    .as_ref()
                    .map(|proposal| basket(factuals, proposal))
                    .unwrap_or_else(|| "none".to_string()),
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        "exchanges",
        &["buyer", "seller", "good", "outcome", "basket"],
        &rows,
    );
}

/// # Print Firms
///
/// Prints each firm's name, owner terms, production lines, and stock under
/// `title`.
///
/// `ids` is the firm order.
fn print_firms(factuals: &Factuals, actors: &Actors, ids: &[usize], title: &str) {
    let rows = ids
        .iter()
        .map(|id| {
            let firm = actors.firm(*id);
            vec![
                id.to_string(),
                firm.name.clone(),
                owner(firm),
                lines(factuals, firm),
                stock(factuals, firm),
            ]
        })
        .collect::<Vec<_>>();
    print_table(title, &["firm", "name", "owner", "lines", "stock"], &rows);
}

/// # Owner
///
/// The owner pop and its terms: `liable`, or its profit share.
///
/// A blank or non-pop owner reads as "none".
fn owner(firm: &Firm) -> String {
    match firm.owners.pop_id() {
        None => "none".to_string(),
        Some(id) if firm.owners.liable => format!("pop {id} liable"),
        Some(id) => format!("pop {id} share {}", qty_text(firm.owners.profit_share)),
    }
}

/// # Lines
///
/// Each production line as `process target`, in line order.
///
/// A missing process keeps its id. A line with no target reads `open`. No
/// lines read as "none".
fn lines(factuals: &Factuals, firm: &Firm) -> String {
    if firm.production_line.is_empty() {
        return "none".to_string();
    }
    firm.production_line
        .iter()
        .map(|line| {
            let name = factuals
                .get_process(line.process)
                .map(|process| process.name.clone())
                .unwrap_or_else(|| line.process.to_string());
            let target = line.target.map(qty_text).unwrap_or_else(|| "open".to_string());
            format!("{name} {target}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// # Stock
///
/// Goods this firm holds, lowest id first, with any reserve in brackets.
///
/// A row with no quantity and no reserve is left out. No stock reads as
/// "nothing".
fn stock(factuals: &Factuals, firm: &Firm) -> String {
    let mut rows: Vec<(usize, f64, f64)> = firm
        .property
        .iter()
        .filter(|(_, row)| row.quantity != 0.0 || row.reserve != 0.0)
        .map(|(&id, row)| (id, row.quantity, row.reserve))
        .collect();
    rows.sort_by_key(|(id, _, _)| *id);
    if rows.is_empty() {
        return "nothing".to_string();
    }
    rows.into_iter()
        .map(|(id, quantity, reserve)| {
            let held = format!("{} {}", qty_text(quantity), good_name(factuals, id));
            if reserve > 0.0 {
                format!("{held} [{} reserved]", qty_text(reserve))
            } else {
                held
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// # Print Pops
///
/// Prints each pop still stored: standard of living and what they hold.
///
/// Standard of living is today's satisfaction, including desire bonuses.
/// Holdings skip goods whose quantity is 0. A pop removed during the day
/// is absent. Rows are lowest id first.
fn print_pops(factuals: &Factuals, actors: &mut Actors) {
    let ids = living_pops(actors);
    let rows = ids
        .iter()
        .map(|id| {
            let pop = actors.pop_mut(*id);
            let sol = pop.calculate_sol(factuals);
            let mut held: Vec<(usize, f64)> = pop
                .property
                .iter()
                .filter(|(_, row)| row.quantity != 0.0)
                .map(|(&good, row)| (good, row.quantity))
                .collect();
            held.sort_by_key(|(good, _)| *good);
            let holdings = if held.is_empty() {
                "nothing".to_string()
            } else {
                held.iter()
                    .map(|(good, qty)| format!("{} {}", qty_text(*qty), good_name(factuals, *good)))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            vec![id.to_string(), qty_text(sol), holdings]
        })
        .collect::<Vec<_>>();
    print_table("pops", &["pop", "sol", "holdings"], &rows);
}

/// # Living Pops
///
/// Pop ids still stored on `actors`, lowest id first.
///
/// The opening scenario list is not consulted. A pruned pop is absent.
fn living_pops(actors: &Actors) -> Vec<usize> {
    let mut ids: Vec<usize> = actors.pops.keys().copied().collect();
    ids.sort_unstable();
    ids
}

/// # Print Market Board
///
/// Prints each good's AMV and salability after the night write.
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
            ]
        })
        .collect::<Vec<_>>();
    print_table("market board", &["good", "amv", "salability"], &rows);
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

/// # Table Line
///
/// `cells` padded to `widths` and joined by two spaces.
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

/// # Basket
///
/// Each good in `proposal` as a signed quantity, lowest id first, then the
/// freight.
///
/// A positive quantity goes to the buyer.
fn basket(factuals: &Factuals, proposal: &ProposedDeal) -> String {
    let mut rows: Vec<(usize, f64)> = proposal.goods.iter().map(|(&id, &qty)| (id, qty)).collect();
    rows.sort_by_key(|(id, _)| *id);
    let mut parts: Vec<String> = rows
        .into_iter()
        .map(|(id, qty)| {
            let sign = if qty > 0.0 { "+" } else { "" };
            format!("{sign}{} {}", qty_text(qty), good_name(factuals, id))
        })
        .collect();
    parts.push(format!("freight {}", qty_text(proposal.freight)));
    parts.join(", ")
}

/// # Actor Name
///
/// The actor's kind and id, such as `firm 2`.
fn actor_name(actor: Actor) -> String {
    match actor {
        Actor::Pop(id) => format!("pop {id}"),
        Actor::Firm(id) => format!("firm {id}"),
        Actor::Institution(id) => format!("institution {id}"),
        Actor::State(id) => format!("state {id}"),
    }
}

/// # Outcome Name
///
/// The meeting outcome in lower case.
fn outcome_name(outcome: MeetingOutcome) -> &'static str {
    match outcome {
        MeetingOutcome::Abandoned => "abandoned",
        MeetingOutcome::Rejected => "rejected",
        MeetingOutcome::Accepted => "accepted",
    }
}

/// # Quantity Text
///
/// A whole number prints without decimals. Anything else prints up to four
/// places with trailing zeros dropped.
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
