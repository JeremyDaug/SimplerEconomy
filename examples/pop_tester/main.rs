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
//! it is the turn those rows finished. A terminal keeps one screen and
//! replaces that day and the tables. A pipe prints them once, when the
//! batch ends. Subsistence and labor are shares of that pop's time.
//! Labor shows the time share and one fraction, the iterations finished
//! over the iterations asked, in whole units, from those lines'
//! `completed` and `attempted`.
//! Savings and stock are priced in the pop's first market currency.
//! SOL is the standard of living recorded before decay. Hope, happy,
//! fear, and anger are that pop's sentiment shares. Contentment is
//! whatever those four leave. The last column is why that pop stopped buying.
//!
//! Under the pops, a craft table lists each craft line. Craft names the
//! process, then its share of morning time and the same finished-over-asked
//! fraction. Profit, salability, demand, decay, durability, living, and idle
//! are that line's planning votes. A vote prints `+1`, `-1`, or `0`. Demand
//! yays when buyers wanted more of the output than traded, and nays when
//! they wanted some and got it. A day with no bids stays quiet. Luck is
//! left out of the tally and prints `-`. Idle prints `+3` when craft and
//! labor are both at 0%. Margin is the percentage gain or loss on the priced
//! inputs, or `-` when those inputs have no cost. Per time is the profit
//! that vote reads. Tally is the sum of the votes.
//! Step is `expand`, `hold`, or `contract`. `bar` means the tally would
//! expand and subsistence turned that into a hold.
//!
//! Under the crafts, each market lists its goods. Supply is the units
//! offered at the start of the market day, in whole units. Demand is the
//! units that market recorded as sought, in whole units. Exchange is the
//! units of that good exchanged today. Consumption is the units consumed
//! today, by processes and by desires. Salability is that good's share, or
//! the config default when the market has no row. Each currency has its own
//! price column, headed by `in` and that currency's name. A price carries
//! `+` when it rose since yesterday, `-` when it fell, and `=` when it
//! held. A good with no stored price prints `-`. The currency priced in
//! itself is 1. The first stored price has no mark. Time is left out.
//!
//! Above the pops, an info table lists one household of each demographic.
//! Subsistence is that household's value per unit of time. Day savings is
//! one day of its basic desires.
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
use simpler_economy::game::good::TIME;
use simpler_economy::game::map::Map;
use simpler_economy::game::mapdata::{EnvironmentData, MapData};
use simpler_economy::game::market::{Market, MarketLookups};
use simpler_economy::game::players::Players;
use simpler_economy::game::pop::{line_value, market_price, plan_votes, PlanVotes, Pop};
use simpler_economy::game::scalingfactor::ScalingFactor;
use simpler_economy::playstate::PlayState;

mod load;

use load::load_scenario;

const WORLD_DIR: &str = "world";
const SCENARIO_DIR: &str = "pop_tester";

fn main() {
    let mut days = tester_args();
    let mut state = stand_up();
    let live = LiveView::enter();
    print_banner(&state);
    let mut rows = Vec::new();
    // `latest` is the day just finished. `earlier` is the day before it,
    // which the price marks compare against. The pipe prints on the next
    // pass, so `earlier` stays put until that print.
    let mut earlier = HashMap::new();
    let mut latest = HashMap::new();
    loop {
        if days == 0 {
            if !live.is_on() && !rows.is_empty() {
                print_table(&state, &rows, false, &earlier);
            }
            days = more_days();
            if days == 0 {
                break;
            }
        }
        state.advance_turn();
        rows = pop_rows(&state);
        earlier = std::mem::replace(&mut latest, snapshot_prices(&state));
        if live.is_on() {
            print_table(&state, &rows, true, &earlier);
        }
        days -= 1;
    }
}

/// One live screen for the tester.
///
/// A terminal switches to the alternate screen and comes back when this
/// value drops. A pipe leaves the terminal alone.
struct LiveView {
    on: bool,
}

impl LiveView {
    /// # Enter
    ///
    /// Opens the alternate screen when stdout is a terminal.
    ///
    /// A pipe does not. Interrupt and terminate restore that screen.
    /// Returns the guard.
    fn enter() -> Self {
        let on = io::stdout().is_terminal();
        if on {
            print!("\x1b[?1049h");
            let _ = io::stdout().flush();
            install_restore();
        }
        Self { on }
    }

    /// # Is On
    ///
    /// Whether this run paints over one screen.
    ///
    /// Returns that check.
    fn is_on(&self) -> bool {
        self.on
    }
}

impl Drop for LiveView {
    fn drop(&mut self) {
        if self.on {
            restore_terminal();
        }
    }
}

/// # Install Restore
///
/// Restores the main screen if the run is interrupted or terminated.
fn install_restore() {
    unsafe {
        signal(2, restore_and_exit);
        signal(15, restore_and_exit);
    }
}

/// # Restore Terminal
///
/// Leaves the alternate screen.
fn restore_terminal() {
    let mut out = io::stdout();
    let _ = write!(out, "\x1b[?1049l");
    let _ = out.flush();
}

/// Restores the main screen and exits.
extern "C" fn restore_and_exit(_signal: i32) {
    let bytes = b"\x1b[?1049l";
    unsafe {
        write_fd(1, bytes.as_ptr(), bytes.len());
    }
    std::process::exit(130);
}

unsafe extern "C" {
    fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
    #[link_name = "write"]
    fn write_fd(fd: i32, buf: *const u8, count: usize) -> isize;
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
    labor: String,
    size: String,
    savings: String,
    stock: String,
    sol: String,
    hope: String,
    happy: String,
    fear: String,
    anger: String,
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
/// `pop` as demographic, subsistence, labor, size, savings, stock, SOL,
/// mood, and why buying stopped.
///
/// Subsistence names its type and the share of this pop's morning time.
/// Labor is the time and the fraction for labor lines. The plan is the
/// one written at the end of the day. The fraction sums `completed` and
/// `attempted` on those lines, with the fractional part dropped. A group
/// with no lines shows a dash. Size is the number of people. Savings and
/// stock come from [`holding_value`].
/// SOL is [`Pop::recorded_living`]. Hope, happy, fear, and anger are
/// those sentiment shares. Stop is why this pop stopped buying.
/// Returns that row.
fn pop_row(state: &PlayState, pop: &Pop) -> PopRow {
    let budget = pop.get_scaling_factor(ScalingFactor::Labor(
        state.factuals.config.labor.time_per_labor,
    ));
    let mut labor = 0.0;
    let mut labor_completed = 0.0;
    let mut labor_attempted = 0.0;
    let mut labor_lines = false;
    for line in &pop.job.lines {
        let Some(process) = state.factuals.get_process(line.process) else {
            continue;
        };
        if !process.is_labor() {
            continue;
        }
        labor += planned_time(line.target, process.required_time());
        labor_completed += line.completed;
        labor_attempted += line.attempted;
        labor_lines = true;
    }
    let (savings, stock) = holding_value(state, pop);
    PopRow {
        demographic: demographic(state, pop),
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
        savings: qty_text(savings),
        stock: qty_text(stock),
        sol: qty_text(pop.recorded_living().unwrap_or(0.0)),
        hope: percent_text(pop.sentiment.hope() * 100.0),
        happy: percent_text(pop.sentiment.happiness() * 100.0),
        fear: percent_text(pop.sentiment.fear() * 100.0),
        anger: percent_text(pop.sentiment.anger() * 100.0),
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
/// Draws an info table, then one row per pop in `rows`, then one table per market.
///
/// The day is [`PlayState::turn`]. The info table is one household of each
/// demographic. Pop columns are demographic, subsistence, labor, size,
/// savings, stock, sol, hope, happy, fear, anger, and stop. The craft
/// table follows the pops, then the market tables, one blank line after
/// each. `prior` is yesterday's prices for the markets. On a terminal
/// the screen is cleared and this call draws the banner, the day, and the
/// tables again. A pipe writes the lines in order.
fn print_table(
    state: &PlayState,
    rows: &[PopRow],
    live: bool,
    prior: &HashMap<(usize, usize, usize), f64>,
) {
    if live {
        // One screen. The previous day, tables, and prompt are wiped.
        print!("\x1b[H\x1b[2J\x1b[3J");
        print_banner(state);
    }
    println!("day: {}", state.turn);
    print_info(state);
    println!();
    let demo_w = col_width("demographic", rows.iter().map(|row| row.demographic.len()));
    let sub_w = col_width("subsistence", rows.iter().map(|row| row.subsistence.len()));
    let labor_w = col_width("labor", rows.iter().map(|row| row.labor.len()));
    let size_w = col_width("size", rows.iter().map(|row| row.size.len()));
    let savings_w = col_width("savings", rows.iter().map(|row| row.savings.len()));
    let stock_w = col_width("stock", rows.iter().map(|row| row.stock.len()));
    let sol_w = col_width("sol", rows.iter().map(|row| row.sol.len()));
    let hope_w = col_width("hope", rows.iter().map(|row| row.hope.len()));
    let happy_w = col_width("happy", rows.iter().map(|row| row.happy.len()));
    let fear_w = col_width("fear", rows.iter().map(|row| row.fear.len()));
    let anger_w = col_width("anger", rows.iter().map(|row| row.anger.len()));
    let stop_w = col_width("stop", rows.iter().map(|row| row.stop.len()));
    println!(
        "{}",
        table_line(
            "demographic", "subsistence", "labor", "size", "savings", "stock",
            "sol", "hope", "happy", "fear", "anger", "stop",
            demo_w, sub_w, labor_w, size_w, savings_w, stock_w,
            sol_w, hope_w, happy_w, fear_w, anger_w, stop_w,
        )
    );
    for row in rows {
        println!(
            "{}",
            table_line(
                &row.demographic, &row.subsistence, &row.labor, &row.size,
                &row.savings, &row.stock, &row.sol, &row.hope, &row.happy, &row.fear,
                &row.anger, &row.stop,
                demo_w, sub_w, labor_w, size_w, savings_w, stock_w,
                sol_w, hope_w, happy_w, fear_w, anger_w, stop_w,
            )
        );
    }
    print_craft_table(state);
    print_market_tables(state, prior);
    let _ = io::stdout().flush();
}

/// One craft line and the votes on its next step.
struct CraftRow {
    pop: String,
    craft: String,
    profit: String,
    margin: String,
    per_time: String,
    salability: String,
    demand: String,
    luck: String,
    decay: String,
    durability: String,
    living: String,
    idle: String,
    tally: String,
    step: String,
}

/// # Print Craft Table
///
/// Draws one row for each craft line, lowest pop id first.
///
/// A blank line separates it from the pops. A pop with no craft is left
/// out. The votes are the ones [`plan_votes`] would cast on the line as it
/// stands. Luck prints a dash. Margin and per time come from [`line_value`].
fn print_craft_table(state: &PlayState) {
    let rows = craft_rows(state);
    if rows.is_empty() {
        return;
    }
    println!();
    println!("crafts");
    let pop_w = col_width("pop", rows.iter().map(|row| row.pop.len()));
    let craft_w = col_width("craft", rows.iter().map(|row| row.craft.len()));
    let profit_w = col_width("profit", rows.iter().map(|row| row.profit.len()));
    let margin_w = col_width("margin", rows.iter().map(|row| row.margin.len()));
    let per_time_w = col_width("per time", rows.iter().map(|row| row.per_time.len()));
    let salability_w = col_width("salability", rows.iter().map(|row| row.salability.len()));
    let demand_w = col_width("demand", rows.iter().map(|row| row.demand.len()));
    let luck_w = col_width("luck", rows.iter().map(|row| row.luck.len()));
    let decay_w = col_width("decay", rows.iter().map(|row| row.decay.len()));
    let durability_w = col_width("durability", rows.iter().map(|row| row.durability.len()));
    let living_w = col_width("living", rows.iter().map(|row| row.living.len()));
    let idle_w = col_width("idle", rows.iter().map(|row| row.idle.len()));
    let tally_w = col_width("tally", rows.iter().map(|row| row.tally.len()));
    let step_w = col_width("step", rows.iter().map(|row| row.step.len()));
    println!(
        "{:<pop_w$}  {:<craft_w$}  {:>profit_w$}  {:>margin_w$}  {:>per_time_w$}  {:>salability_w$}  {:>demand_w$}  {:>luck_w$}  {:>decay_w$}  {:>durability_w$}  {:>living_w$}  {:>idle_w$}  {:>tally_w$}  {:>step_w$}",
        "pop",
        "craft",
        "profit",
        "margin",
        "per time",
        "salability",
        "demand",
        "luck",
        "decay",
        "durability",
        "living",
        "idle",
        "tally",
        "step",
        pop_w = pop_w,
        craft_w = craft_w,
        profit_w = profit_w,
        margin_w = margin_w,
        per_time_w = per_time_w,
        salability_w = salability_w,
        demand_w = demand_w,
        luck_w = luck_w,
        decay_w = decay_w,
        durability_w = durability_w,
        living_w = living_w,
        idle_w = idle_w,
        tally_w = tally_w,
        step_w = step_w,
    );
    for row in &rows {
        println!(
            "{:<pop_w$}  {:<craft_w$}  {:>profit_w$}  {:>margin_w$}  {:>per_time_w$}  {:>salability_w$}  {:>demand_w$}  {:>luck_w$}  {:>decay_w$}  {:>durability_w$}  {:>living_w$}  {:>idle_w$}  {:>tally_w$}  {:>step_w$}",
            row.pop,
            row.craft,
            row.profit,
            row.margin,
            row.per_time,
            row.salability,
            row.demand,
            row.luck,
            row.decay,
            row.durability,
            row.living,
            row.idle,
            row.tally,
            row.step,
            pop_w = pop_w,
            craft_w = craft_w,
            profit_w = profit_w,
            margin_w = margin_w,
            per_time_w = per_time_w,
            salability_w = salability_w,
            demand_w = demand_w,
            luck_w = luck_w,
            decay_w = decay_w,
            durability_w = durability_w,
            living_w = living_w,
            idle_w = idle_w,
            tally_w = tally_w,
            step_w = step_w,
        );
    }
}

/// # Craft Rows
///
/// One row for each craft line in `state`, lowest pop id first.
///
/// A labor line is left out. A line whose process is missing is left out.
/// Returns those rows.
fn craft_rows(state: &PlayState) -> Vec<CraftRow> {
    let mut ids: Vec<usize> = state.actors.pops.keys().copied().collect();
    ids.sort_unstable();
    let mut rows = Vec::new();
    for id in ids {
        let pop = state.actors.pop(id);
        let market = home_market(state, id);
        let bar = pop.subsistence_per_time(&state.factuals, market);
        let budget = pop.get_scaling_factor(ScalingFactor::Labor(
            state.factuals.config.labor.time_per_labor,
        ));
        for line in &pop.job.lines {
            let Some(process) = state.factuals.get_process(line.process) else {
                continue;
            };
            if process.is_labor() {
                continue;
            }
            let votes = plan_votes(pop, process, line, market, &state.factuals, bar);
            let value = line_value(process, market);
            let time = planned_time(line.target, process.required_time());
            rows.push(CraftRow {
                pop: id.to_string(),
                craft: plan_text(
                    Some(process.name.clone()),
                    time_share(time, budget),
                    Some((line.completed, line.attempted)),
                ),
                profit: vote_text(votes.profit),
                margin: margin_text(value.margin_percent()),
                per_time: qty_text(value.per_time),
                salability: vote_text(votes.salability),
                demand: vote_text(votes.demand),
                luck: "-".to_string(),
                decay: vote_text(votes.decay),
                durability: vote_text(votes.durability),
                living: vote_text(votes.living),
                idle: vote_text(votes.idle),
                tally: vote_text(votes.tally),
                step: step_text(&votes),
            });
        }
    }
    rows
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

/// # Vote Text
///
/// `vote` with a sign on a positive mark.
///
/// Zero prints as `0`. Returns that text.
fn vote_text(vote: i32) -> String {
    if vote > 0 {
        format!("+{vote}")
    } else {
        vote.to_string()
    }
}

/// # Step Text
///
/// The planning step in `votes`.
///
/// An expansion the subsistence bar returned to a hold prints `bar`.
/// Returns that word.
fn step_text(votes: &PlanVotes) -> String {
    if votes.barred {
        "bar".to_string()
    } else {
        match votes.direction {
            1 => "expand",
            -1 => "contract",
            _ => "hold",
        }
        .to_string()
    }
}

/// One good on a market table.
struct MarketRow {
    good: String,
    supply: String,
    demand: String,
    exchange: String,
    consumption: String,
    salability: String,
    prices: Vec<String>,
}

/// # Print Market Tables
///
/// Draws one table for each market in `state`, lowest id first.
///
/// A blank line separates a table from the crafts, and from the market
/// before it. `prior` is yesterday's prices.
fn print_market_tables(state: &PlayState, prior: &HashMap<(usize, usize, usize), f64>) {
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
/// exchange, consumption, salability, then one price column per currency.
/// A currency column is headed by `in` and that currency's name. The good
/// column sits on the left. The other columns sit on the right. `prior` is
/// yesterday's prices.
fn print_market(
    state: &PlayState,
    market: &Market,
    prior: &HashMap<(usize, usize, usize), f64>,
) {
    let currencies: Vec<usize> = market.currencies().to_vec();
    let headers: Vec<String> = currencies
        .iter()
        .map(|&id| format!("in {}", good_name(&state.factuals, id)))
        .collect();
    let rows: Vec<MarketRow> = listed_goods(state, market)
        .into_iter()
        .map(|id| market_row(state, market, id, &currencies, prior))
        .collect();
    println!("market: {}", market.id);
    let good_w = col_width("good", rows.iter().map(|row| row.good.len()));
    let supply_w = col_width("supply", rows.iter().map(|row| row.supply.len()));
    let demand_w = col_width("demand", rows.iter().map(|row| row.demand.len()));
    let exchange_w = col_width("exchange", rows.iter().map(|row| row.exchange.len()));
    let consumption_w = col_width("consumption", rows.iter().map(|row| row.consumption.len()));
    let salability_w = col_width("salability", rows.iter().map(|row| row.salability.len()));
    let price_w: Vec<usize> = headers
        .iter()
        .enumerate()
        .map(|(index, header)| col_width(header, rows.iter().map(|row| row.prices[index].len())))
        .collect();
    let mut head = vec![
        "good".to_string(),
        "supply".to_string(),
        "demand".to_string(),
        "exchange".to_string(),
        "consumption".to_string(),
        "salability".to_string(),
    ];
    head.extend(headers.iter().cloned());
    let mut widths = vec![good_w, supply_w, demand_w, exchange_w, consumption_w, salability_w];
    widths.extend(price_w);
    println!("{}", market_line(&head, &widths));
    for row in &rows {
        let mut cells = vec![
            row.good.clone(),
            row.supply.clone(),
            row.demand.clone(),
            row.exchange.clone(),
            row.consumption.clone(),
            row.salability.clone(),
        ];
        cells.extend(row.prices.iter().cloned());
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
/// rot, or stock for it, or some currency has a rate for it. Time is
/// left out. Returns those ids.
fn listed_goods(state: &PlayState, market: &Market) -> Vec<usize> {
    let mut ids = Vec::new();
    let mut remember = |id: usize| {
        if id != TIME && !ids.contains(&id) {
            ids.push(id);
        }
    };
    for &id in market.currencies() {
        remember(id);
    }
    for &id in market.goods.keys() {
        remember(id);
    }
    for rates in market.exchange_rates.values() {
        for &id in rates.keys() {
            remember(id);
        }
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
/// rate, and a good whose market row recorded sought, offered, volume,
/// output, use, rot, or stock. Returns that check.
fn on_the_market(state: &PlayState, market: &Market, good: usize) -> bool {
    if market.currencies().contains(&good) || has_rate(market, good) {
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
/// [`salability_of`]. Each entry of `currencies` is priced by [`price_in`]
/// and marked against `prior`. Returns that row.
fn market_row(
    state: &PlayState,
    market: &Market,
    good: usize,
    currencies: &[usize],
    prior: &HashMap<(usize, usize, usize), f64>,
) -> MarketRow {
    MarketRow {
        good: good_name(&state.factuals, good),
        supply: whole_text(offered_units(market, good)),
        demand: whole_text(sought_units(market, good)),
        exchange: qty_text(exchanged_units(market, good)),
        consumption: qty_text(consumed_units(market, good)),
        salability: percent_text(salability_of(state, market, good) * 100.0),
        prices: currencies
            .iter()
            .map(|&currency| {
                let price = price_in(market, currency, good);
                price_text(price, prior.get(&(market.id, good, currency)).copied())
            })
            .collect(),
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

/// # Has Rate
///
/// Whether some currency on `market` stores a rate for `good`.
///
/// Returns that check.
fn has_rate(market: &Market, good: usize) -> bool {
    market
        .exchange_rates
        .values()
        .any(|row| row.contains_key(&good))
}

/// # Price In
///
/// What one unit of `good` costs in `currency` on `market`.
///
/// The currency itself costs 1. A stored rate is goods per currency unit,
/// and the price is its inverse. A missing rate is `None`. Returns that
/// price.
fn price_in(market: &Market, currency: usize, good: usize) -> Option<f64> {
    if good == currency {
        return Some(1.0);
    }
    match market
        .exchange_rates
        .get(&currency)
        .and_then(|row| row.get(&good))
        .copied()
    {
        Some(rate) if rate.is_finite() && rate > 0.0 => Some(1.0 / rate),
        _ => None,
    }
}

/// # Snapshot Prices
///
/// Today's price of each listed good in each market currency.
///
/// The key is market id, good id, and currency id. The value is
/// [`price_in`]. Returns that map.
fn snapshot_prices(state: &PlayState) -> HashMap<(usize, usize, usize), f64> {
    let mut prices = HashMap::new();
    let mut ids: Vec<usize> = state.map_data.markets.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let market = state.map_data.market(id);
        for good in listed_goods(state, market) {
            for &currency in market.currencies() {
                if let Some(price) = price_in(market, currency, good) {
                    prices.insert((id, good, currency), price);
                }
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
/// The widths are the columns. Demographic and subsistence sit on the
/// left. The other columns sit on the right. Returns that line.
fn table_line(
    demographic: &str, subsistence: &str, labor: &str, size: &str, savings: &str, stock: &str,
    sol: &str, hope: &str, happy: &str, fear: &str, anger: &str, stop: &str, demo_w: usize,
    sub_w: usize, labor_w: usize, size_w: usize, savings_w: usize, stock_w: usize, sol_w: usize,
    hope_w: usize, happy_w: usize, fear_w: usize, anger_w: usize, stop_w: usize,
) -> String {
    format!(
        "{demographic:<demo_w$}  {subsistence:<sub_w$}  {labor:>labor_w$}  {size:>size_w$}  {savings:>savings_w$}  {stock:>stock_w$}  {sol:>sol_w$}  {hope:>hope_w$}  {happy:>happy_w$}  {fear:>fear_w$}  {anger:>anger_w$}  {stop:>stop_w$}"
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
