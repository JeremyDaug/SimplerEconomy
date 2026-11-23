//! The firm tester. `data/firm_tester` adds farmhand labor, agribusiness,
//! millhand labor, the cotton farm, the thread mill, the fabric mill, and
//! the clothes mill. The grain pop and one other pop sell farmhand labor.
//! The clothes pop and one other pop sell millhand labor. Pop 30 owns
//! Agribusiness. Pop 32 owns the Clothes Mill. Pop 33 owns the Cotton Farm.
//! Pop 34 owns the Thread Mill. Pop 35 owns the Fabric Mill. It runs the
//! current turn.
//!
//! Loads `data/world`, then `goods.toml`, `processes.toml`, `desires.toml`,
//! `species.toml`, `cultures.toml`, `pops.toml`, `market.toml`, and
//! `firms.toml` from that folder, and runs [`PlayState::advance_turn`].
//! The scenario goods and processes are added before names resolve. A species
//! or culture desire is derived from a platonic desire.
//!
//! The first argument is how many days to run before asking for more
//! (default 1). The second is how many times a pop's people its ceiling
//! share is when that cap is set (default 1000). A pop with no cap receives
//! that share before the turn. After those days, enter runs one more, a
//! number runs that many, and 0 quits. `pops` shows or hides the pop table.
//! A terminal prints that day again without advancing. The table is one row
//! per pop. The line above it is the turn those rows finished. A terminal
//! prints each finished
//! day below the last one, so the window can scroll. A pipe prints them
//! once, when the batch ends. Craft names the process, then its share of
//! morning time and the iterations finished over the iterations asked, in
//! whole units, from that line's `completed` and `attempted`. A pop with
//! no craft prints `-` there. Margin is the percentage gain or loss on
//! the priced inputs, or `-` when those inputs have no cost. Per time is
//! the profit on that line. Subsistence and labor are shares of that pop's
//! time. Labor shows the time share and the same finished-over-asked
//! fraction. Savings and stock are priced per household, in the pop's
//! first market currency. In and out are the households that arrived and
//! the households that left. SOL is the standard of living recorded before
//! decay. The last column is why that pop stopped buying.
//!
//! Under the pops, a firm table lists each production line. Process is
//! the name. Target is the iterations sought, `open` when the line has no
//! cap, and `off` at 0. Success is the iterations finished over the
//! iterations asked, in whole units. Inputs, output, sold, wage, and
//! dividend are that firm's day in abstract value. Inputs are the
//! destroyed or consumed inputs. Output is what the runs put out. Sold is
//! what sold, at the price then. Wage and dividend are what the owners
//! were paid.
//!
//! Under the firms, a founding table lists the next attempt and the three
//! splits most likely if that attempt hits. Chance is that attempt. Rest
//! is the days before it. Pop is the parent and the craft they already
//! do. Process is the work they would take. Split is that pair's share of
//! a hit. Result is `new`, or `join` and the pop who already does that
//! work. The line under the rows is what this day did, and the chance it
//! used. A wait prints `today rest`.
//!
//! Under that, each market lists its goods. Supply is the units
//! offered at the start of the market day, in whole units. Demand is the
//! units that market recorded as sought, in whole units. Exchange is the
//! units of that good exchanged today. Consumption is the units consumed
//! today, by processes and by desires. Salability is that good's share, or
//! the config default when the market has no row. `amv` is the abstract
//! market value of one unit. A value carries `+` when it rose since
//! yesterday, `-` when it fell, and `=` when it held. A good with no stored
//! value prints `-`. The first stored value has no mark. Time and skills
//! are left out.
//!
//! Above the pops, an info table lists one household of each demographic.
//! Subsistence is that household's value per unit of time. Day savings is
//! one day of its basic desires.
//!
//! ```text
//! cargo run --example firm_tester
//! cargo run --example firm_tester -- 1
//! cargo run --example firm_tester -- 30 10
//! ```

use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use simpler_economy::game::actors::Actors;
use simpler_economy::game::desire::{DemoDesire, DesireTarget};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::firm::Firm;
use simpler_economy::game::good::TIME;
use simpler_economy::game::map::Map;
use simpler_economy::game::mapdata::{EnvironmentData, MapData};
use simpler_economy::game::market::{FoundingEvent, Market};
use simpler_economy::game::players::Players;
use simpler_economy::game::pop::{line_value, market_price, Pop};
use simpler_economy::game::scalingfactor::ScalingFactor;
use simpler_economy::playstate::PlayState;

mod load;

use load::load_scenario;

const WORLD_DIR: &str = "world";
const SCENARIO_DIR: &str = "firm_tester";
/// Parent and process pairs the founding table keeps.
const FOUNDING_ROWS: usize = 3;
/// A pop's ceiling share is this many times its people when the cap is set.
const CAP_MULTIPLIER: f64 = 1000.0;

fn main() {
    let args = tester_args();
    let mut days = args.days;
    let mut state = stand_up();
    // Starters are capped before the banner. A later pop is capped before its next turn.
    assign_ceilings(&mut state, args.cap_multiplier);
    let live = io::stdout().is_terminal();
    print_banner(&state, args.cap_multiplier);
    let mut rows = Vec::new();
    let mut show_pops = true;
    // `latest` is the day just finished. `earlier` is the day before it,
    // which the price marks compare against. The pipe prints on the next
    // pass, so `earlier` stays put until that print.
    let mut earlier = Prices::default();
    let mut latest = Prices::default();
    loop {
        if days == 0 {
            if !live && !rows.is_empty() {
                print_table(&state, &rows, &earlier, show_pops);
            }
            loop {
                match read_prompt() {
                    Prompt::Days(0) => return,
                    Prompt::Days(next) => {
                        days = next;
                        break;
                    }
                    Prompt::TogglePops => {
                        show_pops = !show_pops;
                        println!("{}", if show_pops { "pop table on" } else { "pop table off" });
                        if live && !rows.is_empty() {
                            print_table(&state, &rows, &earlier, show_pops);
                        }
                    }
                }
            }
        }
        assign_ceilings(&mut state, args.cap_multiplier);
        state.advance_turn();
        rows = pop_rows(&state);
        earlier = std::mem::replace(&mut latest, snapshot_prices(&state));
        if live {
            print_table(&state, &rows, &earlier, show_pops);
        }
        days -= 1;
    }
}

/// Days to run, and how wide each pop's ceiling share is.
struct TesterArgs {
    days: u32,
    cap_multiplier: f64,
}

/// # Tester Args
///
/// Days to run before the prompt, and the ceiling multiplier.
///
/// The first argument is the day count. A missing argument or one that is
/// not a whole number is 1. The second is how many times a pop's people its
/// cap is. A missing argument, or one that is not a positive finite number,
/// is [`CAP_MULTIPLIER`]. Returns those two.
fn tester_args() -> TesterArgs {
    let days = match std::env::args().nth(1) {
        Some(arg) => arg.parse::<u32>().unwrap_or(1),
        None => 1,
    };
    let cap_multiplier = match std::env::args().nth(2) {
        Some(arg) => arg
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.0)
            .unwrap_or(CAP_MULTIPLIER),
        None => CAP_MULTIPLIER,
    };
    TesterArgs { days, cap_multiplier }
}

/// # Assign Ceilings
///
/// Gives each uncapped pop its share of the market ceiling.
///
/// `multiplier` is how many times that pop's current people the share is.
/// A pop that already has a cap keeps it, so a share does not grow with the
/// pop. A pop created during the turn stays uncapped until the next call.
/// [`Market::rebalance_capped`] reads these shares after growth.
fn assign_ceilings(state: &mut PlayState, multiplier: f64) {
    debug_assert!(multiplier.is_finite() && multiplier > 0.0);
    for pop in state.actors.pops.values_mut() {
        if pop.population_cap.is_some() {
            continue;
        }
        let people = pop.demographics.total_population();
        pop.population_cap = Some(people * multiplier);
    }
}

/// # Ceiling People
///
/// The market ceiling, in people.
///
/// The ceiling is the sum of each pop's [`Pop::population_cap`]. A pop
/// with no cap adds nothing. Returns that sum.
fn ceiling_people(state: &PlayState) -> f64 {
    state
        .actors
        .pops
        .values()
        .filter_map(|pop| pop.population_cap)
        .sum()
}

/// # Stand Up
///
/// The world folder and `data/firm_tester`, at turn 0.
///
/// Species, cultures, pops, firms, and markets come from [`load_scenario`].
/// The map is one hex. Returns that play state.
fn stand_up() -> PlayState {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    let world = root.join(WORLD_DIR);
    let mut factuals = Factuals::load_from_path(&world).unwrap_or_else(|err| {
        eprintln!("load {}: {err}", world.display());
        std::process::exit(1);
    });
    let scenario = root.join(SCENARIO_DIR);
    let loaded = load_scenario(&scenario, &mut factuals).unwrap_or_else(|err| {
        eprintln!("load {}: {err}", scenario.display());
        std::process::exit(1);
    });
    for species in loaded.species {
        factuals.species.insert(species.id, species);
    }
    for culture in loaded.cultures {
        factuals.cultures.insert(culture.id, culture);
    }
    let mut actors = Actors::new();
    for pop in loaded.pops {
        actors.pops.insert(pop.id, pop);
    }
    for firm in loaded.firms {
        actors.firms.insert(firm.id, firm);
    }
    let mut markets = HashMap::new();
    for market in loaded.markets {
        markets.insert(market.id, market);
    }
    PlayState {
        factuals,
        map_data: MapData {
            map: Map::new(1, 1),
            markets,
            environment: EnvironmentData {},
        },
        actors,
        players: Players { players: HashMap::new() },
        turn: 0,
        is_paused: false,
    }
}

/// # Print Banner
///
/// The opening lines for this run.
///
/// `state` is the loaded stand-up. Desires, the job, and currencies are
/// read from that state. `cap_multiplier` is the share width already
/// applied to each uncapped pop. The turn is printed with the table.
fn print_banner(state: &PlayState, cap_multiplier: f64) {
    println!("firm tester — data/{SCENARIO_DIR}");
    println!("world: data/{WORLD_DIR}");
    println!(
        "ceiling: {} people, {}× size when capped",
        qty_text(ceiling_people(state)),
        qty_text(cap_multiplier)
    );
    let mut species_ids: Vec<usize> = state.factuals.species.keys().copied().collect();
    species_ids.sort_unstable();
    for id in &species_ids {
        let species = &state.factuals.species[id];
        let summary = desire_summary(&state.factuals, &species.desires);
        println!("{} desires: {summary}", species.name);
    }
    let mut culture_ids: Vec<usize> = state.factuals.cultures.keys().copied().collect();
    culture_ids.sort_unstable();
    for id in &culture_ids {
        let culture = &state.factuals.cultures[id];
        let summary = desire_summary(&state.factuals, &culture.desires);
        println!("{} desires: {summary}", culture.name);
    }
    let mut pop_ids: Vec<usize> = state.actors.pops.keys().copied().collect();
    pop_ids.sort_unstable();
    for id in &pop_ids {
        let pop = state.actors.pop(*id);
        let summary = job_summary(&state.factuals, pop);
        if pop_ids.len() == 1 {
            println!("{summary}");
        } else {
            println!("{}: {summary}", demographic(state, pop));
        }
    }
    let mut firm_ids: Vec<usize> = state.actors.firms.keys().copied().collect();
    firm_ids.sort_unstable();
    for id in firm_ids {
        let firm = state.actors.firm(id);
        println!("firm: {} — {}", firm.name, firm_lines(&state.factuals, firm));
    }
    let mut market_ids: Vec<usize> = state.map_data.markets.keys().copied().collect();
    market_ids.sort_unstable();
    for id in &market_ids {
        let market = state.map_data.market(*id);
        let names: Vec<String> = market.currencies.iter().copied().map(|good| good_name(&state.factuals, good)).collect();
        let names = names.join(", ");
        if market_ids.len() == 1 {
            println!("currencies: {names}");
        } else {
            println!("market {id}: {names}");
        }
    }
}

/// # Firm Lines
///
/// The production lines on `firm`, in list order.
///
/// A missing process prints its id. A target of 0 prints `off`. No target
/// prints `open`. A set target prints that number. Returns that text.
fn firm_lines(factuals: &Factuals, firm: &Firm) -> String {
    if firm.production_line.is_empty() {
        return "no lines".to_string();
    }
    let mut names = Vec::new();
    for line in &firm.production_line {
        let name = factuals
            .get_process(line.process)
            .map(|process| process.name.clone())
            .unwrap_or_else(|| line.process.to_string());
        match line.target {
            Some(target) if target == 0.0 => names.push(format!("{name} off")),
            Some(target) => names.push(format!("{name} {}", qty_text(target))),
            None => names.push(format!("{name} open")),
        }
    }
    names.join(", ")
}

/// # Desire Summary
///
/// `desires` as one line.
///
/// Desires are ordered by tier, then priority, then id. Goods in one
/// desire are joined with "or". Desires in one tier are joined with
/// "and", then the tier name. When every desire shares an amount, a
/// scalar, and a decay, that phrase is appended once. Otherwise each
/// desire carries its own. Returns that text.
fn desire_summary(factuals: &Factuals, desires: &HashMap<usize, DemoDesire>) -> String {
    let mut desires: Vec<_> = desires.values().collect();
    desires.sort_by_key(|desire| (desire.tier, desire.priority, desire.id));
    if desires.is_empty() {
        return "none".to_string();
    }
    let shared = desires.iter().all(|desire| {
        desire.amount == desires[0].amount
            && desire.decay == desires[0].decay
            && scalar_scale(desire.scalar) == scalar_scale(desires[0].scalar)
            && std::mem::discriminant(&desire.scalar) == std::mem::discriminant(&desires[0].scalar)
    });
    let mut parts = Vec::new();
    let mut index = 0;
    while index < desires.len() {
        let tier = desires[index].tier;
        let mut names = Vec::new();
        while index < desires.len() && desires[index].tier == tier {
            let label = bucket_label(factuals, &desires[index].bucket);
            if shared {
                names.push(label);
            } else {
                let terms = terms_phrase(desires[index].amount, desires[index].scalar, desires[index].decay);
                names.push(format!("{label} ({terms})"));
            }
            index += 1;
        }
        parts.push(format!("{} {}", names.join(" and "), tier_name(tier)));
    }
    if shared {
        parts.push(terms_phrase(desires[0].amount, desires[0].scalar, desires[0].decay));
    }
    parts.join(", ")
}

/// # Bucket Label
///
/// The goods in `bucket`, in bucket order.
///
/// Names are joined with "or". An empty bucket is "empty". Returns that label.
fn bucket_label(factuals: &Factuals, bucket: &[DesireTarget]) -> String {
    if bucket.is_empty() {
        return "empty".to_string();
    }
    bucket.iter().map(|target| good_name(factuals, target.good)).collect::<Vec<_>>().join(" or ")
}

/// # Tier Name
///
/// The name of desire tier `tier`.
///
/// 0 is basic, 1 is common, and 2 is luxury. Any other tier is its number.
/// Returns that name.
fn tier_name(tier: usize) -> String {
    match tier {
        0 => "basic".to_string(),
        1 => "common".to_string(),
        2 => "luxury".to_string(),
        other => other.to_string(),
    }
}

/// # Terms Phrase
///
/// Amount, scalar, and decay as one phrase.
///
/// Returns that phrase.
fn terms_phrase(amount: f64, scalar: ScalingFactor, decay: f64) -> String {
    format!("{}, {}", amount_phrase(amount, scalar), decay_phrase(decay))
}

/// # Amount Phrase
///
/// `amount` with the unit `scalar` names.
///
/// A scale of 1 leaves the multiplier off. Returns that phrase.
fn amount_phrase(amount: f64, scalar: ScalingFactor) -> String {
    let (scale, unit) = match scalar {
        ScalingFactor::Fixed(scale) => (scale, "fixed"),
        ScalingFactor::All(scale) => (scale, "per person"),
        ScalingFactor::Household(scale) => (scale, "per household"),
        ScalingFactor::Adults(scale) => (scale, "per adult"),
        ScalingFactor::Children(scale) => (scale, "per child"),
        ScalingFactor::Elders(scale) => (scale, "per elder"),
        ScalingFactor::Labor(scale) => (scale, "per labor"),
    };
    if unit == "fixed" {
        return if scale == 1.0 { qty_text(amount) } else { format!("{} x {}", qty_text(amount), qty_text(scale)) };
    }
    if scale == 1.0 {
        format!("{} {unit}", qty_text(amount))
    } else {
        format!("{} x {} {unit}", qty_text(amount), qty_text(scale))
    }
}

/// # Decay Phrase
///
/// How satisfaction lasts the night.
///
/// Decay 1 clears it. Decay 0 keeps it. Any other decay is named.
/// Returns that phrase.
fn decay_phrase(decay: f64) -> String {
    if decay == 1.0 {
        "cleared each night".to_string()
    } else if decay == 0.0 {
        "kept each night".to_string()
    } else {
        format!("decay {}", qty_text(decay))
    }
}

/// # Scalar Scale
///
/// The multiplier carried by `scalar`.
///
/// Returns that number.
fn scalar_scale(scalar: ScalingFactor) -> f64 {
    match scalar {
        ScalingFactor::Fixed(scale)
        | ScalingFactor::All(scale)
        | ScalingFactor::Household(scale)
        | ScalingFactor::Adults(scale)
        | ScalingFactor::Children(scale)
        | ScalingFactor::Elders(scale)
        | ScalingFactor::Labor(scale) => scale,
    }
}

/// # Job Summary
///
/// The job line for `pop`.
///
/// A line whose process is missing is left out. Every remaining line at
/// target 0 is "job opens off" and the process names. Any other target
/// is printed after its process. No lines is "job: none". Returns that line.
fn job_summary(factuals: &Factuals, pop: &Pop) -> String {
    let mut names = Vec::new();
    let mut off = true;
    for line in &pop.job.lines {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        match line.target {
            Some(target) if target == 0.0 => names.push(process.name.clone()),
            Some(target) => {
                off = false;
                names.push(format!("{} {}", process.name, qty_text(target)));
            }
            None => {
                off = false;
                names.push(format!("{} open", process.name));
            }
        }
    }
    if names.is_empty() {
        return "job: none".to_string();
    }
    let head = if off { "job opens off" } else { "job" };
    format!("{head}: {}", names.join(", "))
}

/// One pop on the current day.
struct PopRow {
    pop: String,
    craft: String,
    margin: String,
    per_time: String,
    subsistence: String,
    labor: String,
    size: String,
    migration_in: String,
    migration_out: String,
    savings: String,
    stock: String,
    sol: String,
    stop: String,
}

/// # Pop Rows
///
/// One row for each pop in `state`, lowest id first.
///
/// Returns those rows.
fn pop_rows(state: &PlayState) -> Vec<PopRow> {
    let mut ids: Vec<usize> = state.actors.pops.keys().copied().collect();
    ids.sort_unstable();
    ids.into_iter().map(|id| pop_row(state, state.actors.pop(id))).collect()
}

/// # Pop Row
///
/// `pop` as its id, craft, margin, profit per time, subsistence, labor,
/// size, households that arrived, households that left, savings, stock,
/// SOL, and why buying stopped.
///
/// Craft is the non-labor line: its name, its share of morning time, and
/// the finished-over-asked fraction. A pop with no craft prints `-` for
/// the craft, the margin, and the profit per time. Margin and per time
/// come from [`line_value`]. Subsistence names its type and the share of
/// this pop's morning time. Labor is the time and the fraction for labor
/// lines. The plan is the one written at the end of the day. The fraction
/// sums `completed` and `attempted` on those lines, with the fractional
/// part dropped. A group with no lines shows a dash. Size is the number of
/// people. Savings and stock come from [`holding_value`], divided by the
/// household count. SOL is [`Pop::recorded_living`]. In and out are the
/// household counts on [`Pop::records`]. Stop is why this pop stopped
/// buying. Returns that row.
fn pop_row(state: &PlayState, pop: &Pop) -> PopRow {
    let budget = pop.time_grant(&state.factuals);
    let market = home_market(state, pop.id);
    let mut craft = "-".to_string();
    let mut margin = "-".to_string();
    let mut per_time = "-".to_string();
    let mut labor = 0.0;
    let mut labor_completed = 0.0;
    let mut labor_attempted = 0.0;
    let mut labor_lines = false;
    for line in &pop.job.lines {
        let Some(process) = state.factuals.get_process(line.process) else {
            continue;
        };
        let time = planned_time(line.target, process.required_time());
        if process.is_labor() {
            labor += time;
            labor_completed += line.completed;
            labor_attempted += line.attempted;
            labor_lines = true;
            continue;
        }
        let value = line_value(process, market);
        craft = plan_text(
            Some(process.name.clone()),
            time_share(time, budget),
            Some((line.completed, line.attempted)),
        );
        margin = margin_text(value.margin_percent());
        per_time = qty_text(value.per_time);
    }
    let (savings, stock) = holding_value(state, pop);
    let households = pop.demographics.household.count;
    // A large pop's pile is the household's pile, so the column stays readable.
    let per_household = |total: f64| {
        if households.is_finite() && households > 0.0 {
            total / households
        } else {
            0.0
        }
    };
    PopRow {
        pop: pop.id.to_string(),
        craft,
        margin,
        per_time,
        subsistence: format!(
            "{} {}",
            pop.subsistence_type.name(),
            percent_text(pop.subsistence_rate * 100.0)
        ),
        labor: plan_text(
            None,
            time_share(labor, budget),
            labor_lines.then_some((labor_completed, labor_attempted)),
        ),
        size: qty_text(pop.demographics.total_population()),
        migration_in: qty_text(pop.records.migration_in),
        migration_out: qty_text(pop.records.migration_out),
        savings: qty_text(per_household(savings)),
        stock: qty_text(per_household(stock)),
        sol: qty_text(pop.recorded_living().unwrap_or(0.0)),
        stop: stop_text(state, pop),
    }
}

/// # Stop Text
///
/// Why `pop` stopped buying, with good names from `state`.
///
/// A missing good is its id. Returns that text.
fn stop_text(state: &PlayState, pop: &Pop) -> String {
    pop.shop_stop.label(|good| {
        state
            .factuals
            .get_good(good)
            .map(|good| good.name.clone())
            .unwrap_or_else(|| good.to_string())
    })
}

/// # Holding Value
///
/// Savings and stock for `pop`, in abstract value.
///
/// The market is the lowest id that lists the pop. Savings sums every
/// good on that market's currency list, from property and from
/// [`Pop::savings`]. Stock sums every other good in those two places.
/// A pop on no market has no currencies, so savings is 0 and stock is
/// the whole holding. Prices come from [`market_price`]. Returns
/// `(savings, stock)`.
fn holding_value(state: &PlayState, pop: &Pop) -> (f64, f64) {
    let market = home_market(state, pop.id);
    let currencies: &[usize] = market.map(|market| market.currencies.as_slice()).unwrap_or(&[]);
    let mut savings = 0.0;
    for &currency in currencies {
        let amount = on_hand(pop, currency) + saved(pop, currency);
        savings += amount * market_price(market, currency);
    }
    let mut stock = 0.0;
    for (&good, row) in &pop.property {
        if currencies.contains(&good) {
            continue;
        }
        stock += row.quantity * market_price(market, good);
    }
    for (&good, &amount) in pop.savings() {
        if currencies.contains(&good) {
            continue;
        }
        stock += amount * market_price(market, good);
    }
    (savings, stock)
}

/// # Home Market
///
/// The market with the lowest id that lists `pop_id`.
///
/// A pop listed on none is `None`. Returns that market.
fn home_market(state: &PlayState, pop_id: usize) -> Option<&Market> {
    let mut ids: Vec<usize> = state.map_data.markets.keys().copied().collect();
    ids.sort_unstable();
    ids.into_iter()
        .find(|id| state.map_data.market(*id).pops.contains(&pop_id))
        .map(|id| state.map_data.market(id))
}

/// # Demographic
///
/// The species name for `pop`, with the culture name when one is set.
///
/// A species id of 0 is human. A culture id of 0 is left off. A missing
/// name is the id. Returns that label.
fn demographic(state: &PlayState, pop: &Pop) -> String {
    let species_id = pop.demographics.species;
    let species = state
        .factuals
        .species
        .get(&species_id)
        .map(|species| species.name.clone())
        .unwrap_or_else(|| {
            if species_id == 0 {
                "human".to_string()
            } else {
                species_id.to_string()
            }
        });
    let culture_id = pop.demographics.culture;
    if culture_id == 0 {
        return species;
    }
    let culture = state
        .factuals
        .cultures
        .get(&culture_id)
        .map(|culture| culture.name.clone())
        .unwrap_or_else(|| culture_id.to_string());
    format!("{species} {culture}")
}

/// # Planned Time
///
/// Time claimed by a job target.
///
/// `target` is the line's iterations. `None` is no time. `time_per` is
/// the required time of one iteration. No required time treats the target
/// as time already. Returns that time.
fn planned_time(target: Option<f64>, time_per: f64) -> f64 {
    let iterations = target.unwrap_or(0.0);
    if time_per > 0.0 {
        iterations * time_per
    } else {
        iterations
    }
}

/// # Plan Text
///
/// A plan share and one finished-over-asked fraction.
///
/// `name` is the processes in the group, when the caller has one.
/// `plan` is the plan's percent of morning time. `run` is
/// `(completed, attempted)`. The fraction drops the fractional part.
/// No lines is a dash. Returns that text.
fn plan_text(name: Option<String>, plan: f64, run: Option<(f64, f64)>) -> String {
    let run = match run {
        Some((completed, attempted)) => {
            format!("{}/{}", whole_text(completed), whole_text(attempted))
        }
        None => "-".to_string(),
    };
    match name {
        Some(name) => format!("{name} {} {run}", percent_text(plan)),
        None => format!("{} {run}", percent_text(plan)),
    }
}

/// # Whole Text
///
/// `qty` with the fractional part dropped.
///
/// A value that is not finite prints as itself. Returns that text.
fn whole_text(qty: f64) -> String {
    if qty.is_finite() {
        format!("{}", qty.trunc() as i64)
    } else {
        format!("{qty}")
    }
}

/// # Time Share
///
/// `part` as a percent of `budget`.
///
/// A budget of 0 is 0. Returns that percent.
fn time_share(part: f64, budget: f64) -> f64 {
    if budget == 0.0 { 0.0 } else { part / budget * 100.0 }
}

/// # On Hand
///
/// Quantity of `good` this pop still holds.
///
/// A missing row is 0. Returns that quantity.
fn on_hand(pop: &Pop, good: usize) -> f64 {
    pop.property.get(&good).map(|row| row.quantity).unwrap_or(0.0)
}

/// # Saved
///
/// Amount of `good` in [`Pop::savings`].
///
/// A missing entry is 0. Returns that amount.
fn saved(pop: &Pop, good: usize) -> f64 {
    pop.savings().get(&good).copied().unwrap_or(0.0)
}

/// # Percent Text
///
/// `percent` with one decimal and a percent sign.
///
/// Returns that text.
fn percent_text(percent: f64) -> String {
    format!("{percent:.1}%")
}

/// One demographic on the info table.
struct InfoRow {
    demographic: String,
    subsistence: String,
    day: String,
}

/// # One Household
///
/// `pop` scaled to a single household of the same makeup.
///
/// Adult, elder, and child averages stay. Household count becomes 1, and
/// every desire amount is divided by the old count. A count of 0 is left
/// as it is. Returns that pop.
fn one_household(pop: &Pop) -> Pop {
    let mut one = pop.clone();
    let count = one.demographics.household.count;
    if count > 0.0 {
        let scale = 1.0 / count;
        for tier in &mut one.desires {
            for desire in tier {
                desire.amount *= scale;
            }
        }
        one.demographics.household.count = 1.0;
    }
    one
}

/// # Info Rows
///
/// One row for each demographic in `state`.
///
/// The pop with the lowest id stands for that demographic. Subsistence is
/// [`Pop::subsistence_per_time`] for [`one_household`] on its market. Day
/// savings is [`Pop::day_savings`] for that same household. A pop on no
/// market prices subsistence at 1 and has day savings of 0. Returns those
/// rows.
fn info_rows(state: &PlayState) -> Vec<InfoRow> {
    let mut ids: Vec<usize> = state.actors.pops.keys().copied().collect();
    ids.sort_unstable();
    let mut rows = Vec::new();
    let mut seen = Vec::new();
    for id in ids {
        let pop = state.actors.pop(id);
        let name = demographic(state, pop);
        if seen.contains(&name) {
            continue;
        }
        seen.push(name.clone());
        let market = home_market(state, id);
        let one = one_household(pop);
        rows.push(InfoRow {
            demographic: name,
            subsistence: qty_text(one.subsistence_per_time(&state.factuals, market)),
            day: qty_text(market.map(|market| one.day_savings(market)).unwrap_or(0.0)),
        });
    }
    rows
}

/// # Print Info
///
/// Draws subsistence per unit of time and one day of basic desires.
///
/// One row per demographic, for a single household of that makeup. The
/// columns are demographic, subsistence/time, and day savings.
fn print_info(state: &PlayState) {
    let rows = info_rows(state);
    println!("info");
    let demo_w = col_width("demographic", rows.iter().map(|row| row.demographic.len()));
    let sub_w = col_width("subsistence/time", rows.iter().map(|row| row.subsistence.len()));
    let day_w = col_width("day savings", rows.iter().map(|row| row.day.len()));
    println!(
        "{:<demo_w$}  {:>sub_w$}  {:>day_w$}",
        "demographic",
        "subsistence/time",
        "day savings",
        demo_w = demo_w,
        sub_w = sub_w,
        day_w = day_w,
    );
    for row in &rows {
        println!(
            "{:<demo_w$}  {:>sub_w$}  {:>day_w$}",
            row.demographic,
            row.subsistence,
            row.day,
            demo_w = demo_w,
            sub_w = sub_w,
            day_w = day_w,
        );
    }
}

/// # Print Table
///
/// Draws the day, the info table, the pop table when `show_pops` is set,
/// then firms, founding, and one table per market.
///
/// The day is [`PlayState::turn`]. `show_pops` leaves the pop rows out
/// when false. The firm table follows, then founding, then the market
/// tables. `prior` is yesterday's prices for the markets. Writes those
/// tables below what is already printed.
fn print_table(
    state: &PlayState,
    rows: &[PopRow],
    prior: &Prices,
    show_pops: bool,
) {
    println!();
    println!("day: {}", state.turn);
    print_info(state);
    if show_pops {
        print_pop_table(rows);
    }
    print_firm_table(state);
    print_founding(state);
    print_market_tables(state, prior);
    let _ = io::stdout().flush();
}

/// # Print Pop Table
///
/// Draws one row per pop in `rows`.
///
/// A blank line separates it from the info table. Columns are pop, craft,
/// margin, per time, subsistence, labor, size, in, out, savings per
/// household, stock per household, sol, and stop.
fn print_pop_table(rows: &[PopRow]) {
    println!();
    let pop_w = col_width("pop", rows.iter().map(|row| row.pop.len()));
    let craft_w = col_width("craft", rows.iter().map(|row| row.craft.len()));
    let margin_w = col_width("margin", rows.iter().map(|row| row.margin.len()));
    let per_time_w = col_width("per time", rows.iter().map(|row| row.per_time.len()));
    let sub_w = col_width("subsistence", rows.iter().map(|row| row.subsistence.len()));
    let labor_w = col_width("labor", rows.iter().map(|row| row.labor.len()));
    let size_w = col_width("size", rows.iter().map(|row| row.size.len()));
    // A zero is narrower than `xx.xxxx`. The floor holds that width every day.
    let in_w = col_width("in", rows.iter().map(|row| row.migration_in.len())).max(7);
    let out_w = col_width("out", rows.iter().map(|row| row.migration_out.len())).max(7);
    let savings_w = col_width("savings/hh", rows.iter().map(|row| row.savings.len()));
    let stock_w = col_width("stock/hh", rows.iter().map(|row| row.stock.len()));
    let sol_w = col_width("sol", rows.iter().map(|row| row.sol.len()));
    let stop_w = col_width("stop", rows.iter().map(|row| row.stop.len()));
    println!(
        "{}",
        table_line(
            "pop", "craft", "margin", "per time", "subsistence", "labor", "size", "in", "out",
            "savings/hh", "stock/hh", "sol", "stop",
            pop_w, craft_w, margin_w, per_time_w, sub_w, labor_w, size_w, in_w, out_w,
            savings_w, stock_w, sol_w, stop_w,
        )
    );
    for row in rows {
        println!(
            "{}",
            table_line(
                &row.pop, &row.craft, &row.margin, &row.per_time, &row.subsistence, &row.labor,
                &row.size, &row.migration_in, &row.migration_out, &row.savings, &row.stock,
                &row.sol, &row.stop,
                pop_w, craft_w, margin_w, per_time_w, sub_w, labor_w, size_w, in_w, out_w,
                savings_w, stock_w, sol_w, stop_w,
            )
        );
    }
}

/// One production line on the firm table.
struct FirmRow {
    firm: String,
    process: String,
    target: String,
    success: String,
    inputs: String,
    output: String,
    sold: String,
    wage: String,
    dividend: String,
}

/// One likely split on the founding table.
struct FoundingRow {
    chance: String,
    rest: String,
    pop: String,
    process: String,
    split: String,
    result: String,
}

/// # Print Founding
///
/// Draws the founding table for each market, lowest id first.
///
/// A blank line separates it from the firms. Chance and rest are the next
/// attempt. The rows are the most likely parent and process pairs. The
/// line under them is the attempt this day already ran.
fn print_founding(state: &PlayState) {
    let mut ids: Vec<usize> = state.map_data.markets.keys().copied().collect();
    ids.sort_unstable();
    let only = ids.len() == 1;
    for id in ids {
        print_founding_market(state, id, only);
    }
}

/// # Print Founding Market
///
/// Draws the founding table for market `id`.
///
/// `only` leaves the market id off the title when the run has one market.
/// Chance is the next attempt. Rest is the days before it. Up to
/// [`FOUNDING_ROWS`] pairs follow, highest split share first. An empty
/// list still prints the chance and the rest. The last line is what this
/// day did.
fn print_founding_market(state: &PlayState, id: usize, only: bool) {
    let market = state.map_data.market(id);
    let outlook = market.founding_outlook(&state.actors.pops, &state.actors.firms, &state.factuals);
    let mut rows = Vec::new();
    if outlook.prospects.is_empty() {
        rows.push(FoundingRow {
            chance: percent_text(outlook.chance * 100.0),
            rest: outlook.rest.to_string(),
            pop: "-".to_string(),
            process: "-".to_string(),
            split: "-".to_string(),
            result: "-".to_string(),
        });
    }
    for (index, prospect) in outlook.prospects.iter().take(FOUNDING_ROWS).enumerate() {
        rows.push(FoundingRow {
            chance: if index == 0 { percent_text(outlook.chance * 100.0) } else { String::new() },
            rest: if index == 0 { outlook.rest.to_string() } else { String::new() },
            pop: pop_label(state, prospect.parent),
            process: match prospect.firm {
                Some(firm) => firm_name(state, firm),
                None => process_name(state, prospect.process),
            },
            split: percent_text(prospect.chance * 100.0),
            result: match prospect.firm {
                Some(firm) => format!("firm {firm}"),
                None => match prospect.joins {
                    Some(dest) => format!("join {}", pop_label(state, dest)),
                    None => "new".to_string(),
                },
            },
        });
    }
    println!();
    if only {
        println!("founding");
    } else {
        println!("founding {id}");
    }
    let chance_w = col_width("chance", rows.iter().map(|row| row.chance.len()));
    let rest_w = col_width("rest", rows.iter().map(|row| row.rest.len()));
    let pop_w = col_width("pop", rows.iter().map(|row| row.pop.len()));
    let process_w = col_width("process", rows.iter().map(|row| row.process.len()));
    let split_w = col_width("split", rows.iter().map(|row| row.split.len()));
    let result_w = col_width("result", rows.iter().map(|row| row.result.len()));
    println!(
        "{:>chance_w$}  {:>rest_w$}  {:<pop_w$}  {:<process_w$}  {:>split_w$}  {:<result_w$}",
        "chance",
        "rest",
        "pop",
        "process",
        "split",
        "result",
        chance_w = chance_w,
        rest_w = rest_w,
        pop_w = pop_w,
        process_w = process_w,
        split_w = split_w,
        result_w = result_w,
    );
    for row in &rows {
        println!(
            "{:>chance_w$}  {:>rest_w$}  {:<pop_w$}  {:<process_w$}  {:>split_w$}  {:<result_w$}",
            row.chance,
            row.rest,
            row.pop,
            row.process,
            row.split,
            row.result,
            chance_w = chance_w,
            rest_w = rest_w,
            pop_w = pop_w,
            process_w = process_w,
            split_w = split_w,
            result_w = result_w,
        );
    }
    println!("{}", founding_today(state, market));
}

/// # Founding Today
///
/// What market `market` did on this day's founding attempt.
///
/// A wait names no chance. Any other result names the chance that attempt
/// used. A new pop names its id, the parent, and the process. A join names
/// the pop who received the households. A restarted firm names the firm,
/// the owner, and the parent. Returns that line.
fn founding_today(state: &PlayState, market: &Market) -> String {
    let Some(day) = market.founding_day() else {
        return "today -".to_string();
    };
    let chance = percent_text(day.chance * 100.0);
    match day.event {
        FoundingEvent::Rest => "today rest".to_string(),
        FoundingEvent::Miss => format!("today miss {chance}"),
        FoundingEvent::Nobody => format!("today nobody {chance}"),
        FoundingEvent::New { parent, process, id } => format!(
            "today {chance} new pop {id} from {} as {}",
            pop_label(state, parent),
            process_name(state, process),
        ),
        FoundingEvent::Join { parent, process, dest } => format!(
            "today {chance} join {} from {} as {}",
            pop_label(state, dest),
            pop_label(state, parent),
            process_name(state, process),
        ),
        FoundingEvent::Firm { parent, id, firm } => format!(
            "today {chance} firm {firm} owned by pop {} from {}",
            pop_label(state, id),
            pop_label(state, parent),
        ),
    }
}

/// # Firm Name
///
/// The loaded name of firm `id`.
///
/// A missing firm panics. Returns that name.
fn firm_name(state: &PlayState, id: usize) -> String {
    state.actors.firm(id).name.clone()
}

/// # Pop Label
///
/// `id` and the craft that pop already runs.
///
/// A missing pop is the id. A pop with no loaded process is idle.
/// Returns that label.
fn pop_label(state: &PlayState, id: usize) -> String {
    let Some(pop) = state.actors.pops.get(&id) else {
        return id.to_string();
    };
    format!("{id} {}", craft_name(state, pop))
}

/// # Craft Name
///
/// The craft process on `pop`, or its labor process when it has no craft.
///
/// A line whose process is missing is left out. No loaded line is idle.
/// Returns that name.
fn craft_name(state: &PlayState, pop: &Pop) -> String {
    let mut labor = None;
    for line in &pop.job.lines {
        let Some(process) = state.factuals.get_process(line.process) else {
            continue;
        };
        if process.is_labor() {
            if labor.is_none() {
                labor = Some(process.name.clone());
            }
        } else {
            return process.name.clone();
        }
    }
    labor.unwrap_or_else(|| "idle".to_string())
}

/// # Process Name
///
/// The loaded name of process `id`.
///
/// A missing process is its id. Returns that name.
fn process_name(state: &PlayState, id: usize) -> String {
    state
        .factuals
        .get_process(id)
        .map(|process| process.name.clone())
        .unwrap_or_else(|| id.to_string())
}

/// # Print Firm Table
///
/// Draws one row for each firm production line, lowest firm id first.
///
/// A blank line separates it from the pops. A firm with no lines still
/// prints, so the day shows that firm. The money columns are that firm's day.
fn print_firm_table(state: &PlayState) {
    let rows = firm_rows(state);
    println!();
    println!("firms");
    let firm_w = col_width("firm", rows.iter().map(|row| row.firm.len()));
    let process_w = col_width("process", rows.iter().map(|row| row.process.len()));
    let target_w = col_width("target", rows.iter().map(|row| row.target.len()));
    let success_w = col_width("success", rows.iter().map(|row| row.success.len()));
    let inputs_w = col_width("inputs", rows.iter().map(|row| row.inputs.len()));
    let output_w = col_width("output", rows.iter().map(|row| row.output.len()));
    let sold_w = col_width("sold", rows.iter().map(|row| row.sold.len()));
    let wage_w = col_width("wage", rows.iter().map(|row| row.wage.len()));
    let dividend_w = col_width("dividend", rows.iter().map(|row| row.dividend.len()));
    println!(
        "{:<firm_w$}  {:<process_w$}  {:>target_w$}  {:>success_w$}  {:>inputs_w$}  {:>output_w$}  {:>sold_w$}  {:>wage_w$}  {:>dividend_w$}",
        "firm",
        "process",
        "target",
        "success",
        "inputs",
        "output",
        "sold",
        "wage",
        "dividend",
        firm_w = firm_w,
        process_w = process_w,
        target_w = target_w,
        success_w = success_w,
        inputs_w = inputs_w,
        output_w = output_w,
        sold_w = sold_w,
        wage_w = wage_w,
        dividend_w = dividend_w,
    );
    for row in &rows {
        println!(
            "{:<firm_w$}  {:<process_w$}  {:>target_w$}  {:>success_w$}  {:>inputs_w$}  {:>output_w$}  {:>sold_w$}  {:>wage_w$}  {:>dividend_w$}",
            row.firm,
            row.process,
            row.target,
            row.success,
            row.inputs,
            row.output,
            row.sold,
            row.wage,
            row.dividend,
            firm_w = firm_w,
            process_w = process_w,
            target_w = target_w,
            success_w = success_w,
            inputs_w = inputs_w,
            output_w = output_w,
            sold_w = sold_w,
            wage_w = wage_w,
            dividend_w = dividend_w,
        );
    }
}

/// # Firm Rows
///
/// One row for each production line in `state`, lowest firm id first.
///
/// A firm with no lines gets one row so it still appears. Target is the
/// iterations sought. A dead firm prints `dead` there. Success is the
/// iterations finished over the iterations asked, with the fractional part
/// dropped. A firm with no lines prints `-` there. The money columns are
/// that firm's day, repeated on each of its lines. Returns those rows.
fn firm_rows(state: &PlayState) -> Vec<FirmRow> {
    let mut ids: Vec<usize> = state.actors.firms.keys().copied().collect();
    ids.sort_unstable();
    let mut rows = Vec::new();
    for id in ids {
        let firm = state.actors.firm(id);
        let inputs = qty_text(firm.budget.input_cost);
        let output = qty_text(firm.budget.output_value);
        let sold = qty_text(firm.budget.sold_value);
        let wage = qty_text(firm.budget.wage);
        let dividend = qty_text(firm.budget.dividend);
        let dead = firm.is_dead();
        if firm.production_line.is_empty() {
            rows.push(FirmRow {
                firm: firm.name.clone(),
                process: "-".to_string(),
                target: if dead {
                    "dead".to_string()
                } else {
                    "-".to_string()
                },
                success: "-".to_string(),
                inputs,
                output,
                sold,
                wage,
                dividend,
            });
            continue;
        }
        for line in &firm.production_line {
            rows.push(FirmRow {
                firm: firm.name.clone(),
                process: process_name(state, line.process),
                target: if dead {
                    "dead".to_string()
                } else {
                    firm_target_text(line.target)
                },
                success: format!(
                    "{}/{}",
                    whole_text(line.completed),
                    whole_text(line.attempted),
                ),
                inputs: inputs.clone(),
                output: output.clone(),
                sold: sold.clone(),
                wage: wage.clone(),
                dividend: dividend.clone(),
            });
        }
    }
    rows
}

/// # Firm Target Text
///
/// The iterations sought on a firm line.
///
/// No target is `open`. A target of 0 is `off`. Any other target is that
/// number. Returns that text.
fn firm_target_text(target: Option<f64>) -> String {
    match target {
        Some(target) if target == 0.0 => "off".to_string(),
        Some(target) => qty_text(target),
        None => "open".to_string(),
    }
}

/// # Margin Text
///
/// `percent` with one decimal, or `-` when the recipe has no priced cost.
///
/// Returns that text.
fn margin_text(percent: Option<f64>) -> String {
    match percent {
        Some(percent) => percent_text(percent),
        None => "-".to_string(),
    }
}

/// Yesterday's abstract values.
#[derive(Default)]
struct Prices {
    amv: HashMap<(usize, usize), f64>,
}

/// One good on a market table.
struct MarketRow {
    good: String,
    supply: String,
    demand: String,
    exchange: String,
    consumption: String,
    salability: String,
    amv: String,
}

/// # Print Market Tables
///
/// Draws one table for each market in `state`, lowest id first.
///
/// A blank line separates a table from the founding table, and from the
/// market before it. `prior` is yesterday's prices.
fn print_market_tables(state: &PlayState, prior: &Prices) {
    let mut ids: Vec<usize> = state.map_data.markets.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        println!();
        print_market(state, state.map_data.market(id), prior);
    }
}

/// # Print Market
///
/// Draws `market`'s goods.
///
/// The first line is the market id. Columns are good, supply, demand,
/// exchange, consumption, salability, and the abstract value. The good
/// column sits on the left. The other columns sit on the right. `prior`
/// is yesterday's values.
fn print_market(state: &PlayState, market: &Market, prior: &Prices) {
    let rows: Vec<MarketRow> = listed_goods(state, market)
        .into_iter()
        .map(|id| market_row(state, market, id, prior))
        .collect();
    println!("market: {}", market.id);
    let good_w = col_width("good", rows.iter().map(|row| row.good.len()));
    let supply_w = col_width("supply", rows.iter().map(|row| row.supply.len()));
    let demand_w = col_width("demand", rows.iter().map(|row| row.demand.len()));
    let exchange_w = col_width("exchange", rows.iter().map(|row| row.exchange.len()));
    let consumption_w = col_width("consumption", rows.iter().map(|row| row.consumption.len()));
    let salability_w = col_width("salability", rows.iter().map(|row| row.salability.len()));
    let amv_w = col_width("amv", rows.iter().map(|row| row.amv.len()));
    let head = vec![
        "good".to_string(),
        "supply".to_string(),
        "demand".to_string(),
        "exchange".to_string(),
        "consumption".to_string(),
        "salability".to_string(),
        "amv".to_string(),
    ];
    let widths = vec![
        good_w, supply_w, demand_w, exchange_w, consumption_w, salability_w, amv_w,
    ];
    println!("{}", market_line(&head, &widths));
    for row in &rows {
        let cells = vec![
            row.good.clone(),
            row.supply.clone(),
            row.demand.clone(),
            row.exchange.clone(),
            row.consumption.clone(),
            row.salability.clone(),
            row.amv.clone(),
        ];
        println!("{}", market_line(&cells, &widths));
    }
}

/// # Market Line
///
/// One aligned line of a market table.
///
/// `cells` and `widths` are the columns, in order. The first column sits
/// on the left. The rest sit on the right. Returns that line.
fn market_line(cells: &[String], widths: &[usize]) -> String {
    let mut line = String::new();
    for (index, (cell, width)) in cells.iter().zip(widths).enumerate() {
        if index > 0 {
            line.push_str("  ");
        }
        if index == 0 {
            line.push_str(&format!("{cell:<width$}"));
        } else {
            line.push_str(&format!("{cell:>width$}"));
        }
    }
    line
}

/// # Listed Goods
///
/// Goods to print for `market`, lowest id first.
///
/// A currency is always included. Any other good is included when members
/// hold some, the market recorded supply, demand, trade, output, use,
/// rot, or stock for it, or it has a stored abstract value. Time and
/// skills are left out. Returns those ids.
fn listed_goods(state: &PlayState, market: &Market) -> Vec<usize> {
    let mut ids = Vec::new();
    let mut remember = |id: usize| {
        let skill = state
            .factuals
            .get_good(id)
            .is_some_and(|good| good.is_skill());
        if id != TIME && !skill && !ids.contains(&id) {
            ids.push(id);
        }
    };
    for &id in market.currencies() {
        remember(id);
    }
    for &id in market.goods.keys() {
        remember(id);
    }
    for id in market.priced_goods() {
        remember(id);
    }
    for &pop_id in &market.pops {
        let pop = state.actors.pop(pop_id);
        for &id in pop.property.keys() {
            remember(id);
        }
        for &id in pop.savings().keys() {
            remember(id);
        }
    }
    for &firm_id in &market.firms {
        let firm = state.actors.firm(firm_id);
        for &id in firm.property.keys() {
            remember(id);
        }
    }
    ids.retain(|&id| on_the_market(state, market, id));
    ids.sort_unstable();
    ids
}

/// # On The Market
///
/// Whether `good` belongs on `market`'s table.
///
/// A currency belongs. So does a good members hold, a good with a stored
/// abstract value, and a good whose market row recorded sought, offered, volume,
/// output, use, rot, or stock. Returns that check.
fn on_the_market(state: &PlayState, market: &Market, good: usize) -> bool {
    if market.currencies().contains(&good) || market.stored_amv(good).is_some() {
        return true;
    }
    if held_units(state, market, good) != 0.0 {
        return true;
    }
    let Some(row) = market.goods.get(&good) else {
        return false;
    };
    row.sought != 0.0
        || row.offered != 0.0
        || row.volume != 0.0
        || row.production != 0.0
        || row.consumption != 0.0
        || row.decayed != 0.0
        || row.stock != 0.0
}

/// # Market Row
///
/// `good` as a market-table row.
///
/// Supply comes from [`offered_units`] and demand from [`sought_units`],
/// both in whole units. Exchange comes from [`exchanged_units`].
/// Consumption comes from [`consumed_units`]. Salability comes from
/// [`salability_of`]. The abstract value is [`Market::stored_amv`], marked
/// against `prior`. Returns that row.
fn market_row(state: &PlayState, market: &Market, good: usize, prior: &Prices) -> MarketRow {
    MarketRow {
        good: good_name(&state.factuals, good),
        supply: whole_text(offered_units(market, good)),
        demand: whole_text(sought_units(market, good)),
        exchange: qty_text(exchanged_units(market, good)),
        consumption: qty_text(consumed_units(market, good)),
        salability: percent_text(salability_of(state, market, good) * 100.0),
        amv: price_text(market.stored_amv(good), prior.amv.get(&(market.id, good)).copied()),
    }
}

/// # Held Units
///
/// Units of `good` the market's pops and firms hold.
///
/// A pop contributes property quantity and [`Pop::savings`]. A firm
/// contributes property quantity. A missing row is 0. Returns that sum.
fn held_units(state: &PlayState, market: &Market, good: usize) -> f64 {
    let mut total = 0.0;
    for &pop_id in &market.pops {
        let pop = state.actors.pop(pop_id);
        total += on_hand(pop, good) + saved(pop, good);
    }
    for &firm_id in &market.firms {
        let firm = state.actors.firm(firm_id);
        total += firm
            .property
            .get(&good)
            .map(|row| row.quantity)
            .unwrap_or(0.0);
    }
    total
}

/// # Offered Units
///
/// Units of `good` posted for sale at the start of the market day.
///
/// A missing row is 0. Returns that amount.
fn offered_units(market: &Market, good: usize) -> f64 {
    market
        .goods
        .get(&good)
        .map(|row| row.offered)
        .unwrap_or(0.0)
}

/// # Consumed Units
///
/// Units of `good` the market recorded as consumed today.
///
/// A missing row is 0. Returns that amount.
fn consumed_units(market: &Market, good: usize) -> f64 {
    market
        .goods
        .get(&good)
        .map(|row| row.consumption)
        .unwrap_or(0.0)
}

/// # Exchanged Units
///
/// Units of `good` the market recorded as exchanged today.
///
/// A missing row is 0. Returns that amount.
fn exchanged_units(market: &Market, good: usize) -> f64 {
    market
        .goods
        .get(&good)
        .map(|row| row.volume)
        .unwrap_or(0.0)
}

/// # Sought Units
///
/// Units of `good` the market recorded as sought.
///
/// A missing row is 0. Returns that amount.
fn sought_units(market: &Market, good: usize) -> f64 {
    market
        .goods
        .get(&good)
        .map(|row| row.sought)
        .unwrap_or(0.0)
}

/// # Salability Of
///
/// The salability share recorded for `good` on `market`.
///
/// A missing row uses the market config default. Returns that share.
fn salability_of(state: &PlayState, market: &Market, good: usize) -> f64 {
    market
        .goods
        .get(&good)
        .map(|row| row.salability)
        .unwrap_or(state.factuals.config.market.salability_default)
}

/// # Snapshot Prices
///
/// Today's abstract value of each listed good.
///
/// A value is keyed by market id and good id. Returns that map.
fn snapshot_prices(state: &PlayState) -> Prices {
    let mut prices = Prices::default();
    let mut ids: Vec<usize> = state.map_data.markets.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let market = state.map_data.market(id);
        for good in listed_goods(state, market) {
            if let Some(amv) = market.stored_amv(good) {
                prices.amv.insert((id, good), amv);
            }
        }
    }
    prices
}

/// # Price Text
///
/// `price` with a mark against `prior` when both are stored.
///
/// No current price is `-`. `+` means the price rose, `-` means it fell,
/// and `=` means it held. A price with no prior is unmarked. Returns that
/// text.
fn price_text(price: Option<f64>, prior: Option<f64>) -> String {
    let Some(price) = price else {
        return "-".to_string();
    };
    match prior {
        Some(earlier) => format!("{} {}", qty_text(price), price_mark(price, earlier)),
        None => qty_text(price),
    }
}

/// # Price Mark
///
/// Whether `price` rose, fell, or held against `prior`.
///
/// A gap smaller than a billionth counts as holding. Returns `+`, `-`,
/// or `=`.
fn price_mark(price: f64, prior: f64) -> char {
    const HOLD: f64 = 1e-9;
    if price > prior + HOLD {
        '+'
    } else if price + HOLD < prior {
        '-'
    } else {
        '='
    }
}

/// # Col Width
///
/// The wider of `header` and the cell lengths in `cells`.
///
/// Returns that width.
fn col_width(header: &str, cells: impl Iterator<Item = usize>) -> usize {
    cells.fold(header.len(), |width, cell| width.max(cell))
}

/// # Table Line
///
/// One aligned line of the pop table.
///
/// The widths are the columns. Pop, craft, and subsistence sit on the
/// left. The other columns sit on the right. Returns that line.
fn table_line(
    pop: &str, craft: &str, margin: &str, per_time: &str, subsistence: &str, labor: &str,
    size: &str, arrived: &str, left: &str, savings: &str, stock: &str, sol: &str, stop: &str,
    pop_w: usize, craft_w: usize, margin_w: usize, per_time_w: usize, sub_w: usize,
    labor_w: usize, size_w: usize, in_w: usize, out_w: usize, savings_w: usize, stock_w: usize,
    sol_w: usize, stop_w: usize,
) -> String {
    format!(
        "{pop:<pop_w$}  {craft:<craft_w$}  {margin:>margin_w$}  {per_time:>per_time_w$}  {subsistence:<sub_w$}  {labor:>labor_w$}  {size:>size_w$}  {arrived:>in_w$}  {left:>out_w$}  {savings:>savings_w$}  {stock:>stock_w$}  {sol:>sol_w$}  {stop:>stop_w$}"
    )
}

/// The next thing the prompt asked for.
enum Prompt {
    /// Run this many more days. Zero quits.
    Days(u32),
    /// Show the pop table, or hide it when it is already shown.
    TogglePops,
}

/// # Read Prompt
///
/// Reads the next command.
///
/// An empty line is one day. `0` quits. Any other whole number is that
/// many days. `pops` shows or hides the pop table. A line that is neither
/// asks again. End of input quits. Returns that command.
fn read_prompt() -> Prompt {
    loop {
        print!("run more days (enter = 1, 0 = quit, pops = table): ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => return Prompt::Days(0),
            Ok(_) => {
                let line = line.trim();
                if line.eq_ignore_ascii_case("pops") {
                    return Prompt::TogglePops;
                }
                match line.parse::<u32>() {
                    Ok(days) => return Prompt::Days(days),
                    Err(_) if line.is_empty() => return Prompt::Days(1),
                    Err(_) => println!("enter a whole number, press enter for one day, or pops"),
                }
            }
            Err(_) => return Prompt::Days(0),
        }
    }
}

/// # Good Name
///
/// The loaded name of `id`, or the id when the good is missing.
fn good_name(factuals: &Factuals, id: usize) -> String {
    factuals
        .get_good(id)
        .map(|good| good.name.clone())
        .unwrap_or_else(|| id.to_string())
}

/// # Qty Text
///
/// `qty` as a short decimal.
///
/// A whole number inside a billion prints as an integer. Any other finite
/// number prints with up to four decimals and no trailing zeros.
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
