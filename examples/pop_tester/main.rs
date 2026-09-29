//! Two pops, one market, a few days.
//!
//! Loads world goods and config, places a buyer and a seller, then runs
//! [`Market::market_day`]. This file does not decide prices, baskets, or
//! accept/reject.
//!
//! ```text
//! cargo run --example pop_tester
//! cargo run --example pop_tester -- 5
//! ```

use std::path::PathBuf;

use rand::rngs::StdRng;
use rand::SeedableRng;
use simpler_economy::game::actor::Actor;
use simpler_economy::game::actors::Actors;
use simpler_economy::game::deal::{Meeting, MeetingOutcome, ProposedDeal};
use simpler_economy::game::desire::{Desire, DesireSource, DesireTarget, DesireTargetType};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::good::Good;
use simpler_economy::game::market::{Market, MarketGood};
use simpler_economy::game::pop::{Pop, PopPRow};
use simpler_economy::game::scalingfactor::ScalingFactor;

fn main() {
    let days = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<u32>().ok())
        .unwrap_or(3);
    let world = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/world");
    let factuals = Factuals::load_from_path(&world).unwrap_or_else(|err| {
        eprintln!("load {}: {err}", world.display());
        std::process::exit(1);
    });

    let bread = good_id(&factuals, "bread");
    let gold = good_id(&factuals, "gold");
    let time = good_id(&factuals, "time");

    let mut buyer = Pop::new(1);
    buyer.property.insert(gold, PopPRow::new(20.0));
    buyer.property.insert(time, PopPRow::new(40.0));
    buyer.desires[0].push(desire(1, bread, 4.0));

    let mut seller = Pop::new(2);
    seller.property.insert(bread, PopPRow::new(10.0));

    let mut market = Market::new(1).with_friction(factuals.config.market.friction);
    market.pops.insert(buyer.id);
    market.pops.insert(seller.id);
    // Opening card. The day reads this and does not invent a price.
    market.goods.insert(bread, MarketGood::new().with_amv(1.0).with_salability(1.0));
    market.goods.insert(gold, MarketGood::new().with_amv(1.0).with_salability(1.0));

    let mut actors = Actors::new();
    actors.pops.insert(buyer.id, buyer);
    actors.pops.insert(seller.id, seller);

    let mut rng = StdRng::seed_from_u64(1);
    println!("pop tester");
    println!("world: {}", world.display());
    println!("days: {days}");
    println!(
        "pop 1 holds gold and time, and wants 4 bread. pop 2 holds bread."
    );
    println!();

    let mut ids: Vec<usize> = market.pops.iter().copied().collect();
    ids.sort_unstable();
    for day in 1..=days {
        println!("=== day {day} ===");
        let meetings = market.market_day(&mut actors, &factuals, &mut rng);
        print_meetings(&factuals, &meetings);
        print_holdings(&factuals, &actors, &ids);
        print_card(&factuals, &market);
        println!();
    }
}

fn desire(id: usize, good: usize, amount: f64) -> Desire {
    Desire {
        source: DesireSource::Species(0, id),
        priority: 0,
        target: vec![DesireTarget::new(good, DesireTargetType::Consume, 1.0)],
        amount,
        satisfaction: 0.0,
        category: None,
        effect: vec![],
        scalar: ScalingFactor::Fixed(1.0),
        decay: 0.0,
    }
}

fn good_id(factuals: &Factuals, name: &str) -> usize {
    factuals
        .goods
        .values()
        .find(|good| good.name == name)
        .unwrap_or_else(|| panic!("world data has no good named {name}"))
        .id
}

fn good_name<'a>(factuals: &'a Factuals, id: usize) -> &'a str {
    factuals
        .goods
        .get(&id)
        .map(|good| good.name.as_str())
        .unwrap_or("unknown")
}

fn print_meetings(factuals: &Factuals, meetings: &[Meeting]) {
    if meetings.is_empty() {
        println!("no meetings");
        return;
    }
    for meeting in meetings {
        println!(
            "match: {} met {} on {}",
            actor_name(meeting.buyer),
            actor_name(meeting.seller),
            good_name(factuals, meeting.match_good)
        );
        match &meeting.proposal {
            Some(proposal) => println!("proposal: {}", basket(factuals, proposal)),
            None => println!("proposal: none"),
        }
        println!("outcome: {}", outcome_name(meeting.outcome));
    }
}

fn print_holdings(factuals: &Factuals, actors: &Actors, ids: &[usize]) {
    for id in ids {
        let pop = actors
            .pops
            .get(id)
            .unwrap_or_else(|| panic!("pop {id} is missing"));
        let mut rows: Vec<(&Good, f64)> = pop
            .property
            .iter()
            .filter(|(_, row)| row.quantity != 0.0)
            .filter_map(|(id, row)| factuals.goods.get(id).map(|good| (good, row.quantity)))
            .collect();
        rows.sort_by_key(|(good, _)| good.id);
        let holdings = if rows.is_empty() {
            "nothing".to_string()
        } else {
            rows.iter()
                .map(|(good, qty)| format!("{} {}", qty_text(*qty), good.name))
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!("pop {id}: {holdings}");
    }
}

fn print_card(factuals: &Factuals, market: &Market) {
    let mut ids: Vec<usize> = market.goods.keys().copied().collect();
    ids.sort_unstable();
    if ids.is_empty() {
        println!("card: empty");
        return;
    }
    let parts: Vec<String> = ids
        .into_iter()
        .map(|id| {
            let good = &market.goods[&id];
            format!(
                "{} amv {} salability {}",
                good_name(factuals, id),
                qty_text(good.amv),
                qty_text(good.salability)
            )
        })
        .collect();
    println!("card: {}", parts.join("; "));
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
