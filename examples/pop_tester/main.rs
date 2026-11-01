//! A few pops, one market, a few days.
//!
//! Loads world data and `data/pop_tester/`, then runs
//! [`Market::market_day`]. The scenario folder sets the opening board, each
//! pop's craft and stock, and that morning's work. Desires come from the
//! species, culture, and religion files. This file does not decide prices,
//! baskets, or accept/reject.
//!
//! The first argument is how many days to run before asking for more
//! (default 3). The second is the population cap, as a multiplier on each
//! pop's starting size (default 1000). Those shares add up to one ceiling.
//! After growth, if the pops pass it, every pop is scaled by the same
//! factor. After those days, enter runs one
//! more, a number runs that many, and 0 quits.
//!
//! ```text
//! cargo run --example pop_tester
//! cargo run --example pop_tester -- 5
//! cargo run --example pop_tester -- 5 10
//! ```

mod load;

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;

use rand::SeedableRng;
use rand::rngs::StdRng;
use simpler_economy::game::actors::Actors;
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::job::{trade_rate, trade_score};
use simpler_economy::game::market::{Market, MarketGood, MarketHistory, MassClose};
use simpler_economy::game::pop::{DayTape, Pop};

fn main() {
    let (days, cap_multiplier) = tester_args();
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
    set_population_caps(&mut actors, &ids, cap_multiplier);
    println!("pop tester");
    println!("world: {}", world.display());
    println!("scenario: {}", scenario_dir.display());
    println!("days: {days}");
    println!(
        "pop cap: {}x starting size, shared",
        qty_text(cap_multiplier)
    );
    println!();

    let mut day = 1u32;
    let mut left = days;
    loop {
        if left == 0 {
            left = more_days();
            if left == 0 {
                break;
            }
        }
        run_day(
            day,
            &mut actors,
            &mut markets,
            &factuals,
            market_id,
            &mut rng,
        );
        day += 1;
        left -= 1;
    }
}

/// # Tester Args
///
/// Days to run first, then the population-cap multiplier.
///
/// Days default to 3. A first argument that is not a whole number also
/// defaults to 3. The multiplier defaults to 1000. A present multiplier
/// that is not a positive finite number stops the tester.
fn tester_args() -> (u32, f64) {
    let mut args = std::env::args().skip(1);
    let days = match args.next() {
        Some(arg) => arg.parse::<u32>().unwrap_or(3),
        None => 3,
    };
    let cap_multiplier = match args.next() {
        Some(arg) => match arg.parse::<f64>() {
            Ok(multiplier) if multiplier.is_finite() && multiplier > 0.0 => multiplier,
            _ => {
                eprintln!("pop cap must be a positive number, got {arg}");
                std::process::exit(1);
            }
        },
        None => 1000.0,
    };
    (days, cap_multiplier)
}

/// # Set Population Caps
///
/// Each pop's share of the ceiling is its current size times `multiplier`.
///
/// The size is read once, before the first market day, and stored on the
/// pop. The shares add up to the ceiling the market fits after growth.
fn set_population_caps(actors: &mut Actors, ids: &[usize], multiplier: f64) {
    for id in ids {
        let pop = actors.pop_mut(*id);
        let start = pop.demographics.total_population();
        pop.population_cap = Some(start * multiplier);
    }
}

/// # Run Day
///
/// One market day: morning grant, the mass market, then the day's tables.
///
/// `day` is the number printed on the header. Sizes for the growth column
/// are taken after the morning grant and before the market day, from the
/// pops still stored. The tables after the day read that store again, so a
/// pop pruned during the day is left off.
fn run_day(
    day: u32,
    actors: &mut Actors,
    markets: &mut HashMap<usize, Market>,
    factuals: &Factuals,
    market_id: usize,
    rng: &mut StdRng,
) {
    println!("=== day {day} ===");
    actors.start_day(markets, factuals);
    let market = markets
        .get_mut(&market_id)
        .unwrap_or_else(|| panic!("market {market_id} is not in the day-start set"));
    // Match prices this morning's card. The night write is inside the day.
    let history = market.history();
    // Households before the day, so growth is on the same scale as migration.
    let households = household_counts(actors);
    market.market_day(actors, factuals, rng);
    print_mass_market(factuals, market);
    print_pops(factuals, actors, &households, &history);
    print_market_board(factuals, market);
}

/// # More Days
///
/// Reads how many more market days to run.
///
/// An empty line is one day. `0` quits. Any other whole number is that
/// many days. A line that is not a whole number asks again. End of input
/// quits.
fn more_days() -> u32 {
    loop {
        println!();
        print!("run more days (enter = 1, 0 = quit): ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => return 0,
            Ok(_) => {
                let line = line.trim();
                if line.is_empty() {
                    return 1;
                }
                match line.parse::<u32>() {
                    Ok(days) => return days,
                    Err(_) => println!("enter a whole number, or press enter for one day"),
                }
            }
            Err(_) => return 0,
        }
    }
}

fn good_name<'a>(factuals: &'a Factuals, id: usize) -> &'a str {
    factuals
        .goods
        .get(&id)
        .map(|good| good.name.as_str())
        .unwrap_or("unknown")
}

/// # Print Mass Market
///
/// Prints one row per good the mass market quoted.
///
/// Columns are the good, units offered, units requested, units exchanged,
/// and the clearing price. A book that did not trade prints `none` for
/// the price. An empty day still prints the header.
fn print_mass_market(factuals: &Factuals, market: &Market) {
    let rows = market
        .mass_closes()
        .iter()
        .map(|close| {
            vec![
                good_name(factuals, close.good).to_string(),
                qty_text(close.offered),
                qty_text(close.requested),
                qty_text(close.exchanged),
                mass_price(close),
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        "mass market",
        &["good", "offered", "requested", "exchanged", "price"],
        &rows,
    );
}

/// # Mass Price
///
/// The clearing AMV of one mass-market row, or `none` when it did not trade.
fn mass_price(close: &MassClose) -> String {
    close
        .price
        .map(qty_text)
        .unwrap_or_else(|| "none".to_string())
}

/// # Print Pops
///
/// Prints the work table, then the pop table, for the pops still stored.
///
/// The work table is the craft, the schedule left on the job, and one
/// line per good. That line is the holding, then buy and sell together.
/// Schedule lines stay in their own column. `history` is the card the
/// schedule's profit is priced on.
///
/// The pop table is one row each: id, standard of living, size, growth,
/// migration, the five sentiment shares, today's books, the meal, and
/// the net. Size is people (`DemoRow::total_population`). Growth is the
/// change in household count since `households`, the count before this
/// market day. A pop missing from `households` prints `none`. Migration
/// is [`Pop::migration`], net households gained this day. Standard of living is today's
/// satisfaction, including desire bonuses. The shares are the partition
/// after this day's sentiment update. Books, the meal, and the net are
/// each divided by the households still on the pop. Books are income over
/// expenses from [`Pop::day_books`], priced on the same morning `history`
/// as the schedule. The consumed column is that meal, written as a cost.
/// The net is income minus both of those costs. A pop removed during the
/// day is absent from both tables.
fn print_pops(
    factuals: &Factuals,
    actors: &mut Actors,
    households: &HashMap<usize, f64>,
    history: &MarketHistory,
) {
    let ids = living_pops(actors);
    let mut pops = Vec::new();
    let mut work = Vec::new();
    for id in ids {
        let pop = actors.pop_mut(id);
        let sol = pop.calculate_sol(factuals);
        let size = pop.demographics.total_population();
        let count = pop.demographics.household.count;
        let growth = households
            .get(&id)
            .map(|before| growth_text(count - before))
            .unwrap_or_else(|| "none".to_string());
        let (income, expenses, meal) = pop.day_books(history);
        let income = per_household(pop, income);
        let expenses = per_household(pop, expenses);
        let meal = per_household(pop, meal);
        let net = income - expenses - meal;
        pops.push(vec![
            id.to_string(),
            qty_text(sol),
            qty_text(size),
            growth,
            growth_text(pop.migration),
            qty_text(pop.sentiment.happiness()),
            qty_text(pop.sentiment.contentment()),
            qty_text(pop.sentiment.anger()),
            qty_text(pop.sentiment.fear()),
            qty_text(pop.sentiment.hope()),
            format!("{}/{}", growth_text(income), growth_text(-expenses)),
            growth_text(-meal),
            growth_text(net),
        ]);
        let tape = pop.day_tape();
        let (held, traded) = good_lines(factuals, pop, &tape);
        work.push(vec![
            id.to_string(),
            craft_name(factuals, pop.job.craft),
            schedule(factuals, pop, history),
            held,
            traded,
        ]);
    }
    print_table(
        "work",
        &["pop", "craft", "schedule", "holdings", "buy/sell"],
        &work,
    );
    print_table(
        "pops",
        &[
            "pop",
            "sol",
            "size",
            "growth",
            "migration",
            "happy",
            "content",
            "anger",
            "fear",
            "hope",
            "books",
            "consumed",
            "net",
        ],
        &pops,
    );
}

/// # Per Household
///
/// `amount` divided by the households still on `pop`.
///
/// A non-positive or non-finite count uses one household, so the share
/// stays finite. Returns that share.
fn per_household(pop: &Pop, amount: f64) -> f64 {
    let count = pop.demographics.household.count;
    let households = if count.is_finite() && count > 0.0 {
        count
    } else {
        1.0
    };
    amount / households
}

/// # Household Counts
///
/// Household count of each pop still stored, read before the market day runs.
///
/// Growth subtracts this from the count after the day, so it shares a scale
/// with migration.
fn household_counts(actors: &Actors) -> HashMap<usize, f64> {
    living_pops(actors)
        .into_iter()
        .map(|id| (id, actors.pop(id).demographics.household.count))
        .collect()
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

/// # Growth Text
///
/// `delta` with a leading plus when it is positive.
///
/// A change that prints as zero has no sign. Growth uses this for the
/// household-count change, including a capped count whose leftover is
/// smaller than the print. Migration uses it for net households. The books column uses it for
/// income and for expenses written as a cost. The consumed column uses
/// it for the meal, written as a cost. The net column uses it for the
/// day's gain or loss.
fn growth_text(delta: f64) -> String {
    let text = qty_text(delta);
    if text == "0" || text == "-0" {
        "0".to_string()
    } else if delta > 0.0 {
        format!("+{text}")
    } else {
        text
    }
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
/// Each job line as `process iterations:profit/percent`, one line per process, in line order.
///
/// `history` prices [`trade_score`] and [`trade_rate`]. Profit is that one
/// run, with a leading plus when the run gains holding. The percent is
/// that gain over the absolute input holding. No priced input reads as
/// `none`. A missing process reads as "unknown" and has no figures. No
/// lines read as "idle".
fn schedule(factuals: &Factuals, pop: &Pop, history: &MarketHistory) -> String {
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
            let target = line
                .target
                .map(qty_text)
                .unwrap_or_else(|| "open".to_string());
            match factuals.get_process(line.process) {
                Some(process) => {
                    let profit = growth_text(trade_score(process, history));
                    let percent = trade_rate(process, history)
                        .map(|share| format!("{}%", growth_text(share * 100.0)))
                        .unwrap_or_else(|| "none".to_string());
                    format!("{name} {target}:{profit}/{percent}")
                }
                None => format!("{name} {target}"),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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

/// # Good Lines
///
/// One line per good, shared by holdings and the columns beside it.
///
/// Goods are lowest id first. A line is kept when the pop holds the good
/// or the day tape has a buy or a sell. A holding that prints as zero
/// shows the name alone, so a buy with no stock still has a line. Buy
/// and sell share the second cell: `buy` and `sell` are offers, `bought`
/// and `sold` are the deal good, and `back+` and `back-` are change.
/// No goods returns `nothing` for the holding and an empty cell beside it.
fn good_lines(factuals: &Factuals, pop: &Pop, tape: &DayTape) -> (String, String) {
    let mut ids = Vec::new();
    for (&id, row) in &pop.property {
        if shown(row.quantity) {
            ids.push(id);
        }
    }
    for map in [
        &tape.buy_offers,
        &tape.sell_offers,
        &tape.bought,
        &tape.sold,
        &tape.change_in,
        &tape.change_out,
    ] {
        for (&id, &qty) in map {
            if shown(qty) {
                ids.push(id);
            }
        }
    }
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return ("nothing".to_string(), String::new());
    }
    let mut held = Vec::new();
    let mut traded = Vec::new();
    for id in ids {
        let name = good_name(factuals, id);
        let qty = pop.property.get(&id).map(|row| row.quantity).unwrap_or(0.0);
        if shown(qty) {
            held.push(format!("{} {name}", qty_text(qty)));
        } else {
            held.push(name.to_string());
        }
        traded.push(join_marks(&[
            mark("buy", tape_qty(&tape.buy_offers, id)),
            mark("sell", tape_qty(&tape.sell_offers, id)),
            mark("bought", tape_qty(&tape.bought, id)),
            mark("sold", tape_qty(&tape.sold, id)),
            mark("back+", tape_qty(&tape.change_in, id)),
            mark("back-", tape_qty(&tape.change_out, id)),
        ]));
    }
    (held.join("\n"), traded.join("\n"))
}

/// # Shown
///
/// True when `qty` prints as something other than zero.
fn shown(qty: f64) -> bool {
    if !qty.is_finite() {
        return false;
    }
    let text = qty_text(qty);
    text != "0" && text != "-0"
}

/// # Tape Qty
///
/// Units of `id` on `map`, or 0 when the good is absent.
fn tape_qty(map: &HashMap<usize, f64>, id: usize) -> f64 {
    map.get(&id).copied().unwrap_or(0.0)
}

/// # Mark
///
/// `label` and the printed quantity, when [`shown`] keeps it.
///
/// A quantity that prints as zero returns nothing.
fn mark(label: &str, qty: f64) -> Option<String> {
    shown(qty).then(|| format!("{label} {}", qty_text(qty)))
}

/// # Join Marks
///
/// `parts` that are present, separated by a comma.
///
/// An empty list is an empty line, so the good stays aligned.
fn join_marks(parts: &[Option<String>]) -> String {
    parts
        .iter()
        .filter_map(|part| part.as_deref())
        .collect::<Vec<_>>()
        .join(", ")
}

/// # Print Table
///
/// Prints `title`, then a header and one row per entry in `rows`.
///
/// Columns line up by the widest line in a cell. A cell with newlines
/// prints one line at a time, and a blank line separates those rows.
/// `rows` may be empty.
fn print_table(title: &str, headers: &[&str], rows: &[Vec<String>]) {
    println!("{title}");
    let widths: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(column, header)| {
            let widest = rows
                .iter()
                .map(|row| row.get(column).map(|cell| cell_width(cell)).unwrap_or(0))
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
    let stacked = rows
        .iter()
        .any(|row| row.iter().any(|cell| cell.contains('\n')));
    for (index, row) in rows.iter().enumerate() {
        if stacked && index > 0 {
            println!();
        }
        for line in row_lines(row) {
            println!("{}", table_line(&line, &widths));
        }
    }
    println!();
}

/// # Cell Width
///
/// The widest line in `cell`. A newline starts another line.
fn cell_width(cell: &str) -> usize {
    cell.split('\n').map(str::len).max().unwrap_or(0)
}

/// # Row Lines
///
/// Splits each cell on newlines and pads shorter cells with blanks.
///
/// Line 0 is the first line of every cell. Later lines leave a cell blank
/// once that cell has run out.
fn row_lines(row: &[String]) -> Vec<Vec<String>> {
    let cells: Vec<Vec<&str>> = row
        .iter()
        .map(|cell| {
            if cell.is_empty() {
                vec![""]
            } else {
                cell.split('\n').collect()
            }
        })
        .collect();
    let height = cells.iter().map(Vec::len).max().unwrap_or(1);
    (0..height)
        .map(|line| {
            cells
                .iter()
                .map(|cell| cell.get(line).copied().unwrap_or("").to_string())
                .collect()
        })
        .collect()
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
