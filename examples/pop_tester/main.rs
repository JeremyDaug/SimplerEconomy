//! One subsistence pop, one market, a few days. The market can found more pops.
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
//! factor. After those days, enter runs one more, a number runs that many,
//! and 0 quits. `pop N` or `p N` prints that pop and asks again.
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
use simpler_economy::game::market::{Market, MarketGood, MarketHistory, MassClose, PopExit};
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
    // Morning card from the last day, so a look prices the schedule the same way.
    let mut morning: Option<MarketHistory> = None;
    loop {
        if left == 0 {
            left = more_days(&factuals, &actors, morning.as_ref());
            if left == 0 {
                break;
            }
        }
        morning = Some(run_day(
            day,
            &mut actors,
            &mut markets,
            &factuals,
            market_id,
            &mut rng,
        ));
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
/// One market day: morning grant, then the day's tables.
///
/// `day` is printed before the tables and again after them. Sizes for the
/// growth column are taken after the morning grant and before the market
/// day, from the pops still stored. The tables after the day read that
/// store again, so a pop pruned during the day is left off. Returns the
/// morning card the schedule was priced on.
fn run_day(
    day: u32,
    actors: &mut Actors,
    markets: &mut HashMap<usize, Market>,
    factuals: &Factuals,
    market_id: usize,
    rng: &mut StdRng,
) -> MarketHistory {
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
    print_pops(factuals, actors, &households, &history);
    print_founding(factuals, actors, market);
    print_market_board(factuals, market, &history);
    println!("=== day {day} ===");
    history
}

/// One line typed at the day prompt.
enum DayPrompt {
    /// Run this many days. Zero quits.
    Days(u32),
    /// Print the pop with this id.
    Look(usize),
    /// A pop look that did not include an id.
    BadLook,
    /// Not a day count and not a pop look.
    Again,
}

/// # More Days
///
/// Reads how many more market days to run.
///
/// An empty line is one day. `0` quits. Any other whole number is that
/// many days. `pop N` or `p N` prints that pop from the last day and asks
/// again. `history` is that day's morning card. A line that is neither
/// asks again. End of input quits.
fn more_days(
    factuals: &Factuals,
    actors: &Actors,
    history: Option<&MarketHistory>,
) -> u32 {
    loop {
        println!();
        print!("run more days (enter = 1, 0 = quit, pop N = look): ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => return 0,
            Ok(_) => match prompt_line(line.trim()) {
                DayPrompt::Days(days) => return days,
                DayPrompt::Look(id) => match history {
                    Some(history) => print_pop_look(factuals, actors, history, id),
                    None => println!("no day yet"),
                },
                DayPrompt::BadLook => println!("use pop N or p N"),
                DayPrompt::Again => {
                    println!("enter a whole number, press enter for one day, or pop N to look")
                }
            },
            Err(_) => return 0,
        }
    }
}

/// # Prompt Line
///
/// What `line` asks the tester to do.
///
/// An empty line is one day. A whole number is that many days, and `0`
/// quits. `pop N` or `p N` looks at that pop. The word ignores case, and
/// the id may sit against the word (`p12`). A pop word with no id is
/// [`DayPrompt::BadLook`]. Anything else is [`DayPrompt::Again`].
fn prompt_line(line: &str) -> DayPrompt {
    if line.is_empty() {
        return DayPrompt::Days(1);
    }
    if let Ok(days) = line.parse::<u32>() {
        return DayPrompt::Days(days);
    }
    let mut parts = line.split_whitespace();
    let Some(word) = parts.next() else {
        return DayPrompt::Again;
    };
    let (is_pop, glued) = pop_word(word);
    if !is_pop {
        return DayPrompt::Again;
    }
    let id_text = if let Some(glued) = glued {
        if parts.next().is_some() {
            return DayPrompt::BadLook;
        }
        glued
    } else {
        match parts.next() {
            Some(id) if parts.next().is_none() => id,
            _ => return DayPrompt::BadLook,
        }
    };
    match id_text.parse::<usize>() {
        Ok(id) => DayPrompt::Look(id),
        Err(_) => DayPrompt::BadLook,
    }
}

/// # Pop Word
///
/// Whether `word` is `pop` or `p`.
///
/// Case is ignored. Digits glued to the word are the id text. A longer
/// word that only starts with those letters is not a pop word. Returns
/// the flag and that id text, when the digits were glued on.
fn pop_word(word: &str) -> (bool, Option<&str>) {
    let rest = if let Some(rest) = strip_ignore(word, "pop") {
        rest
    } else if let Some(rest) = strip_ignore(word, "p") {
        rest
    } else {
        return (false, None);
    };
    if rest.is_empty() {
        return (true, None);
    }
    if rest.bytes().all(|byte| byte.is_ascii_digit()) {
        (true, Some(rest))
    } else {
        (false, None)
    }
}

/// # Strip Ignore
///
/// The tail of `word` after `prefix`, ignoring ASCII case.
///
/// A word shorter than `prefix`, or one whose head is not that prefix,
/// returns nothing. The tail is the rest of `word`.
fn strip_ignore<'a>(word: &'a str, prefix: &str) -> Option<&'a str> {
    if !word.is_char_boundary(prefix.len()) {
        return None;
    }
    let (head, tail) = word.split_at(prefix.len());
    head.eq_ignore_ascii_case(prefix).then_some(tail)
}

fn good_name<'a>(factuals: &'a Factuals, id: usize) -> &'a str {
    factuals
        .goods
        .get(&id)
        .map(|good| good.name.as_str())
        .unwrap_or("unknown")
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
/// The work table is the craft and the schedule left on the job.
/// Schedule lines stay in their own column. `history` is the card the
/// schedule's profit is priced on. Holdings and the day's book are
/// [`print_pop_look`].
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
/// The net is income minus both of those costs. The last row is
/// [`PopulationTally::row`]: the whole population. A pop removed during the
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
    let mut tally = PopulationTally::default();
    for id in ids {
        let pop = actors.pop_mut(id);
        let sol = pop.calculate_sol(factuals);
        let size = pop.demographics.total_population();
        let count = pop.demographics.household.count;
        let growth = households.get(&id).map(|before| count - before);
        let (income, expenses, meal) = pop.day_books(history);
        tally.note(PopShare {
            people: size,
            households: count,
            sol,
            growth,
            migration: pop.migration,
            happy: pop.sentiment.happiness(),
            content: pop.sentiment.contentment(),
            anger: pop.sentiment.anger(),
            fear: pop.sentiment.fear(),
            hope: pop.sentiment.hope(),
            income,
            expenses,
            meal,
        });
        let income = per_household(pop, income);
        let expenses = per_household(pop, expenses);
        let meal = per_household(pop, meal);
        let net = income - expenses - meal;
        pops.push(vec![
            id.to_string(),
            qty_text(sol),
            qty_text(size),
            growth.map(growth_text).unwrap_or_else(|| "none".to_string()),
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
        work.push(vec![
            id.to_string(),
            craft_name(factuals, pop.job.craft),
            schedule(factuals, pop, history),
        ]);
    }
    if tally.count > 0.0 {
        pops.push(tally.row());
    }
    print_table("work", &["pop", "craft", "schedule"], &work);
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

/// # Print Founding
///
/// One row per pop still stored: set-aside, whether that pop may found,
/// the earliest desire still short, and the goods it offered or made.
/// A pop removed today is one more row.
///
/// Savings is [`Pop::set_aside_days`] on the card after this day: how many
/// one-day desire baskets the set-aside covers. Found is `Y` when that
/// count clears the world's founding wealth and the pop has enough households.
/// That answer does not include the wait between foundings. A wait still
/// running prints on the title line. When the wait is open and no pop is
/// ready, the title is the chance the next day will use. The desire is
/// [`Pop::lowest_unmet_desire`]: basic, then common, then luxury. Its goods
/// are listed highest efficiency first. No short desire prints `none`.
/// The goods column is [`offered_or_produced`]. A removed pop prints
/// `left` in days and [`exit_text`] in the desire column.
fn print_founding(factuals: &Factuals, actors: &Actors, market: &Market) {
    let card = market.history();
    let wealth = factuals.config.pop.founding_wealth;
    let mut rows = living_pops(actors)
        .into_iter()
        .map(|id| {
            let pop = actors.pop(id);
            let ready = if market.is_founding_ready(pop, wealth) {
                "Y"
            } else {
                "N"
            };
            vec![
                id.to_string(),
                qty_text(pop.set_aside_days(&card)),
                ready.to_string(),
                unmet_desire(factuals, pop),
                offered_or_produced(factuals, pop),
            ]
        })
        .collect::<Vec<_>>();
    let mut gone: Vec<_> = market.pop_removals().to_vec();
    gone.sort_by_key(|removal| removal.id);
    for removal in gone {
        rows.push(vec![
            removal.id.to_string(),
            "left".to_string(),
            String::new(),
            exit_text(&removal.why),
            String::new(),
        ]);
    }
    let wait = market.founding_wait();
    let ready = rows.iter().any(|row| row.get(2).is_some_and(|cell| cell == "Y"));
    let title = if wait > 0 {
        format!("founding wait: {wait}")
    } else if !ready {
        let percent = (market.founding_drought_chance() * 100.0).round() as u32;
        format!("founding drought: {percent}%")
    } else {
        "founding".to_string()
    };
    print_table(
        &title,
        &["pop", "days", "found", "desire", "goods"],
        &rows,
    );
}

/// # Exit Text
///
/// The short reason a removed pop prints in the desire column.
///
/// Under one household is the prune. No work and no coin names who
/// received that pop. A move that would have left under one household
/// names who received that pop. The same demographic and process set names
/// the earlier pop. No receiver omits the destination.
fn exit_text(why: &PopExit) -> String {
    match why {
        PopExit::UnderOneHousehold => "under 1 household".to_string(),
        PopExit::NoWorkNoCoin { to } => exit_dest("no work, no coin", to),
        PopExit::UnderOneMove { to } => exit_dest("would leave under 1", to),
        PopExit::SameJob { to } => exit_dest("same job", to),
    }
}

/// # Exit Dest
///
/// `reason`, then the pop ids in `to`.
///
/// Ids stay in the order recorded. An empty list is the reason alone.
fn exit_dest(reason: &str, to: &[usize]) -> String {
    if to.is_empty() {
        return reason.to_string();
    }
    let ids = to
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{reason}, to {ids}")
}

/// # Offered Or Produced
///
/// Goods this pop offered for sale or made today, names only.
///
/// `factuals` names the goods. `pop` supplies `produced` on each property
/// row and today's sell offers. A good is kept when either amount prints
/// as more than zero. A skill is left out. Lowest id comes first. No such
/// good is `none`.
fn offered_or_produced(factuals: &Factuals, pop: &Pop) -> String {
    let tape = pop.day_tape();
    let mut ids = Vec::new();
    for (&id, row) in &pop.property {
        if shown(row.produced) {
            ids.push(id);
        }
    }
    for (&id, &qty) in &tape.sell_offers {
        if shown(qty) {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    ids.dedup();
    let names = ids
        .into_iter()
        .filter(|id| !factuals.get_good(*id).is_some_and(|good| good.is_skill()))
        .map(|id| good_name(factuals, id))
        .collect::<Vec<_>>();
    if names.is_empty() {
        "none".to_string()
    } else {
        names.join(", ")
    }
}

/// # Unmet Desire
///
/// The tier and goods of [`Pop::lowest_unmet_desire`].
///
/// `factuals` names the goods. Highest efficiency comes first. A desire
/// with no goods is the tier alone. No short desire is `none`.
fn unmet_desire(factuals: &Factuals, pop: &Pop) -> String {
    let Some((tier, desire)) = pop.lowest_unmet_desire() else {
        return "none".to_string();
    };
    let tier = match tier {
        0 => "basic",
        1 => "common",
        _ => "luxury",
    };
    let mut targets = desire.target.clone();
    targets.sort_by(|left, right| {
        right
            .efficiency
            .partial_cmp(&left.efficiency)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let goods = targets
        .iter()
        .map(|target| good_name(factuals, target.good))
        .collect::<Vec<_>>()
        .join(", ");
    if goods.is_empty() {
        tier.to_string()
    } else {
        format!("{tier}: {goods}")
    }
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

/// One pop's figures for [`PopulationTally`].
struct PopShare {
    people: f64,
    households: f64,
    sol: f64,
    growth: Option<f64>,
    migration: f64,
    happy: f64,
    content: f64,
    anger: f64,
    fear: f64,
    hope: f64,
    income: f64,
    expenses: f64,
    meal: f64,
}

/// Running sums for the last row of the pop table.
#[derive(Default)]
struct PopulationTally {
    count: f64,
    people: f64,
    households: f64,
    growth: f64,
    saw_growth: bool,
    migration: f64,
    sol: Weighted,
    happy: Weighted,
    content: Weighted,
    anger: Weighted,
    fear: Weighted,
    hope: Weighted,
    income: f64,
    expenses: f64,
    meal: f64,
}

impl PopulationTally {
    /// # Note
    ///
    /// Adds one printed pop.
    ///
    /// `share.people` adds to the population when it is finite. A positive
    /// finite household count adds to the household total. Income, expenses,
    /// and the meal add when they are finite. Growth adds when this pop had
    /// a morning count. The other fields go to [`Weighted::note`].
    fn note(&mut self, share: PopShare) {
        self.count += 1.0;
        if share.people.is_finite() {
            self.people += share.people;
        }
        if share.households.is_finite() && share.households > 0.0 {
            self.households += share.households;
        }
        if let Some(growth) = share.growth.filter(|growth| growth.is_finite()) {
            self.growth += growth;
            self.saw_growth = true;
        }
        if share.migration.is_finite() {
            self.migration += share.migration;
        }
        let people = share.people;
        self.sol.note(share.sol, people);
        self.happy.note(share.happy, people);
        self.content.note(share.content, people);
        self.anger.note(share.anger, people);
        self.fear.note(share.fear, people);
        self.hope.note(share.hope, people);
        if share.income.is_finite() {
            self.income += share.income;
        }
        if share.expenses.is_finite() {
            self.expenses += share.expenses;
        }
        if share.meal.is_finite() {
            self.meal += share.meal;
        }
    }

    /// # Row
    ///
    /// The whole-population row.
    ///
    /// People, the household change, and migration are sums. A day with no
    /// morning count prints `none` for growth. Standard of living and the
    /// five sentiment shares are [`Weighted::average`], weighted by people.
    /// Books, the meal, and the net pool every pop's totals and divide by
    /// the households still stored, the same per-household scale as the
    /// other rows. Returns that row. The first cell is `all`.
    fn row(&self) -> Vec<String> {
        let share = if self.households.is_finite() && self.households > 0.0 {
            self.households
        } else {
            1.0
        };
        let income = self.income / share;
        let expenses = self.expenses / share;
        let meal = self.meal / share;
        let growth = if self.saw_growth {
            growth_text(self.growth)
        } else {
            "none".to_string()
        };
        vec![
            "all".to_string(),
            qty_text(self.sol.average()),
            qty_text(self.people),
            growth,
            growth_text(self.migration),
            qty_text(self.happy.average()),
            qty_text(self.content.average()),
            qty_text(self.anger.average()),
            qty_text(self.fear.average()),
            qty_text(self.hope.average()),
            format!("{}/{}", growth_text(income), growth_text(-expenses)),
            growth_text(-meal),
            growth_text(income - expenses - meal),
        ]
    }
}

/// A people-weighted average, with an equal average when no pop has people.
#[derive(Default)]
struct Weighted {
    weighted: f64,
    people: f64,
    plain: f64,
    samples: f64,
}

impl Weighted {
    /// # Note
    ///
    /// Adds one finite `value`.
    ///
    /// A positive finite `people` weights that value. A non-finite value
    /// is left out.
    fn note(&mut self, value: f64, people: f64) {
        if !value.is_finite() {
            return;
        }
        self.plain += value;
        self.samples += 1.0;
        if people.is_finite() && people > 0.0 {
            self.weighted += value * people;
            self.people += people;
        }
    }

    /// # Average
    ///
    /// The people-weighted mean of the values [`Self::note`] kept.
    ///
    /// When no noted pop had people, the mean is equal across the pops
    /// that had a finite value. No values returns 0.
    fn average(&self) -> f64 {
        if self.people > 0.0 {
            self.weighted / self.people
        } else if self.samples > 0.0 {
            self.plain / self.samples
        } else {
            0.0
        }
    }
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
/// lines read as "idle". The last line is the complexity cost of the
/// current targets, from [`Job::plan_complexity_cost`](simpler_economy::game::job::Job::plan_complexity_cost).
/// The modifier is [`Pop::complexity_cost`].
fn schedule(factuals: &Factuals, pop: &Pop, history: &MarketHistory) -> String {
    let body = if pop.job.lines.is_empty() {
        "idle".to_string()
    } else {
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
    };
    let cost = pop
        .job
        .plan_complexity_cost(factuals, pop.complexity_cost(factuals));
    format!("{body}\ncomplexity {}", qty_text(cost))
}

/// # Print Market Board
///
/// Prints each good's AMV, salability, rot, and today's mass book.
///
/// A row is kept when the good is stored on the board or it drew a quote.
/// A skill is omitted unless [`shows_good`] keeps it. Columns are the good,
/// AMV, salability, units lost to rot, that loss as a percent of the stock
/// it came from, then units offered, requested, and exchanged, and the
/// clearing price. The AMV starts with [`amv_move`]: how tonight's ask
/// moved from `morning`. A good with no quote leaves the book blank. A quote
/// that did not trade prints `none` for the price. A quote with no stored
/// good leaves the board columns blank.
fn print_market_board(factuals: &Factuals, market: &Market, morning: &MarketHistory) {
    let mut quoted: HashMap<usize, &MassClose> = HashMap::new();
    for close in market.mass_closes() {
        quoted.insert(close.good, close);
    }
    let mut ids: Vec<usize> = market.goods.keys().copied().collect();
    ids.extend(quoted.keys().copied());
    ids.sort_unstable();
    ids.dedup();
    let rows = ids
        .into_iter()
        .filter(|id| {
            let exchanged = quoted.get(id).map(|close| close.exchanged).unwrap_or(0.0);
            shows_good(factuals, *id, shown(exchanged))
        })
        .map(|id| {
            let (amv, salability, decay, percent) = match market.goods.get(&id) {
                Some(good) => {
                    let marks = amv_move(morning.prices.get(&id).copied(), good.amv);
                    (
                        format!("{marks} {}", qty_text(good.amv)),
                        qty_text(good.salability),
                        qty_text(good.decayed),
                        format!("{}%", qty_text(decay_percent(good))),
                    )
                }
                None => (
                    String::new(),
                    String::new(),
                    String::new(),
                    String::new(),
                ),
            };
            let (offered, requested, exchanged, price) = match quoted.get(&id) {
                Some(close) => (
                    qty_text(close.offered),
                    qty_text(close.requested),
                    qty_text(close.exchanged),
                    mass_price(close),
                ),
                None => (String::new(), String::new(), String::new(), String::new()),
            };
            vec![
                good_name(factuals, id).to_string(),
                amv,
                salability,
                decay,
                percent,
                offered,
                requested,
                exchanged,
                price,
            ]
        })
        .collect::<Vec<_>>();
    print_table(
        "market board",
        &[
            "good",
            "amv",
            "salability",
            "decay",
            "decay %",
            "offered",
            "requested",
            "exchanged",
            "price",
        ],
        &rows,
    );
}

/// Share of the morning ask that still counts as one mark.
const AMV_MOVE_MILD: f64 = 0.03;

/// Share of the morning ask that still counts as two marks.
const AMV_MOVE_STRONG: f64 = 0.10;

/// # AMV Move
///
/// Three columns for how `after` moved from the morning ask.
///
/// `before` is that ask, when the good was on the morning card. No card
/// leaves the columns blank. `=` is a price the board still prints the same,
/// so a negative ask is not written with a dash. `^` is a rise and `v` is a
/// fall. One mark is under [`AMV_MOVE_MILD`]. Two marks are under
/// [`AMV_MOVE_STRONG`]. Three marks are anything larger. The result is
/// always three columns, so the two extra marks have room.
fn amv_move(before: Option<f64>, after: f64) -> String {
    let Some(before) = before else {
        return "   ".to_string();
    };
    if !before.is_finite() || !after.is_finite() || qty_text(before) == qty_text(after) {
        return format!("{:<3}", "=");
    }
    let share = (after - before).abs() / before.abs().max(1e-9);
    let count = if share < AMV_MOVE_MILD {
        1
    } else if share < AMV_MOVE_STRONG {
        2
    } else {
        3
    };
    let glyph = if after > before { "^" } else { "v" };
    format!("{:<3}", glyph.repeat(count))
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

/// # Shows Good
///
/// Whether a table keeps `id`.
///
/// A skill stays only when `traded` is true. Any other good stays.
/// `traded` is the deal: units exchanged, or units this pop bought or
/// sold. A holding, a listing, and decay are not a deal. Returns that
/// answer.
fn shows_good(factuals: &Factuals, id: usize, traded: bool) -> bool {
    traded || !factuals.get_good(id).is_some_and(|good| good.is_skill())
}

/// # Print Pop Look
///
/// The work row for one pop: craft, schedule, holdings, and buy/sell.
///
/// `history` prices the schedule, the morning card from the day just
/// printed. Holdings and the book are the pop's state now. A skill line
/// stays when [`shows_good`] keeps it. A missing pop prints that and
/// returns.
fn print_pop_look(factuals: &Factuals, actors: &Actors, history: &MarketHistory, id: usize) {
    if !living_pops(actors).contains(&id) {
        println!("no pop {id}");
        return;
    }
    let pop = actors.pop(id);
    let tape = pop.day_tape();
    let (held, traded) = good_lines(factuals, pop, &tape);
    print_table(
        &format!("pop {id}"),
        &["pop", "craft", "schedule", "holdings", "buy/sell"],
        &[vec![
            id.to_string(),
            craft_name(factuals, pop.job.craft),
            schedule(factuals, pop, history),
            held,
            traded,
        ]],
    );
}

/// # Good Lines
///
/// One line per good, shared by holdings and the columns beside it.
///
/// Goods are lowest id first. A line is kept when the pop holds the good
/// or the day tape has a buy or a sell, and [`shows_good`] keeps it. A
/// holding that prints as zero shows the name alone, so a buy with no
/// stock still has a line. Buy and sell share the second cell: `buy` and
/// `sell` are offers, `bought` and `sold` are the deal good, and `back+`
/// and `back-` are change. No goods returns `nothing` for the holding and an
/// empty cell beside it.
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
        let dealt = shown(tape_qty(&tape.bought, id)) || shown(tape_qty(&tape.sold, id));
        if !shows_good(factuals, id, dealt) {
            continue;
        }
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
    if held.is_empty() {
        return ("nothing".to_string(), String::new());
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

#[cfg(test)]
mod amv_move_tests {
    use super::amv_move;

    /// A held price is `=`. A rise and a fall take one, two, or three marks.
    #[test]
    fn amv_move_marks_direction_and_size() {
        assert_eq!(amv_move(None, 1.0), "   ");
        assert_eq!(amv_move(Some(1.0), 1.0), "=  ");
        assert_eq!(amv_move(Some(1.0), 1.00004), "=  ");
        assert_eq!(amv_move(Some(1.0), 1.02), "^  ");
        assert_eq!(amv_move(Some(1.0), 1.05), "^^ ");
        assert_eq!(amv_move(Some(1.0), 1.2), "^^^");
        assert_eq!(amv_move(Some(2.0), 1.95), "v  ");
        assert_eq!(amv_move(Some(2.0), 1.9), "vv ");
        assert_eq!(amv_move(Some(-2.0), -2.4), "vvv");
        assert_eq!(amv_move(Some(-2.0), -1.6), "^^^");
    }
}
