//! The pops in `data/pop_tester`, and the current turn.
//!
//! Loads `data/world`, then `desires.toml`, `species.toml`, `cultures.toml`,
//! `pops.toml`, and `market.toml` from that folder, and runs
//! [`PlayState::advance_turn`]. Good and process names resolve against the
//! world. A species or culture desire is derived from a platonic desire.
//!
//! The first argument is how many days to run before asking for more
//! (default 1). After those days, enter runs one more, a number runs
//! that many, and 0 quits. The table is one row per pop. The line above
//! it is the turn those rows finished. A terminal draws that line and
//! the table again after each day. A pipe prints them once, when the
//! batch ends. Subsistence, craft, and labor are shares of that pop's time.
//! Craft names its processes, then shows the time share and one fraction,
//! the iterations finished over the iterations asked, in whole units,
//! from those lines' `completed` and `attempted`. Labor shows the same
//! share and fraction.
//! Savings and stock are priced in the pop's first market currency.
//! SOL is the standard of living recorded before decay. Hope, happy,
//! fear, and anger are that pop's sentiment shares. Contentment is
//! whatever those four leave.
//!
//! ```text
//! cargo run --example pop_tester
//! cargo run --example pop_tester -- 1
//! ```

use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use simpler_economy::game::actors::Actors;
use simpler_economy::game::desire::{DemoDesire, DesireTarget};
use simpler_economy::game::factuals::Factuals;
use simpler_economy::game::map::Map;
use simpler_economy::game::mapdata::{EnvironmentData, MapData};
use simpler_economy::game::market::{Market, MarketLookups};
use simpler_economy::game::players::Players;
use simpler_economy::game::pop::{market_price, Pop};
use simpler_economy::game::scalingfactor::ScalingFactor;
use simpler_economy::playstate::PlayState;

mod load;

use load::load_scenario;

const WORLD_DIR: &str = "world";
const SCENARIO_DIR: &str = "pop_tester";

fn main() {
    let mut days = tester_args();
    let mut state = stand_up();
    let live = io::stdout().is_terminal();
    print_banner(&state);
    let mut rows = Vec::new();
    let mut origin_saved = false;
    loop {
        if days == 0 {
            if !live && !rows.is_empty() {
                print_table(&rows, state.turn, false, &mut origin_saved);
            }
            days = more_days();
            if days == 0 {
                break;
            }
        }
        state.advance_turn();
        rows = pop_rows(&state);
        if live {
            print_table(&rows, state.turn, true, &mut origin_saved);
        }
        days -= 1;
    }
}

/// # Tester Args
///
/// Days to run before the prompt.
///
/// The first argument is that count. A missing argument or one that is
/// not a whole number is 1. Returns that count.
fn tester_args() -> u32 {
    match std::env::args().nth(1) {
        Some(arg) => arg.parse::<u32>().unwrap_or(1),
        None => 1,
    }
}

/// # Stand Up
///
/// The world folder and `data/pop_tester`, at turn 0.
///
/// Species, cultures, pops, and markets come from [`load_scenario`]. The
/// map is one hex. Returns that play state.
fn stand_up() -> PlayState {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    let world = root.join(WORLD_DIR);
    let mut factuals = Factuals::load_from_path(&world).unwrap_or_else(|err| {
        eprintln!("load {}: {err}", world.display());
        std::process::exit(1);
    });
    let scenario = root.join(SCENARIO_DIR);
    let loaded = load_scenario(&scenario, &factuals).unwrap_or_else(|err| {
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
        market_lookups: MarketLookups::new(),
    }
}

/// # Print Banner
///
/// The opening lines for this run.
///
/// `state` is the loaded stand-up. Desires, the job, and currencies are
/// read from that state. The turn is printed with the table.
fn print_banner(state: &PlayState) {
    println!("pop tester — data/{SCENARIO_DIR}");
    println!("world: data/{WORLD_DIR}");
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
    demographic: String,
    subsistence: String,
    craft: String,
    labor: String,
    size: String,
    savings: String,
    stock: String,
    sol: String,
    hope: String,
    happy: String,
    fear: String,
    anger: String,
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
/// `pop` as demographic, subsistence, craft, labor, size, savings, stock,
/// SOL, and mood.
///
/// Subsistence names its type and the share of this pop's morning time.
/// Craft names every job line that is not labor, then the planned time
/// and one fraction for those lines: iterations finished over iterations
/// asked, with the fractional part dropped. Labor is the time and the
/// fraction for labor lines. The plan is the one written at the end of
/// the day. The fraction sums `completed` and `attempted` on the lines
/// in that group. A group with no lines shows a dash. Size
/// is the number of people. Savings and stock come from [`holding_value`].
/// SOL is [`Pop::recorded_living`]. Hope, happy, fear, and anger are
/// those sentiment shares. Returns that row.
fn pop_row(state: &PlayState, pop: &Pop) -> PopRow {
    let budget = pop.get_scaling_factor(ScalingFactor::Labor(
        state.factuals.config.labor.time_per_labor,
    ));
    let mut craft = 0.0;
    let mut labor = 0.0;
    let mut craft_names: Vec<&str> = Vec::new();
    let mut craft_completed = 0.0;
    let mut craft_attempted = 0.0;
    let mut craft_lines = false;
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
        } else {
            craft += time;
            craft_completed += line.completed;
            craft_attempted += line.attempted;
            craft_lines = true;
            if !craft_names.contains(&process.name.as_str()) {
                craft_names.push(process.name.as_str());
            }
        }
    }
    let (savings, stock) = holding_value(state, pop);
    PopRow {
        demographic: demographic(state, pop),
        subsistence: format!(
            "{} {}",
            pop.subsistence_type.name(),
            percent_text(pop.subsistence_rate * 100.0)
        ),
        craft: plan_text(
            (!craft_names.is_empty()).then(|| craft_names.join(", ")),
            time_share(craft, budget),
            craft_lines.then_some((craft_completed, craft_attempted)),
        ),
        labor: plan_text(
            None,
            time_share(labor, budget),
            labor_lines.then_some((labor_completed, labor_attempted)),
        ),
        size: qty_text(pop.demographics.total_population()),
        savings: qty_text(savings),
        stock: qty_text(stock),
        sol: qty_text(pop.recorded_living().unwrap_or(0.0)),
        hope: percent_text(pop.sentiment.hope() * 100.0),
        happy: percent_text(pop.sentiment.happiness() * 100.0),
        fear: percent_text(pop.sentiment.fear() * 100.0),
        anger: percent_text(pop.sentiment.anger() * 100.0),
    }
}

/// # Holding Value
///
/// Savings and stock for `pop`, in its market's first currency.
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

/// # Print Table
///
/// Draws one row per pop in `rows`.
///
/// `day` is the turn `rows` finished. Columns are demographic,
/// subsistence, craft, labor, size, savings, stock, sol, hope, happy,
/// fear, and anger. On a terminal the cursor returns to the day line and
/// the lines under it are cleared, so this call replaces the previous
/// day and table. A pipe writes the lines in order. `origin_saved`
/// records that the terminal cursor was stored.
fn print_table(rows: &[PopRow], day: u64, live: bool, origin_saved: &mut bool) {
    if live {
        // Return to the day line and erase the previous table and prompt.
        if *origin_saved {
            print!("\x1b[u\x1b[J");
        } else {
            print!("\x1b[s");
            *origin_saved = true;
        }
    }
    println!("day: {day}");
    let demo_w = col_width("demographic", rows.iter().map(|row| row.demographic.len()));
    let sub_w = col_width("subsistence", rows.iter().map(|row| row.subsistence.len()));
    let craft_w = col_width("craft", rows.iter().map(|row| row.craft.len()));
    let labor_w = col_width("labor", rows.iter().map(|row| row.labor.len()));
    let size_w = col_width("size", rows.iter().map(|row| row.size.len()));
    let savings_w = col_width("savings", rows.iter().map(|row| row.savings.len()));
    let stock_w = col_width("stock", rows.iter().map(|row| row.stock.len()));
    let sol_w = col_width("sol", rows.iter().map(|row| row.sol.len()));
    let hope_w = col_width("hope", rows.iter().map(|row| row.hope.len()));
    let happy_w = col_width("happy", rows.iter().map(|row| row.happy.len()));
    let fear_w = col_width("fear", rows.iter().map(|row| row.fear.len()));
    let anger_w = col_width("anger", rows.iter().map(|row| row.anger.len()));
    println!(
        "{}",
        table_line(
            "demographic", "subsistence", "craft", "labor", "size", "savings", "stock",
            "sol", "hope", "happy", "fear", "anger",
            demo_w, sub_w, craft_w, labor_w, size_w, savings_w, stock_w,
            sol_w, hope_w, happy_w, fear_w, anger_w,
        )
    );
    for row in rows {
        println!(
            "{}",
            table_line(
                &row.demographic, &row.subsistence, &row.craft, &row.labor, &row.size,
                &row.savings, &row.stock, &row.sol, &row.hope, &row.happy, &row.fear,
                &row.anger,
                demo_w, sub_w, craft_w, labor_w, size_w, savings_w, stock_w,
                sol_w, hope_w, happy_w, fear_w, anger_w,
            )
        );
    }
    let _ = io::stdout().flush();
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
/// The widths are the columns. Demographic and subsistence sit on the
/// left. The other columns sit on the right. Returns that line.
fn table_line(
    demographic: &str, subsistence: &str, craft: &str, labor: &str, size: &str,
    savings: &str, stock: &str, sol: &str, hope: &str, happy: &str, fear: &str,
    anger: &str, demo_w: usize, sub_w: usize, craft_w: usize, labor_w: usize,
    size_w: usize, savings_w: usize, stock_w: usize, sol_w: usize, hope_w: usize,
    happy_w: usize, fear_w: usize, anger_w: usize,
) -> String {
    format!(
        "{demographic:<demo_w$}  {subsistence:<sub_w$}  {craft:>craft_w$}  {labor:>labor_w$}  {size:>size_w$}  {savings:>savings_w$}  {stock:>stock_w$}  {sol:>sol_w$}  {hope:>hope_w$}  {happy:>happy_w$}  {fear:>fear_w$}  {anger:>anger_w$}"
    )
}

/// # More Days
///
/// Reads how many more turns to run.
///
/// An empty line is one day. `0` quits. Any other whole number is that
/// many days. A line that is not a whole number asks again. End of input
/// quits. Returns that count.
fn more_days() -> u32 {
    loop {
        print!("run more days (enter = 1, 0 = quit): ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => return 0,
            Ok(_) => match line.trim().parse::<u32>() {
                Ok(days) => return days,
                Err(_) if line.trim().is_empty() => return 1,
                Err(_) => println!("enter a whole number, or press enter for one day"),
            },
            Err(_) => return 0,
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
