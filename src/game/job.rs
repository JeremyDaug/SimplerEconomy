//! A pop's cottage work: the processes it can run and the day's plan.
//!
//! The stock is the pop's. The pop posts the offer.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::game::actor::Actor;
use crate::game::config::TIME_PER_LABOR;
use crate::game::craft::Craft;
use crate::game::factuals::Factuals;
use crate::game::good::TIME;
use crate::game::market::{amv_scale, founding_goods_fraction, MarketHistory};
use crate::game::marketorder::MarketOrder;
use crate::game::pop_property::PopPRow;
use crate::game::process::{
    InputEffect, InputType, Process, ProcessEffect, ProcessInput, ProcessResult,
};
use crate::game::util::whole_units_up;

/// Share of this pop's own output where surplus holds.
/// Under it, surplus may grow. Over it, surplus pulls back.
const SURPLUS_ROT_SHARE: f64 = 0.1;

/// Fraction one plan step may close.
///
/// [`march_target`] caps a night's move at this share of the current
/// target. [`step_plan_targets`] closes this share of the morning gap.
const PLAN_MARCH: f64 = 0.25;

/// Share of a weaker line given up to the strongest line in one night.
///
/// Surplus moves first. A line that does not feed the strongest one can
/// also give up this share of its runs when that line is below the floor
/// in [`press_profit`].
const PROFIT_NUDGE: f64 = 0.25;

/// # Job
///
/// The Job of a pop, helping to define their work, skills, and the work they do at home.
///
/// As compared to a firm, this is disorganized work done by pops within their own home
/// and with their own property.
///
/// It's primary advantage is what it gives to the pop, subsistence and stability when
/// the wider market is underdeveloped. It's also highly stable in that it typically
/// supplies most of it's effective wages directly rather than needing to trade for them.
/// This makes it highly stable, even in shaky markets.
///
/// The disadvantages: It can't do directed research, simply pushing a bit of research everywhere
/// and quickly distributing it to the wider society, reducing it's competative
/// advantage. It can't separate pop consumption from the work fully. It has a higher
/// complexity penalty, as it's not unified.
///
/// ## Additional Notes
///
/// Two pops with the same craft do similar work and still keep separate
/// lines, targets, and stock. The craft changes only when the pop changes work.
///
/// ## Production Complexity Cost
///
/// Firms have their own complexity costs. A job's cost grows with the
/// square of each process's complexity, so a subsistence process stays
/// light. [`Self::complexity_cost`] is the modifier: the craft's
/// complexity modifier, plus a penalty when the lines diverge from that
/// craft.
///
/// [`Self::plan`] stores the cost of those targets. For each line it is
/// `modifier * (complexity^2 - overlap) * iterations`. Overlap is the
/// share of that line's goods that another line also uses. The weights
/// on overlap and on iterations are both 1.
#[derive(Debug, Clone)]
pub struct Job {
    /// The craft Id, which point's towards a baseline set of processes the job has/had.
    ///
    /// Craft does not define the processes. `0` means this pop has no baseline job.
    /// Craft ID should change as it's processes and owning pop
    /// approaches a different craft.
    pub craft: usize,
    /// Processes this pop can run, and the quota for each.
    ///
    /// If there are no processes, the job does not run.
    pub lines: Vec<JobLine>,
    /// Skill goods this job's processes take.
    ///
    /// Good ids, sorted. [`Self::attach_skills`] rewrites the list from the
    /// lines. Empty when no line takes a skill.
    pub skills: Vec<usize>,
    /// Complexity cost of the targets [`Job::plan`] last wrote.
    ///
    /// `0` before the first plan, and when no line has a positive target.
    pub plan_cost: f64,
    /// Units this job added to `PopPRow.reserved` this morning.
    ///
    /// Produce spends this claim and leaves desire reserves alone.
    claimed: HashMap<usize, f64>,
    /// Units to buy for the next run, recorded by [`Job::claim_one`]
    /// and [`Job::shop_unpaid_outputs`].
    ///
    /// Produce does not clear it. [`Self::settle_shopping`] rebuilds it
    /// from stock still free after that produce. The next [`Job::reset_day`]
    /// drops it. [`Self::plan_purchases`] writes the pre-market buy.
    shopping: HashMap<usize, f64>,
    /// Tender [`Self::plan_purchases`] kept in working stock.
    ///
    /// [`Self::unlisted_of`] reads it. [`Self::reset_day`] drops it.
    pub(crate) unlisted: HashMap<usize, f64>,
    /// Units of each input set aside in the pop's saved pile for this plan.
    ///
    /// [`Self::plan_purchases`] names them. The pop moves that stock out of
    /// working property. Produce spends them after they return to property.
    /// They do not count as currency savings.
    pub(crate) input_hold: HashMap<usize, f64>,
    /// Inputs that survive the night, and the output each one makes.
    ///
    /// [`Self::reserve`] records this for the day. `per_output` is input
    /// units consumed for one unit of that output. A good that decays
    /// completely is left out.
    input_feeds: Vec<InputFeed>,
    /// Units [`Self::leave`] is holding back this morning.
    ///
    /// Reserve, produce, and settle do not spend these. [`Self::reset_day`]
    /// drops the map. Empty means the run may use every free unit.
    kept: HashMap<usize, f64>,
    /// Destroyed inputs today, and the output units each run credited to them.
    ///
    /// [`Self::apply_result`] adds a run. [`Self::reset_day`] drops the map.
    process_credit: HashMap<usize, ProcessCredit>,
    /// Sale pile noted for the next [`Self::plan`], per output good.
    ///
    /// [`Self::note_sale`] writes it. The plan reads it once.
    sale_pile: HashMap<usize, f64>,
    /// Units of each output this pop sold today.
    ///
    /// [`Self::note_sale`] writes it. The plan reads it once.
    sale_sold: HashMap<usize, f64>,
    /// Units of each output posted for sale today.
    ///
    /// [`Self::note_listed`] writes it. A good left out is offered as the
    /// pile. [`Self::note_sale`] clears the map.
    sale_listed: HashMap<usize, f64>,
    /// True when [`Self::note_sale`] armed the next plan.
    sale_on: bool,
    /// Households the last [`Self::plan_purchases_for`] planned for.
    ///
    /// The next plan scales stored targets by today's count over this one.
    /// `0` means no plan has stored a count yet, and the targets stay as written.
    planned_households: f64,
    /// Inputs the morning plan will set aside when the ladder reaches production.
    ///
    /// [`Self::plan_purchases_for`] writes it. The buy ladder stashes it and
    /// clears it. [`Self::reset_day`] drops a plan the ladder never reached.
    pub(crate) input_reserve: HashMap<usize, f64>,
    /// Output of the founding-stake runs, held out of working stock.
    ///
    /// [`Self::park_stake_output`] writes it. [`Self::release_stake_hold`]
    /// puts it back. Reset leaves it, so a day that has not released yet
    /// does not drop the goods.
    pub(crate) stake_hold: HashMap<usize, f64>,
}

/// Destroyed units of one input, and the output units credited to them.
///
/// Each run adds its whole positive changes to every input it destroyed.
#[derive(Debug, Clone, Default)]
pub(crate) struct ProcessCredit {
    /// Units of this input the runs destroyed.
    pub units: f64,
    /// Output good, and the units of it those runs made.
    pub outputs: HashMap<usize, f64>,
}

/// One surviving input and an output it is consumed to make.
#[derive(Debug, Clone)]
struct InputFeed {
    input: usize,
    output: usize,
    /// Input units per one output unit.
    per_output: f64,
}

/// One process the job can run, plus the day's quota.
///
/// Same shape as a firm's production line. `inputs` is the on/off list of
/// optional goods this line may draw. Required inputs are always drawn.
/// [`Job::plan`] rewrites the list.
#[derive(Debug, Clone)]
pub struct JobLine {
    /// Process id in [`Factuals`].
    pub process: usize,
    /// Iterations sought today.
    ///
    /// `None` means as many as the inputs on hand allow.
    /// `Some(0.0)` means do not run.
    /// `Some(n)` with `n > 0` means run up to `n` iterations.
    ///
    /// [`Job::plan`] writes `Some` on every line.
    pub target: Option<f64>,
    /// Cover iterations last snapped into [`Self::target`].
    ///
    /// Surplus on the line is `target - cover`. A night that does not snap
    /// leaves this where the last snap put it.
    pub cover: f64,
    /// True when those cover runs spend the subsistence floor.
    ///
    /// A later desire that no subsistence process makes sets it in
    /// [`Job::plan_purchases_for`]. Other covers spend [`cover_complexity`].
    pub cover_cheap: bool,
    /// Optional inputs this line draws. Empty draws none.
    ///
    /// [`Job::plan`] rewrites it from [`chosen_inputs`].
    pub inputs: Vec<usize>,
    /// True when the morning plan scored this line above zero.
    ///
    /// [`Job::plan`] and [`Job::plan_purchases`] write it. Stays true until
    /// a plan runs, so the first morning does not shop an output.
    pub pays: bool,
    /// Mornings in a row this line was at zero, not profitable, and not feeding.
    ///
    /// [`Job::plan_purchases`] adds one when the target is not above zero,
    /// the trade score is not positive, nothing sold, and no basic gap
    /// reserved a run. Any of those clears it. A count that reaches one
    /// morning can drop the line. `turn_length` does not extend that wait.
    pub(crate) idle_days: u32,
    /// Order this line was added. Set once in [`Self::new`].
    home: u64,
    /// Cover runs whose output is the founding stake.
    ///
    /// [`Job::plan_purchases_for`] writes the runs that sit above the meal
    /// and still fit in [`Self::cover`]. Produce moves that output into
    /// [`Job::stake_hold`]. A line with none stays at zero.
    pub(crate) stake_runs: f64,
}

/// Next value for [`JobLine::home`].
fn next_home() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl JobLine {
    /// # New
    ///
    /// A line for `process`.
    ///
    /// `target` is the day's iterations. `None` runs as many as inputs allow.
    /// `inputs` are the optional goods this line may draw. Required inputs
    /// are always included.
    pub fn new(process: usize, target: Option<f64>, inputs: Vec<usize>) -> Self {
        Self {
            process,
            target,
            cover: 0.0,
            cover_cheap: false,
            inputs,
            pays: true,
            idle_days: 0,
            home: next_home(),
            stake_runs: 0.0,
        }
    }
}

/// # Pile Units
///
/// Working units plus units already in the input hold.
///
/// `property_free` is working stock the meal has not claimed. `held` is
/// [`Job::input_hold`]. `good` is the id. Returns the sum.
fn pile_units(
    property_free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
    good: usize,
) -> f64 {
    property_free.get(&good).copied().unwrap_or(0.0) + held.get(&good).copied().unwrap_or(0.0)
}

/// # Pile Hand
///
/// Working units plus the input hold, as one map.
///
/// `property_free` is working stock the meal has not claimed. `held` is
/// [`Job::input_hold`]. Returns that sum.
fn pile_hand(
    property_free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
) -> HashMap<usize, f64> {
    let mut hand = property_free.clone();
    for (&good, &qty) in held {
        if qty > 0.0 {
            *hand.entry(good).or_insert(0.0) += qty;
        }
    }
    hand
}

/// # Time Ceiling
///
/// Runs the time on hand can finish, or `None` when the process takes no time.
///
/// `property_free` and `held` are the two piles. The pace is the required
/// time input. Returns that run count.
fn time_ceiling(
    process: &Process,
    property_free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
) -> Option<f64> {
    let pace = process
        .requirements()
        .iter()
        .find(|input| input.good == TIME)
        .map(|input| input.amount)
        .unwrap_or(0.0);
    if pace <= 0.0 {
        return None;
    }
    Some((pile_units(property_free, held, TIME) / pace).max(0.0))
}

/// # Line Blocked
///
/// True when a required factor is missing or required capital is short.
///
/// Those piles are not shopped.
fn line_blocked(
    process: &Process,
    property_free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
) -> bool {
    process.factors().iter().any(|factor| {
        !factor.is_optional() && pile_units(property_free, held, factor.good) <= 0.0
    }) || process.requirements().iter().any(|input| {
        matches!(input.input_type, InputType::Capital)
            && pile_units(property_free, held, input.good) < input.amount
    })
}

/// # Covered Runs
///
/// Runs the stock already on hand can finish.
///
/// `factuals` names skills. A missing required factor, or required capital
/// short of its amount, is no runs. Destroyed and consumed inputs, time
/// included, take the tightest `stock / amount`. A process with none of
/// those inputs returns 0.
fn covered_runs(
    process: &Process,
    factuals: &Factuals,
    property_free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
) -> f64 {
    for factor in process.factors() {
        if !factor.is_optional() && pile_units(property_free, held, factor.good) <= 0.0 {
            return 0.0;
        }
    }
    let mut runs = f64::INFINITY;
    for input in process.requirements() {
        if factuals
            .get_good(input.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let have = pile_units(property_free, held, input.good);
        match input.input_type {
            InputType::Capital => {
                if have < input.amount {
                    return 0.0;
                }
            }
            InputType::Destroyed | InputType::Consumed => {
                if input.amount > 0.0 {
                    runs = runs.min(have / input.amount);
                }
            }
            InputType::Factor => {}
        }
    }
    if runs.is_finite() { runs.max(0.0) } else { 0.0 }
}

/// # Sought Sale Runs
///
/// Runs that cover the largest sought bid on a sale output.
///
/// `process` supplies the outputs. `factuals` names skills. `history` is
/// yesterday's book. A fixed output, a skill, time, and a currency are
/// left out. A bid under one unit is zero. Returns the largest remaining
/// bid divided by that output's amount.
pub(crate) fn sought_sale_runs(
    process: &Process,
    factuals: &Factuals,
    history: &MarketHistory,
) -> f64 {
    let mut runs = 0.0;
    for output in &process.outputs {
        if output.fixed || output.amount <= 0.0 || output.good == TIME {
            continue;
        }
        if history.currencies().contains(&output.good) {
            continue;
        }
        if factuals
            .get_good(output.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let sought = history.sought(output.good);
        if !sought.is_finite() || sought < 1.0 {
            continue;
        }
        let extra = sought / output.amount;
        if extra.is_finite() && extra > runs {
            runs = extra;
        }
    }
    runs
}

/// # Short Runs
///
/// Runs that cover the largest unfilled bid on an output.
///
/// `process` supplies the outputs. `history` is yesterday's book. The bid
/// is [`MarketHistory::sought`]. A bid under one unit is zero. Returns the
/// largest bid divided by that output's amount.
fn short_runs(process: &Process, history: &MarketHistory) -> f64 {
    let mut runs = 0.0;
    for output in &process.outputs {
        if output.amount <= 0.0 {
            continue;
        }
        let sought = history.sought(output.good);
        if !sought.is_finite() || sought < 1.0 {
            continue;
        }
        let extra = sought / output.amount;
        if extra.is_finite() && extra > runs {
            runs = extra;
        }
    }
    runs
}

/// # Basic Output Runs
///
/// Runs of `process` that cover the basic gaps its output can fill.
///
/// `gaps` is each short desire: goods with satisfaction per unit, then the
/// missing satisfaction. Each gap adds its missing satisfaction divided by
/// the best output that fills it. A process that fills none returns 0.
fn basic_output_runs(process: &Process, gaps: &[(Vec<(usize, f64)>, f64)]) -> f64 {
    let mut runs = 0.0;
    for (goods, sat) in gaps {
        if *sat <= 0.0 {
            continue;
        }
        let mut per: f64 = 0.0;
        for output in &process.outputs {
            if output.amount <= 0.0 {
                continue;
            }
            let Some((_, efficiency)) = goods.iter().find(|(good, _)| *good == output.good) else {
                continue;
            };
            if *efficiency <= 0.0 {
                continue;
            }
            per = per.max(output.amount * *efficiency);
        }
        if per > 0.0 {
            let extra = sat / per;
            if extra.is_finite() {
                runs += extra;
            }
        }
    }
    if runs.is_finite() { runs.max(0.0) } else { 0.0 }
}

/// # Household Target Scale
///
/// Ratio that brings yesterday's targets up to today's household count.
///
/// `before` is the count last planned. `now` is this morning. A count that
/// is not finite and positive returns 1, and the stored targets stay put.
fn household_target_scale(before: f64, now: f64) -> f64 {
    if before.is_finite() && now.is_finite() && before > 0.0 && now > 0.0 {
        now / before
    } else {
        1.0
    }
}

/// # Step Plan Targets
///
/// Moves each stored target from yesterday toward this morning's aim.
///
/// `lines` hold that aim in [`JobLine::target`]. `previous` is the target
/// before this plan, already scaled to today's households, one entry per
/// line. `reserved` is the cover reserved for each line. `factuals` scores
/// the process. `history` is yesterday's book. `on_hand` is the free pile
/// this morning.
///
/// A line with no previous target keeps the aim. A profitable line whose
/// product cleared, or was not offered, closes [`PLAN_MARCH`] of the gap
/// toward a higher aim and keeps its target when the aim is lower. The
/// target steps down when the score is not positive, toward the morning
/// aim, or when a product was offered, a full unit did not sell, and a
/// buyer could have paid. A cleared book on a line another line feeds,
/// whose free pile outruns
/// that sale by more than [`PLAN_MARCH`], steps the same way. That
/// sale steps toward the reserved cover plus the sold share of the surplus,
/// and the cover is put back on the line. A step that follows the aim cuts
/// [`JobLine::cover`] when it sits above the result. Returns whether any
/// target changed from the aim that was already shopped.
fn step_plan_targets(
    lines: &mut [JobLine],
    previous: &[Option<f64>],
    reserved: &[f64],
    factuals: &Factuals,
    history: &MarketHistory,
    on_hand: &HashMap<usize, f64>,
) -> bool {
    let share = PLAN_MARCH;
    let empty_pile = HashMap::new();
    let fed: Vec<bool> = (0..lines.len())
        .map(|index| is_line_fed(lines, index, factuals))
        .collect();
    let mut moved = false;
    for (index, line) in lines.iter_mut().enumerate() {
        let Some(aim) = line.target else {
            continue;
        };
        let Some(old) = previous
            .get(index)
            .copied()
            .flatten()
            .filter(|old| old.is_finite())
        else {
            continue;
        };
        let meal = reserved.get(index).copied().unwrap_or(0.0);
        // The free pile is the make a feeder left for this line. Other lines
        // keep the book.
        let pile = if fed[index] { on_hand } else { &empty_pile };
        let (step_aim, current) = stored_step_aim(
            line,
            old,
            aim,
            meal,
            factuals,
            history,
            pile,
        );
        let stepped = step_plan_target(current, step_aim, share);
        if (stepped - aim).abs() > 1e-9 {
            moved = true;
        }
        if (step_aim - aim).abs() > 1e-9 {
            line.cover = meal.max(0.0).min(stepped.max(0.0));
        } else if line.cover > stepped {
            line.cover = stepped.max(0.0);
        }
        line.target = Some(stepped);
    }
    moved
}

/// # Stored Step Aim
///
/// The aim one stored target steps toward.
///
/// `line` names the process. `old` is yesterday's target after the
/// household scale. `raw` is this morning's aim. `meal` is the cover
/// reserved before that aim. `factuals` and `history` score the line and
/// read yesterday's book. `on_hand` is the free pile this morning.
///
/// A score above zero keeps `old` when [`unsold_product_share`] is `None`,
/// including when `raw` is lower. A product left unsold keeps the meal as
/// its floor, even when that meal sits above `old`, and keeps that share
/// of any surplus above the meal. A currency output is left out of that
/// sale. A score that is not above zero follows `raw`. A missing process
/// follows `raw`. The morning closes a quarter of the gap from `old`
/// toward the aim this returns. Returns the aim and the target the step
/// starts from.
fn stored_step_aim(
    line: &JobLine,
    old: f64,
    raw: f64,
    meal: f64,
    factuals: &Factuals,
    history: &MarketHistory,
    on_hand: &HashMap<usize, f64>,
) -> (f64, f64) {
    let Some(process) = factuals.get_process(line.process) else {
        return (raw, old);
    };
    let score = trade_score(process, history);
    if !score.is_finite() || score <= 0.0 {
        return (raw, old);
    }
    if let Some(sold) = unsold_product_share(process, factuals, history, on_hand) {
        let floor = meal.max(0.0);
        let aim = floor + (old.max(0.0) - floor).max(0.0) * sold;
        return (aim, old);
    }
    if !raw.is_finite() || raw < old {
        return (old, old);
    }
    (raw, old)
}

/// # Unsold Product Share
///
/// Smallest share of an offered product that exchanged, when a full unit stayed.
///
/// `process` supplies the outputs. `factuals` names skills. `history` is
/// yesterday's mass book. `on_hand` is the free pile this morning. A fixed
/// output, a skill, time, and a currency are left out. An output offered
/// for less than one unit is left out. An output that exchanged within one
/// unit of its offer is left out, unless the free pile is past that sale by
/// more than [`PLAN_MARCH`]. An output nobody could buy is left out of that
/// missed offer. The free pile still counts when the book cleared. The book
/// listed yesterday's pile, not the one
/// still free. The share is the sale over the sale plus that pile. When the
/// book and the pile both count, the smaller share stays. No such output
/// returns `None`. Returns that share, from 0 to 1.
fn unsold_product_share(
    process: &Process,
    factuals: &Factuals,
    history: &MarketHistory,
    on_hand: &HashMap<usize, f64>,
) -> Option<f64> {
    let mut worst: Option<f64> = None;
    for output in &process.outputs {
        if output.fixed || output.amount <= 0.0 || output.good == TIME {
            continue;
        }
        if history.currencies().contains(&output.good) {
            continue;
        }
        if factuals
            .get_good(output.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let offered = history.mass_offered(output.good);
        let exchanged = history.mass_exchanged(output.good);
        let sold = exchanged.max(0.0);
        let mut share: Option<f64> = None;
        if !history.is_unaffordable(output.good)
            && offered >= 1.0
            && offered - exchanged >= 1.0
        {
            share = Some((sold / offered).clamp(0.0, 1.0));
        }
        // Yesterday's book cleared. The free pile is the make it did not list.
        let waiting = on_hand.get(&output.good).copied().unwrap_or(0.0).max(0.0);
        let cleared = offered >= 1.0 && offered - exchanged < 1.0;
        if cleared && waiting >= 1.0 && waiting > sold * (1.0 + PLAN_MARCH) {
            let made = (sold / (sold + waiting)).clamp(0.0, 1.0);
            share = Some(match share {
                Some(kept) if kept <= made => kept,
                _ => made,
            });
        }
        if let Some(share) = share {
            worst = Some(match worst {
                Some(kept) if kept <= share => kept,
                _ => share,
            });
        }
    }
    worst
}

/// # Step Plan Target
///
/// One morning's step from `current` toward `aim`.
///
/// `share` is the fraction of the gap to close. A share of 1 or more
/// returns `aim`. A step under one run whose aim is zero writes zero, so
/// a line aimed at nothing can sit at zero. The result is never negative.
fn step_plan_target(current: f64, aim: f64, share: f64) -> f64 {
    let aim = if aim.is_finite() { aim.max(0.0) } else { 0.0 };
    let current = if current.is_finite() {
        current.max(0.0)
    } else {
        aim
    };
    if share >= 1.0 {
        return aim;
    }
    let stepped = current + (aim - current) * share.max(0.0);
    let stepped = if stepped.is_finite() {
        stepped.max(0.0)
    } else {
        aim
    };
    if aim == 0.0 && stepped < 1.0 {
        0.0
    } else {
        stepped
    }
}

/// # Fund Short Lines
///
/// Moves spare time onto a line whose output was still sought.
///
/// `lines` are the targets just written. `factuals` supplies processes.
/// `history` is yesterday's book. `gaps` is each short desire being held.
/// `reserved` is the held runs per line. `time_free` is time no target
/// took. `points_left` is complexity still free.
///
/// A subsistence line keeps the runs that cover a held desire. It gives up
/// [`PROFIT_NUDGE`] of the runs above that hold, so a large line shifts more
/// runs than a small one. A specialty keeps the runs it has. A line already
/// above zero then adds that same share of itself when an output was sought,
/// and not the raw bid. A subsistence line at zero starts [`short_runs`].
/// The higher unfilled bid is first. Time and points left go back onto the
/// lines that gave them up, in order, and not above the runs they already
/// had. Nothing is returned.
fn fund_short_lines(
    lines: &mut [JobLine],
    factuals: &Factuals,
    history: &MarketHistory,
    gaps: &[(Vec<(usize, f64)>, f64)],
    reserved: &[f64],
    time_free: &mut HashMap<usize, f64>,
    points_left: &mut f64,
) {
    let mut spare = time_free.get(&TIME).copied().unwrap_or(0.0).max(0.0);
    let mut soak: Vec<(usize, f64)> = Vec::new();
    for (index, line) in lines.iter_mut().enumerate() {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let Some(target) = line.target else {
            continue;
        };
        if !target.is_finite() || target <= 0.0 {
            continue;
        }
        let basic = basic_output_runs(process, gaps)
            .max(reserved.get(index).copied().unwrap_or(0.0))
            .min(target);
        // A specialty keeps the day. Subsistence keeps the basic gap.
        let floor = if process.is_subsistence() { basic } else { target };
        // A quarter of the spare, so the shift scales with the line.
        let give = (target - floor).max(0.0) * PROFIT_NUDGE;
        if give <= 1e-9 {
            continue;
        }
        let pace = time_spent(process);
        let spent = complexity_spent(process);
        spare += pace.max(0.0) * give;
        if spent > 0.0 && points_left.is_finite() {
            *points_left += spent * give;
        }
        line.target = Some((target - give).max(floor));
        soak.push((index, target));
    }
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&left, &right| {
        let bid = |index: usize| {
            factuals
                .get_process(lines[index].process)
                .map(|process| {
                    process
                        .outputs
                        .iter()
                        .map(|output| history.sought(output.good))
                        .fold(0.0_f64, f64::max)
                })
                .unwrap_or(0.0)
        };
        bid(right).total_cmp(&bid(left)).then(left.cmp(&right))
    });
    for index in order {
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let Some(current) = lines[index].target else {
            continue;
        };
        if !current.is_finite() || current < 0.0 {
            continue;
        }
        let pace = time_spent(process);
        if pace <= 0.0 {
            continue;
        }
        let wanted = short_runs(process, history);
        if wanted <= 0.0 {
            continue;
        }
        // A running line shifts a quarter of itself. Subsistence at zero still covers the bid.
        let mut add = if current > 0.0 {
            current * PROFIT_NUDGE
        } else if process.is_subsistence() {
            wanted
        } else {
            continue;
        };
        add = add.min((spare / pace).max(0.0));
        let spent = complexity_spent(process);
        if spent > 0.0 && points_left.is_finite() {
            add = add.min((*points_left / spent).max(0.0));
        }
        if !add.is_finite() || add <= 0.0 {
            continue;
        }
        lines[index].target = Some(current + add);
        spare = (spare - pace * add).max(0.0);
        if spent > 0.0 && points_left.is_finite() {
            *points_left = (*points_left - spent * add).max(0.0);
        }
    }
    for (index, old) in soak {
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let pace = time_spent(process);
        let current = lines[index].target.unwrap_or(0.0).max(0.0);
        let room = (old - current).max(0.0);
        if pace <= 0.0 || spare <= 0.0 || room <= 0.0 {
            continue;
        }
        let mut runs = (spare / pace).min(room);
        let spent = complexity_spent(process);
        if spent > 0.0 && points_left.is_finite() {
            runs = runs.min((*points_left / spent).max(0.0));
        }
        if !runs.is_finite() || runs <= 0.0 {
            continue;
        }
        lines[index].target = Some(current + runs);
        spare = (spare - pace * runs).max(0.0);
        if spent > 0.0 && points_left.is_finite() {
            *points_left = (*points_left - spent * runs).max(0.0);
        }
    }
    time_free.insert(TIME, spare.max(0.0));
}

/// # Loss Shares
///
/// Run aims when every line on the job is losing.
///
/// `lines` are the job. `factuals` supplies processes. `property_free`
/// and `held` are the piles, with reserved time already out of the free
/// time. `reserved` is the held runs per line. `extra_points` is
/// complexity still free above that hold.
///
/// A line that is blocked, or that spends neither time nor a finite point
/// cost, keeps its reserved runs. The others split the spare time and the
/// spare points equally. Each aim is the reserved runs plus the tighter of
/// those two shares. Returns one aim per line.
fn loss_shares(
    lines: &[JobLine],
    factuals: &Factuals,
    property_free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
    reserved: &[f64],
    extra_points: f64,
) -> Vec<f64> {
    let mut open = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        if line_blocked(process, property_free, held) {
            continue;
        }
        let pace = time_spent(process);
        let spent = complexity_spent(process);
        let points_bind = spent > 0.0 && extra_points.is_finite();
        if pace > 0.0 || points_bind {
            open.push(index);
        }
    }
    let mut aims: Vec<f64> = reserved
        .iter()
        .map(|runs| runs.max(0.0))
        .collect();
    aims.resize(lines.len(), 0.0);
    if open.is_empty() {
        return aims;
    }
    let spare_time = pile_units(property_free, held, TIME).max(0.0);
    let count = open.len() as f64;
    let time_each = spare_time / count;
    let points_each = if extra_points.is_finite() {
        extra_points.max(0.0) / count
    } else {
        f64::INFINITY
    };
    for index in open {
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let pace = time_spent(process);
        let spent = complexity_spent(process);
        let mut extra = f64::INFINITY;
        if pace > 0.0 {
            extra = extra.min((time_each / pace).max(0.0));
        }
        if spent > 0.0 && points_each.is_finite() {
            extra = extra.min((points_each / spent).max(0.0));
        }
        if !extra.is_finite() {
            extra = 0.0;
        }
        aims[index] += extra.max(0.0);
    }
    aims
}

/// # Line Aim
///
/// Iterations `process` keeps from this morning's piles.
///
/// `free` and `held` are the piles this line is scored against. `pays` is
/// the line's mark. `any_pays` is whether the job has one. `held_runs` is
/// the cover already reserved. `sale` is the extra sale runs. `loss_share`
/// is this line's equal share when nobody pays. `extra_points` caps runs
/// above that cover.
///
/// A blocked line is zero. A priced paying line aims at the funded runs.
/// An unpriced paying line aims at the time ceiling. The other lines aim
/// at the loss share, the cover, or the runs the stock covers. The point
/// cap then cuts runs above the cover. Returns that aim.
fn line_aim(
    process: &Process,
    factuals: &Factuals,
    history: &MarketHistory,
    free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
    pays: bool,
    any_pays: bool,
    held_runs: f64,
    sale: f64,
    loss_share: f64,
    extra_points: f64,
) -> f64 {
    let blocked = line_blocked(process, free, held);
    let pays = pays && !blocked;
    let covered = covered_runs(process, factuals, free, held);
    let currencies = history.currencies();
    let priced = pays && !currencies.is_empty();
    let ceiling = time_ceiling(process, free, held).unwrap_or(covered);
    let ceiling = if ceiling.is_finite() { ceiling.max(0.0) } else { 0.0 };
    let hand = pile_hand(free, held);
    let funded = if priced {
        pushed_runs(process, ceiling, &hand, factuals, history, currencies)
    } else {
        0.0
    };
    let stretch = !any_pays && !blocked;
    let shared = stretch && loss_share > held_runs + 1e-9;
    let target = if blocked {
        0.0
    } else if priced {
        funded.max(held_runs + sale)
    } else if pays {
        ceiling.max(held_runs + sale)
    } else if shared {
        loss_share.max(held_runs + sale)
    } else if held_runs + sale > 0.0 {
        held_runs + sale
    } else {
        covered
    };
    let mut target = if target.is_finite() { target.max(0.0) } else { 0.0 };
    let kept = held_runs.max(0.0) + sale.max(0.0);
    let spent = complexity_spent(process);
    if spent > 0.0 && extra_points.is_finite() {
        let room = kept + (extra_points / spent).max(0.0);
        if target > room {
            target = room;
        }
    }
    target
}

/// # Spend Aim Points
///
/// Points `target` spends above the cover already held.
///
/// `kept` is that cover. `points` is what is still free. A process that
/// spends nothing leaves `points` as it stands. Returns the remainder.
fn spend_aim_points(process: &Process, target: f64, kept: f64, points: f64) -> f64 {
    let spent = complexity_spent(process);
    if spent <= 0.0 || !points.is_finite() {
        return points;
    }
    let extra = (target - kept).max(0.0);
    (points - spent * extra).max(0.0)
}

/// # Draw Material Inputs
///
/// Removes the goods one aim takes from the planning piles.
///
/// Time stays for the caller. A skill and a factor stay. A currency a
/// paying line keeps stays. Capital is taken once. Nothing is returned.
fn draw_material_inputs(
    process: &Process,
    factuals: &Factuals,
    history: &MarketHistory,
    target: f64,
    profitable: bool,
    free: &mut HashMap<usize, f64>,
    held: &mut HashMap<usize, f64>,
) {
    if target <= 0.0 {
        return;
    }
    for input in process.requirements() {
        if input.good == TIME {
            continue;
        }
        if factuals
            .get_good(input.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let need = match input.input_type {
            InputType::Destroyed | InputType::Consumed => input.amount * target,
            InputType::Capital => input.amount,
            InputType::Factor => continue,
        };
        if !need.is_finite() || need <= 0.0 {
            continue;
        }
        if profitable && history.currencies().contains(&input.good) {
            continue;
        }
        let from_hold = need.min(held.get(&input.good).copied().unwrap_or(0.0));
        let property_have = free.get(&input.good).copied().unwrap_or(0.0);
        let from_property = (need - from_hold).min(property_have).max(0.0);
        if from_hold > 0.0 {
            if let Some(left) = held.get_mut(&input.good) {
                *left = (*left - from_hold).max(0.0);
            }
        }
        if from_property > 0.0 {
            if let Some(left) = free.get_mut(&input.good) {
                *left = (*left - from_property).max(0.0);
            }
        }
    }
}

/// Iterations written for each line, the time those runs lock, and the points left.
struct PlannedAims {
    target: Vec<f64>,
    time_runs: Vec<f64>,
    points_left: f64,
}

/// # Plan Aims
///
/// One iteration aim per line, and the time runs that aim locks.
///
/// `free` and `held` are the piles with reserved time already out.
/// `reserved` is the cover. `sale_extra` is the sale runs. `loss_share`
/// is the equal split when nobody pays. `line_time` is reserved time.
/// `any_pays` is whether a line still pays. `points` is complexity still free.
///
/// When a line pays, the walk is [`priority_order`]. The first line sees
/// the spare time and the spare points. Each later line buys what that
/// line left. When nobody pays, each line is scored on its own loss share
/// against the full spare. Returns the aims, the time runs, and the points
/// the aims did not spend.
fn plan_aims(
    lines: &[JobLine],
    factuals: &Factuals,
    history: &MarketHistory,
    free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
    reserved: &[f64],
    sale_extra: &[f64],
    loss_share: &[f64],
    line_time: &[f64],
    any_pays: bool,
    points: f64,
) -> PlannedAims {
    let n = lines.len();
    let mut target = vec![0.0; n];
    let mut time_runs = vec![0.0; n];
    let spare_time = free.get(&TIME).copied().unwrap_or(0.0).max(0.0);
    let mut points_left = points;
    if any_pays {
        // Buy in list order. The pile shrinks as each line takes its runs.
        let mut seen_free = free.clone();
        let mut seen_held = held.clone();
        let mut time_left = spare_time;
        for &index in &priority_order(lines, factuals, history) {
            let Some(process) = factuals.get_process(lines[index].process) else {
                continue;
            };
            let mut clock = seen_free.clone();
            let mine = time_left + line_time.get(index).copied().unwrap_or(0.0);
            clock.insert(TIME, mine.max(0.0));
            let held_runs = reserved.get(index).copied().unwrap_or(0.0).max(0.0);
            let sale = sale_extra.get(index).copied().unwrap_or(0.0).max(0.0);
            let share = loss_share.get(index).copied().unwrap_or(0.0);
            let pays = lines[index].pays && !line_blocked(process, &clock, &seen_held);
            let aim = line_aim(
                process,
                factuals,
                history,
                &clock,
                &seen_held,
                pays,
                true,
                held_runs,
                sale,
                share,
                points_left,
            );
            let covered = covered_runs(process, factuals, &clock, &seen_held);
            let priced = pays && !history.currencies().is_empty();
            let lock = if pays {
                if priced { aim } else { covered.min(aim) }
            } else {
                aim
            };
            target[index] = aim;
            time_runs[index] = if lock.is_finite() { lock.max(0.0) } else { 0.0 };
            points_left = spend_aim_points(process, aim, held_runs + sale, points_left);
            time_left += line_time.get(index).copied().unwrap_or(0.0);
            time_left -= time_spent(process).max(0.0) * time_runs[index];
            time_left = time_left.max(0.0);
            draw_material_inputs(
                process,
                factuals,
                history,
                target[index],
                pays,
                &mut seen_free,
                &mut seen_held,
            );
            seen_free.insert(TIME, time_left);
        }
    } else {
        for index in 0..n {
            let Some(process) = factuals.get_process(lines[index].process) else {
                continue;
            };
            let mut clock = free.clone();
            let mine = spare_time + line_time.get(index).copied().unwrap_or(0.0);
            clock.insert(TIME, mine.max(0.0));
            let held_runs = reserved.get(index).copied().unwrap_or(0.0).max(0.0);
            let sale = sale_extra.get(index).copied().unwrap_or(0.0).max(0.0);
            let share = loss_share.get(index).copied().unwrap_or(0.0);
            let aim = line_aim(
                process,
                factuals,
                history,
                &clock,
                held,
                false,
                false,
                held_runs,
                sale,
                share,
                points,
            );
            target[index] = aim;
            time_runs[index] = aim;
            points_left = spend_aim_points(process, aim, held_runs + sale, points_left);
        }
    }
    PlannedAims {
        target,
        time_runs,
        points_left,
    }
}

/// # Is Selling Enough
///
/// True when an offered output traded.
///
/// `process` supplies the outputs. `history` is yesterday's mass book.
/// An output with a positive amount, offered for at least one unit, and
/// exchanged for at least one unit, counts. An output that was not offered
/// does not. Returns that answer.
fn is_selling_enough(process: &Process, history: &MarketHistory) -> bool {
    process.outputs.iter().any(|output| {
        output.amount > 0.0
            && history.mass_offered(output.good) >= 1.0
            && history.mass_exchanged(output.good) >= 1.0
    })
}

/// # Yield Subsistence
///
/// Clears [`JobLine::pays`] on subsistence lines when a specialty pays.
///
/// `lines` already have that mark set. `factuals` supplies the processes.
/// A subsistence line keeps the mark when every paying line is subsistence.
/// Nothing is returned.
fn yield_subsistence(lines: &mut [JobLine], factuals: &Factuals) {
    let specialty_pays = lines.iter().any(|line| {
        line.pays
            && factuals
                .get_process(line.process)
                .is_some_and(|process| !process.is_subsistence())
    });
    if !specialty_pays {
        return;
    }
    for line in lines.iter_mut() {
        if factuals
            .get_process(line.process)
            .is_some_and(|process| process.is_subsistence())
        {
            line.pays = false;
        }
    }
}

/// # Material Covered Runs
///
/// Runs `process` can finish from goods other than time.
///
/// `factuals` names skills. `property_free` and `held` are the piles.
/// Time is left out of the limit. A missing required factor, or required
/// capital short of its amount, returns 0. A process with no other
/// destroyed or consumed input returns 0. Otherwise the tightest of those
/// inputs is the count.
fn material_covered_runs(
    process: &Process,
    factuals: &Factuals,
    property_free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
) -> f64 {
    for factor in process.factors() {
        if !factor.is_optional() && pile_units(property_free, held, factor.good) <= 0.0 {
            return 0.0;
        }
    }
    let mut runs = f64::INFINITY;
    let mut limited = false;
    for input in process.requirements() {
        if input.good == TIME {
            continue;
        }
        if factuals
            .get_good(input.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let have = pile_units(property_free, held, input.good);
        match input.input_type {
            InputType::Capital => {
                if have < input.amount {
                    return 0.0;
                }
            }
            InputType::Destroyed | InputType::Consumed => {
                if input.amount > 0.0 {
                    limited = true;
                    runs = runs.min(have / input.amount);
                }
            }
            InputType::Factor => {}
        }
    }
    if !limited || !runs.is_finite() {
        0.0
    } else {
        runs.max(0.0)
    }
}

/// # Gaps For Cover
///
/// Short desires that keep time before a higher score.
///
/// `gaps` is every short desire, earlier first. `basic_count` is how many
/// of those are basic. `specialty_pays` is true when a line that is not
/// subsistence pays. Every gap is kept when that is false. Otherwise the
/// basic gaps are kept, and so is a later gap that asks for a good those
/// basic gaps already ask for. A later gap for some other good is left
/// out. Returns the gaps to reserve.
fn gaps_for_cover(
    gaps: &[(Vec<(usize, f64)>, f64)],
    basic_count: usize,
    specialty_pays: bool,
) -> Vec<(Vec<(usize, f64)>, f64)> {
    if !specialty_pays {
        return gaps.to_vec();
    }
    let count = basic_count.min(gaps.len());
    let mut kept = Vec::new();
    let mut goods = HashSet::new();
    for gap in &gaps[..count] {
        for (good, _) in &gap.0 {
            goods.insert(*good);
        }
        kept.push(gap.clone());
    }
    // A later tier of a basic good stays in the cover.
    for gap in &gaps[count..] {
        if gap.0.iter().any(|(good, _)| goods.contains(good)) {
            kept.push(gap.clone());
        }
    }
    kept
}

/// # Reserve Basic Runs
///
/// Runs each line needs to cover the short desires it is given.
///
/// `lines` are the job, with [`JobLine::pays`] already set. `factuals`
/// supplies processes. `property_free` and `held` are the piles. `gaps`
/// is each short desire, earlier first: goods with satisfaction per unit,
/// then the missing satisfaction. A paying line's stocked material counts
/// first, through [`material_covered_runs`]. Time is shared, so that count
/// leaves time out. The line that yields the most satisfaction per unit of
/// time fills what is left, whether or not it pays. Runs stop when the time
/// in `property_free` is gone, so an earlier desire keeps its runs. Returns
/// the basic runs and the later runs, one count per line.
///
/// `basic_len` is how many leading gaps are basic. A gap subsistence can
/// fill with a different good stays on that subsistence line.
fn reserve_basic_runs(
    lines: &[JobLine],
    factuals: &Factuals,
    property_free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
    gaps: &[(Vec<(usize, f64)>, f64)],
    basic_len: usize,
) -> (Vec<f64>, Vec<f64>) {
    let mut basic_runs = vec![0.0; lines.len()];
    let mut later_runs = vec![0.0; lines.len()];
    if gaps.is_empty() {
        return (basic_runs, later_runs);
    }
    let made = subsistence_goods(factuals);
    let basic_len = basic_len.min(gaps.len());
    let mut remaining: Vec<f64> = gaps.iter().map(|(_, sat)| sat.max(0.0)).collect();
    for (index, line) in lines.iter().enumerate() {
        if !line.pays {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let covered = material_covered_runs(process, factuals, property_free, held);
        if covered <= 0.0 {
            continue;
        }
        for output in &process.outputs {
            if output.amount <= 0.0 {
                continue;
            }
            let made = output.amount * covered;
            for (gap_index, (goods, _)) in gaps.iter().enumerate() {
                if remaining[gap_index] <= 0.0 {
                    continue;
                }
                let Some((_, efficiency)) = goods.iter().find(|(good, _)| *good == output.good)
                else {
                    continue;
                };
                if *efficiency <= 0.0 {
                    continue;
                }
                let sat = made * efficiency;
                remaining[gap_index] = (remaining[gap_index] - sat).max(0.0);
            }
        }
    }
    let mut time_left = property_free
        .get(&TIME)
        .copied()
        .unwrap_or(0.0)
        .max(0.0);
    for (gap_index, (goods, _)) in gaps.iter().enumerate() {
        if time_left <= 1e-9 {
            break;
        }
        if remaining[gap_index] <= 1e-9 {
            continue;
        }
        let mut best_index: Option<usize> = None;
        let mut best_score = 0.0;
        let mut best_per_run = 0.0;
        let prefer = subsistence_fills_gap(lines, factuals, goods, &made);
        for (index, line) in lines.iter().enumerate() {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            if prefer && !process.is_subsistence() {
                continue;
            }
            let pace = time_spent(process);
            if pace <= 0.0 {
                continue;
            }
            let mut per_run: f64 = 0.0;
            for output in &process.outputs {
                if output.amount <= 0.0 {
                    continue;
                }
                let Some((_, efficiency)) = goods.iter().find(|(good, _)| *good == output.good)
                else {
                    continue;
                };
                if *efficiency <= 0.0 {
                    continue;
                }
                per_run = per_run.max(output.amount * efficiency);
            }
            if per_run <= 0.0 {
                continue;
            }
            let score = per_run / pace;
            if best_index.is_none() || score > best_score {
                best_index = Some(index);
                best_score = score;
                best_per_run = per_run;
            }
        }
        let Some(index) = best_index else {
            continue;
        };
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let pace = time_spent(process);
        if best_per_run <= 0.0 || pace <= 0.0 {
            continue;
        }
        let room = (time_left / pace).max(0.0);
        let runs = (remaining[gap_index] / best_per_run).min(room);
        if !runs.is_finite() || runs <= 0.0 {
            break;
        }
        if gap_index < basic_len {
            basic_runs[index] += runs;
        } else {
            later_runs[index] += runs;
        }
        remaining[gap_index] = (remaining[gap_index] - runs * best_per_run).max(0.0);
        time_left = (time_left - pace * runs).max(0.0);
    }
    (basic_runs, later_runs)
}

/// # Subsistence Goods
///
/// Variable goods made by a subsistence process.
///
/// `factuals` supplies the processes. A process that is not subsistence
/// adds nothing. Returns those good ids.
fn subsistence_goods(factuals: &Factuals) -> HashSet<usize> {
    let mut made = HashSet::new();
    for process in factuals.each_process() {
        if !process.is_subsistence() {
            continue;
        }
        for good in process.variable_goods() {
            made.insert(good);
        }
    }
    made
}

/// # Subsistence Fills Gap
///
/// True when a subsistence line should cover `goods` alone.
///
/// `lines` is the job. `factuals` supplies the processes. `made` is
/// [`subsistence_goods`]. A subsistence line that outputs one of `goods`
/// covers the gap, unless a specialty outputs one of those goods that
/// subsistence also makes. Returns that answer.
fn subsistence_fills_gap(
    lines: &[JobLine],
    factuals: &Factuals,
    goods: &[(usize, f64)],
    made: &HashSet<usize>,
) -> bool {
    let fills = lines.iter().any(|line| {
        factuals.get_process(line.process).is_some_and(|process| {
            process.is_subsistence()
                && process.outputs.iter().any(|output| {
                    output.amount > 0.0 && goods.iter().any(|(good, _)| *good == output.good)
                })
        })
    });
    if !fills {
        return false;
    }
    let replaces = lines.iter().any(|line| {
        factuals.get_process(line.process).is_some_and(|process| {
            !process.is_subsistence()
                && process.outputs.iter().any(|output| {
                    output.amount > 0.0
                        && made.contains(&output.good)
                        && goods.iter().any(|(good, _)| *good == output.good)
                })
        })
    });
    !replaces
}

/// # Shed Idle Line
///
/// Drops one line that has sat at zero for a turn without paying or feeding.
///
/// `lines` are the job after this morning's targets. `factuals` supplies
/// the processes. `history` scores each line and reads yesterday's sale.
/// `reserved` is the basic-gap runs per line. A line with a target above
/// zero, a positive trade score, a sale, or a reserved run clears its
/// count. Any other line adds one day. A line that has sat out this
/// morning is removed, the longest idle streak first. A worse score
/// breaks a tie, then the lower process id. The last line stays.
/// `turn_length` does not extend the wait. Nothing is returned.
fn shed_idle_line(
    lines: &mut Vec<JobLine>,
    factuals: &Factuals,
    history: &MarketHistory,
    reserved: &[f64],
) {
    let limit = 1u32;
    for (index, line) in lines.iter_mut().enumerate() {
        let running = match line.target {
            None => true,
            Some(target) => target.is_finite() && target > 0.0,
        };
        let feeding = reserved.get(index).copied().unwrap_or(0.0) > 0.0;
        let worth_keeping = factuals.get_process(line.process).is_some_and(|process| {
            let score = trade_score(process, history);
            (score.is_finite() && score > 0.0) || is_selling_enough(process, history)
        });
        if running || feeding || worth_keeping {
            line.idle_days = 0;
        } else {
            line.idle_days = line.idle_days.saturating_add(1);
        }
    }
    if lines.len() < 2 {
        return;
    }
    let Some((drop, _)) = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.idle_days >= limit)
        .max_by(|(_, left), (_, right)| {
            left.idle_days.cmp(&right.idle_days).then_with(|| {
                let score = |line: &JobLine| {
                    factuals
                        .get_process(line.process)
                        .map(|process| trade_score(process, history))
                        .unwrap_or(0.0)
                };
                score(right)
                    .total_cmp(&score(left))
                    .then(right.process.cmp(&left.process))
            })
        })
    else {
        return;
    };
    lines.remove(drop);
}

/// # Is Currency Input
///
/// True when `input` is currency a run keeps instead of buying.
///
/// Time is left out. `history` names the tender slots. A capital pile and
/// a factor are left out. Returns that answer.
fn is_currency_input(input: &ProcessInput, history: &MarketHistory) -> bool {
    input.good != TIME
        && input.amount > 0.0
        && matches!(input.input_type, InputType::Destroyed | InputType::Consumed)
        && history.currencies().contains(&input.good)
}

/// # Best Input Line
///
/// The line that does not pay and makes the most of `good` per unit of time.
///
/// `lines` carry [`JobLine::pays`]. `factuals` supplies processes. A line
/// that pays, or that does not output `good`, is skipped. A tie keeps the
/// earlier line. Returns the line index, the output per run, and the time
/// one run spends. No such line returns `None`.
fn best_input_line(
    lines: &[JobLine],
    factuals: &Factuals,
    good: usize,
) -> Option<(usize, f64, f64)> {
    let mut best_index: Option<usize> = None;
    let mut best_score = 0.0;
    let mut best_amount: f64 = 0.0;
    let mut best_pace = 0.0;
    for (index, line) in lines.iter().enumerate() {
        if line.pays {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let mut amount: f64 = 0.0;
        for output in &process.outputs {
            if output.good == good && output.amount > 0.0 {
                amount = amount.max(output.amount);
            }
        }
        if amount <= 0.0 {
            continue;
        }
        let pace = time_spent(process);
        let score = if pace <= 0.0 {
            f64::INFINITY
        } else {
            amount / pace
        };
        if best_index.is_none() || score > best_score {
            best_index = Some(index);
            best_score = score;
            best_amount = amount;
            best_pace = pace;
        }
    }
    best_index.map(|index| (index, best_amount, best_pace))
}

/// # Reserve Input Runs
///
/// Runs a line that does not pay needs to make a paying line's missing inputs.
///
/// `lines` have [`JobLine::pays`] set. `factuals` supplies processes.
/// `property_free` and `held` are the piles. `history` names currencies
/// and the card. `time_left` is the day after the basic gap has taken its
/// time. When `history` names no currency, a paying line asks for the gap
/// between its time ceiling and the runs its stock can finish, and those
/// finished runs keep their time. When it names a currency, the runs the
/// pile can fund keep their time, and the gap is only the runs above that.
/// Currency left after those runs caps the gap. Paying lines ask in
/// [`priority_order`]. Each missing input that is
/// not time and not a currency goes to the line that does not pay and
/// makes the most of it per unit of time. An input with no such line stays
/// a purchase. The reserved time stays inside what those runs leave of
/// `time_left`. Returns one run count per line.
fn reserve_input_runs(
    lines: &[JobLine],
    factuals: &Factuals,
    property_free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
    history: &MarketHistory,
    time_left: f64,
) -> Vec<f64> {
    let mut reserved = vec![0.0; lines.len()];
    if time_left <= 0.0 || !time_left.is_finite() {
        return reserved;
    }
    let currencies = history.currencies();
    let priced = !currencies.is_empty();
    let mut budget = time_left;
    let mut hand = pile_hand(property_free, held);
    let mut funded_at = vec![0.0; lines.len()];
    let mut ceiling_at = vec![0.0; lines.len()];
    // Lock the time a paying line will run before a home-made input takes it.
    for index in priority_order(lines, factuals, history) {
        if !lines[index].pays {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let covered = covered_runs(process, factuals, property_free, held);
        let pace = time_spent(process);
        if priced {
            let mut ceiling: f64 = time_ceiling(process, property_free, held).unwrap_or(covered);
            if !ceiling.is_finite() || ceiling < 0.0 {
                ceiling = 0.0;
            }
            if pace > 0.0 {
                ceiling = ceiling.min((budget / pace).max(0.0));
            }
            let funded = pushed_runs(process, ceiling, &hand, factuals, history, currencies);
            funded_at[index] = funded;
            ceiling_at[index] = ceiling;
            if pace > 0.0 && funded > 0.0 {
                let spend = (pace * funded).min(budget);
                budget = (budget - spend).max(0.0);
            }
            let purse = trade_rate(process, history).is_some_and(|rate| rate > 0.0);
            if let Some(bill) = bill_runs(
                process,
                funded,
                &hand,
                factuals,
                history,
                currencies,
                purse,
            ) {
                for (good, qty) in &bill.take {
                    if currencies.contains(good) && *qty > 0.0 {
                        let left = hand.entry(*good).or_insert(0.0);
                        *left = (*left - *qty).max(0.0);
                    }
                }
                for (good, qty) in &bill.barter {
                    if *qty > 0.0 {
                        let left = hand.entry(*good).or_insert(0.0);
                        *left = (*left - *qty).max(0.0);
                    }
                }
            }
        } else if pace > 0.0 && covered > 0.0 {
            let spend = (pace * covered).min(budget);
            budget = (budget - spend).max(0.0);
        }
    }
    let mut spent: HashMap<usize, f64> = HashMap::new();
    for index in priority_order(lines, factuals, history) {
        if !lines[index].pays || budget <= 1e-12 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let covered = covered_runs(process, factuals, property_free, held);
        let inputs = process.requirements();
        let extra: f64 = if priced {
            let mut extra: f64 = (ceiling_at[index] - funded_at[index]).max(0.0);
            if !extra.is_finite() {
                extra = 0.0;
            }
            let mut room: f64 = f64::INFINITY;
            let mut any = false;
            for input in &inputs {
                if !is_currency_input(input, history) {
                    continue;
                }
                any = true;
                let have = hand.get(&input.good).copied().unwrap_or(0.0).max(0.0);
                room = room.min(have / input.amount);
            }
            if any {
                extra = extra.min(room.max(0.0));
            }
            extra
        } else {
            let ceiling = time_ceiling(process, property_free, held).unwrap_or(covered);
            let mut extra: f64 = (ceiling - covered).max(0.0);
            if !extra.is_finite() {
                extra = 0.0;
            }
            for input in &inputs {
                if !is_currency_input(input, history) {
                    continue;
                }
                let have = pile_units(property_free, held, input.good)
                    - spent.get(&input.good).copied().unwrap_or(0.0);
                let room = (have - covered * input.amount) / input.amount;
                extra = extra.min(room.max(0.0));
            }
            extra
        };
        if extra > 1e-12 {
            let funded = if priced { funded_at[index] } else { covered };
            for input in &inputs {
                if input.good == TIME
                    || input.amount <= 0.0
                    || !matches!(input.input_type, InputType::Destroyed | InputType::Consumed)
                    || is_currency_input(input, history)
                    || factuals
                        .get_good(input.good)
                        .is_some_and(|good| good.is_skill())
                {
                    continue;
                }
                let have = pile_units(property_free, held, input.good);
                let short = if priced {
                    let used = (input.amount * funded).min(have);
                    (input.amount * extra - (have - used)).max(0.0)
                } else {
                    (input.amount * (covered + extra) - have).max(0.0)
                };
                if short <= 1e-12 {
                    continue;
                }
                let Some((supplier, per_run, pace)) =
                    best_input_line(lines, factuals, input.good)
                else {
                    continue;
                };
                if per_run <= 0.0 {
                    continue;
                }
                // Runs already reserved on this line make this good too.
                let still = (short - per_run * reserved[supplier]).max(0.0);
                if still <= 1e-12 {
                    continue;
                }
                let mut runs = still / per_run;
                if pace > 0.0 {
                    runs = runs.min(budget / pace);
                }
                if !runs.is_finite() || runs <= 1e-12 {
                    continue;
                }
                reserved[supplier] += runs;
                if pace > 0.0 {
                    budget = (budget - pace * runs).max(0.0);
                }
            }
        }
        if priced {
            for input in &inputs {
                if is_currency_input(input, history) {
                    let left = hand.entry(input.good).or_insert(0.0);
                    *left = (*left - input.amount * extra.max(0.0)).max(0.0);
                }
            }
        } else {
            for input in &inputs {
                if is_currency_input(input, history) {
                    let used = spent.entry(input.good).or_insert(0.0);
                    *used += input.amount * (covered + extra.max(0.0));
                }
            }
        }
    }
    reserved
}

/// # Add Stake Runs
///
/// Adds founding-input runs onto `reserved`, inside the points still free.
///
/// `lines` and `factuals` supply the job. `gaps` are the short desires.
/// `households` sizes the traveling share. `points` is the day's complexity
/// budget. The added runs are [`stake_input_runs`]. When those runs cost
/// more than the points left after `reserved`, they are scaled down to
/// that remainder. The runs already in `reserved` stay. Nothing is returned.
fn add_stake_runs(
    lines: &[JobLine],
    factuals: &Factuals,
    reserved: &mut [f64],
    gaps: &[(Vec<(usize, f64)>, f64)],
    households: f64,
    points: f64,
) {
    let mut stake = stake_input_runs(lines, factuals, gaps, households);
    if points.is_finite() {
        let spare = (points - cover_points(lines, factuals, reserved)).max(0.0);
        let cost = cover_points(lines, factuals, &stake);
        if cost > spare && cost > 0.0 {
            let scale = spare / cost;
            for runs in &mut stake {
                *runs *= scale;
            }
        }
    }
    for (slot, extra) in reserved.iter_mut().zip(stake) {
        *slot += extra;
    }
}

/// # Stake Input Runs
///
/// Extra runs that leave inputs for one recipe the job does not run.
///
/// `lines` is the job. `factuals` supplies recipes and the founding
/// settings. `gaps` are short desires, earlier first. `households` is the
/// pop's count. A gap some line already fills is skipped. The first
/// remaining gap with a [`recipe_for_good`] whose material inputs this job
/// makes is the one that is stocked. Each destroyed input asks for enough
/// output that [`crate::game::config::PopConfig::founding_runs`] of it fit
/// in [`founding_goods_fraction`]. Time and skills are not stocked. A
/// share that is not positive returns zeros. Returns one run count per line.
fn stake_input_runs(
    lines: &[JobLine],
    factuals: &Factuals,
    gaps: &[(Vec<(usize, f64)>, f64)],
    households: f64,
) -> Vec<f64> {
    let mut extra = vec![0.0; lines.len()];
    let fraction = founding_goods_fraction(households);
    let batches = factuals.config.pop.founding_runs;
    if fraction <= 0.0 || !batches.is_finite() || batches <= 0.0 {
        return extra;
    }
    for gap in gaps {
        let made = gap
            .0
            .iter()
            .any(|(good, _)| line_makes_good(lines, factuals, *good));
        if made {
            continue;
        }
        let Some(recipe) = suppliable_recipe(gap, lines, factuals) else {
            continue;
        };
        for input in recipe.requirements() {
            if input.good == TIME
                || !input.amount.is_finite()
                || input.amount <= 0.0
                || !matches!(input.input_type, InputType::Destroyed | InputType::Consumed)
                || factuals
                    .get_good(input.good)
                    .is_some_and(|good| good.is_skill())
            {
                continue;
            }
            let Some((index, per)) = best_output_line(lines, factuals, input.good) else {
                continue;
            };
            if per <= 0.0 {
                continue;
            }
            extra[index] += input.amount * batches / fraction / per;
        }
        break;
    }
    extra
}

/// # Sale Runs For Unmade
///
/// Extra runs that sell for a desire no line can make.
///
/// `lines` is the job, with [`JobLine::pays`] set. `factuals` supplies
/// processes. `history` prices the buy and names tender. `gaps` are the
/// short desires. `reserved` is the held runs. Input runs above `meal_keep`
/// are not a meal, so a bill drops them. Other lines then drop toward
/// `floor` when the bill still needs the points or the time. `free` and
/// `held` are the piles. `points` is the day's complexity budget.
///
/// The bill is the cheapest card that buys each unmade gap, minus tender
/// already on hand. The highest trade score that can sell takes that bill,
/// then the next. A line that does not pay is not a seller. Runs stop when
/// the points or the time above `floor` run out. Material inputs those
/// runs still need are added to `reserved` on the line that makes them.
/// Returns the sale runs, one count per line.
fn sale_runs_for_unmade(
    lines: &[JobLine],
    factuals: &Factuals,
    history: &MarketHistory,
    gaps: &[(Vec<(usize, f64)>, f64)],
    reserved: &mut [f64],
    floor: &[f64],
    meal_keep: &[f64],
    free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
    points: f64,
) -> Vec<f64> {
    let mut extra = vec![0.0; lines.len()];
    let bill = (unmade_buy_cost(lines, factuals, history, gaps) - tender_on_hand(history, free, held))
        .max(0.0);
    if bill <= 1e-9 {
        return extra;
    }
    let sellers = sale_line_order(lines, factuals, history);
    if sellers.is_empty() {
        return extra;
    }
    // The input pile above the desire hold was for a ceiling the sale replaces.
    for (index, slot) in reserved.iter_mut().enumerate() {
        if sellers.contains(&index) {
            continue;
        }
        let keep = meal_keep.get(index).copied().unwrap_or(0.0).max(0.0);
        if *slot > keep {
            *slot = keep;
        }
    }
    let time_on_hand = free.get(&TIME).copied().unwrap_or(0.0).max(0.0);
    let mut left = bill;
    for seller in sellers {
        if left <= 1e-9 {
            break;
        }
        let Some(process) = factuals.get_process(lines[seller].process) else {
            continue;
        };
        let per = sale_per_run(process, factuals, history);
        if per <= 0.0 {
            continue;
        }
        let fitted = commit_sale_runs(
            seller,
            left / per,
            lines,
            factuals,
            history,
            reserved,
            floor,
            free,
            held,
            points,
            time_on_hand,
        );
        if fitted <= 1e-12 {
            continue;
        }
        extra[seller] = fitted;
        left -= fitted * per;
    }
    extra
}

/// # Unmade Buy Cost
///
/// Card cost of the gaps no line can make.
///
/// `lines` and `factuals` say which goods the job makes. `history` prices
/// each target. `gaps` are short desires. A gap some line outputs is
/// skipped. Each other gap takes the target with the lowest positive card
/// per unit of satisfaction. A gap with no such target adds nothing.
/// Returns that sum.
fn unmade_buy_cost(
    lines: &[JobLine],
    factuals: &Factuals,
    history: &MarketHistory,
    gaps: &[(Vec<(usize, f64)>, f64)],
) -> f64 {
    let mut bill = 0.0;
    for (goods, sat) in gaps {
        if !sat.is_finite() || *sat <= 1e-9 {
            continue;
        }
        if goods
            .iter()
            .any(|(good, _)| line_makes_good(lines, factuals, *good))
        {
            continue;
        }
        let mut best: Option<f64> = None;
        for &(good, efficiency) in goods {
            if !efficiency.is_finite() || efficiency <= 0.0 {
                continue;
            }
            let price = history.price(good);
            if !price.is_finite() || price <= 0.0 {
                continue;
            }
            let cost = *sat / efficiency * price;
            if !cost.is_finite() || cost <= 0.0 {
                continue;
            }
            best = Some(best.map(|held| held.min(cost)).unwrap_or(cost));
        }
        if let Some(cost) = best {
            bill += cost;
        }
    }
    bill
}

/// # Tender On Hand
///
/// Currency in `free` and `held`, priced on `history`.
///
/// A slot with a non-positive card adds nothing. Returns that AMV.
fn tender_on_hand(
    history: &MarketHistory,
    free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
) -> f64 {
    let mut sum = 0.0;
    for &id in history.currencies() {
        let price = history.price(id);
        if !price.is_finite() || price <= 0.0 {
            continue;
        }
        let have = free.get(&id).copied().unwrap_or(0.0).max(0.0)
            + held.get(&id).copied().unwrap_or(0.0).max(0.0);
        sum += have * price;
    }
    sum
}

/// # Sale Line Order
///
/// Paying lines that can sell a variable product, highest trade score first.
///
/// `factuals` supplies processes. `history` prices [`sale_per_run`] and
/// [`trade_score`]. A line that does not pay, or whose sale value is not
/// positive, is left out. A tie keeps the lower index. Returns those indexes.
fn sale_line_order(lines: &[JobLine], factuals: &Factuals, history: &MarketHistory) -> Vec<usize> {
    let mut sellers = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !line.pays {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        if sale_per_run(process, factuals, history) <= 0.0 {
            continue;
        }
        sellers.push(index);
    }
    sellers.sort_by(|&left, &right| {
        let score = |index: usize| {
            factuals
                .get_process(lines[index].process)
                .map(|process| trade_score(process, history))
                .unwrap_or(f64::MIN)
        };
        score(right).total_cmp(&score(left)).then(left.cmp(&right))
    });
    sellers
}

/// # Sale Per Run
///
/// Card value of one run's variable product.
///
/// `process` supplies the outputs. `factuals` names skills. `history`
/// prices the card and names tender. A fixed output, a skill, time, and a
/// currency add nothing. A non-positive amount or card adds nothing.
/// Returns the sum.
fn sale_per_run(process: &Process, factuals: &Factuals, history: &MarketHistory) -> f64 {
    let mut sum = 0.0;
    for output in &process.outputs {
        if output.fixed || output.amount <= 0.0 || output.good == TIME {
            continue;
        }
        if history.currencies().contains(&output.good) {
            continue;
        }
        if factuals
            .get_good(output.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let price = history.price(output.good);
        if !price.is_finite() || price <= 0.0 {
            continue;
        }
        sum += output.amount * price;
    }
    sum
}

/// # Add Short Book Runs
///
/// Adds sale runs for a paying line whose product is still sought.
///
/// `lines` are the job. `factuals` supplies each process. `history`
/// prices the score and the bid. `sale` is the unmade-desire sale, one
/// count per line, and this adds to it. `points` is complexity still
/// free after that sale and the meal.
///
/// A line that does not pay, or whose score is not above zero, adds
/// nothing. The runs are [`sought_sale_runs`]. Lines walk in
/// [`priority_order`]. Each add stops at the runs `points` can still
/// fund. A process that spends no points is not limited by them.
/// Nothing is returned.
fn add_short_book_runs(
    lines: &[JobLine],
    factuals: &Factuals,
    history: &MarketHistory,
    sale: &mut [f64],
    points: f64,
) {
    let mut left = if points.is_finite() {
        points.max(0.0)
    } else {
        f64::INFINITY
    };
    for index in priority_order(lines, factuals, history) {
        if !lines.get(index).is_some_and(|line| line.pays) {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let score = trade_score(process, history);
        if !score.is_finite() || score <= 0.0 {
            continue;
        }
        let want = sought_sale_runs(process, factuals, history);
        if want <= 0.0 {
            continue;
        }
        let spent = complexity_spent(process);
        let mut add = want;
        if spent > 0.0 && left.is_finite() {
            add = add.min((left / spent).max(0.0));
        }
        if !add.is_finite() || add <= 0.0 {
            continue;
        }
        if let Some(slot) = sale.get_mut(index) {
            *slot += add;
        }
        if spent > 0.0 && left.is_finite() {
            left = (left - spent * add).max(0.0);
        }
    }
}

/// # Sale Point Cost
///
/// Complexity the sale runs spend at each line's own rate.
///
/// `lines` and `factuals` supply the processes. `runs` is one count per
/// line. A missing process adds nothing. Returns that cost.
fn sale_point_cost(lines: &[JobLine], factuals: &Factuals, runs: &[f64]) -> f64 {
    let mut sum = 0.0;
    for (index, &n) in runs.iter().enumerate() {
        if n <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        sum += complexity_spent(process) * n;
    }
    sum
}

/// # Commit Sale Runs
///
/// Holds up to `want` sale runs of `seller` and the inputs those runs need.
///
/// The other arguments match [`sale_runs_for_unmade`]. Lines that are not
/// sellers may fall toward `floor` until the runs fit, and no further.
/// The input runs are added to `reserved`. Returns the sale runs that fit.
fn commit_sale_runs(
    seller: usize,
    want: f64,
    lines: &[JobLine],
    factuals: &Factuals,
    history: &MarketHistory,
    reserved: &mut [f64],
    floor: &[f64],
    free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
    points: f64,
    time_on_hand: f64,
) -> f64 {
    if !want.is_finite() || want <= 1e-12 {
        return 0.0;
    }
    let sellers = sale_line_order(lines, factuals, history);
    let floored = reserved_at_floor(reserved, floor, &sellers);
    let additions = supplier_additions(seller, want, lines, factuals, history, &floored, free, held);
    let (need_points, need_time) = sale_bundle_cost(seller, want, lines, factuals, &additions);
    let (virt_points, virt_time) = spare_budget(&floored, lines, factuals, points, time_on_hand);
    let mut scale: f64 = 1.0;
    if need_points > 0.0 && virt_points.is_finite() {
        scale = scale.min((virt_points / need_points).max(0.0));
    }
    if need_time > 0.0 && virt_time.is_finite() {
        scale = scale.min((virt_time / need_time).max(0.0));
    }
    if !scale.is_finite() || scale <= 1e-12 {
        return 0.0;
    }
    let mut runs = want * scale.min(1.0);
    let additions = supplier_additions(seller, runs, lines, factuals, history, &floored, free, held);
    let (need_points, need_time) = sale_bundle_cost(seller, runs, lines, factuals, &additions);
    let (have_points, have_time) = spare_budget(reserved, lines, factuals, points, time_on_hand);
    release_toward_floor(
        reserved,
        &floored,
        lines,
        factuals,
        (need_points - have_points).max(0.0),
        (need_time - have_time).max(0.0),
    );
    let additions = supplier_additions(seller, runs, lines, factuals, history, reserved, free, held);
    let (need_points, need_time) = sale_bundle_cost(seller, runs, lines, factuals, &additions);
    let (have_points, have_time) = spare_budget(reserved, lines, factuals, points, time_on_hand);
    let mut fit: f64 = 1.0;
    if need_points > have_points + 1e-9 && need_points > 0.0 {
        fit = fit.min((have_points / need_points).max(0.0));
    }
    if need_time > have_time + 1e-9 && need_time > 0.0 {
        fit = fit.min((have_time / need_time).max(0.0));
    }
    if fit < 1.0 {
        runs *= fit;
    }
    if !runs.is_finite() || runs <= 1e-12 {
        return 0.0;
    }
    let additions = supplier_additions(seller, runs, lines, factuals, history, reserved, free, held);
    for (index, add) in additions.into_iter().enumerate() {
        if add > 0.0 && index < reserved.len() {
            reserved[index] += add;
        }
    }
    runs
}

/// # Reserved At Floor
///
/// `reserved` with every non-seller brought down to `floor`.
///
/// A seller keeps its count. A line already at or under its floor stays.
/// Returns that count per line.
fn reserved_at_floor(reserved: &[f64], floor: &[f64], sellers: &[usize]) -> Vec<f64> {
    reserved
        .iter()
        .enumerate()
        .map(|(index, &runs)| {
            if sellers.contains(&index) {
                runs
            } else {
                let keep = floor.get(index).copied().unwrap_or(0.0).max(0.0);
                runs.min(keep)
            }
        })
        .collect()
}

/// # Release Toward Floor
///
/// Lowers `reserved` toward `virtual` until `need_points` and `need_time` are free.
///
/// `virtual` is the floor from [`reserved_at_floor`]. `lines` and
/// `factuals` price the released runs. Each line gives the same share of
/// the gap. A need the gap cannot fund releases the whole gap. Nothing
/// is returned.
fn release_toward_floor(
    reserved: &mut [f64],
    virtual_reserved: &[f64],
    lines: &[JobLine],
    factuals: &Factuals,
    need_points: f64,
    need_time: f64,
) {
    if need_points <= 1e-12 && need_time <= 1e-12 {
        return;
    }
    let rates = subsistence_cover(factuals);
    let mut give = vec![0.0; reserved.len()];
    let mut pool_points = 0.0;
    let mut pool_time = 0.0;
    for index in 0..reserved.len() {
        let floor = virtual_reserved.get(index).copied().unwrap_or(0.0);
        let releasable = (reserved[index] - floor).max(0.0);
        if releasable <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        give[index] = releasable;
        pool_points += cover_rate(process, &rates, lines[index].cover_cheap) * releasable;
        pool_time += time_spent(process) * releasable;
    }
    let point_share = if need_points > 1e-12 && pool_points > 0.0 {
        (need_points / pool_points).min(1.0)
    } else {
        0.0
    };
    let time_share = if need_time > 1e-12 && pool_time > 0.0 {
        (need_time / pool_time).min(1.0)
    } else {
        0.0
    };
    let share = point_share.max(time_share);
    if share <= 0.0 {
        return;
    }
    for (slot, releasable) in reserved.iter_mut().zip(&give) {
        *slot = (*slot - releasable * share).max(0.0);
    }
}

/// # Spare Budget
///
/// Points and time `reserved` has not taken.
///
/// `points` is the day's complexity budget. `time_on_hand` is the morning
/// grant. An open point budget stays open. Returns the two remainders,
/// never negative.
fn spare_budget(
    reserved: &[f64],
    lines: &[JobLine],
    factuals: &Factuals,
    points: f64,
    time_on_hand: f64,
) -> (f64, f64) {
    let spare_points = if points.is_finite() {
        (points - cover_points(lines, factuals, reserved)).max(0.0)
    } else {
        f64::INFINITY
    };
    let spent = reservation_time(lines, factuals, reserved)
        .into_iter()
        .sum::<f64>();
    let spare_time = (time_on_hand - spent).max(0.0);
    (spare_points, spare_time)
}

/// # Sale Bundle Cost
///
/// Points and time for `runs` of `seller` plus `additions`.
///
/// The sale runs spend [`complexity_spent`]. Each added input run spends
/// [`cover_rate`] and its time. Returns those two costs.
fn sale_bundle_cost(
    seller: usize,
    runs: f64,
    lines: &[JobLine],
    factuals: &Factuals,
    additions: &[f64],
) -> (f64, f64) {
    let rates = subsistence_cover(factuals);
    let mut points = 0.0;
    let mut time = 0.0;
    if runs > 0.0 {
        if let Some(process) = factuals.get_process(lines[seller].process) {
            points += complexity_spent(process) * runs;
            time += time_spent(process) * runs;
        }
    }
    for (index, &add) in additions.iter().enumerate() {
        if add <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        points += cover_rate(process, &rates, lines[index].cover_cheap) * add;
        time += time_spent(process) * add;
    }
    (points, time)
}

/// # Supplier Additions
///
/// Runs that make the material inputs `runs` of `seller` are still short.
///
/// `reserved` is the hold those runs sit on top of. `free` and `held` are
/// the piles. Time, skills, currency, and capital are not made here. The
/// seller does not supply itself. Returns one added count per line.
fn supplier_additions(
    seller: usize,
    runs: f64,
    lines: &[JobLine],
    factuals: &Factuals,
    history: &MarketHistory,
    reserved: &[f64],
    free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
) -> Vec<f64> {
    let mut extra = vec![0.0; lines.len()];
    if !runs.is_finite() || runs <= 0.0 {
        return extra;
    }
    let Some(process) = factuals.get_process(lines[seller].process) else {
        return extra;
    };
    let mut spare = goods_after_reserve(lines, factuals, reserved, free, held);
    for input in process.requirements() {
        if input.good == TIME
            || input.amount <= 0.0
            || !matches!(input.input_type, InputType::Destroyed | InputType::Consumed)
            || history.currencies().contains(&input.good)
            || factuals
                .get_good(input.good)
                .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let need = input.amount * runs;
        let have = spare.get(&input.good).copied().unwrap_or(0.0).max(0.0);
        let short = (need - have).max(0.0);
        if let Some(pile) = spare.get_mut(&input.good) {
            *pile = (*pile - need).max(0.0);
        }
        if short <= 1e-12 {
            continue;
        }
        let Some((index, per)) = best_output_line(lines, factuals, input.good) else {
            continue;
        };
        if index == seller || per <= 0.0 {
            continue;
        }
        let add = short / per;
        if !add.is_finite() || add <= 0.0 {
            continue;
        }
        extra[index] += add;
        if let Some(maker) = factuals.get_process(lines[index].process) {
            for output in &maker.outputs {
                if output.good == input.good && output.amount > 0.0 {
                    *spare.entry(input.good).or_insert(0.0) += output.amount * add;
                }
            }
        }
    }
    extra
}

/// # Goods After Reserve
///
/// Stock left after `reserved` runs make and consume their goods.
///
/// `free` and `held` are the piles. Time is left out. A fixed output is
/// left out. Destroyed and consumed inputs are taken out. The result is
/// never negative. Returns units by good.
fn goods_after_reserve(
    lines: &[JobLine],
    factuals: &Factuals,
    reserved: &[f64],
    free: &HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
) -> HashMap<usize, f64> {
    let mut spare = HashMap::new();
    for (&good, &units) in free {
        if good != TIME && units > 0.0 {
            spare.insert(good, units);
        }
    }
    for (&good, &units) in held {
        if good != TIME && units > 0.0 {
            *spare.entry(good).or_insert(0.0) += units;
        }
    }
    for (index, &runs) in reserved.iter().enumerate() {
        if runs <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        for output in &process.outputs {
            if output.fixed || output.amount <= 0.0 || output.good == TIME {
                continue;
            }
            *spare.entry(output.good).or_insert(0.0) += output.amount * runs;
        }
        for input in process.requirements() {
            if input.good == TIME
                || input.amount <= 0.0
                || !matches!(input.input_type, InputType::Destroyed | InputType::Consumed)
            {
                continue;
            }
            *spare.entry(input.good).or_insert(0.0) -= input.amount * runs;
        }
    }
    for units in spare.values_mut() {
        *units = units.max(0.0);
    }
    spare
}

/// # Line Makes Good
///
/// True when a line in `lines` outputs `good`.
///
/// `factuals` supplies the processes. A missing process is skipped. A
/// non-positive output does not count. Returns that answer.
fn line_makes_good(lines: &[JobLine], factuals: &Factuals, good: usize) -> bool {
    lines.iter().any(|line| {
        factuals.get_process(line.process).is_some_and(|process| {
            process
                .outputs
                .iter()
                .any(|output| output.good == good && output.amount > 0.0)
        })
    })
}

/// # Suppliable Recipe
///
/// The first recipe in `gap` whose material inputs `lines` make.
///
/// `factuals` supplies the processes. Targets are walked in stored order.
/// [`recipe_for_good`] picks the process. A recipe with a destroyed input
/// this job does not make is skipped. Returns that process, or `None`.
fn suppliable_recipe<'a>(
    gap: &(Vec<(usize, f64)>, f64),
    lines: &[JobLine],
    factuals: &'a Factuals,
) -> Option<&'a Process> {
    for (good, _) in &gap.0 {
        let Some(id) = recipe_for_good(factuals, *good) else {
            continue;
        };
        let Some(recipe) = factuals.get_process(id) else {
            continue;
        };
        if recipe_inputs_are_made(recipe, lines, factuals) {
            return Some(recipe);
        }
    }
    None
}

/// # Recipe Inputs Are Made
///
/// True when every material `recipe` destroys is an output of `lines`.
///
/// `factuals` names skills. Time is not a material. A skill is not one.
/// Capital and factors are not required. A recipe with no such input is
/// made. Returns that answer.
fn recipe_inputs_are_made(recipe: &Process, lines: &[JobLine], factuals: &Factuals) -> bool {
    recipe.requirements().iter().all(|input| {
        input.good == TIME
            || !matches!(input.input_type, InputType::Destroyed | InputType::Consumed)
            || factuals
                .get_good(input.good)
                .is_some_and(|good| good.is_skill())
            || line_makes_good(lines, factuals, input.good)
    })
}

/// # Best Output Line
///
/// The line in `lines` that makes the most of `good` per unit of time.
///
/// `factuals` supplies the processes. A line that does not output `good`
/// is skipped. A line with no time cost sorts ahead of one that spends
/// time. The first line wins a tie. Returns that index and the output
/// per run, or `None`.
fn best_output_line(lines: &[JobLine], factuals: &Factuals, good: usize) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (index, line) in lines.iter().enumerate() {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let amount = process
            .outputs
            .iter()
            .filter(|output| output.good == good && output.amount > 0.0)
            .map(|output| output.amount)
            .fold(0.0_f64, f64::max);
        if amount <= 0.0 {
            continue;
        }
        let pace = time_spent(process);
        let score = if pace <= 0.0 {
            f64::INFINITY
        } else {
            amount / pace
        };
        let replace = match best {
            None => true,
            Some((_, _, have)) => score > have,
        };
        if replace {
            best = Some((index, amount, score));
        }
    }
    best.map(|(index, amount, _)| (index, amount))
}

/// # Reservation Time
///
/// Time each reserved run count spends.
///
/// `lines` and `reserved` share indexes. `factuals` supplies the processes.
/// A missing process, or a run count at or below zero, spends nothing.
/// Returns one time total per line.
fn reservation_time(lines: &[JobLine], factuals: &Factuals, reserved: &[f64]) -> Vec<f64> {
    let mut time = vec![0.0; lines.len()];
    for (index, runs) in reserved.iter().enumerate() {
        if *runs <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        time[index] = time_spent(process) * runs;
    }
    time
}

/// # Cap Reservation
///
/// Shrinks `reserved` so its time fits in `time_free`.
///
/// `lines` and `factuals` price that time. A reservation already inside
/// the day is left as it is. Returns the time each line spends after the
/// shrink.
fn cap_reservation(
    lines: &[JobLine],
    factuals: &Factuals,
    reserved: &mut [f64],
    time_free: f64,
) -> Vec<f64> {
    let mut line_time = reservation_time(lines, factuals, reserved);
    let withhold: f64 = line_time.iter().copied().sum();
    if withhold > time_free && withhold > 0.0 {
        let scale = time_free / withhold;
        for (index, runs) in reserved.iter_mut().enumerate() {
            *runs *= scale;
            line_time[index] *= scale;
        }
    }
    line_time
}

/// One funded slice of a paying line: what it buys and what it takes off the pile.
struct RunBill {
    buys: HashMap<usize, f64>,
    take: HashMap<usize, f64>,
    /// Other goods that pay for a currency input. They stay off the sell book.
    barter: HashMap<usize, f64>,
}

/// # Is Other Tender Offered
///
/// True when a tender other than `good` was offered for at least one unit.
///
/// `history` is yesterday's mass book. `currencies` are the tender slots.
/// `good` is the currency a run is short. Returns that answer.
fn is_other_tender_offered(history: &MarketHistory, currencies: &[usize], good: usize) -> bool {
    currencies
        .iter()
        .any(|&id| id != good && history.mass_offered(id) >= 1.0)
}

/// # Is Barter Good
///
/// True when `good` can pay for a currency.
///
/// `factuals` names the good. `history` prices it. `currencies` are the
/// tender slots. A currency, a skill, a transport good, and an untradeable
/// good are left out. A missing good is left out. A card that is not
/// positive is left out. Returns that answer.
fn is_barter_good(
    good: usize,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
) -> bool {
    if currencies.contains(&good) {
        return false;
    }
    let price = history.price(good);
    if !price.is_finite() || price <= 0.0 {
        return false;
    }
    factuals.get_good(good).is_some_and(|row| {
        row.is_buyable() && !row.is_skill() && !row.is_transport()
    })
}

/// # Is Recipe Good
///
/// True when `process` takes `good` as any input.
///
/// That stock is the run, so it does not also pay for a currency input.
/// Returns that answer.
fn is_recipe_good(process: &Process, good: usize) -> bool {
    process.inputs.iter().any(|input| input.good == good)
}

/// # Cover With Goods
///
/// Whole goods on `hand` that cover `due`, or `None` when they cannot.
///
/// `factuals` and `history` choose and price those goods. `currencies` are
/// the tender slots. `process` inputs are left out. The largest card is
/// spent first. One extra unit covers a remainder, the lowest card that
/// still has a unit. Returns the units by good.
fn cover_with_goods(
    due: f64,
    hand: &HashMap<usize, f64>,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    process: &Process,
) -> Option<HashMap<usize, f64>> {
    if !due.is_finite() || due <= 1e-9 {
        return Some(HashMap::new());
    }
    let mut coins: Vec<(usize, f64, f64)> = hand
        .iter()
        .filter_map(|(&id, &qty)| {
            if is_recipe_good(process, id) || !is_barter_good(id, factuals, history, currencies) {
                return None;
            }
            let have = qty.floor();
            (have >= 1.0).then_some((id, history.price(id), have))
        })
        .collect();
    coins.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.0.cmp(&right.0))
    });
    let mut remain = due;
    let mut pay = HashMap::new();
    for &(id, price, have) in &coins {
        if remain <= 1e-6 {
            break;
        }
        let n = (remain / price).floor().min(have);
        if n >= 1.0 {
            pay.insert(id, n);
            remain -= n * price;
        }
    }
    if remain > 1e-6 {
        let spare = coins.iter().filter_map(|&(id, price, have)| {
            let used = pay.get(&id).copied().unwrap_or(0.0);
            (have >= used + 1.0 && price + 1e-9 >= remain).then_some((id, price))
        });
        let mut best: Option<(usize, f64)> = None;
        for (id, price) in spare {
            let take = match best {
                None => true,
                Some((kept, kept_price)) => {
                    price < kept_price || (price == kept_price && id < kept)
                }
            };
            if take {
                best = Some((id, price));
            }
        }
        let (id, _) = best?;
        *pay.entry(id).or_insert(0.0) += 1.0;
    }
    Some(pay)
}

/// # Bill Runs
///
/// The buy and the stock `runs` of `process` take, or `None` when the pile cannot fund them.
///
/// `hand` is working stock plus input hold. `factuals` names skills.
/// `history` prices the buy and yesterday's offers. `purse_buys_currency`
/// lets a short currency input be paid with other tender. A currency the
/// pile already holds is kept, and it is not bought. A currency the pile
/// is short is bought with other goods, even when another tender was
/// offered. When that tender was offered and the purse covers the card,
/// the purse pays it instead. The buy is rounded up to a whole unit.
/// Returns the bill.
fn bill_runs(
    process: &Process,
    runs: f64,
    hand: &HashMap<usize, f64>,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    purse_buys_currency: bool,
) -> Option<RunBill> {
    if !runs.is_finite() || runs <= 0.0 {
        return Some(RunBill {
            buys: HashMap::new(),
            take: HashMap::new(),
            barter: HashMap::new(),
        });
    }
    let mut buys = HashMap::new();
    let mut take = HashMap::new();
    let mut cost = 0.0;
    let mut goods_cost = 0.0;
    let mut purse_currency = 0.0;
    for input in process.requirements() {
        if factuals
            .get_good(input.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        if input.good == TIME {
            continue;
        }
        match input.input_type {
            InputType::Capital => {
                if hand.get(&input.good).copied().unwrap_or(0.0) + 1e-9 < input.amount {
                    return None;
                }
            }
            InputType::Destroyed | InputType::Consumed => {
                if input.amount <= 0.0 {
                    continue;
                }
                let need = runs * input.amount;
                let have = hand.get(&input.good).copied().unwrap_or(0.0).max(0.0);
                if currencies.contains(&input.good) {
                    if have + 1e-9 < need {
                        let buy = (need - have).ceil();
                        let price = history.price(input.good);
                        if buy < 1.0 || !price.is_finite() || price <= 0.0 {
                            return None;
                        }
                        if is_other_tender_offered(history, currencies, input.good)
                            && purse_buys_currency
                        {
                            // The purse may pay this card. Other goods pay it when the purse is short.
                            purse_currency += buy * price;
                        } else {
                            goods_cost += buy * price;
                        }
                        *buys.entry(input.good).or_insert(0.0) += buy;
                        if have > 0.0 {
                            *take.entry(input.good).or_insert(0.0) += have.min(need);
                        }
                    } else if need > 0.0 {
                        *take.entry(input.good).or_insert(0.0) += need;
                    }
                } else if need > have {
                    let buy = (need - have).ceil();
                    let price = history.price(input.good);
                    if buy >= 1.0 && price.is_finite() && price > 0.0 {
                        cost += buy * price;
                        *buys.entry(input.good).or_insert(0.0) += buy;
                    }
                    if have > 0.0 {
                        *take.entry(input.good).or_insert(0.0) += have.min(need);
                    }
                } else if need > 0.0 {
                    *take.entry(input.good).or_insert(0.0) += need;
                }
            }
            InputType::Factor => {}
        }
    }
    let mut purse = 0.0;
    for &id in currencies {
        let price = history.price(id);
        if !price.is_finite() || price <= 0.0 {
            continue;
        }
        let have = hand.get(&id).copied().unwrap_or(0.0);
        let held = take.get(&id).copied().unwrap_or(0.0);
        purse += (have - held).floor().max(0.0) * price;
    }
    if purse + 1e-6 < cost + purse_currency {
        // The purse cannot cover the currency card, so other goods pay it.
        goods_cost += purse_currency;
    } else {
        cost += purse_currency;
    }
    if purse + 1e-6 < cost {
        return None;
    }
    let mut left = cost;
    let mut coins: Vec<(usize, f64)> = currencies
        .iter()
        .copied()
        .filter_map(|id| {
            let price = history.price(id);
            if !price.is_finite() || price <= 0.0 {
                return None;
            }
            let have = hand.get(&id).copied().unwrap_or(0.0);
            let held = take.get(&id).copied().unwrap_or(0.0);
            let spendable = (have - held).floor().max(0.0);
            (spendable >= 1.0).then_some((id, price))
        })
        .collect();
    coins.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.0.cmp(&right.0))
    });
    for (id, price) in coins {
        if left <= 1e-9 {
            break;
        }
        let have = hand.get(&id).copied().unwrap_or(0.0);
        let held = take.get(&id).copied().unwrap_or(0.0);
        let spendable = (have - held).floor().max(0.0);
        let n = (left / price).ceil().min(spendable).max(0.0);
        if n <= 0.0 {
            continue;
        }
        *take.entry(id).or_insert(0.0) += n;
        left -= n * price;
    }
    if left > 1e-6 {
        return None;
    }
    // Other goods cover a currency input the purse did not pay.
    let barter = cover_with_goods(goods_cost, hand, factuals, history, currencies, process)?;
    Some(RunBill { buys, take, barter })
}

/// # Funded Runs
///
/// Largest run count at or under `target` that [`bill_runs`] can fund.
///
/// The arguments match that bill. `purse_buys_currency` is passed through.
/// A target the pile can already fund is returned whole. Otherwise the
/// search keeps the largest count that still bills. Returns that count.
fn funded_runs(
    process: &Process,
    target: f64,
    hand: &HashMap<usize, f64>,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    purse_buys_currency: bool,
) -> f64 {
    if !target.is_finite() || target <= 0.0 {
        return 0.0;
    }
    if bill_runs(
        process,
        target,
        hand,
        factuals,
        history,
        currencies,
        purse_buys_currency,
    )
    .is_some()
    {
        return target;
    }
    let mut low = 0.0;
    let mut high = target;
    for _ in 0..48 {
        let mid = (low + high) / 2.0;
        if bill_runs(
            process,
            mid,
            hand,
            factuals,
            history,
            currencies,
            purse_buys_currency,
        )
        .is_some()
        {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

/// # Note Funded Bill
///
/// Shops `bill`'s buys and keeps its tender in the planning piles' working stock.
///
/// `currencies` are the tender. `property_free` and `held` are those piles.
/// Tender taken from working stock is added to `unlisted` and removed from
/// that pile. Tender already in `held` is removed there. Goods that pay for
/// a currency input are added to `unlisted` the same way. Buys are added to
/// `shopping`. Nothing is returned.
fn note_funded_bill(
    shopping: &mut HashMap<usize, f64>,
    unlisted: &mut HashMap<usize, f64>,
    property_free: &mut HashMap<usize, f64>,
    held: &mut HashMap<usize, f64>,
    currencies: &[usize],
    bill: RunBill,
) {
    for (good, qty) in bill.buys {
        if qty > 0.0 {
            *shopping.entry(good).or_insert(0.0) += qty;
        }
    }
    for (good, qty) in bill.take {
        if qty <= 0.0 || !currencies.contains(&good) {
            continue;
        }
        let working = property_free.get(&good).copied().unwrap_or(0.0);
        let from_working = qty.min(working).max(0.0);
        if from_working > 0.0 {
            *unlisted.entry(good).or_insert(0.0) += from_working;
            let left = property_free.get_mut(&good).expect("working stock vanished");
            *left = (*left - from_working).max(0.0);
        }
        let rest = (qty - from_working).max(0.0);
        if rest > 0.0 {
            if let Some(left) = held.get_mut(&good) {
                *left = (*left - rest).max(0.0);
            }
        }
    }
    for (good, qty) in bill.barter {
        if qty <= 0.0 {
            continue;
        }
        let working = property_free.get(&good).copied().unwrap_or(0.0);
        let from_working = qty.min(working).max(0.0);
        if from_working > 0.0 {
            *unlisted.entry(good).or_insert(0.0) += from_working;
            let left = property_free.get_mut(&good).expect("working stock vanished");
            *left = (*left - from_working).max(0.0);
        }
    }
}

/// # Keep Extra Tender
///
/// Tender still in the piles that runs above `funded` would take.
///
/// `ceiling` is the time cap. The gap stops at the tender `property_free`
/// and `held` still hold. That working tender is added to `unlisted` and
/// removed from `property_free`. `history` names the tender. Nothing is returned.
fn keep_extra_tender(
    process: &Process,
    ceiling: f64,
    funded: f64,
    history: &MarketHistory,
    property_free: &mut HashMap<usize, f64>,
    held: &HashMap<usize, f64>,
    unlisted: &mut HashMap<usize, f64>,
) {
    if !ceiling.is_finite() || funded >= ceiling {
        return;
    }
    let mut extra: f64 = (ceiling - funded).max(0.0);
    let inputs = process.requirements();
    let mut room: f64 = f64::INFINITY;
    let mut any = false;
    for input in &inputs {
        if !is_currency_input(input, history) {
            continue;
        }
        any = true;
        let have = pile_units(property_free, held, input.good).max(0.0);
        room = room.min(have / input.amount);
    }
    if !any {
        return;
    }
    extra = extra.min(room.max(0.0));
    if !extra.is_finite() || extra <= 0.0 {
        return;
    }
    for input in &inputs {
        if !is_currency_input(input, history) {
            continue;
        }
        let keep = input.amount * extra;
        let working = property_free.get(&input.good).copied().unwrap_or(0.0);
        let from_working = keep.min(working).max(0.0);
        if from_working > 0.0 {
            *unlisted.entry(input.good).or_insert(0.0) += from_working;
            let left = property_free.get_mut(&input.good).expect("working stock vanished");
            *left = (*left - from_working).max(0.0);
        }
    }
}

impl Job {
    /// # None
    ///
    /// A job with craft `0`, no lines, and nothing claimed or shopped.
    pub fn none() -> Self {
        Self::new(0, Vec::new())
    }

    /// # New
    ///
    /// A job of `craft` with these lines.
    ///
    /// Claimed inputs and the shopping list start empty.
    pub fn new(craft: usize, lines: Vec<JobLine>) -> Self {
        Self {
            craft,
            lines,
            skills: Vec::new(),
            plan_cost: 0.0,
            claimed: HashMap::new(),
            shopping: HashMap::new(),
            unlisted: HashMap::new(),
            input_hold: HashMap::new(),
            input_feeds: Vec::new(),
            kept: HashMap::new(),
            process_credit: HashMap::new(),
            sale_pile: HashMap::new(),
            sale_sold: HashMap::new(),
            sale_listed: HashMap::new(),
            sale_on: false,
            planned_households: 0.0,
            input_reserve: HashMap::new(),
            stake_hold: HashMap::new(),
        }
    }

    /// # Attach Skills
    ///
    /// Rewrites [`Self::skills`] from the skill inputs on this job's lines.
    ///
    /// `factuals` supplies the processes and which goods are skills. An
    /// optional input tagged [`crate::game::good::GoodTag::Skill`] is kept.
    /// A missing process is skipped. The list is sorted and has no repeats.
    pub fn attach_skills(&mut self, factuals: &Factuals) {
        let mut skills = Vec::new();
        for line in &self.lines {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            for input in process.optional_inputs() {
                let Some(good) = factuals.get_good(input.good) else {
                    continue;
                };
                if good.is_skill() && !skills.contains(&input.good) {
                    skills.push(input.good);
                }
            }
        }
        skills.sort_unstable();
        self.skills = skills;
    }

    /// # Leave
    ///
    /// Units the morning reserve and the next buy leave in quantity.
    ///
    /// `pile` replaces the previous map. A non-positive amount is dropped.
    /// Reserve claims only the free stock above the pile, and settle does
    /// not count it as stock for the next buy.
    pub fn leave(&mut self, pile: &HashMap<usize, f64>) {
        self.kept.clear();
        for (&good, &units) in pile {
            if units > 0.0 {
                self.kept.insert(good, units);
            }
        }
    }

    /// # Feeds Of
    ///
    /// Outputs `input` is consumed to make, and the input units per output unit.
    ///
    /// Recorded by [`Self::reserve`]. A good that was not recorded is empty.
    pub(crate) fn feeds_of(&self, input: usize) -> Vec<(usize, f64)> {
        self.input_feeds
            .iter()
            .filter(|feed| feed.input == input)
            .map(|feed| (feed.output, feed.per_output))
            .collect()
    }

    /// # Claimed Of
    ///
    /// Units of `good` the job holds in its claim book today.
    ///
    /// [`Self::reserve`] adds to the claim, produce spends it, and
    /// [`Self::reset_day`] drops it. A good with no claim is `0`.
    pub(crate) fn claimed_of(&self, good: usize) -> f64 {
        self.claimed.get(&good).copied().unwrap_or(0.0)
    }

    /// # Complexity Cost
    ///
    /// This job's modifier against `baseline`.
    ///
    /// `weight` is the full distance of one extra process. Each line's
    /// process id is counted once. Returns [`Craft::complexity_cost`]:
    /// the craft's complexity modifier plus that distance, capped at 1.0.
    pub fn complexity_cost(&self, baseline: &Craft, weight: f64) -> f64 {
        let mut processes = Vec::new();
        for line in &self.lines {
            if !processes.contains(&line.process) {
                processes.push(line.process);
            }
        }
        baseline.complexity_cost(&processes, weight)
    }

    /// # Craft Point Scale
    ///
    /// The share of a day's points this job's craft spends.
    ///
    /// `factuals` supplies the craft and the craft-distance weight. A
    /// missing craft, and a [`Self::complexity_cost`] that is not positive,
    /// spend the whole budget. Returns that share.
    pub(crate) fn craft_point_scale(&self, factuals: &Factuals) -> f64 {
        let Some(craft) = factuals.get_craft(self.craft) else {
            return 1.0;
        };
        let cost = self.complexity_cost(craft, factuals.config.pop.craft_distance);
        if cost.is_finite() && cost > 0.0 {
            cost
        } else {
            1.0
        }
    }

    /// # Plan Complexity Cost
    ///
    /// Complexity cost of the current targets.
    ///
    /// `modifier` is [`Self::complexity_cost`]. A line with a positive
    /// target adds `modifier * complexity * (1 - overlap / 2) * iterations`.
    /// Overlap comes from [`Self::overlap_percent`]. A missing process,
    /// or a target that is not positive, adds nothing.
    ///
    /// TODO: Check the math on this. It will either need to be corrected, planned around, or limited to keep funny values out.
    pub fn plan_complexity_cost(&self, factuals: &Factuals, modifier: f64) -> f64 {
        let mut total = 0.0;
        for (index, line) in self.lines.iter().enumerate() {
            let Some(iterations) = line.target.filter(|n| *n > 0.0) else {
                continue;
            };
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let overlap = self.overlap_percent(index, factuals);
            let weight = process.complexity * (1.0 - overlap / 2.0);
            total += modifier * weight * iterations;
        }
        total
    }

    /// # Overlap Percent
    ///
    /// Fraction of line `index`'s goods that another line also uses.
    ///
    /// Goods are that process's inputs and outputs. A missing process,
    /// or a process with no goods, returns `0`.
    fn overlap_percent(&self, index: usize, factuals: &Factuals) -> f64 {
        let Some(line) = self.lines.get(index) else {
            return 0.0;
        };
        let Some(mine) = process_goods(factuals, line.process) else {
            return 0.0;
        };
        if mine.is_empty() {
            return 0.0;
        }
        let mut others = HashSet::new();
        for (other_index, other) in self.lines.iter().enumerate() {
            if other_index == index {
                continue;
            }
            let Some(goods) = process_goods(factuals, other.process) else {
                continue;
            };
            others.extend(goods);
        }
        let shared = mine.iter().filter(|good| others.contains(good)).count();
        shared as f64 / mine.len() as f64
    }

    /// # Ensure Lines
    ///
    /// Adds a resting line for each process id this job does not already have.
    ///
    /// `processes` is the list to cover, in order. A process already on a line
    /// is left as it is, including its target and optional inputs. A new line
    /// uses `Some(0.0)` and no optional inputs.
    pub fn ensure_lines(&mut self, processes: &[usize]) {
        for process in processes {
            if self.lines.iter().any(|line| line.process == *process) {
                continue;
            }
            self.lines
                .push(JobLine::new(*process, Some(0.0), Vec::new()));
        }
    }

    /// # Reset Day
    ///
    /// Drops the morning's claim book, the shopping list, the input links,
    /// and today's process credit.
    ///
    /// Line targets stay, so last night's plan is what the next reserve runs.
    /// Does not touch pop property. [`crate::game::pop::Pop::reset_day`]
    /// zeros `reserved` on the rows, which releases the claim itself.
    /// The next [`Self::reserve`] writes the input links again.
    pub fn reset_day(&mut self) {
        self.claimed.clear();
        self.shopping.clear();
        self.unlisted.clear();
        self.input_reserve.clear();
        self.input_feeds.clear();
        self.kept.clear();
        self.process_credit.clear();
    }

    /// # Unlisted Of
    ///
    /// Units of `good` left in working stock and off the sell book.
    ///
    /// [`Self::plan_purchases`] writes the tender a paying line will spend.
    /// A good that was not kept is `0`.
    pub(crate) fn unlisted_of(&self, good: usize) -> f64 {
        self.unlisted.get(&good).copied().unwrap_or(0.0).max(0.0)
    }

    /// # Shopping Of
    ///
    /// Units of `good` still on the purchase list.
    ///
    /// A good that is not shopped is `0`.
    pub(crate) fn shopping_of(&self, good: usize) -> f64 {
        self.shopping.get(&good).copied().unwrap_or(0.0).max(0.0)
    }

    /// # Shopping Keys
    ///
    /// Good ids on the purchase list, lowest id first.
    ///
    /// The list is this job's shopping map. Returns those ids.
    pub(crate) fn shopping_keys(&self) -> Vec<usize> {
        let mut goods: Vec<usize> = self.shopping.keys().copied().collect();
        goods.sort_unstable();
        goods
    }

    /// # Shopping Unfilled
    ///
    /// True when a shopped good still asks for a whole unit.
    ///
    /// The amount is [`whole_units_up`]. A fractional remainder under one
    /// unit is filled.
    pub(crate) fn shopping_unfilled(&self) -> bool {
        self.shopping
            .values()
            .any(|amount| whole_units_up(*amount) >= 1.0)
    }

    /// # Plan Purchases
    ///
    /// Writes each line's run target and the input buy for this market.
    ///
    /// `property` is working stock. `meal` is the units about to leave that
    /// stock for consumption, so they are not leftovers. `factuals` supplies
    /// processes. `history` scores [`trade_score`] and carries yesterday's
    /// mass book. `gaps` is each short desire, earlier first, as goods with
    /// satisfaction per unit and the missing satisfaction. `basic_count` is
    /// how many leading entries are basic.
    ///
    /// A line that is not blocked pays when its trade score is positive.
    /// A line that is not subsistence also pays when an offered output
    /// traded. Subsistence drops that mark when another line keeps it.
    /// The morning stores the lines upstream first, and by the higher trade
    /// score inside a band. Paying lines buy in that order. The first line
    /// sees the spare time and the spare points, and each later line buys
    /// what that line left. When `history` names no currency, the time they take from the other
    /// lines is the part the stock can already finish. When it names one,
    /// that target is the runs the pile can fund, that time is locked, and
    /// the tender those runs take stays in working stock
    /// ([`Self::unlisted_of`]). A currency input the pile is short is bought
    /// with other goods, and those goods stay off the sell book the same way.
    /// When another tender was offered and the purse covers that card, the
    /// purse pays it instead. A positive trade rate buys more of that input
    /// from the purse, for [`profit_pull`] of the runs still under the time
    /// ceiling. A short desire holds time for the line that fills
    /// it best, including a line that pays, before a higher score takes the
    /// rest. A specialty that pays holds only the basic entries. With no
    /// specialty paying, every gap is held. Spare time then raises a line
    /// whose output was still sought. Before that,
    /// [`add_short_book_runs`] adds sought runs on a paying line.
    /// A line that stays at zero for a turn, with no positive
    /// score, no sale, and no reserved run, is dropped. The last line stays.
    /// A paying
    /// line whose stock covers less than its ceiling also holds time for
    /// a line that does not pay and makes a missing input. Each of those
    /// holds comes back when that line is planned. With none
    /// paying, each line that can run takes an equal share of the spare
    /// time and the spare points, on top of the runs already held
    /// ([`loss_shares`]), and buys the inputs that share is short. A
    /// sought bid does not take that share back.
    /// Leftovers, including [`Self::input_hold`],
    /// reduce the buy. A line at or below zero buys nothing. A missing
    /// process is `Some(0)`. Time
    /// is not shopped. Households are not passed, so no founding stake is
    /// reserved. No meal already set aside is passed. Returns the working
    /// units to move into [`Self::input_hold`].
    pub(crate) fn plan_purchases(
        &mut self,
        property: &HashMap<usize, PopPRow>,
        meal: &HashMap<usize, f64>,
        factuals: &Factuals,
        history: &MarketHistory,
        gaps: &[(Vec<(usize, f64)>, f64)],
        basic_count: usize,
    ) -> HashMap<usize, f64> {
        self.plan_purchases_for(
            property,
            meal,
            &HashMap::new(),
            factuals,
            history,
            gaps,
            basic_count,
            0.0,
        )
    }

    /// # Plan Purchases For
    ///
    /// [`Self::plan_purchases`] for a pop of `households`.
    ///
    /// The other arguments are that function's. `saved` is a meal already
    /// outside working stock. `households` sizes the
    /// traveling share. A desire no line can fill keeps extra runs on the
    /// lines that make a recipe for it, enough that the share can start
    /// that recipe. A desire no line can make also adds runs on a line
    /// that can sell, enough that the extra output pays for buying it,
    /// and the inputs those runs spend are held on the lines that make
    /// them. [`add_short_book_runs`] then adds sought runs on a paying
    /// line. A count that does not yield a share adds none. Runs
    /// that supply a paying line's missing inputs are added after the
    /// desire cover, and only in the points that cover leaves. The cover
    /// written on each line is that reservation. When no line pays, that
    /// aim is [`loss_shares`]. A stored target is scaled by today's
    /// households over the count last planned, then moved in
    /// [`step_plan_targets`]. `turn_length` does not change the step.
    /// When that move changes a target, the buy is written again for the
    /// runs left. Returns the
    /// input hold, as [`Self::plan_purchases`] does, and stores the same
    /// map on [`Self::input_reserve`] for the buy ladder.
    pub(crate) fn plan_purchases_for(
        &mut self,
        property: &HashMap<usize, PopPRow>,
        meal: &HashMap<usize, f64>,
        saved: &HashMap<usize, f64>,
        factuals: &Factuals,
        history: &MarketHistory,
        gaps: &[(Vec<(usize, f64)>, f64)],
        basic_count: usize,
        households: f64,
    ) -> HashMap<usize, f64> {
        // Yesterday's targets grow with the pop before the mix step.
        let scale = household_target_scale(self.planned_households, households);
        let previous: Vec<Option<f64>> = self
            .lines
            .iter()
            .map(|line| line.target.map(|target| target * scale))
            .collect();
        self.shopping.clear();
        self.unlisted.clear();
        let mut property_free: HashMap<usize, f64> = HashMap::new();
        for (&good, row) in property {
            let locked = meal.get(&good).copied().unwrap_or(0.0);
            let free = (row.quantity.max(0.0) - locked).max(0.0);
            if free > 0.0 {
                property_free.insert(good, free);
            }
        }
        // The step reads the morning pile. Later draws spend inputs from it.
        let morning_free = property_free.clone();
        let mut held = self.input_hold.clone();
        let mut hold_plan: HashMap<usize, f64> = HashMap::new();
        for (index, line) in self.lines.clone().iter().enumerate() {
            let Some(process) = factuals.get_process(line.process) else {
                self.lines[index].target = Some(0.0);
                self.lines[index].pays = false;
                continue;
            };
            // A missing factor or capital pile cannot run, and it is not bought.
            let blocked = line_blocked(process, &property_free, &held);
            // Positive score pays. A specialty that already sold pays too.
            let profitable = trade_score(process, history) > 0.0;
            let selling = !process.is_subsistence() && is_selling_enough(process, history);
            self.lines[index].pays = !blocked && (profitable || selling);
        }
        yield_subsistence(&mut self.lines, factuals);
        let any_pays = self.lines.iter().any(|line| line.pays);
        // A specialty keeps the day after the basic cover. Otherwise every short desire is held.
        let specialty_pays = self.lines.iter().any(|line| {
            line.pays
                && factuals
                    .get_process(line.process)
                    .is_some_and(|process| !process.is_subsistence())
        });
        let cover = gaps_for_cover(gaps, basic_count, specialty_pays);
        let held_gaps = cover.as_slice();
        let basic_len = basic_count.min(held_gaps.len());
        let (mut basic_runs, mut later_runs) = reserve_basic_runs(
            &self.lines,
            factuals,
            &property_free,
            &held,
            held_gaps,
            basic_len,
        );
        // An earlier meal of this good is part of the later cover.
        add_earlier_meals(
            &self.lines,
            factuals,
            &held_gaps[..basic_len],
            &basic_runs,
            &held_gaps[basic_len..],
            &mut later_runs,
            &property_free,
            meal,
            saved,
        );
        // The cover's material inputs are made the same day.
        add_cover_inputs(
            &self.lines,
            factuals,
            &mut basic_runs,
            &later_runs,
            &held_gaps[..basic_len],
            &property_free,
        );
        let time_free = property_free.get(&TIME).copied().unwrap_or(0.0);
        // The meal's time is set aside before a craft asks for its inputs.
        let basic_time = cap_reservation(&self.lines, factuals, &mut basic_runs, time_free);
        let basic_withhold: f64 = basic_time.iter().copied().sum();
        cap_reservation(
            &self.lines,
            factuals,
            &mut later_runs,
            (time_free - basic_withhold).max(0.0),
        );
        // The desire cover keeps its points. Later targets spend the rest.
        let day_points = scaled_points(complexity_budget(time_free), self.craft_point_scale(factuals));
        // A later desire no subsistence process makes spends the subsistence floor.
        // One that makes a basic good is fed before a later specialty that does not.
        let mut meal_line = vec![false; self.lines.len()];
        for (index, line) in self.lines.iter().enumerate() {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            meal_line[index] = process.outputs.iter().any(|output| {
                output.amount > 0.0
                    && held_gaps[..basic_len]
                        .iter()
                        .any(|(goods, _)| goods.iter().any(|(good, _)| *good == output.good))
            });
        }
        fit_layers(
            &self.lines,
            factuals,
            &mut basic_runs,
            &mut later_runs,
            &meal_line,
            day_points,
        );
        let rates = subsistence_cover(factuals);
        for (index, line) in self.lines.iter_mut().enumerate() {
            let later = later_runs.get(index).copied().unwrap_or(0.0);
            line.cover_cheap = later > 0.0
                && factuals.get_process(line.process).is_some_and(|process| {
                    !process.is_subsistence()
                        && cover_complexity(process, &rates) >= complexity_spent(process)
                });
        }
        let mut reserved = vec![0.0; self.lines.len()];
        for index in 0..reserved.len() {
            reserved[index] = basic_runs.get(index).copied().unwrap_or(0.0)
                + later_runs.get(index).copied().unwrap_or(0.0);
        }
        // A paying line's missing inputs take the points this cover leaves.
        let cover_hold = reserved.clone();
        if any_pays {
            let fitted = reservation_time(&self.lines, factuals, &reserved);
            let time_left = (time_free - fitted.iter().copied().sum::<f64>()).max(0.0);
            let mut extra = reserve_input_runs(
                &self.lines,
                factuals,
                &property_free,
                &held,
                history,
                time_left,
            );
            if day_points.is_finite() {
                let spare = (day_points - cover_points(&self.lines, factuals, &reserved)).max(0.0);
                let cost = cover_points(&self.lines, factuals, &extra);
                if cost > spare && cost > 0.0 {
                    let scale = spare / cost;
                    for runs in &mut extra {
                        *runs *= scale;
                    }
                }
            }
            for (slot, runs) in reserved.iter_mut().zip(extra) {
                *slot += runs;
            }
        }
        // An unmade desire keeps input runs so the traveling share can start its recipe.
        let before_stake = reserved.clone();
        add_stake_runs(
            &self.lines,
            factuals,
            &mut reserved,
            gaps,
            households,
            day_points,
        );
        // A desire no line can make adds sale runs, and the inputs those runs spend.
        let mut floor = basic_runs.clone();
        floor.resize(self.lines.len(), 0.0);
        let mut meal_keep = cover_hold;
        meal_keep.resize(self.lines.len(), 0.0);
        let mut stake_added = vec![0.0; self.lines.len()];
        for (index, (after, before)) in reserved.iter().zip(&before_stake).enumerate() {
            let stake = (after - before).max(0.0);
            stake_added[index] = stake;
            floor[index] += stake;
            meal_keep[index] += stake;
        }
        let mut sale_extra = sale_runs_for_unmade(
            &self.lines,
            factuals,
            history,
            gaps,
            &mut reserved,
            &floor,
            &meal_keep,
            &property_free,
            &held,
            day_points,
        );
        let meal_points = cover_points(&self.lines, factuals, &reserved);
        let booked = sale_point_cost(&self.lines, factuals, &sale_extra);
        let spare_points = if day_points.is_finite() {
            (day_points - meal_points - booked).max(0.0)
        } else {
            f64::INFINITY
        };
        // Sought sale runs, in the points the meal and the unmade sale left.
        add_short_book_runs(
            &self.lines,
            factuals,
            history,
            &mut sale_extra,
            spare_points,
        );
        let sale_points = sale_point_cost(&self.lines, factuals, &sale_extra);
        let mut extra_points = if day_points.is_finite() {
            (day_points - meal_points - sale_points).max(0.0)
        } else {
            f64::INFINITY
        };
        let mut line_time = reservation_time(&self.lines, factuals, &reserved);
        for (index, &runs) in sale_extra.iter().enumerate() {
            if runs <= 0.0 {
                continue;
            }
            let Some(process) = factuals.get_process(self.lines[index].process) else {
                continue;
            };
            line_time[index] += time_spent(process) * runs;
        }
        let withhold: f64 = line_time.iter().copied().sum();
        if withhold > 0.0 {
            if let Some(time) = property_free.get_mut(&TIME) {
                *time = (*time - withhold).max(0.0);
            }
        }
        // Every line is losing. Each open line gets an equal share of what the meal left.
        let loss_share = if any_pays {
            Vec::new()
        } else {
            loss_shares(
                &self.lines,
                factuals,
                &property_free,
                &held,
                &reserved,
                extra_points,
            )
        };
        // Aims buy in list order. The walk below reserves that buy.
        let order = priority_order(&self.lines, factuals, history);
        let aims = plan_aims(
            &self.lines,
            factuals,
            history,
            &property_free,
            &held,
            &reserved,
            &sale_extra,
            &loss_share,
            &line_time,
            any_pays,
            extra_points,
        );
        extra_points = aims.points_left;
        for &index in &order {
            let Some(process) = factuals.get_process(self.lines[index].process) else {
                continue;
            };
            // This line's reserved time comes back, including when the line pays.
            if line_time[index] > 0.0 {
                *property_free.entry(TIME).or_insert(0.0) += line_time[index];
            }
            let blocked = line_blocked(process, &property_free, &held);
            let pays = self.lines[index].pays && !blocked;
            let currencies = history.currencies();
            let priced = pays && !currencies.is_empty();
            let covered = covered_runs(process, factuals, &property_free, &held);
            let ceiling = time_ceiling(process, &property_free, &held).unwrap_or(covered);
            let ceiling = if ceiling.is_finite() { ceiling.max(0.0) } else { 0.0 };
            let hand = pile_hand(&property_free, &held);
            let purse = trade_rate(process, history).is_some_and(|rate| rate > 0.0);
            // The extra tender is the gap under the time still in this pile.
            let funded = if priced {
                pushed_runs(process, ceiling, &hand, factuals, history, currencies)
            } else {
                0.0
            };
            let stretch = !any_pays && !blocked;
            let held_runs = reserved[index].max(0.0);
            let shared = stretch
                && loss_share.get(index).copied().unwrap_or(0.0) > held_runs + 1e-9;
            let target = aims.target.get(index).copied().unwrap_or(0.0);
            let time_runs = aims.time_runs.get(index).copied().unwrap_or(0.0);
            self.lines[index].target = Some(target);
            // The reservation is the cover. Runs past it are surplus.
            self.lines[index].cover = reserved[index].max(0.0).min(target);
            if blocked {
                self.lines[index].pays = false;
            }
            // A losing job still buys the inputs its share is short. The purse funds what it can.
            let shop_runs = if priced {
                target
            } else if shared && !currencies.is_empty() {
                funded_runs(
                    process,
                    target,
                    &hand,
                    factuals,
                    history,
                    currencies,
                    false,
                )
            } else {
                0.0
            };
            let bill = if shop_runs > 0.0 && !currencies.is_empty() && (priced || shared) {
                bill_runs(
                    process,
                    shop_runs,
                    &hand,
                    factuals,
                    history,
                    currencies,
                    purse && priced,
                )
            } else {
                None
            };
            self.assign_line(
                process,
                factuals,
                history,
                target,
                time_runs,
                pays || shared,
                &mut property_free,
                &mut held,
                &mut hold_plan,
            );
            if let Some(bill) = bill {
                note_funded_bill(
                    &mut self.shopping,
                    &mut self.unlisted,
                    &mut property_free,
                    &mut held,
                    currencies,
                    bill,
                );
            }
            if priced {
                keep_extra_tender(
                    process,
                    ceiling,
                    funded,
                    history,
                    &mut property_free,
                    &held,
                    &mut self.unlisted,
                );
            }
        }
        // A sought output takes spare time, up to the runs already planned.
        // A losing job keeps the share. Giving it back would let one bid take the day.
        if any_pays {
            fund_short_lines(
                &mut self.lines,
                factuals,
                history,
                held_gaps,
                &reserved,
                &mut property_free,
                &mut extra_points,
            );
        }
        // A stored target steps. A moved target shops the runs it landed on.
        if households.is_finite() && households > 0.0 {
            self.planned_households = households;
        }
        let hold_plan = if step_plan_targets(
            &mut self.lines,
            &previous,
            &reserved,
            factuals,
            history,
            &morning_free,
        ) {
            self.assign_stored_targets(&order, property, meal, factuals, history)
        } else {
            hold_plan
        };
        // The ladder stashes this when it reaches production, not before the book.
        self.input_reserve.clone_from(&hold_plan);
        // The stake sits on top of the reserved pile. A cover below the meal keeps none of it.
        for (index, line) in self.lines.iter_mut().enumerate() {
            let added = stake_added.get(index).copied().unwrap_or(0.0);
            let still = reserved.get(index).copied().unwrap_or(0.0).max(0.0);
            let top = added.min(still);
            let non_stake = (still - top).max(0.0);
            line.stake_runs = (line.cover - non_stake).max(0.0).min(top);
        }
        // A line at zero for a turn, and neither paying nor feeding, leaves.
        shed_idle_line(&mut self.lines, factuals, history, &reserved);
        // Upstream first, then the higher score. The iterations stay on the line.
        order_job_lines(&mut self.lines, factuals, history);
        hold_plan
    }

    /// # Craft Input Keep
    ///
    /// Units of each input a paying line can already finish.
    ///
    /// `property` is working stock after produce. `factuals` names skills
    /// and processes. Time is left in that stock. The runs are the tightest
    /// of the other destroyed inputs, and they stop at the line's target.
    /// A line that does not pay is skipped. Earlier lines in the job take
    /// the stock first. Returns the units to move into [`Self::input_hold`].
    pub(crate) fn craft_input_keep(
        &self,
        property: &HashMap<usize, PopPRow>,
        factuals: &Factuals,
    ) -> HashMap<usize, f64> {
        let mut free: HashMap<usize, f64> = HashMap::new();
        for (&good, row) in property {
            let have = row.available().max(0.0);
            if have > 0.0 {
                free.insert(good, have);
            }
        }
        let mut keep: HashMap<usize, f64> = HashMap::new();
        for line in &self.lines {
            if !line.pays {
                continue;
            }
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let mut runs = f64::INFINITY;
            let mut inputs: Vec<(usize, f64)> = Vec::new();
            for input in process.requirements() {
                if input.good == TIME
                    || input.amount <= 0.0
                    || !matches!(input.input_type, InputType::Destroyed | InputType::Consumed)
                    || factuals
                        .get_good(input.good)
                        .is_some_and(|good| good.is_skill())
                {
                    continue;
                }
                let have = free.get(&input.good).copied().unwrap_or(0.0);
                runs = runs.min(have / input.amount);
                inputs.push((input.good, input.amount));
            }
            if inputs.is_empty() || !runs.is_finite() || runs <= 0.0 {
                continue;
            }
            if let Some(target) = line.target {
                runs = runs.min(target.max(0.0));
            }
            if runs <= 0.0 {
                continue;
            }
            for (good, amount) in inputs {
                let take = amount * runs;
                *keep.entry(good).or_insert(0.0) += take;
                if let Some(left) = free.get_mut(&good) {
                    *left = (*left - take).max(0.0);
                }
            }
        }
        keep
    }

    /// # Fit Shopping To Purse
    ///
    /// Cuts the purchase list to the runs the working pile can fund.
    ///
    /// `on_hand` is working stock plus input hold. `factuals` supplies
    /// processes. `history` prices the buy. `currencies` are kept as inputs
    /// and pay for the rest. A currency the pile is short is bought with
    /// other goods, and those goods leave the pile. When another tender was
    /// offered and the purse covers that card, the purse pays it instead.
    /// A positive trade rate may pay other tender for that input.
    /// Paying lines buy from that pile in [`priority_order`].
    /// When no line pays, every line walks that same purse. A list the
    /// purse cannot start is cleared. Nothing is returned.
    pub(crate) fn fit_shopping_to_purse(
        &mut self,
        on_hand: &HashMap<usize, f64>,
        factuals: &Factuals,
        history: &MarketHistory,
        currencies: &[usize],
    ) {
        let mut hand = on_hand.clone();
        let mut shopping = HashMap::new();
        let lines = self.lines.clone();
        // A job whose every line lost still funds the targets just written.
        let stretch = lines.iter().all(|line| !line.pays);
        for index in priority_order(&lines, factuals, history) {
            if !lines[index].pays && !stretch {
                continue;
            }
            let Some(process) = factuals.get_process(lines[index].process) else {
                continue;
            };
            let target = lines[index].target.unwrap_or(0.0).max(0.0);
            let purse = trade_rate(process, history).is_some_and(|rate| rate > 0.0);
            let runs = funded_runs(
                process,
                target,
                &hand,
                factuals,
                history,
                currencies,
                purse,
            );
            let Some(bill) = bill_runs(
                process,
                runs,
                &hand,
                factuals,
                history,
                currencies,
                purse,
            ) else {
                continue;
            };
            for (good, qty) in bill.take {
                let left = hand.entry(good).or_insert(0.0);
                *left = (*left - qty).max(0.0);
            }
            for (good, qty) in bill.barter {
                if qty > 0.0 {
                    let left = hand.entry(good).or_insert(0.0);
                    *left = (*left - qty).max(0.0);
                }
            }
            for (good, qty) in bill.buys {
                if qty > 0.0 {
                    *shopping.entry(good).or_insert(0.0) += qty;
                }
            }
        }
        self.shopping = shopping;
    }

    /// # Assign Stored Targets
    ///
    /// Writes the purchase list for the targets already on the lines.
    ///
    /// `order` is the plan walk. `property` is working stock. `meal` is the
    /// units about to leave that stock. `factuals` supplies processes.
    /// `history` prices a funded bill.
    ///
    /// The shopping list and the unlisted tender are cleared first. Each
    /// line then reserves the inputs of its stored target, and a paying
    /// line buys what that target is short. When no line pays, a line that
    /// can run buys the inputs its target is short, and the purse funds
    /// what it can. A blocked line stops paying.
    /// Returns the working units to move into [`Self::input_hold`].
    fn assign_stored_targets(
        &mut self,
        order: &[usize],
        property: &HashMap<usize, PopPRow>,
        meal: &HashMap<usize, f64>,
        factuals: &Factuals,
        history: &MarketHistory,
    ) -> HashMap<usize, f64> {
        self.shopping.clear();
        self.unlisted.clear();
        let mut property_free: HashMap<usize, f64> = HashMap::new();
        for (&good, row) in property {
            let locked = meal.get(&good).copied().unwrap_or(0.0);
            let free = (row.quantity.max(0.0) - locked).max(0.0);
            if free > 0.0 {
                property_free.insert(good, free);
            }
        }
        let mut held = self.input_hold.clone();
        let mut hold_plan: HashMap<usize, f64> = HashMap::new();
        let stretch_job = self.lines.iter().all(|line| !line.pays);
        for &index in order {
            let Some(process) = factuals.get_process(self.lines[index].process) else {
                continue;
            };
            let blocked = line_blocked(process, &property_free, &held);
            let pays = self.lines[index].pays && !blocked;
            let stretch = stretch_job && !blocked;
            let target = self.lines[index].target.unwrap_or(0.0).max(0.0);
            let target = if target.is_finite() { target } else { 0.0 };
            let covered = covered_runs(process, factuals, &property_free, &held);
            let currencies = history.currencies();
            let priced = pays && !currencies.is_empty();
            let ceiling = time_ceiling(process, &property_free, &held).unwrap_or(covered);
            let ceiling = if ceiling.is_finite() { ceiling.max(0.0) } else { 0.0 };
            let hand = pile_hand(&property_free, &held);
            let purse = trade_rate(process, history).is_some_and(|rate| rate > 0.0);
            let funded = if priced {
                pushed_runs(process, ceiling, &hand, factuals, history, currencies)
            } else {
                0.0
            };
            let time_runs = if priced {
                target
            } else if pays {
                covered.min(target)
            } else {
                target
            };
            if blocked {
                self.lines[index].pays = false;
            }
            // A losing job still buys the inputs its share is short. The purse funds what it can.
            let shop_runs = if priced {
                target
            } else if stretch && !currencies.is_empty() {
                funded_runs(
                    process,
                    target,
                    &hand,
                    factuals,
                    history,
                    currencies,
                    false,
                )
            } else {
                0.0
            };
            let bill = if shop_runs > 0.0 && !currencies.is_empty() && (priced || stretch) {
                bill_runs(
                    process,
                    shop_runs,
                    &hand,
                    factuals,
                    history,
                    currencies,
                    purse && priced,
                )
            } else {
                None
            };
            self.assign_line(
                process,
                factuals,
                history,
                target,
                time_runs,
                pays || stretch,
                &mut property_free,
                &mut held,
                &mut hold_plan,
            );
            if let Some(bill) = bill {
                note_funded_bill(
                    &mut self.shopping,
                    &mut self.unlisted,
                    &mut property_free,
                    &mut held,
                    currencies,
                    bill,
                );
            }
            if priced {
                keep_extra_tender(
                    process,
                    ceiling,
                    funded,
                    history,
                    &mut property_free,
                    &held,
                    &mut self.unlisted,
                );
            }
        }
        hold_plan
    }

    /// # Assign Line
    ///
    /// Reserves one line's inputs and records the buy for what is short.
    ///
    /// `target` is the runs just written. `time_runs` is the part of that
    /// target the stock can already finish, and the time those runs take.
    /// `history` names currencies. `profitable` shops the gap. An
    /// unprofitable line only reserves stock it will use. Time stays in
    /// working property. When the line pays and `history` names a currency,
    /// that tender is left in the planning piles for the caller. A skill is
    /// left out. Capital is reserved once.
    fn assign_line(
        &mut self,
        process: &Process,
        factuals: &Factuals,
        history: &MarketHistory,
        target: f64,
        time_runs: f64,
        profitable: bool,
        property_free: &mut HashMap<usize, f64>,
        held: &mut HashMap<usize, f64>,
        hold_plan: &mut HashMap<usize, f64>,
    ) {
        if target <= 0.0 {
            return;
        }
        for input in process.requirements() {
            if factuals
                .get_good(input.good)
                .is_some_and(|good| good.is_skill())
            {
                continue;
            }
            let need = match input.input_type {
                InputType::Destroyed | InputType::Consumed => {
                    let runs = if input.good == TIME { time_runs } else { target };
                    input.amount * runs
                }
                InputType::Capital => input.amount,
                InputType::Factor => continue,
            };
            if !need.is_finite() || need <= 0.0 {
                continue;
            }
            let from_hold = need.min(held.get(&input.good).copied().unwrap_or(0.0));
            let property_have = property_free.get(&input.good).copied().unwrap_or(0.0);
            let from_property = (need - from_hold).min(property_have).max(0.0);
            // A paying line's tender stays in working stock. The plan records it.
            let leave_tender = profitable
                && input.good != TIME
                && history.currencies().contains(&input.good);
            if leave_tender {
                continue;
            }
            if input.good != TIME {
                if from_property > 0.0 {
                    *hold_plan.entry(input.good).or_insert(0.0) += from_property;
                }
                // The funded bill writes the buy when a tender is on the card.
                if profitable && history.currencies().is_empty() {
                    let buy = (need - from_hold - from_property).max(0.0);
                    if buy > 0.0 {
                        *self.shopping.entry(input.good).or_insert(0.0) += buy;
                    }
                }
            }
            if from_hold > 0.0 {
                let left = held.get_mut(&input.good).expect("hold vanished");
                *left = (*left - from_hold).max(0.0);
            }
            if from_property > 0.0 {
                let left = property_free
                    .get_mut(&input.good)
                    .expect("working stock vanished");
                *left = (*left - from_property).max(0.0);
            }
        }
    }

    /// # Process Credit
    ///
    /// Destroyed inputs today and the output units credited to each.
    pub(crate) fn process_credit(&self) -> &HashMap<usize, ProcessCredit> {
        &self.process_credit
    }

    /// # Note Sale
    ///
    /// Arms the next [`Self::plan`] with the sale pile and what sold.
    ///
    /// `pile` is units left for sale after the meal and the saved day.
    /// `sold` is units of those goods this pop sold today. Both are keyed
    /// by good. A listing stored earlier is cleared, so [`Self::note_listed`]
    /// comes after this. The plan reads them once. Nothing is returned.
    pub fn note_sale(&mut self, pile: &HashMap<usize, f64>, sold: &HashMap<usize, f64>) {
        self.sale_pile.clone_from(pile);
        self.sale_sold.clone_from(sold);
        self.sale_listed.clear();
        self.sale_on = true;
    }

    /// # Note Listed
    ///
    /// Stores the units of `good` posted for sale today.
    ///
    /// [`Self::note_sale`] clears the map, so this follows that call.
    /// [`sale_runs`] treats a stored amount as the offer that can sell out.
    /// A good left out is offered as the pile. Nothing is returned.
    pub fn note_listed(&mut self, good: usize, units: f64) {
        self.sale_listed.insert(good, units.max(0.0));
    }

    /// # Has Craft
    ///
    /// True when this job's craft is `craft`.
    ///
    /// A `craft` of `0` does not match.
    pub fn has_craft(&self, craft: usize) -> bool {
        craft != 0 && self.craft == craft
    }

    /// # Is Same Craft
    ///
    /// True when both jobs share a craft.
    ///
    /// Two empty jobs are not the same craft. Craft `0` never matches.
    pub fn is_same_craft(&self, other: &Self) -> bool {
        self.has_craft(other.craft)
    }

    /// # Plan
    ///
    /// Sets each line's target for the next morning.
    ///
    /// `cover` is the units of each good still short. `property` is the
    /// pop's rows. `factuals` supplies processes. `history` is yesterday's
    /// market board. `modifier` is [`Self::complexity_cost`].
    ///
    /// The line that makes the most of a short good per unit of time takes
    /// that good. Its cover iterations are the largest
    /// `(gap + 1) / output.amount` across the short outputs it takes. The
    /// extra unit is one more of that output than the gap. A missing
    /// process is no cover. Inputs a cover line consumes, when this job
    /// also makes them, are added to the cover and assigned the same way.
    /// [`release_stocked_buys`] takes a bought input back off once those
    /// units are in `quantity`, and takes a worse meal off once the mass
    /// book has supplied it.
    /// [`cap_to_morning`] stops those runs at what this morning can finish,
    /// including output an earlier line will make. That cover is a floor
    /// under the line.
    ///
    /// Every line with a positive [`trade_score`] adds its own surplus on
    /// that floor. The extra iterations start from [`surplus_iterations`]
    /// on the inputs the cover leaves, after earlier lines have taken
    /// theirs, then [`paced_surplus`] pulls that run toward the output
    /// just made. Unmet buys of an output, plus units downstream production
    /// used, above stock on hand, add more runs through [`sought_runs`].
    /// A line that does not pay still takes that order when it is the best
    /// yield known, and [`release_surplus_for_sought`] gives back the surplus
    /// inputs those runs need. [`cap_sought`] stops that extra at the inputs
    /// still left. [`nudge_surplus`] then shifts surplus toward the line
    /// [`best_paying_line`] picks. [`press_profit`] keeps that line from
    /// being planned at nothing. [`trim_morning_extra`] cuts the extra
    /// back when the total would pass what those lines still finish. When
    /// [`Self::note_sale`] armed this plan, [`pace_sale`] then replaces
    /// the surplus, and [`march_target`] steps toward that sale. The cover
    /// is not cut. A sale does not rewrite the line in one night.
    /// The ideal is the cover plus
    /// the surplus and those runs. A line
    /// that does not pay aims at the cover and those runs. A snap still aims there: the new cover is written in whole when
    /// the step has not reached it, and the old target is not added on top.
    /// Once the cover and those runs are gone the line stands down, including
    /// one that is already running.
    ///
    /// [`march_target`] moves the stored target one
    /// step toward its aim. [`probe_one_run`] then gives the best maker
    /// one run when that target is under one, the offer is empty, and bids
    /// are waiting, and plans the inputs of that run. [`fit_to_morning`]
    /// then lowers a target the next morning cannot finish.
    /// [`march_unmet_sought`] then takes one step toward market runs the
    /// input cap removed, when the sale left the march. A snap writes a cover above the current target
    /// in whole, and does not add the old target to it. A covering line
    /// returns to the order it was added, ahead of a line that only pays or
    /// only feeds one of those lines. A line that is not covering, and does
    /// not feed one that is, moves to the back. A line that makes an input
    /// runs before the line that consumes it. The
    /// line records that score for [`Self::shop_unpaid_outputs`].
    /// `snap_cover` is a population change or a new or removed desire. A
    /// paying line that already has a stored cover writes the new cover
    /// whole and steps the surplus above that old cover. Sought runs then
    /// step from that whole line. A paying line whose stored cover is
    /// still 0 steps its current target toward the new cover plus the extra.
    ///
    /// An empty line list stores [`Self::plan_cost`] as `0` and returns.
    /// Otherwise [`Self::plan_cost`] is the complexity cost of the targets
    /// just written. Each line's optional inputs are then rewritten by
    /// [`chosen_inputs`].
    pub fn plan(
        &mut self,
        cover: &HashMap<usize, f64>,
        property: &HashMap<usize, PopPRow>,
        factuals: &Factuals,
        history: &MarketHistory,
        modifier: f64,
        snap_cover: bool,
    ) {
        // The note applies to this plan only.
        let sale_on = std::mem::replace(&mut self.sale_on, false);
        if self.lines.is_empty() {
            self.plan_cost = 0.0;
            return;
        }

        // Cover. A worse meal the book can supply comes off. The rest stays.
        // Made inputs are added, a bought input already on hand comes back
        // off, and the runs stop at what this morning can finish.
        let mut cover_units = cover.clone();
        let mut cover_targets = assign_cover(&self.lines, &cover_units, factuals);
        add_input_needs(&self.lines, &cover_targets, &mut cover_units, factuals);
        release_stocked_buys(
            &self.lines,
            cover,
            &mut cover_units,
            property,
            factuals,
            history,
        );
        cover_targets = assign_cover(&self.lines, &cover_units, factuals);
        let stock = stock_on_hand(property);
        cap_to_morning(&self.lines, &mut cover_targets, &stock, factuals, history);

        // Inputs left for a surplus. Start from that stock, add the cover's
        // morning output, then subtract each cover's required inputs. What
        // remains can fund a surplus.
        let mut remaining = stock.clone();
        add_morning_output(&self.lines, &cover_targets, &mut remaining, factuals);
        for (line, &iterations) in self.lines.iter().zip(&cover_targets) {
            if iterations <= 0.0 {
                continue;
            }
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            for input in process.requirements() {
                let have = remaining.get(&input.good).copied().unwrap_or(0.0);
                let left = (have - input.amount * iterations).max(0.0);
                if left > 0.0 {
                    remaining.insert(input.good, left);
                } else {
                    remaining.remove(&input.good);
                }
            }
        }

        // Working surplus. Walk the lines in order. A positive score takes
        // the paced extra from what remains, and those inputs leave the pool.
        // A score at or below zero, or a missing process, takes none.
        // [`nudge_surplus`] may move some of that extra afterward.
        let mut scores = Vec::with_capacity(self.lines.len());
        for line in &self.lines {
            let score = factuals
                .get_process(line.process)
                .map(|process| trade_score(process, history));
            scores.push(score);
        }
        let mut extras = vec![0.0; self.lines.len()];
        for (index, line) in self.lines.iter().enumerate() {
            if !scores[index].is_some_and(|score| score > 0.0) {
                continue;
            }
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let raw = surplus_iterations(process, &remaining);
            let extra = paced_surplus(process, property, factuals, raw);
            extras[index] = extra;
            if extra <= 0.0 {
                continue;
            }
            for input in process.requirements() {
                let have = remaining.get(&input.good).copied().unwrap_or(0.0);
                let left = (have - input.amount * extra).max(0.0);
                if left > 0.0 {
                    remaining.insert(input.good, left);
                } else {
                    remaining.remove(&input.good);
                }
            }
        }

        // Unmet buys the best yield can still fill, after stock on hand.
        // A line that does not pay takes its inputs back out of surplus.
        // The extra then stops at what that pool can finish.
        let mut sought_extra =
            sought_runs(&self.lines, &cover_targets, property, factuals, history);
        // The market order before the input cap. A later step uses this.
        let sought_full = sought_extra.clone();
        release_surplus_for_sought(
            &self.lines,
            &scores,
            &mut extras,
            &sought_extra,
            &mut remaining,
            factuals,
        );
        cap_sought(&self.lines, &mut sought_extra, &mut remaining, factuals);
        // Shift surplus toward the stronger line before the morning trim.
        nudge_surplus(
            &self.lines,
            &scores,
            &mut extras,
            &mut remaining,
            property,
            factuals,
        );
        // The strongest line stays in work when a weaker line is holding its inputs.
        press_profit(
            &self.lines,
            &scores,
            &mut cover_targets,
            &mut extras,
            &sought_extra,
            &mut remaining,
            property,
            factuals,
        );
        // The cover stays. Extra runs stop when the morning would no longer finish them.
        trim_morning_extra(
            &self.lines,
            &cover_targets,
            &mut extras,
            &mut sought_extra,
            &stock,
            factuals,
            history,
        );
        // A noted sale replaces that surplus. A sold-out card above cost leaves the march.
        let sale_surplus = if sale_on {
            pace_sale(self, property, factuals, history)
        } else {
            vec![None; self.lines.len()]
        };

        // Step toward the ideal. A paying line aims at cover plus extra.
        // An unpaid line aims at the cover and the sought runs. Once both are gone it stands down.
        let mut previous: Vec<f64> = self
            .lines
            .iter()
            .map(|line| line.target.unwrap_or(0.0))
            .collect();
        for (index, line) in self.lines.iter_mut().enumerate() {
            let new_cover = cover_targets[index];
            let current = line.target.unwrap_or(0.0);
            let pays = scores[index].is_some_and(|score| score > 0.0);
            let ideal = new_cover + extras[index] + sought_extra[index];
            // The meal stays. The sale steps. One night does not jump to the units sold.
            if let Some(surplus) = sale_surplus[index] {
                line.pays = pays;
                if snap_cover {
                    line.cover = new_cover;
                }
                let ideal = (new_cover + surplus).max(0.0);
                line.target = Some(march_target(current, ideal).max(new_cover));
                continue;
            }
            // The next morning's reserve reads this when it shops an output.
            line.pays = pays;
            let target = if snap_cover && pays && line.cover == 0.0 && current > 0.0 {
                // No stored cover yet, so march the current target instead of adding it to the cover.
                march_target(current, ideal)
            } else if snap_cover && pays {
                // Cover is written whole. Surplus above the old cover steps on its own.
                // Sought steps from that whole line, so a full cover does not leave the order on a zero surplus.
                let old_surplus = (current - line.cover).max(0.0);
                let surplus = march_target(old_surplus, extras[index]);
                let base = new_cover + surplus;
                march_target(base, base + sought_extra[index])
            } else if snap_cover && new_cover > current {
                // The new cover replaces the old target. Unmet buys stay in the aim.
                let aim = new_cover + extras[index] + sought_extra[index];
                march_target(current, aim).max(new_cover)
            } else if pays {
                march_target(current, ideal)
            } else {
                // A short cover stays. A line with nothing left to cover stands down.
                let aim = if ideal > 0.0 { ideal } else { 0.0 };
                march_target(current, aim)
            };
            if snap_cover {
                line.cover = new_cover;
            }
            line.target = Some(target);
        }
        // A stood-down best maker with an empty offer and waiting bids takes one run.
        probe_one_run(self, &scores, &mut previous, property, factuals, history);
        // A target the next morning cannot finish is cut. The meal stays.
        fit_to_morning(
            self,
            &previous,
            &cover_targets,
            property,
            factuals,
            history,
            sale_on,
        );
        // Market runs the input cap removed. One step, when the sale left the march.
        march_unmet_sought(
            self,
            &scores,
            &sale_surplus,
            &sought_full,
            &sought_extra,
            &cover_targets,
            property,
            factuals,
            sale_on,
        );

        // Priority. Covering lines return to the order they were added,
        // ahead of a line that only pays or only feeds one of them. Any
        // other line moves to the back. A producer then moves in front of
        // the line that consumes its output.
        let mut front_flag = vec![false; self.lines.len()];
        for index in 0..self.lines.len() {
            let pays = scores[index].is_some_and(|score| score > 0.0);
            front_flag[index] = pays || cover_targets[index] > 0.0;
        }
        loop {
            let mut changed = false;
            for index in 0..self.lines.len() {
                if front_flag[index] {
                    continue;
                }
                let feeds = self.lines.iter().enumerate().any(|(other, line)| {
                    front_flag[other] && line_feeds(&self.lines[index], line, factuals)
                });
                if feeds {
                    front_flag[index] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        let lines = std::mem::take(&mut self.lines);
        let mut covering = Vec::new();
        let mut rest = Vec::new();
        let mut back = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            if cover_targets[index] > 0.0 {
                covering.push(line);
            } else if front_flag[index] {
                rest.push(line);
            } else {
                back.push(line);
            }
        }
        covering.sort_by_key(|line| line.home);
        rest.sort_by_key(|line| line.home);
        back.sort_by_key(|line| line.home);
        let mut next = covering;
        next.append(&mut rest);
        order_producers_first(&mut next, factuals);
        next.append(&mut back);
        self.lines = next;
        let chosen: Vec<Vec<usize>> = (0..self.lines.len())
            .map(|index| chosen_inputs(&self.lines, index, factuals, history))
            .collect();
        for (line, inputs) in self.lines.iter_mut().zip(chosen) {
            line.inputs = inputs;
        }
        // Complexity cost of the targets just written.
        self.plan_cost = self.plan_complexity_cost(factuals, modifier);
    }

    /// # Reserve
    ///
    /// Claims inputs the pop already holds and records a buy for the next run.
    ///
    /// `property` is the pop's rows. Desires have already reserved, so this
    /// takes only [`PopPRow::available`] above the pile from [`Self::leave`].
    /// `factuals` supplies processes.
    ///
    /// A required factor that is missing shops one unit, and that line's
    /// other inputs stay free. An optional factor is claimed when held and
    /// is not shopped. Any other optional, including capital, is claimed and
    /// shopped only when the line lists it. A skill on that list is claimed
    /// and is not shopped. `None` claims what is free and does not
    /// shop an open-ended amount. `Some(0.0)` skips the line.
    /// [`Self::shop_unpaid_outputs`] then shops for lines that did not pay.
    /// [`Self::note_input_feeds`] records which surviving inputs make each output.
    pub fn reserve(&mut self, property: &mut HashMap<usize, PopPRow>, factuals: &Factuals) {
        self.attach_skills(factuals);
        self.note_input_feeds(factuals);
        if self.lines.is_empty() {
            return;
        }

        let lines = self.lines.clone();
        for line in &lines {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let iterations = match line.target {
                Some(n) if n > 0.0 => Some(n),
                None => None,
                Some(_) => continue,
            };
            // Missing a required factor: shop it, and do not lock inputs that cannot run.
            if !self.claim_factors(property, process) {
                continue;
            }
            self.claim_inputs(property, process, line, iterations, factuals);
            self.claim_skills(property, process, line, iterations);
        }
        // One output unit for each line the last plan did not score as paying.
        self.shop_unpaid_outputs(property, factuals);
    }

    /// # Shop Unpaid Outputs
    ///
    /// Records a buy for one unit of each output a line that does not pay makes.
    ///
    /// `property` is the pop's rows. `factuals` supplies processes and decay.
    /// A line [`Job::plan`] has not scored is skipped. A missing process is
    /// skipped. Each output good is shopped once.
    ///
    /// The buy is `1 / durability` minus the units already on hand.
    /// Durability is `1 - decay`, clamped to `0..=1`. A good missing from
    /// `factuals` does not decay. A good that decays completely is not
    /// bought, because nothing would remain. A result at or below zero is
    /// not shopped. A larger amount already on the shopping list for that
    /// good stays.
    fn shop_unpaid_outputs(&mut self, property: &HashMap<usize, PopPRow>, factuals: &Factuals) {
        let mut seen = HashSet::new();
        let mut bids = Vec::new();
        for line in &self.lines {
            if line.pays {
                continue;
            }
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            for output in &process.outputs {
                if output.amount <= 0.0 || !seen.insert(output.good) {
                    continue;
                }
                // A skill output is not shopped. The household floor keeps the minimum.
                if factuals
                    .get_good(output.good)
                    .is_some_and(|good| good.is_skill())
                {
                    continue;
                }
                if !survives_night(factuals, output.good) {
                    continue;
                }
                let decay = factuals
                    .get_good(output.good)
                    .map(|row| row.decay_rate)
                    .unwrap_or(0.0);
                let durability = (1.0 - decay).clamp(0.0, 1.0);
                let raw = 1.0 / durability - held(property, output.good);
                bids.push((output.good, raw));
            }
        }
        for (good, raw) in bids {
            self.shop_at_least(good, raw);
        }
    }

    /// # Note Input Feeds
    ///
    /// Records each destroyed or consumed input that survives the night,
    /// and the output it makes.
    ///
    /// `factuals` supplies processes and decay. A missing process is skipped.
    /// Optional inputs count when the line lists them. A skill stays out.
    /// Factors and capital stay out. A good that decays completely stays
    /// out. The same input and output keep the smaller input-per-output.
    /// The pop reads the record through [`Self::feeds_of`].
    fn note_input_feeds(&mut self, factuals: &Factuals) {
        self.input_feeds.clear();
        for line in &self.lines {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let mut inputs = process.requirements();
            for input in process.optional_inputs() {
                if line.inputs.contains(&input.good) {
                    inputs.push(input);
                }
            }
            for input in inputs {
                if !matches!(input.input_type, InputType::Destroyed | InputType::Consumed) {
                    continue;
                }
                if factuals
                    .get_good(input.good)
                    .is_some_and(|good| good.is_skill())
                {
                    continue;
                }
                if input.amount <= 0.0 || !survives_night(factuals, input.good) {
                    continue;
                }
                for output in &process.outputs {
                    if output.amount <= 0.0 {
                        continue;
                    }
                    let per_output = input.amount / output.amount;
                    if let Some(feed) = self
                        .input_feeds
                        .iter_mut()
                        .find(|feed| feed.input == input.good && feed.output == output.good)
                    {
                        if per_output < feed.per_output {
                            feed.per_output = per_output;
                        }
                    } else {
                        self.input_feeds.push(InputFeed {
                            input: input.good,
                            output: output.good,
                            per_output,
                        });
                    }
                }
            }
        }
    }

    /// # Release Stake Hold
    ///
    /// Moves [`Self::stake_hold`] back into `property`.
    ///
    /// The units are today's make, so they return as fresh stock. Founding
    /// then reads them in quantity. Nothing is returned.
    pub(crate) fn release_stake_hold(&mut self, property: &mut HashMap<usize, PopPRow>) {
        let hold = std::mem::take(&mut self.stake_hold);
        for (good, units) in hold {
            if !units.is_finite() || units <= 0.0 {
                continue;
            }
            let row = property.entry(good).or_insert_with(|| PopPRow::new(0.0));
            row.quantity += units;
            row.fresh += units;
        }
    }

    /// # Park Stake Output
    ///
    /// Moves the founding-stake share of `result` out of `property`.
    ///
    /// `line` carries [`JobLine::stake_runs`] and [`JobLine::cover`].
    /// `factuals` names skills. Runs up through the cover, minus those stake
    /// runs, stay in the pile. Further runs are surplus and stay too. The
    /// stake runs' positive output, other than time and skills, moves into
    /// [`Self::stake_hold`], and the fresh count drops with it. `produced`
    /// stays. Nothing is returned.
    fn park_stake_output(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        factuals: &Factuals,
        line: &JobLine,
        result: &ProcessResult,
    ) {
        let stake = line.stake_runs.max(0.0);
        let ran = result.iterations.max(0.0);
        if stake <= 1e-12 || ran <= 1e-12 {
            return;
        }
        let meal = (line.cover - stake).max(0.0);
        let happened = (ran - meal).max(0.0).min(stake);
        if happened <= 1e-12 {
            return;
        }
        let share = happened / ran;
        for (&good, &change) in &result.changes {
            if good == TIME || change <= 0.0 {
                continue;
            }
            if factuals
                .get_good(good)
                .is_some_and(|row| row.is_skill())
            {
                continue;
            }
            let want = change * share;
            if !want.is_finite() || want <= 0.0 {
                continue;
            }
            let Some(row) = property.get_mut(&good) else {
                continue;
            };
            let take = want.min(row.quantity.max(0.0));
            if take <= 0.0 {
                continue;
            }
            row.quantity -= take;
            row.fresh = (row.fresh - take).max(0.0);
            *self.stake_hold.entry(good).or_insert(0.0) += take;
        }
    }

    /// # Produce
    ///
    /// Runs the job and spends every free input.
    ///
    /// `property` and `factuals` are the same ones [`Self::produce_keeping`]
    /// takes. Nothing is held back for a drink. Returns that function's effects.
    pub fn produce(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        factuals: &Factuals,
    ) -> Vec<ProcessEffect> {
        self.produce_keeping(property, factuals, &HashMap::new())
    }

    /// # Produce Keeping
    ///
    /// Runs each line up to its target and writes the result onto `property`.
    ///
    /// `factuals` names the processes. `leave` is units of each good that
    /// stay unspent. A line may use only the surplus above that, including
    /// output an earlier line just made. Returns the process effects.
    /// Negative changes leave `quantity` and the claim. Positive changes
    /// are new stock, added to `quantity`, `fresh`, and `produced`. Capital
    /// moves from `quantity` into `used`. Factor claims stay reserved so
    /// the factor is not sold. Other leftover claims are released. Shopping
    /// stays for [`Job::buy_orders`].
    ///
    /// Lines run in the order the morning stored, upstream first.
    /// [`feeders_before`] then places a maker the morning left behind
    /// ahead of the line that consumes its output. An unlimited line can spend inputs a later
    /// line also claimed. A planned line stops at its own quota. A line
    /// that spends time leaves enough for a later line in that order which
    /// it feeds, when that line yields more per unit of time. The run
    /// count also stops at the complexity points still free.
    /// [`complexity_budget`] funds them from the time on hand. Cover runs
    /// spend [`cover_rate`]. Runs past the cover spend
    /// [`complexity_spent`]. [`points_held_for_later`] keeps a later cover,
    /// and the rest of a paying target another line feeds. Runs that make
    /// the leave are funded before that hold. The feeder may
    /// spend the points that input is still short.
    pub fn produce_keeping(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        factuals: &Factuals,
        leave: &HashMap<usize, f64>,
    ) -> Vec<ProcessEffect> {
        self.attach_skills(factuals);
        if self.lines.is_empty() {
            return Vec::new();
        }
        let mut effects = Vec::new();
        let lines = self.lines.clone();
        let stored: Vec<usize> = (0..lines.len()).collect();
        let order = feeders_before(&lines, stored, factuals);
        let rates = subsistence_cover(factuals);
        let mut points = scaled_points(
            complexity_budget(time_ready(property, &self.claimed)),
            self.craft_point_scale(factuals),
        );
        for &index in &order {
            let line = &lines[index];
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let mut running = line.clone();
            let held_points = points_held_for_later(&lines, &order, index, factuals, &rates);
            // The consumer's missing inputs may draw on the points kept for it.
            let feed_runs = feeding_runs_for_later(
                &lines,
                &order,
                index,
                factuals,
                property,
                &self.claimed,
                &rates,
                points,
            );
            let feed_cap = match line.target {
                Some(n) if n > 0.0 => feed_runs.min(n.max(0.0)),
                None => feed_runs,
                Some(_) => 0.0,
            };
            let feed_points = runs_point_cost(
                process,
                &rates,
                feed_cap,
                line.cover,
                line.cover_cheap,
            );
            // The drink is made before a later line keeps the day.
            let drink_runs = match line.target {
                Some(n) if n > 0.0 => {
                    runs_for_leave(process, property, &self.claimed, leave).min(n)
                }
                None => runs_for_leave(process, property, &self.claimed, leave),
                Some(_) => 0.0,
            };
            let drink_points = runs_point_cost(
                process,
                &rates,
                drink_runs,
                line.cover,
                line.cover_cheap,
            );
            let open_points = if points.is_finite() {
                let drink_cost = drink_points.min(points.max(0.0));
                let held_after = held_points.min((points - drink_cost).max(0.0));
                let spare = (points - held_after - drink_cost).max(0.0);
                drink_cost + spare + feed_points.min(held_after.max(0.0))
            } else {
                f64::INFINITY
            };
            match line.target {
                Some(n) if n > 0.0 => {
                    let mut capped = n;
                    let pace = time_spent(process);
                    if pace > 0.0 {
                        let hold = time_held_for_later(&lines, &order, index, factuals);
                        let room = (time_ready(property, &self.claimed) - hold).max(0.0);
                        capped = capped.min(room / pace);
                    }
                    // Cover runs spend the subsistence rate. The surplus spends its own.
                    capped = capped.min(runs_in_points(
                        process,
                        &rates,
                        open_points,
                        line.cover,
                        line.cover_cheap,
                    ));
                    if capped <= 0.0 {
                        continue;
                    }
                    running.target = Some(capped);
                }
                None => {
                    let room = complexity_room(process, open_points);
                    if room.is_finite() {
                        if room <= 0.0 {
                            continue;
                        }
                        running.target = Some(room);
                    }
                }
                Some(_) => continue,
            }
            // Pass this line's inputs only, capped at its quota when it has one.
            let inputs = self.inputs_for(process, &running, property, leave);
            if running.target.is_none() && !inputs_bound(process, &inputs) {
                continue;
            }
            let result = process.do_process(&inputs, running.target, factuals);
            // Points follow the runs that finished, not the cap.
            if points.is_finite() {
                let spent = runs_point_cost(
                    process,
                    &rates,
                    result.iterations.max(0.0),
                    line.cover,
                    line.cover_cheap,
                );
                points = (points - spent).max(0.0);
            }
            self.apply_result(property, &result, factuals);
            self.park_stake_output(property, factuals, line, &result);
            effects.extend(result.effects);
        }
        // Factors stay reserved through the market. Everything else can be sold.
        let factors = self.factor_goods(factuals);
        self.release_claims(property, &factors);
        effects
    }

    /// # Settle Shopping
    ///
    /// Rebuilds the shopping list from stock still free after production.
    ///
    /// `property` is the pop's rows after produce and after the desire
    /// reserve on that output. `factuals` supplies decay. Morning
    /// [`Self::reserve`] shopped from stock before this morning's make, so
    /// that list is dropped. Each positive target shops its next run.
    /// Destroyed and consumed inputs count leftover free stock. Capital
    /// counts that free stock plus [`PopPRow::used`], because the pile
    /// returns at decay. Output an earlier line will make tomorrow morning
    /// covers that need first: produce runs in this order, so the make is
    /// on hand before the later line takes it, and it has not decayed yet.
    /// A line with no quota speaks for the free stock and does not shop. A
    /// missing required factor is recorded the same way as
    /// [`Self::claim_factors`], and that line's other inputs are not shopped.
    /// Unpaid outputs are shopped as in reserve.
    pub fn settle_shopping(&mut self, property: &HashMap<usize, PopPRow>, factuals: &Factuals) {
        self.shopping.clear();
        if self.lines.is_empty() {
            return;
        }
        // Stock an earlier line has already counted is not free for the next.
        let mut spoken: HashMap<usize, f64> = HashMap::new();
        // Units earlier lines will make tomorrow, not yet counted by a buyer.
        let mut forthcoming: HashMap<usize, f64> = HashMap::new();
        let lines = self.lines.clone();
        for line in &lines {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let iterations = match line.target {
                Some(n) if n > 0.0 => Some(n),
                None => None,
                Some(_) => continue,
            };
            if self.required_factor_missing(property, process) {
                continue;
            }
            self.settle_inputs(
                property,
                process,
                line,
                iterations,
                factuals,
                &mut spoken,
                &mut forthcoming,
            );
            let Some(n) = iterations else {
                continue;
            };
            for output in &process.outputs {
                if output.amount > 0.0 {
                    *forthcoming.entry(output.good).or_insert(0.0) += output.amount * n;
                }
            }
        }
        self.shop_unpaid_outputs(property, factuals);
    }

    /// # Required Factor Missing
    ///
    /// Records a buy for each required factor that is not on hand.
    ///
    /// `property` is the pop's rows. Returns true when a required factor is
    /// missing, so the caller skips that line's other inputs. A held factor,
    /// and a missing optional factor, do not block the line.
    fn required_factor_missing(
        &mut self,
        property: &HashMap<usize, PopPRow>,
        process: &Process,
    ) -> bool {
        let mut missing = false;
        for factor in process.factors() {
            if held(property, factor.good) > 0.0 || factor.is_optional() {
                continue;
            }
            self.shop_at_least(factor.good, 1.0);
            missing = true;
        }
        missing
    }

    /// # Settle Inputs
    ///
    /// Shops one line's inputs from the stock [`Self::settle_shopping`] can see.
    ///
    /// Required inputs and listed optionals are visited. A skill is left
    /// out. `iterations` `None` only marks the free stock as spoken for.
    /// `spoken` is stock earlier lines in this rebuild have already counted.
    /// `forthcoming` is output those lines will make tomorrow morning, and
    /// this line's share is taken out of it.
    fn settle_inputs(
        &mut self,
        property: &HashMap<usize, PopPRow>,
        process: &Process,
        line: &JobLine,
        iterations: Option<f64>,
        factuals: &Factuals,
        spoken: &mut HashMap<usize, f64>,
        forthcoming: &mut HashMap<usize, f64>,
    ) {
        for input in process.requirements() {
            self.settle_one(property, &input, iterations, factuals, spoken, forthcoming);
        }
        for input in process.optional_inputs() {
            if factuals
                .get_good(input.good)
                .is_some_and(|good| good.is_skill())
            {
                continue;
            }
            if line.inputs.contains(&input.good) {
                self.settle_one(property, &input, iterations, factuals, spoken, forthcoming);
            }
        }
    }

    /// # Settle One
    ///
    /// Records the next-run buy for `input`, or marks its stock spoken for.
    ///
    /// `iterations` `None` does not shop. `Some(n)` needs `input.amount * n`.
    /// Output still in `forthcoming` covers that need before anything is
    /// bought, and that share is removed. The stock counted is what is still
    /// free, not the morning pile before this run. Capital adds
    /// [`PopPRow::used`]. `spoken` keeps a later line from counting the same
    /// units.
    fn settle_one(
        &mut self,
        property: &HashMap<usize, PopPRow>,
        input: &ProcessInput,
        iterations: Option<f64>,
        factuals: &Factuals,
        spoken: &mut HashMap<usize, f64>,
        forthcoming: &mut HashMap<usize, f64>,
    ) {
        let good = input.good;
        let capital = matches!(input.input_type, InputType::Capital);
        let kept = self.kept.get(&good).copied().unwrap_or(0.0).max(0.0);
        let base = (next_stock(property, good, capital) - kept).max(0.0);
        let visible = (base - spoken.get(&good).copied().unwrap_or(0.0)).max(0.0);
        let Some(n) = iterations else {
            if visible > 0.0 {
                *spoken.entry(good).or_insert(0.0) += visible;
            }
            return;
        };
        let need = input.amount * n;
        let made = forthcoming.get(&good).copied().unwrap_or(0.0);
        let use_make = made.min(need);
        if use_make > 0.0 {
            forthcoming.insert(good, made - use_make);
        }
        let uncovered = need - use_make;
        // This pile already survived today's run, so it is not reduced by another take.
        self.add_next_buy(good, uncovered, visible, factuals);
        let Some(durability) = night_durability(factuals, good) else {
            return;
        };
        // Capital is claimed per run and returned. A destroyed input is
        // counted up to the pre-decay units that run will use.
        let counted = if uncovered <= 0.0 {
            0.0
        } else if capital {
            visible.min(uncovered)
        } else {
            visible.min(uncovered / durability)
        };
        if counted > 0.0 {
            *spoken.entry(good).or_insert(0.0) += counted;
        }
    }

    /// # Buy Orders
    ///
    /// One buy per shopped good, lowest good id first.
    ///
    /// The amount is the shopping list rounded up to a whole unit with
    /// [`whole_units_up`]. A non-positive amount is skipped. These are buys:
    /// the job does not offer stock for sale.
    pub fn buy_orders(&self, origin: Actor) -> Vec<MarketOrder> {
        if self.lines.is_empty() {
            return Vec::new();
        }
        let mut goods: Vec<usize> = self.shopping.keys().copied().collect();
        goods.sort_unstable();
        let mut orders = Vec::new();
        for good in goods {
            let amount = whole_units_up(self.shopping[&good]);
            if amount >= 1.0 {
                orders.push(MarketOrder::buy(origin, good, amount));
            }
        }
        orders
    }

    /// # Note Purchase
    ///
    /// Reduces the shopping list for `good` by `amount` just received.
    ///
    /// Stops at zero and drops the entry. A good this job was not shopping
    /// for is ignored. `amount` is the units the buyer received.
    pub fn note_purchase(&mut self, good: usize, amount: f64) {
        if let Some(left) = self.shopping.get_mut(&good) {
            *left = (*left - amount.max(0.0)).max(0.0);
        }
        if self.shopping.get(&good).copied().unwrap_or(0.0) == 0.0 {
            self.shopping.remove(&good);
        }
    }

    /// # Claim Factors
    ///
    /// Claims every free unit of a factor the pop holds.
    ///
    /// A missing required factor shops one unit. A missing optional factor
    /// is skipped. Returns false when a required factor is missing, so the
    /// caller does not reserve inputs the line cannot use today.
    fn claim_factors(&mut self, property: &mut HashMap<usize, PopPRow>, process: &Process) -> bool {
        let mut ready = true;
        for factor in process.factors() {
            if held(property, factor.good) > 0.0 {
                // Any amount covers every iteration. Keep the pile off the market.
                let free = free_of(property, factor.good);
                claim(property, &mut self.claimed, factor.good, free);
            } else if !factor.is_optional() {
                self.shop_at_least(factor.good, 1.0);
                ready = false;
            }
        }
        ready
    }

    /// # Claim Inputs
    ///
    /// Claims required inputs, and optional inputs the line lists.
    ///
    /// `iterations` is `None` when the line has no quota. That claims the
    /// free stock and does not shop. `Some(n)` claims up to `amount * n`
    /// and shops the next run. `factuals` supplies each input's decay. A
    /// skill is left for [`Self::claim_skills`], so listing it does not
    /// shop it. Capital is claimed here only when the line lists it.
    fn claim_inputs(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        process: &Process,
        line: &JobLine,
        iterations: Option<f64>,
        factuals: &Factuals,
    ) {
        for input in process.requirements() {
            self.claim_one(property, &input, iterations, factuals);
        }
        for input in process.optional_inputs() {
            if factuals
                .get_good(input.good)
                .is_some_and(|good| good.is_skill())
            {
                continue;
            }
            if line.inputs.contains(&input.good) {
                self.claim_one(property, &input, iterations, factuals);
            }
        }
    }

    /// # Claim Skills
    ///
    /// Claims skill inputs this line may feed, and does not shop them.
    ///
    /// `property` is the pop's rows. `process` supplies the optional inputs.
    /// `line` is the on/off list. `iterations` is the line's quota. `None`
    /// claims the free stock. `Some(n)` claims up to `amount * n`. A skill
    /// this job does not take, or one the line leaves off, is skipped.
    /// [`Self::claim_inputs`] leaves every skill for this claim.
    fn claim_skills(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        process: &Process,
        line: &JobLine,
        iterations: Option<f64>,
    ) {
        for input in process.optional_inputs() {
            if !self.skills.contains(&input.good) || !line.inputs.contains(&input.good) {
                continue;
            }
            let free = self.free_above_kept(property, input.good);
            let take = match iterations {
                None => free,
                Some(n) => free.min(input.amount * n),
            };
            claim(property, &mut self.claimed, input.good, take);
        }
    }

    /// # Claim One
    ///
    /// Claims `input` for today's run and records a buy for the next one.
    ///
    /// `iterations` `None` takes the free stock only and does not shop.
    /// `Some(n)` needs `input.amount * n` today. The buy is recorded by
    /// [`Self::add_next_buy`]. Stock counted as still on hand is leftover
    /// free stock above the pile [`Self::leave`] is holding back, for a
    /// destroyed or consumed input, or that whole free stock for capital,
    /// which comes back at decay. Fresh units sit in that stock and are not
    /// spared. Today's claim still runs when the good is not bought.
    fn claim_one(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        input: &ProcessInput,
        iterations: Option<f64>,
        factuals: &Factuals,
    ) {
        let good = input.good;
        let free = self.free_above_kept(property, good);
        match iterations {
            None => claim(property, &mut self.claimed, good, free),
            Some(n) => {
                let need = input.amount * n;
                let take = free.min(need);
                claim(property, &mut self.claimed, good, take);
                // Capital comes back at decay. Spent destroyed and consumed stock does not.
                let kept = if matches!(input.input_type, InputType::Capital) {
                    free
                } else {
                    free - take
                };
                self.add_next_buy(good, need, kept, factuals);
            }
        }
    }

    /// # Add Next Buy
    ///
    /// Adds a buy so `need` of `good` is on hand after decay.
    ///
    /// `kept` is stock that will still be there, before decay. The amount is
    /// `need / durability - kept`, where durability comes from
    /// [`night_durability`]. A non-positive amount is not recorded. Two lines
    /// that need the same good add.
    fn add_next_buy(&mut self, good: usize, need: f64, kept: f64, factuals: &Factuals) {
        if need <= 0.0 {
            return;
        }
        let Some(durability) = night_durability(factuals, good) else {
            return;
        };
        let raw = need / durability - kept;
        if raw > 0.0 {
            *self.shopping.entry(good).or_insert(0.0) += raw;
        }
    }

    /// # Shop At Least
    ///
    /// Raises the shopping list for `good` to `amount` when the list is lower.
    ///
    /// A larger amount already recorded stays. A non-positive `amount` does
    /// nothing.
    fn shop_at_least(&mut self, good: usize, amount: f64) {
        if amount <= 0.0 {
            return;
        }
        let slot = self.shopping.entry(good).or_insert(0.0);
        *slot = (*slot).max(amount);
    }

    /// # Inputs For
    ///
    /// The input map for one run of this line.
    ///
    /// Required inputs and listed optionals contribute the claimed amount
    /// plus free stock, capped at this line's quota when it has one. A held
    /// factor is entered so the process sees it; the amount is not spent.
    fn inputs_for(
        &self,
        process: &Process,
        line: &JobLine,
        property: &HashMap<usize, PopPRow>,
        leave: &HashMap<usize, f64>,
    ) -> HashMap<usize, f64> {
        let mut inputs = HashMap::new();
        for input in &process.inputs {
            let is_factor = matches!(input.input_type, InputType::Factor);
            if input.is_optional() && !is_factor && !line.inputs.contains(&input.good) {
                continue;
            }
            if is_factor {
                let have = held(property, input.good);
                if have > 0.0 {
                    inputs.insert(input.good, have);
                }
                continue;
            }
            // Claimed stock plus free stock. Output an earlier line just made
            // is in that free stock. The drink `leave` names stays out of it.
            let claimed_amount = self.claimed.get(&input.good).copied().unwrap_or(0.0);
            let unreserved = free_of(property, input.good);
            let kept = leave.get(&input.good).copied().unwrap_or(0.0).max(0.0);
            let have = (claimed_amount + unreserved - kept).max(0.0);
            let amount = match line.target {
                Some(n) if n > 0.0 => have.min(input.amount * n),
                _ => have,
            };
            if amount > 0.0 {
                inputs.insert(input.good, amount);
            }
        }
        inputs
    }

    /// # Apply Result
    ///
    /// Writes one [`ProcessResult`] onto the pop's rows and this job's claim.
    ///
    /// A negative change is input spent. A positive change is new output,
    /// added to `quantity`, [`PopPRow::fresh`], and [`PopPRow::produced`].
    /// `used_inputs` is capital set aside for the day. When a fed good's
    /// net did not fall, the claim for [`ProcessResult::spent`] is released
    /// without taking that quantity again. `factuals` distinguishes a skill.
    /// Aged skill that was fed comes back fresh with the output, so a pile
    /// that was fully used has nothing left to decay that night.
    /// [`Self::credit_run`] records the destroyed inputs.
    fn apply_result(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        result: &ProcessResult,
        factuals: &Factuals,
    ) {
        // Aged skill fed this run. Captured before quantity moves.
        let mut aged_fed: HashMap<usize, f64> = HashMap::new();
        for (&good, &spent) in &result.spent {
            if spent <= 0.0 {
                continue;
            }
            if !factuals.get_good(good).is_some_and(|row| row.is_skill()) {
                continue;
            }
            let (qty, fresh) = property
                .get(&good)
                .map(|row| (row.quantity, row.fresh))
                .unwrap_or((0.0, 0.0));
            let aged = (qty - fresh.max(0.0)).max(0.0);
            let consumed_aged = spent.min(aged);
            if consumed_aged > 0.0 {
                aged_fed.insert(good, consumed_aged);
            }
        }
        for (&good, &change) in &result.changes {
            if change < 0.0 {
                self.spend(property, good, -change, false);
            } else if change > 0.0 {
                let row = property.entry(good).or_insert_with(|| PopPRow::new(0.0));
                row.quantity += change;
                row.fresh += change;
                row.produced += change;
            }
        }
        for (&good, &used) in &result.used_inputs {
            self.spend(property, good, used, true);
        }
        // Fed units whose net did not fall stay out of the claim. Quantity
        // already includes that net, so this does not take them again.
        for (&good, &spent) in &result.spent {
            if result.changes.get(&good).copied().unwrap_or(0.0) < 0.0 {
                continue;
            }
            if let Some(left) = self.claimed.get_mut(&good) {
                *left = (*left - spent).max(0.0);
            }
            if let Some(row) = property.get_mut(&good) {
                row.reserved = (row.reserved - spent).max(0.0);
            }
        }
        // Used skill is new stock. The aged units that were fed join fresh.
        for (good, aged) in aged_fed {
            if result.changes.get(&good).copied().unwrap_or(0.0) < 0.0 {
                continue;
            }
            if let Some(row) = property.get_mut(&good) {
                row.fresh = (row.fresh + aged).min(row.quantity.max(0.0));
            }
        }
        self.credit_run(result);
    }

    /// # Credit Run
    ///
    /// Adds this result's used-up inputs to today's process credit.
    ///
    /// Each good in [`ProcessResult::spent`] whose net change is negative gains
    /// those units. A good that comes back with a non-negative net was returned,
    /// not used up, and is left out. Every positive change, including a decay
    /// product, is added to each credited input's outputs. A later run of the
    /// same input adds to the same credit. Nothing is returned.
    fn credit_run(&mut self, result: &ProcessResult) {
        let mut outputs: Vec<(usize, f64)> = result
            .changes
            .iter()
            .filter(|(_, change)| **change > 0.0)
            .map(|(&good, &change)| (good, change))
            .collect();
        outputs.sort_unstable_by_key(|(good, _)| *good);
        for (&input, &units) in &result.spent {
            if units <= 0.0 {
                continue;
            }
            // Returned stock is not consumption. A non-negative net stays out.
            if result.changes.get(&input).copied().unwrap_or(0.0) >= 0.0 {
                continue;
            }
            let credit = self.process_credit.entry(input).or_default();
            credit.units += units;
            for &(output, qty) in &outputs {
                *credit.outputs.entry(output).or_insert(0.0) += qty;
            }
        }
    }

    /// # Spend
    ///
    /// Removes `amount` of `good` from quantity.
    ///
    /// Units a desire reserved, and this job did not claim, stay in quantity.
    /// Free stock is taken first, including a pile [`Self::leave`] recorded
    /// and output an earlier line just made. Reserved units the job claimed
    /// cover the rest, and the claim is reduced only for that part. When
    /// `capital` is set, the units taken are added to `used` so decay can
    /// return them. Otherwise the take is added to [`PopPRow::spent`].
    fn spend(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        good: usize,
        amount: f64,
        capital: bool,
    ) {
        if amount <= 0.0 {
            return;
        }
        let Some(row) = property.get_mut(&good) else {
            return;
        };
        // A desire's reserve stays. Free stock, including the saved pile, goes first.
        let claimed = self.claimed.get(&good).copied().unwrap_or(0.0).max(0.0);
        let desire_held = (row.reserved - claimed).max(0.0);
        let can = (row.quantity.max(0.0) - desire_held).max(0.0);
        let take = amount.min(can);
        if take <= 0.0 {
            return;
        }
        let free = (row.quantity - row.reserved).max(0.0);
        let from_reserved = (take - free.min(take)).max(0.0);
        row.quantity -= take;
        row.reserved = (row.reserved - from_reserved).max(0.0);
        if capital {
            row.used += take;
        } else {
            row.spent += take;
        }
        if let Some(left) = self.claimed.get_mut(&good) {
            *left = (*left - from_reserved).max(0.0);
        }
    }

    /// # Free Above Kept
    ///
    /// Free units of `good` reserve may claim.
    ///
    /// [`PopPRow::available`] minus the pile [`Self::leave`] is holding back.
    /// Missing rows and a non-positive result are 0.
    fn free_above_kept(&self, property: &HashMap<usize, PopPRow>, good: usize) -> f64 {
        let kept = self.kept.get(&good).copied().unwrap_or(0.0).max(0.0);
        (free_of(property, good) - kept).max(0.0)
    }

    /// # Factor Goods
    ///
    /// Good ids that any of this job's processes uses as a factor.
    ///
    /// A missing process adds nothing. The set is what produce keeps reserved.
    fn factor_goods(&self, factuals: &Factuals) -> HashSet<usize> {
        let mut goods = HashSet::new();
        for line in &self.lines {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            for factor in process.factors() {
                goods.insert(factor.good);
            }
        }
        goods
    }

    /// # Release Claims
    ///
    /// Returns claimed units to free stock, except goods in `keep`.
    ///
    /// `keep` is the factor set that must stay off the market. Released
    /// goods are removed from the claim. Kept goods stay claimed and reserved.
    fn release_claims(&mut self, property: &mut HashMap<usize, PopPRow>, keep: &HashSet<usize>) {
        let goods: Vec<usize> = self.claimed.keys().copied().collect();
        for good in goods {
            if keep.contains(&good) {
                continue;
            }
            let amount = self.claimed.remove(&good).unwrap_or(0.0);
            if amount <= 0.0 {
                continue;
            }
            if let Some(row) = property.get_mut(&good) {
                row.reserved = (row.reserved - amount).max(0.0);
            }
        }
    }
}

/// # Assign Cover
///
/// Iterations each line needs to make `cover`.
///
/// `lines` is the job, in its current order. `factuals` supplies processes.
/// The line with the highest [`yield_per_time`] for a good takes that good.
/// A tie keeps the earlier line. Iterations are the largest
/// `(gap + 1) / output.amount` among the goods that line took. A missing
/// process is 0. Returns one iteration count per line.
fn assign_cover(lines: &[JobLine], cover: &HashMap<usize, f64>, factuals: &Factuals) -> Vec<f64> {
    let winner = cover_winners(lines, cover, factuals);
    let mut iterations = vec![0.0; lines.len()];
    for (good, index) in winner {
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let Some(output) = process
            .outputs
            .iter()
            .find(|output| output.good == good && output.amount > 0.0)
        else {
            continue;
        };
        let gap = cover.get(&good).copied().unwrap_or(0.0);
        let runs = (gap + 1.0) / output.amount;
        if runs > iterations[index] {
            iterations[index] = runs;
        }
    }
    iterations
}

/// # Cover Winners
///
/// The line index that takes each good in `cover`.
///
/// `lines` is the job. `factuals` supplies processes. The highest
/// [`yield_per_time`] wins. A tie keeps the earlier line. A missing
/// process, a gap at or below zero, and an output of zero are skipped.
fn cover_winners(
    lines: &[JobLine],
    cover: &HashMap<usize, f64>,
    factuals: &Factuals,
) -> HashMap<usize, usize> {
    let mut winner: HashMap<usize, (usize, f64)> = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        for output in &process.outputs {
            if output.amount <= 0.0 {
                continue;
            }
            let gap = cover.get(&output.good).copied().unwrap_or(0.0);
            if gap <= 0.0 {
                continue;
            }
            let pace = yield_per_time(process, output.good);
            let replace = match winner.get(&output.good) {
                None => true,
                Some((_, best)) => pace > *best,
            };
            if replace {
                winner.insert(output.good, (index, pace));
            }
        }
    }
    winner
        .into_iter()
        .map(|(good, (index, _))| (good, index))
        .collect()
}

/// # Buys Cheaper Than Making
///
/// Units to buy of each good that costs more to make than to buy.
///
/// `lines` is the job. `cover` is the gap in units. `meal` is one day of
/// each good this job makes for a desire, before stock. `factuals` supplies
/// processes and which goods survive the night. `history` supplies holding.
/// The line is the one [`assign_cover`] would pick after made inputs are
/// added back onto the gap. [`is_bought_instead`] decides the good. A good
/// that does not survive the night is skipped.
///
/// A line at the best known yield posts the gap, and posts nothing when
/// that gap is gone. A worse line posts `meal` instead, including when the
/// bin already holds that day. An input gap larger than that meal stays.
/// Returns the morning buy.
pub(crate) fn buys_cheaper_than_making(
    lines: &[JobLine],
    cover: &HashMap<usize, f64>,
    meal: &HashMap<usize, f64>,
    factuals: &Factuals,
    history: &MarketHistory,
) -> HashMap<usize, f64> {
    let mut units = cover.clone();
    let runs = assign_cover(lines, &units, factuals);
    add_input_needs(lines, &runs, &mut units, factuals);
    let mut buys = HashMap::new();
    for (good, index) in cover_winners(lines, &units, factuals) {
        if !survives_night(factuals, good) {
            continue;
        }
        let gap = units.get(&good).copied().unwrap_or(0.0);
        if gap <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        if is_bought_instead(lines, index, process, good, factuals, history) {
            buys.insert(good, gap);
        }
    }
    // A worse recipe keeps tomorrow's meal on order after the hole is gone.
    for (good, index) in cover_winners(lines, meal, factuals) {
        if !survives_night(factuals, good) {
            continue;
        }
        let day = meal.get(&good).copied().unwrap_or(0.0);
        if day <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        if yield_per_time(process, good) >= best_known_yield(factuals, good) {
            continue;
        }
        if !is_bought_instead(lines, index, process, good, factuals, history) {
            continue;
        }
        let hole = buys.get(&good).copied().unwrap_or(0.0);
        buys.insert(good, hole.max(day));
    }
    buys
}

/// # Is Bought Instead
///
/// True when the cover of `good` is posted as a buy.
///
/// `lines[index]` is the line that would make it. `process` is that line.
/// `factuals` and `history` price the make and the other lines. The best
/// known yield buys only when [`is_cheaper_to_buy`]. A slower line also
/// buys when [`has_specialty_to_sell`] is true. Returns that choice.
fn is_bought_instead(
    lines: &[JobLine],
    index: usize,
    process: &Process,
    good: usize,
    factuals: &Factuals,
    history: &MarketHistory,
) -> bool {
    if is_cheaper_to_buy(process, good, history) {
        return true;
    }
    yield_per_time(process, good) < best_known_yield(factuals, good)
        && has_specialty_to_sell(lines, index, factuals, history)
}

/// # Has Specialty To Sell
///
/// True when a line other than `except` pays at the best known yield.
///
/// `lines` is the job. `factuals` supplies processes and [`best_known_yield`].
/// `history` prices [`trade_score`]. A line counts when that score is
/// positive and one output yields at least as much per time as any process
/// known for that good. Returns false when no line qualifies.
fn has_specialty_to_sell(
    lines: &[JobLine],
    except: usize,
    factuals: &Factuals,
    history: &MarketHistory,
) -> bool {
    lines.iter().enumerate().any(|(index, line)| {
        if index == except {
            return false;
        }
        let Some(process) = factuals.get_process(line.process) else {
            return false;
        };
        if trade_score(process, history) <= 0.0 {
            return false;
        }
        process.outputs.iter().any(|output| {
            if output.amount <= 0.0 {
                return false;
            }
            let known = best_known_yield(factuals, output.good);
            known > 0.0 && yield_per_time(process, output.good) >= known
        })
    })
}

/// # Is Cheaper To Buy
///
/// True when making `good` with `process` costs more than a positive card.
///
/// `history` prices the required inputs and the card.
/// [`make_cost_per_unit`] is that cost. A holding at or below zero is
/// false: the card is not a purchase. A process that does not make `good`
/// is false.
fn is_cheaper_to_buy(process: &Process, good: usize, history: &MarketHistory) -> bool {
    let holding = history.holding_per_unit(good);
    if holding <= 0.0 {
        return false;
    }
    make_cost_per_unit(process, good, history).is_some_and(|cost| cost > holding)
}

/// # Release Stocked Buys
///
/// Takes a bought input off `cover` once `property` already holds it.
///
/// `lines` is the job. `meal` is the desire gap before made inputs were
/// added, already the shortfall above stock. `cover` is that gap plus
/// those inputs. `factuals` and `history` price [`is_bought_instead`].
/// The winner is [`cover_winners`]. A line at the best known yield keeps
/// the good. A worse line that is bought instead keeps its meal gap until
/// [`MarketHistory::is_mass_supplied`]. Stock offsets only the inputs added
/// on top of that gap, and only once the gap is gone. A good that does not
/// survive the night stays.
fn release_stocked_buys(
    lines: &[JobLine],
    meal: &HashMap<usize, f64>,
    cover: &mut HashMap<usize, f64>,
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
    history: &MarketHistory,
) {
    let winners = cover_winners(lines, cover, factuals);
    for (good, index) in winners {
        if !survives_night(factuals, good) {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let pace = yield_per_time(process, good);
        if pace >= best_known_yield(factuals, good)
            || !is_bought_instead(lines, index, process, good, factuals, history)
        {
            continue;
        }
        let meal_gap = meal.get(&good).copied().unwrap_or(0.0);
        let total = cover.get(&good).copied().unwrap_or(0.0);
        let added = (total - meal_gap).max(0.0);
        // A supplied book drops the meal. Otherwise the short meal stays.
        let base = if history.is_mass_supplied(good) {
            0.0
        } else {
            meal_gap
        };
        // The meal is still short, so none of this stock is free for the input.
        let offset = if base > 0.0 {
            0.0
        } else {
            property
                .get(&good)
                .map(|row| row.quantity.max(0.0))
                .unwrap_or(0.0)
        };
        let kept = base + (added - offset).max(0.0);
        if kept > 0.0 {
            cover.insert(good, kept);
        } else {
            cover.remove(&good);
        }
    }
}

/// # Stock On Hand
///
/// Quantity of each good in `property`.
///
/// A row at or below zero is left out. Returns good id to quantity.
fn stock_on_hand(property: &HashMap<usize, PopPRow>) -> HashMap<usize, f64> {
    property
        .iter()
        .filter(|(_, row)| row.quantity > 0.0)
        .map(|(&good, row)| (good, row.quantity))
        .collect()
}

/// # Runs On Hand
///
/// How many runs of `process` the units in `on_hand` can finish.
///
/// Required inputs limit it to the minimum of `on_hand / amount`. Factors
/// and optional inputs do not. A process with no required input is not
/// limited, so this returns infinity. A missing required input returns 0.
fn runs_on_hand(process: &Process, on_hand: &HashMap<usize, f64>) -> f64 {
    let requirements = process.requirements();
    if requirements.is_empty() {
        return f64::INFINITY;
    }
    let mut iterations = f64::INFINITY;
    for input in &requirements {
        if input.amount <= 0.0 {
            continue;
        }
        let have = on_hand.get(&input.good).copied().unwrap_or(0.0);
        iterations = iterations.min(have / input.amount);
    }
    if iterations.is_finite() {
        iterations.max(0.0)
    } else {
        f64::INFINITY
    }
}

/// # Cap To Morning
///
/// Lowers each line's runs to what this morning can finish.
///
/// `lines` and `factuals` supply the processes. `runs` is one iteration
/// count per line. `stock` is quantity on hand. `history` prices which
/// lines pay, so the order matches the one [`Job::plan`] stores. A missing
/// process becomes 0.
///
/// Stock limits a line. Output an earlier line finishes this morning counts
/// too. That output is what the earlier line still finishes after
/// [`time_held_for_later`] leaves time for a later line.
fn cap_to_morning(
    lines: &[JobLine],
    runs: &mut [f64],
    stock: &HashMap<usize, f64>,
    factuals: &Factuals,
    history: &MarketHistory,
) {
    let wants = runs.to_vec();
    let order = morning_order(lines, &wants, factuals, history);
    let funded = funded_morning(lines, &order, &wants, stock, factuals);
    for (index, amount) in funded.into_iter().enumerate() {
        runs[index] = amount;
    }
}

/// # Morning Order
///
/// Line indexes in the order [`Job::plan`] will store.
///
/// `cover` is one run count per line. `factuals` supplies processes.
/// `history` prices [`trade_score`]. A line with runs, a line that pays,
/// and a line that feeds one of those, come first. Producers move in front
/// of the line that consumes their output. The rest keep added order.
fn morning_order(
    lines: &[JobLine],
    cover: &[f64],
    factuals: &Factuals,
    history: &MarketHistory,
) -> Vec<usize> {
    let mut front_flag = vec![false; lines.len()];
    for index in 0..lines.len() {
        let pays = factuals
            .get_process(lines[index].process)
            .is_some_and(|process| trade_score(process, history) > 0.0);
        let runs = cover.get(index).copied().unwrap_or(0.0);
        front_flag[index] = pays || runs > 0.0;
    }
    loop {
        let mut changed = false;
        for index in 0..lines.len() {
            if front_flag[index] {
                continue;
            }
            let feeds = lines.iter().enumerate().any(|(other, line)| {
                front_flag[other] && line_feeds(&lines[index], line, factuals)
            });
            if feeds {
                front_flag[index] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut covering = Vec::new();
    let mut rest = Vec::new();
    let mut back = Vec::new();
    for index in 0..lines.len() {
        let runs = cover.get(index).copied().unwrap_or(0.0);
        if runs > 0.0 {
            covering.push(index);
        } else if front_flag[index] {
            rest.push(index);
        } else {
            back.push(index);
        }
    }
    covering.sort_by_key(|&index| lines[index].home);
    rest.sort_by_key(|&index| lines[index].home);
    back.sort_by_key(|&index| lines[index].home);
    let mut front = covering;
    front.append(&mut rest);
    let mut front_lines: Vec<JobLine> = front.iter().map(|&index| lines[index].clone()).collect();
    order_producers_first(&mut front_lines, factuals);
    let mut by_home = HashMap::new();
    for &index in &front {
        by_home.insert(lines[index].home, index);
    }
    let mut ordered = Vec::with_capacity(lines.len());
    for line in &front_lines {
        ordered.push(by_home[&line.home]);
    }
    ordered.extend(back);
    ordered
}

/// # Funded Morning
///
/// The most of `wants` this morning can finish, in `order`.
///
/// `lines`, `stock`, and `factuals` are the job, the quantity, and the
/// processes. A pass that repeats at the want, or at a positive run count,
/// is returned. A want that finishes as zero is scaled with the rest.
/// The scale drops until [`finish_morning`] can finish every run.
fn funded_morning(
    lines: &[JobLine],
    order: &[usize],
    wants: &[f64],
    stock: &HashMap<usize, f64>,
    factuals: &Factuals,
) -> Vec<f64> {
    let once = finish_morning(lines, order, wants, stock, factuals);
    let twice = finish_morning(lines, order, &once, stock, factuals);
    // A want the morning cannot start still finishes as zero, and that zero
    // repeats. That is not the morning. Keep a pass only when it repeats at
    // the want, or at a positive run count.
    let settled = morning_close(&once, &twice)
        && (morning_close(&once, wants) || once.iter().any(|runs| *runs > 1e-6));
    if settled {
        return once
            .into_iter()
            .zip(wants)
            .map(|(got, want)| got.min(*want).max(0.0))
            .collect();
    }
    let mut lo = 0.0;
    let mut hi = 1.0;
    for _ in 0..32 {
        let mid = (lo + hi) * 0.5;
        let trial = scale_runs(wants, mid);
        if morning_feasible(lines, order, &trial, stock, factuals) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    scale_runs(wants, lo)
}

/// # Finish Morning
///
/// How many of `targets` each line finishes on `stock`.
///
/// `order` is the morning order. `factuals` supplies processes. Each line
/// sees the full `stock`. Morning output is added for a later line and
/// spent once, in order. Time held for a later line comes out of the time
/// in `stock`. Other lines do not spend that time down. A missing process
/// finishes nothing.
fn finish_morning(
    lines: &[JobLine],
    order: &[usize],
    targets: &[f64],
    stock: &HashMap<usize, f64>,
    factuals: &Factuals,
) -> Vec<f64> {
    let time_on_hand = stock.get(&TIME).copied().unwrap_or(0.0);
    let mut points = complexity_budget(time_on_hand);
    let mut made: HashMap<usize, f64> = HashMap::new();
    let mut finished = vec![0.0; lines.len()];
    for &index in order {
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let want = targets.get(index).copied().unwrap_or(0.0).max(0.0);
        if want <= 0.0 {
            continue;
        }
        let mut cap = want;
        let pace = time_spent(process);
        if pace > 0.0 {
            let hold = held_for_targets(lines, order, index, targets, factuals);
            let room = (time_on_hand - hold).max(0.0);
            cap = cap.min(room / pace);
        }
        // Full stock, plus morning output not yet spent.
        let mut available = stock.clone();
        available.remove(&TIME);
        for (&good, &qty) in &made {
            if qty > 0.0 {
                *available.entry(good).or_insert(0.0) += qty;
            }
        }
        let goods = runs_without_time(process, &available);
        if goods.is_finite() {
            cap = cap.min(goods);
        }
        let held_points = complexity_held_for_targets(lines, order, index, targets, factuals);
        let open_points = if points.is_finite() {
            (points - held_points).max(0.0)
        } else {
            f64::INFINITY
        };
        cap = cap.min(complexity_room(process, open_points));
        cap = cap.max(0.0);
        finished[index] = cap;
        if points.is_finite() {
            points = (points - complexity_spent(process) * cap).max(0.0);
        }
        // Stock stays visible to every line. Only runs above that pile spend
        // the morning output.
        let from_stock = runs_without_time(process, stock);
        let excess = if from_stock.is_finite() {
            (cap - from_stock).max(0.0)
        } else {
            0.0
        };
        for input in process.requirements() {
            if input.good == TIME || input.amount <= 0.0 || excess <= 0.0 {
                continue;
            }
            let have = made.get(&input.good).copied().unwrap_or(0.0);
            let left = (have - input.amount * excess).max(0.0);
            if left > 0.0 {
                made.insert(input.good, left);
            } else {
                made.remove(&input.good);
            }
        }
        for output in &process.outputs {
            if output.amount <= 0.0 || output.good == TIME {
                continue;
            }
            *made.entry(output.good).or_insert(0.0) += output.amount * cap;
        }
    }
    finished
}

/// # Held For Targets
///
/// Time `lines[index]` leaves for a later line, using `targets`.
///
/// `order` is the morning order. `factuals` supplies processes. The lines
/// that count are the ones [`time_held_for_later`] would hold for, and the
/// time is that line's target times its required time.
fn held_for_targets(
    lines: &[JobLine],
    order: &[usize],
    index: usize,
    targets: &[f64],
    factuals: &Factuals,
) -> f64 {
    let Some(pos) = order.iter().position(|&later| later == index) else {
        return 0.0;
    };
    let mut held = 0.0;
    for &later in order.iter().skip(pos + 1) {
        if !holds_time_for(&lines[index], &lines[later], factuals) {
            continue;
        }
        let Some(process) = factuals.get_process(lines[later].process) else {
            continue;
        };
        let n = targets.get(later).copied().unwrap_or(0.0);
        if n > 0.0 {
            held += time_spent(process) * n;
        }
    }
    held
}

/// # Holds Time For
///
/// True when `earlier` must leave time for `later`.
///
/// `factuals` supplies both processes. [`time_held_for_later`] uses this
/// test. A missing process is false.
fn holds_time_for(earlier: &JobLine, later: &JobLine, factuals: &Factuals) -> bool {
    let Some(mine) = factuals.get_process(earlier.process) else {
        return false;
    };
    let Some(theirs) = factuals.get_process(later.process) else {
        return false;
    };
    line_yield(theirs) > line_yield(mine) && line_feeds(earlier, later, factuals)
}

/// # Runs Without Time
///
/// How many runs of `process` the non-time units in `pile` can finish.
///
/// Required inputs other than [`TIME`] limit it to the minimum of
/// `pile / amount`. A process with no such input is not limited, so this
/// returns infinity. A missing input returns 0.
fn runs_without_time(process: &Process, pile: &HashMap<usize, f64>) -> f64 {
    let mut iterations = f64::INFINITY;
    let mut any = false;
    for input in process.requirements() {
        if input.good == TIME || input.amount <= 0.0 {
            continue;
        }
        any = true;
        let have = pile.get(&input.good).copied().unwrap_or(0.0);
        iterations = iterations.min(have / input.amount);
    }
    if !any || !iterations.is_finite() {
        f64::INFINITY
    } else {
        iterations.max(0.0)
    }
}

/// # Morning Close
///
/// True when `left` and `right` are the same run counts.
fn morning_close(left: &[f64], right: &[f64]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(one, other)| (one - other).abs() <= 1e-6)
}

/// # Morning Feasible
///
/// True when every run in `targets` finishes on `stock`.
///
/// `lines`, `order`, and `factuals` are the morning [`finish_morning`] walks.
fn morning_feasible(
    lines: &[JobLine],
    order: &[usize],
    targets: &[f64],
    stock: &HashMap<usize, f64>,
    factuals: &Factuals,
) -> bool {
    let got = finish_morning(lines, order, targets, stock, factuals);
    got.iter()
        .zip(targets)
        .all(|(finished, want)| finished + 1e-6 >= *want)
}

/// # Scale Runs
///
/// `scale` times each run in `runs`. A negative result is 0.
fn scale_runs(runs: &[f64], scale: f64) -> Vec<f64> {
    runs.iter().map(|runs| (runs * scale).max(0.0)).collect()
}

/// # Add Morning Output
///
/// Adds the units `runs` will make onto `remaining`.
///
/// `lines` and `factuals` supply the processes. Time is left out. A missing
/// process adds nothing.
fn add_morning_output(
    lines: &[JobLine],
    runs: &[f64],
    remaining: &mut HashMap<usize, f64>,
    factuals: &Factuals,
) {
    for (line, &n) in lines.iter().zip(runs) {
        if n <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        for output in &process.outputs {
            if output.amount <= 0.0 || output.good == TIME {
                continue;
            }
            *remaining.entry(output.good).or_insert(0.0) += output.amount * n;
        }
    }
}

/// # Trim Morning Extra
///
/// Cuts surplus and sought runs back until the morning can still finish them.
///
/// `cover` stays. `extras` and `sought` are scaled by the same amount.
/// `lines`, `stock`, and `factuals` are the morning. `history` prices the
/// order. A total that already finishes is left as it is.
fn trim_morning_extra(
    lines: &[JobLine],
    cover: &[f64],
    extras: &mut [f64],
    sought: &mut [f64],
    stock: &HashMap<usize, f64>,
    factuals: &Factuals,
    history: &MarketHistory,
) {
    let order = morning_order(lines, cover, factuals, history);
    let total_at = |scale: f64| -> Vec<f64> {
        cover
            .iter()
            .zip(extras.iter())
            .zip(sought.iter())
            .map(|((cover_runs, extra), sought_runs)| cover_runs + scale * (extra + sought_runs))
            .collect()
    };
    if morning_feasible(lines, &order, &total_at(1.0), stock, factuals) {
        return;
    }
    let mut lo = 0.0;
    let mut hi = 1.0;
    for _ in 0..32 {
        let mid = (lo + hi) * 0.5;
        if morning_feasible(lines, &order, &total_at(mid), stock, factuals) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    for index in 0..extras.len() {
        extras[index] *= lo;
        sought[index] *= lo;
    }
}

/// # Probe One Run
///
/// Gives a stood-down best maker one run when bids are waiting and the offer is empty.
///
/// `job` holds the targets just written. `scores` is one [`trade_score`]
/// per line. `previous` is the target before this plan, and a line this
/// raises has that mark lifted to the new target. `property` is stock on
/// hand. `factuals` supplies processes. `history` is the board.
///
/// The line is the best known yield of its product, the score is not
/// positive, [`MarketHistory::mass_offered`] is under one, and
/// [`MarketHistory::sought`] is at least one. A target already at one run
/// stays. The new target is one run. An input this job makes is raised on
/// that maker until one of its runs covers the input, and at least one run
/// is planned. An input this job does not make, other than [`TIME`], is
/// put on the shopping list for the amount still short of one run. Nothing
/// is returned.
fn probe_one_run(
    job: &mut Job,
    scores: &[Option<f64>],
    previous: &mut [f64],
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
    history: &MarketHistory,
) {
    let mut picked = Vec::new();
    for (index, line) in job.lines.iter().enumerate() {
        let Some(score) = scores.get(index).copied().flatten() else {
            continue;
        };
        if score > 0.0 || line.target.unwrap_or(0.0) >= 1.0 {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let Some((good, _)) = sale_product(process, factuals) else {
            continue;
        };
        let pace = yield_per_time(process, good);
        if pace < best_known_yield(factuals, good) {
            continue;
        }
        if history.mass_offered(good) >= 1.0 || history.sought(good) < 1.0 {
            continue;
        }
        picked.push(index);
    }
    for index in picked {
        job.lines[index].target = Some(1.0);
        if let Some(mark) = previous.get_mut(index) {
            *mark = (*mark).max(1.0);
        }
        let Some(process) = factuals.get_process(job.lines[index].process) else {
            continue;
        };
        let inputs = process.requirements();
        for input in &inputs {
            if input.good == TIME || input.amount <= 0.0 || !survives_night(factuals, input.good) {
                continue;
            }
            // A line on this job makes the input, so the dig runs with the probe.
            if let Some((maker, per_run)) = maker_on_job(&job.lines, input.good, factuals) {
                if maker != index {
                    let current = job.lines[maker].target.unwrap_or(0.0).max(0.0);
                    if current * per_run + 1e-9 < input.amount {
                        let runs = (input.amount / per_run).max(1.0);
                        job.lines[maker].target = Some(runs);
                        if let Some(mark) = previous.get_mut(maker) {
                            *mark = (*mark).max(runs);
                        }
                    }
                    continue;
                }
            }
            let on_hand = property
                .get(&input.good)
                .map(|row| row.quantity.max(0.0))
                .unwrap_or(0.0);
            job.shop_at_least(input.good, (input.amount - on_hand).max(0.0));
        }
    }
}

/// # Maker On Job
///
/// The line on `lines` with the best [`yield_per_time`] of `good`, and that output per run.
///
/// `factuals` supplies processes. A tie keeps the earlier line. A job that
/// does not make `good` returns `None`.
fn maker_on_job(lines: &[JobLine], good: usize, factuals: &Factuals) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64, f64)> = None;
    for (index, line) in lines.iter().enumerate() {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let Some(per_run) = process
            .outputs
            .iter()
            .find(|output| output.good == good && output.amount > 0.0)
            .map(|output| output.amount)
        else {
            continue;
        };
        let pace = yield_per_time(process, good);
        let replace = match best {
            None => true,
            Some((_, _, was)) => pace > was,
        };
        if replace {
            best = Some((index, per_run, pace));
        }
    }
    best.map(|(index, per_run, _)| (index, per_run))
}

/// # Fit To Morning
///
/// Cuts each written target to the runs the next morning can finish.
///
/// `previous` is the target before this plan's write, one per line.
/// `cover` is the meal. `property` is the pop's rows, `produced` included.
/// `factuals` supplies processes. `history` prices the morning order.
/// `noted` is true when this plan was armed with today's make.
///
/// The pile is quantity on hand plus the open shopping list, the buy
/// already planned for this job. Output an earlier line finishes is
/// counted by [`funded_morning`]. A stock that does not name [`TIME`]
/// does not limit the runs. A line with no process is left as written.
/// A product row with no make, on a noted plan, does not take a target
/// above `previous`. A cut stops at the meal when the written target
/// was already there. Nothing is returned.
fn fit_to_morning(
    job: &mut Job,
    previous: &[f64],
    cover: &[f64],
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
    history: &MarketHistory,
    noted: bool,
) {
    let mut stock = stock_on_hand(property);
    for (&good, &qty) in &job.shopping {
        if good != TIME && qty > 0.0 {
            *stock.entry(good).or_insert(0.0) += qty;
        }
    }
    // A plan that does not name time is not a morning with an empty grant.
    if !stock.contains_key(&TIME) {
        stock.insert(TIME, 1.0e12);
    }
    let wants: Vec<f64> = job
        .lines
        .iter()
        .map(|line| line.target.unwrap_or(0.0).max(0.0))
        .collect();
    let order = morning_order(&job.lines, &wants, factuals, history);
    let fitted = funded_morning(&job.lines, &order, &wants, &stock, factuals);
    for (index, line) in job.lines.iter_mut().enumerate() {
        // No recipe, so this morning cannot say the runs are impossible.
        if factuals.get_process(line.process).is_none() {
            continue;
        }
        let written = wants[index];
        let mut finishable = fitted.get(index).copied().unwrap_or(0.0).max(0.0);
        if finishable < 1e-5 {
            finishable = 0.0;
        }
        let meal = cover.get(index).copied().unwrap_or(0.0).max(0.0);
        let old = previous.get(index).copied().unwrap_or(0.0).max(0.0);
        let mut next = written.min(finishable);
        // The oven made none of this product, so the target does not rise.
        if noted && old > 0.0 && line_made_nothing(line, property, factuals) && next > old {
            next = old;
        }
        // A cut stops at the meal. A target still marching up is left mid-step.
        if written >= meal && next < meal {
            next = meal;
        }
        line.target = Some(next.max(0.0));
    }
}

/// # March Unmet Sought
///
/// One step toward market runs the morning could not fund.
///
/// `job` holds the targets [`fit_to_morning`] left. `scores` is one
/// [`trade_score`] per line. `sale_surplus` is [`pace_sale`]: a `Some`
/// already wrote that surplus, and the line stays. `sought_full` is the
/// unmet-buy runs before the input cap. `sought_kept` is what remained
/// after that cap and [`trim_morning_extra`]. `cover` is the meal, in
/// runs. `property` supplies `produced`. `factuals` names the product.
/// `sale_on` is today's sale note.
///
/// A line steps when the sale left its march, the score pays, the product
/// was made, the full order is at least one run, and the cap kept none.
/// The aim is the meal plus that full order. [`march_target`] takes one
/// step from the target already written. The next morning buys the inputs
/// this step cannot finish from stock. Nothing is returned.
fn march_unmet_sought(
    job: &mut Job,
    scores: &[Option<f64>],
    sale_surplus: &[Option<f64>],
    sought_full: &[f64],
    sought_kept: &[f64],
    cover: &[f64],
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
    sale_on: bool,
) {
    if !sale_on {
        return;
    }
    for (index, line) in job.lines.iter_mut().enumerate() {
        if sale_surplus.get(index).and_then(|surplus| *surplus).is_some() {
            continue;
        }
        if !scores
            .get(index)
            .copied()
            .flatten()
            .is_some_and(|score| score > 0.0)
        {
            continue;
        }
        let full = sought_full.get(index).copied().unwrap_or(0.0);
        let kept = sought_kept.get(index).copied().unwrap_or(0.0);
        if full < 1.0 || kept >= 1.0 {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let Some((good, _)) = sale_product(process, factuals) else {
            continue;
        };
        let made = property
            .get(&good)
            .map(|row| row.produced.max(0.0))
            .unwrap_or(0.0);
        if made <= 0.0 {
            continue;
        }
        let meal = cover.get(index).copied().unwrap_or(0.0).max(0.0);
        let current = line.target.unwrap_or(0.0).max(0.0);
        let aim = meal + full;
        if aim <= current {
            continue;
        }
        line.target = Some(march_target(current, aim));
    }
}

/// # Line Made Nothing
///
/// True when `line`'s product row is on hand and none of it was made.
///
/// `property` holds `produced`. `factuals` names the product. A missing
/// process, a line with no product, and a product with no row are false.
fn line_made_nothing(
    line: &JobLine,
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
) -> bool {
    let Some(process) = factuals.get_process(line.process) else {
        return false;
    };
    let Some((good, _)) = sale_product(process, factuals) else {
        return false;
    };
    property
        .get(&good)
        .is_some_and(|row| row.produced.max(0.0) <= 0.0)
}

/// # Release Surplus For Sought
///
/// Puts surplus inputs back so a sought line that does not pay can run.
///
/// `sought` is the extra from [`sought_runs`]. A line with a positive
/// score keeps the pool [`cap_sought`] already has. A line that does not
/// pay, and still cannot finish its sought runs from `remaining`, takes
/// inputs from other lines' surplus. The lowest [`pay_per_time`] gives
/// its extra up first, and only the runs that cover the short inputs.
/// Covers stay. `extras` shrinks by the runs returned to `remaining`.
fn release_surplus_for_sought(
    lines: &[JobLine],
    scores: &[Option<f64>],
    extras: &mut [f64],
    sought: &[f64],
    remaining: &mut HashMap<usize, f64>,
    factuals: &Factuals,
) {
    for index in 0..lines.len() {
        let want = sought.get(index).copied().unwrap_or(0.0);
        if want <= 0.0 {
            continue;
        }
        if scores
            .get(index)
            .copied()
            .flatten()
            .is_some_and(|score| score > 0.0)
        {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let mut donors: Vec<usize> = (0..lines.len())
            .filter(|&donor| donor != index && extras.get(donor).is_some_and(|extra| *extra > 0.0))
            .collect();
        // The slowest surplus gives its inputs up first.
        donors.sort_by(|&left, &right| {
            ranked_pay(lines, left, scores, factuals)
                .partial_cmp(&ranked_pay(lines, right, scores, factuals))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.cmp(&right))
        });
        for donor in donors {
            let funded = runs_on_hand(process, remaining);
            let funded = if funded.is_finite() { funded } else { want };
            if funded >= want {
                break;
            }
            let Some(donor_process) = factuals.get_process(lines[donor].process) else {
                continue;
            };
            let give = surplus_runs_to_free(donor_process, process, remaining, want - funded)
                .min(extras[donor]);
            if !(give > 0.0) {
                continue;
            }
            add_required(remaining, donor_process, give);
            extras[donor] = (extras[donor] - give).max(0.0);
        }
    }
}

/// # Surplus Runs To Free
///
/// Donor runs that put `missing_runs` of `need` back into `pool`.
///
/// Each required input of `need` that `pool` cannot cover counts, when
/// `donor` also spends that input. The largest of those run counts is
/// the gift. An input `donor` does not spend adds nothing. Returns 0
/// when the donor cannot cover any short input.
fn surplus_runs_to_free(
    donor: &Process,
    need: &Process,
    pool: &HashMap<usize, f64>,
    missing_runs: f64,
) -> f64 {
    if missing_runs <= 0.0 {
        return 0.0;
    }
    let mut give: f64 = 0.0;
    for input in need.requirements() {
        if input.amount <= 0.0 {
            continue;
        }
        let have = pool.get(&input.good).copied().unwrap_or(0.0);
        let short = (input.amount * missing_runs - have).max(0.0);
        if short <= 0.0 {
            continue;
        }
        let per: f64 = donor
            .requirements()
            .iter()
            .filter(|row| row.good == input.good && row.amount > 0.0)
            .map(|row| row.amount)
            .sum();
        if per <= 0.0 {
            continue;
        }
        give = give.max(short / per);
    }
    give
}

/// # Cap Sought
///
/// Lowers sought runs to what `remaining` can still finish, and spends them.
///
/// `lines` and `factuals` supply the processes. `sought` is the extra from
/// [`sought_runs`]. `remaining` is the stock left after the cover and the
/// surplus. A missing process becomes 0. A process with no required input
/// is left as it is. The inputs of the runs that remain leave `remaining`.
fn cap_sought(
    lines: &[JobLine],
    sought: &mut [f64],
    remaining: &mut HashMap<usize, f64>,
    factuals: &Factuals,
) {
    for (index, line) in lines.iter().enumerate() {
        if sought[index] <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            sought[index] = 0.0;
            continue;
        };
        let funded = runs_on_hand(process, remaining);
        if funded.is_finite() {
            sought[index] = sought[index].min(funded);
        }
        if sought[index] <= 0.0 {
            continue;
        }
        for input in process.requirements() {
            let have = remaining.get(&input.good).copied().unwrap_or(0.0);
            let left = (have - input.amount * sought[index]).max(0.0);
            if left > 0.0 {
                remaining.insert(input.good, left);
            } else {
                remaining.remove(&input.good);
            }
        }
    }
}

/// # Make Cost Per Unit
///
/// Holding one unit of `good` costs to make with `process`.
///
/// `history` prices the required inputs through [`input_holding`].
/// Optional inputs, factors, and capital stay out. Returns `None` when
/// `process` does not make `good`.
pub(crate) fn make_cost_per_unit(
    process: &Process,
    good: usize,
    history: &MarketHistory,
) -> Option<f64> {
    let output = process
        .outputs
        .iter()
        .find(|output| output.good == good && output.amount > 0.0)?;
    let mut cost = 0.0;
    for input in &process.inputs {
        if input.is_optional() {
            continue;
        }
        if !matches!(input.input_type, InputType::Destroyed | InputType::Consumed) {
            continue;
        }
        cost += input.amount * input_holding(history, input.good);
    }
    if !cost.is_finite() {
        return None;
    }
    Some(cost / output.amount)
}

/// # Listed Decay
///
/// One night of decay for `good`.
///
/// `factuals` supplies the good. A missing good is 0. The rate is clamped
/// to 0..=1. Returns that rate.
pub(crate) fn listed_decay(factuals: &Factuals, good: usize) -> f64 {
    factuals
        .get_good(good)
        .map(|good| good.decay_rate)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

/// # Card Covers Cost
///
/// True when `card` is at least `cost` after one night of `decay`.
///
/// The floor is `cost` times the share that survives that night. `decay`
/// is clamped to 0..=1. A non-finite cost or card is false. A zero card
/// is false. Returns that comparison.
pub(crate) fn card_covers_cost(cost: f64, card: f64, decay: f64) -> bool {
    if !cost.is_finite() || !card.is_finite() || card == 0.0 {
        return false;
    }
    let decay = decay.clamp(0.0, 1.0);
    card >= cost * (1.0 - decay)
}

/// # Desire Cover
///
/// The gap a job plans, in units of each good.
///
/// `consumption` is the short meal. `savings` is the short saved day.
/// `lines` and `factuals` pick the line that would take each saved good,
/// by the same yield rule as [`assign_cover`]. `history` prices
/// [`trade_score`]. A score above zero adds that saved day onto the meal.
/// A score at or below zero leaves the meal alone.
pub(crate) fn desire_cover(
    lines: &[JobLine],
    consumption: &HashMap<usize, f64>,
    savings: &HashMap<usize, f64>,
    factuals: &Factuals,
    history: &MarketHistory,
) -> HashMap<usize, f64> {
    let mut cover = consumption.clone();
    if savings.is_empty() {
        return cover;
    }
    for (good, index) in cover_winners(lines, savings, factuals) {
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        if trade_score(process, history) <= 0.0 {
            continue;
        }
        let units = savings.get(&good).copied().unwrap_or(0.0);
        if units > 0.0 {
            *cover.entry(good).or_insert(0.0) += units;
        }
    }
    cover
}

/// # Add Input Needs
///
/// Adds the inputs `iterations` will consume to `cover`, when this job makes them.
///
/// `lines` and `factuals` supply the processes. A good no line outputs is
/// left for shopping. A good that does not survive the night is left out.
/// The added units sit on top of any desire gap already in `cover`.
fn add_input_needs(
    lines: &[JobLine],
    iterations: &[f64],
    cover: &mut HashMap<usize, f64>,
    factuals: &Factuals,
) {
    let mut produced = HashSet::new();
    for line in lines {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        for output in &process.outputs {
            if output.amount > 0.0 {
                produced.insert(output.good);
            }
        }
    }
    let mut extra: HashMap<usize, f64> = HashMap::new();
    for (line, &runs) in lines.iter().zip(iterations) {
        if runs <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        for input in process.requirements() {
            if !produced.contains(&input.good) || !survives_night(factuals, input.good) {
                continue;
            }
            *extra.entry(input.good).or_insert(0.0) += input.amount * runs;
        }
    }
    for (good, units) in extra {
        *cover.entry(good).or_insert(0.0) += units;
    }
}

/// # Input Used
///
/// Units of `good` that today's production consumed as a required input.
///
/// `factuals` supplies processes. `history` supplies
/// [`MarketHistory::produced`]. Each output with production is assigned to
/// the highest [`yield_per_time`] process that makes it. That process counts
/// once, at its largest such run count. Optional inputs and factors are not
/// counted. Returns the sum.
fn input_used(factuals: &Factuals, history: &MarketHistory, good: usize) -> f64 {
    let processes: Vec<&Process> = factuals.each_process().collect();
    let mut best: HashMap<usize, (usize, f64)> = HashMap::new();
    for (index, process) in processes.iter().enumerate() {
        for output in &process.outputs {
            if output.amount <= 0.0 {
                continue;
            }
            let pace = yield_per_time(process, output.good);
            let replace = match best.get(&output.good) {
                None => true,
                Some((_, was)) => pace > *was,
            };
            if replace {
                best.insert(output.good, (index, pace));
            }
        }
    }
    let mut runs_of: HashMap<usize, f64> = HashMap::new();
    for (output_good, (index, _)) in best {
        let made = history.produced(output_good);
        if made <= 0.0 {
            continue;
        }
        let amount = processes[index]
            .outputs
            .iter()
            .find(|output| output.good == output_good && output.amount > 0.0)
            .map(|output| output.amount)
            .unwrap_or(0.0);
        if amount <= 0.0 {
            continue;
        }
        let runs = made / amount;
        let entry = runs_of.entry(index).or_insert(0.0);
        if runs > *entry {
            *entry = runs;
        }
    }
    let mut used = 0.0;
    for (index, runs) in runs_of {
        for input in processes[index].requirements() {
            if input.good == good && input.amount > 0.0 {
                used += runs * input.amount;
            }
        }
    }
    used
}

/// # Sought Runs
///
/// Extra iterations for outputs the market still wants or just used.
///
/// `lines`, `cover_targets`, and `factuals` supply the plan so far.
/// `property` is stock on hand. `history` supplies [`MarketHistory::sought`]
/// and [`MarketHistory::produced`]. Demand is units still sought plus
/// [`input_used`]. The best yield in `factuals` takes the whole shortfall
/// above stock, including when [`trade_score`] is not positive. A worse
/// line that does not pay adds none. A worse paying line adds no more
/// than its cover, and one [`is_cheaper_to_buy`] rejects adds none.
/// Returns one extra per line.
fn sought_runs(
    lines: &[JobLine],
    cover_targets: &[f64],
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
    history: &MarketHistory,
) -> Vec<f64> {
    let mut best: HashMap<usize, (usize, f64)> = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        for output in &process.outputs {
            if output.amount <= 0.0 {
                continue;
            }
            let pace = yield_per_time(process, output.good);
            let replace = match best.get(&output.good) {
                None => true,
                Some((_, was)) => pace > *was,
            };
            if replace {
                best.insert(output.good, (index, pace));
            }
        }
    }
    let mut extra = vec![0.0; lines.len()];
    for (good, (index, _)) in best {
        let demand = history.sought(good) + input_used(factuals, history, good);
        if demand <= 0.0 {
            continue;
        }
        let on_hand = property
            .get(&good)
            .map(|row| row.quantity.max(0.0))
            .unwrap_or(0.0);
        let short = demand - on_hand;
        if short <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let Some(output) = process
            .outputs
            .iter()
            .find(|output| output.good == good && output.amount > 0.0)
        else {
            continue;
        };
        let runs = short / output.amount;
        let pace = yield_per_time(process, good);
        // The best known yield takes this shortfall even when the run loses.
        // A worse line that does not pay adds none. A worse paying line
        // stays inside its cover, and one the card already beats takes none.
        let known = best_known_yield(factuals, good);
        let allow = if pace >= known {
            runs
        } else if trade_score(process, history) <= 0.0 || is_cheaper_to_buy(process, good, history)
        {
            0.0
        } else {
            runs.min(cover_targets[index])
        };
        if allow > extra[index] {
            extra[index] = allow;
        }
    }
    extra
}

/// # Best Known Yield
///
/// The highest [`yield_per_time`] of `good` among processes in `factuals`.
///
/// A process that does not make `good` scores 0. No process returns 0.
fn best_known_yield(factuals: &Factuals, good: usize) -> f64 {
    factuals
        .each_process()
        .map(|process| yield_per_time(process, good))
        .fold(0.0_f64, f64::max)
}

/// # Yield Per Time
///
/// How much of `good` one unit of time makes in `process`.
///
/// Time is a required destroyed or consumed input of [`TIME`]. A process
/// with no time input returns the output amount, so it beats a process
/// that spends time. A missing output is 0.
fn yield_per_time(process: &Process, good: usize) -> f64 {
    let amount = process
        .outputs
        .iter()
        .find(|output| output.good == good)
        .map(|output| output.amount)
        .unwrap_or(0.0);
    let time = time_spent(process);
    if time <= 0.0 { amount } else { amount / time }
}

/// # Recipe For Good
///
/// The process that founds `good`.
///
/// `factuals` supplies the processes. A variable output of `good` qualifies.
/// A fixed output does not. Time does not. A non-subsistence process comes
/// before a subsistence one, and the lower id breaks the rest. Returns that
/// id, or `None`.
pub(crate) fn recipe_for_good(factuals: &Factuals, good: usize) -> Option<usize> {
    if good == TIME {
        return None;
    }
    let mut found: Vec<&Process> = factuals
        .each_process()
        .filter(|process| {
            process.outputs.iter().any(|output| {
                !output.fixed && output.good == good && output.amount > 0.0
            })
        })
        .collect();
    found.sort_by(|left, right| {
        left.is_subsistence()
            .cmp(&right.is_subsistence())
            .then(left.id.cmp(&right.id))
    });
    found.first().map(|process| process.id)
}

/// # Runs Point Cost
///
/// Complexity points `runs` of `process` spend.
///
/// `rates` is [`subsistence_cover`]. Runs up through `cover_runs` spend
/// [`cover_rate`]. Runs past that spend [`complexity_spent`]. `cheap`
/// selects the subsistence floor for that cover. A non-positive run count
/// spends nothing. Returns that cost.
fn runs_point_cost(
    process: &Process,
    rates: &HashMap<usize, f64>,
    runs: f64,
    cover_runs: f64,
    cheap: bool,
) -> f64 {
    if !runs.is_finite() || runs <= 0.0 {
        return 0.0;
    }
    let cheap_rate = cover_rate(process, rates, cheap);
    let full = complexity_spent(process);
    let cheap = runs.min(cover_runs.max(0.0));
    let extra = (runs - cheap).max(0.0);
    cheap_rate * cheap + full * extra
}

/// # Runs In Points
///
/// How many runs of `process` `points` can start.
///
/// `rates` is [`subsistence_cover`]. The first `cover_runs` spend
/// [`cover_rate`], and later runs spend [`complexity_spent`]. `cheap`
/// selects the subsistence floor for that cover. No cost, or a budget
/// that is not finite, is not a limit. Returns that count, never negative.
fn runs_in_points(
    process: &Process,
    rates: &HashMap<usize, f64>,
    points: f64,
    cover_runs: f64,
    cheap: bool,
) -> f64 {
    if !points.is_finite() {
        return f64::INFINITY;
    }
    if points <= 0.0 {
        return 0.0;
    }
    let cheap_rate = cover_rate(process, rates, cheap);
    let full = complexity_spent(process);
    let cheap_cap = cover_runs.max(0.0);
    if cheap_rate <= 0.0 && full <= 0.0 {
        return f64::INFINITY;
    }
    let cheap_cost = if cheap_rate > 0.0 {
        cheap_rate * cheap_cap
    } else {
        0.0
    };
    if points <= cheap_cost {
        if cheap_rate <= 0.0 {
            return cheap_cap;
        }
        return (points / cheap_rate).min(cheap_cap).max(0.0);
    }
    if full <= 0.0 {
        return f64::INFINITY;
    }
    cheap_cap + (points - cheap_cost) / full
}

/// # Cover Rate
///
/// Points one cover run of `process` spends.
///
/// `rates` is [`subsistence_cover`]. `cheap` selects [`subsistence_floor`]
/// when that floor exists, and never above [`complexity_spent`]. Otherwise
/// the rate is [`cover_complexity`]. Returns that cost.
fn cover_rate(process: &Process, rates: &HashMap<usize, f64>, cheap: bool) -> f64 {
    if cheap {
        if let Some(floor) = subsistence_floor(rates) {
            return floor.min(complexity_spent(process));
        }
    }
    cover_complexity(process, rates)
}

/// # Complexity Spent
///
/// Complexity points one run of `process` spends.
///
/// The points are the process complexity. A non-finite or non-positive
/// complexity spends nothing. Returns that cost.
fn complexity_spent(process: &Process) -> f64 {
    if process.complexity.is_finite() && process.complexity > 0.0 {
        process.complexity
    } else {
        0.0
    }
}

/// # Complexity Budget
///
/// Complexity points `time_on_hand` can fund.
///
/// One labor-day of time, [`TIME_PER_LABOR`], funds one point. No time on
/// hand leaves the cap open, so a pile that never recorded time does not
/// block a run. A non-finite pile does the same. Returns that budget.
fn complexity_budget(time_on_hand: f64) -> f64 {
    if !time_on_hand.is_finite() || time_on_hand <= 0.0 {
        f64::INFINITY
    } else {
        time_on_hand / TIME_PER_LABOR
    }
}

/// # Scaled Points
///
/// `budget` after a craft's point scale.
///
/// `scale` is [`Job::craft_point_scale`]. An open budget stays open. A
/// scale that is not finite and positive leaves the budget as it is.
/// Returns the points a day may spend.
fn scaled_points(budget: f64, scale: f64) -> f64 {
    if !budget.is_finite() || !scale.is_finite() || scale <= 0.0 {
        budget
    } else {
        budget / scale
    }
}

/// # Complexity Room
///
/// Runs `process` can still start from `points`.
///
/// A process that spends nothing is not limited by the points. An open
/// budget is not a limit. Returns that run count, never negative.
fn complexity_room(process: &Process, points: f64) -> f64 {
    let cost = complexity_spent(process);
    if cost <= 0.0 || !points.is_finite() {
        f64::INFINITY
    } else {
        (points / cost).max(0.0)
    }
}

/// # Complexity Of Runs
///
/// Complexity points the run counts in `runs` spend.
///
/// `lines` and `factuals` supply each process. A missing process, or a
/// count at or below zero, spends nothing. Returns that total.
fn complexity_of_runs(lines: &[JobLine], factuals: &Factuals, runs: &[f64]) -> f64 {
    let mut total = 0.0;
    for (index, &n) in runs.iter().enumerate() {
        if n <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        total += complexity_spent(process) * n;
    }
    total
}

/// # Subsistence Cover
///
/// Lowest subsistence complexity of each variable good.
///
/// `factuals` supplies the processes. A process that is not subsistence
/// adds nothing. A good made by several subsistence processes keeps the
/// lowest [`complexity_spent`]. Returns that cost by good.
fn subsistence_cover(factuals: &Factuals) -> HashMap<usize, f64> {
    let mut cover = HashMap::new();
    for process in factuals.each_process() {
        if !process.is_subsistence() {
            continue;
        }
        let cost = complexity_spent(process);
        if cost <= 0.0 {
            continue;
        }
        for good in process.variable_goods() {
            let entry = cover.entry(good).or_insert(cost);
            if cost < *entry {
                *entry = cost;
            }
        }
    }
    cover
}

/// # Cover Complexity
///
/// Points one desire-cover run of `process` spends.
///
/// `rates` is [`subsistence_cover`]. A specialty spends the subsistence
/// rate of a variable good it also makes. Several matches use the lowest
/// rate. Any other process spends [`complexity_spent`]. Returns that cost.
fn cover_complexity(process: &Process, rates: &HashMap<usize, f64>) -> f64 {
    let own = complexity_spent(process);
    if process.is_subsistence() {
        return own;
    }
    let mut best: Option<f64> = None;
    for good in process.variable_goods() {
        let Some(&cost) = rates.get(&good) else {
            continue;
        };
        best = Some(best.map_or(cost, |have| have.min(cost)));
    }
    best.unwrap_or(own)
}

/// # Subsistence Floor
///
/// Lowest subsistence complexity in `rates`.
///
/// `rates` is [`subsistence_cover`]. A non-positive cost is skipped. No
/// rate returns `None`. Returns that cost.
fn subsistence_floor(rates: &HashMap<usize, f64>) -> Option<f64> {
    let mut best: Option<f64> = None;
    for &cost in rates.values() {
        if cost > 0.0 && cost.is_finite() {
            best = Some(best.map_or(cost, |have| have.min(cost)));
        }
    }
    best
}

/// # Cover Points
///
/// Desire-cover points the run counts in `runs` spend.
///
/// `lines` and `factuals` supply each process. Each run spends
/// [`cover_rate`]. A missing process, or a count at or below zero, spends
/// nothing. Returns that total.
fn cover_points(lines: &[JobLine], factuals: &Factuals, runs: &[f64]) -> f64 {
    let rates = subsistence_cover(factuals);
    let mut total = 0.0;
    for (index, &n) in runs.iter().enumerate() {
        if n <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        total += cover_rate(process, &rates, lines[index].cover_cheap) * n;
    }
    total
}

/// # Add Earlier Meals
///
/// Adds runs for a good an earlier desire will eat before the later one.
///
/// `lines` and `factuals` supply the job. `earlier` is the basic gaps.
/// `basic_runs` is what those gaps already reserved. `later_gaps` are the
/// desires after them. `free` is working stock. `meal` is about to leave
/// that stock. `saved` is a meal already outside it. A good `basic_runs`
/// already makes is left as it is. Otherwise the missing satisfaction of
/// that good, divided by its efficiency, is more runs on the line that
/// makes the most of it per unit of time. Stock of a worse target in any
/// of those piles is added too, because this good is eaten first and that
/// stock is left. Those runs are added to `later_runs`. Nothing is returned.
fn add_earlier_meals(
    lines: &[JobLine],
    factuals: &Factuals,
    earlier: &[(Vec<(usize, f64)>, f64)],
    basic_runs: &[f64],
    later_gaps: &[(Vec<(usize, f64)>, f64)],
    later_runs: &mut [f64],
    free: &HashMap<usize, f64>,
    meal: &HashMap<usize, f64>,
    saved: &HashMap<usize, f64>,
) {
    let mut made = HashSet::new();
    for (index, &runs) in basic_runs.iter().enumerate() {
        if runs <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        for output in &process.outputs {
            if output.amount > 0.0 {
                made.insert(output.good);
            }
        }
    }
    let mut seen = HashSet::new();
    for gap in later_gaps {
        for &(good, _) in &gap.0 {
            if !seen.insert(good) || made.contains(&good) {
                continue;
            }
            let mut units = 0.0;
            for (goods, sat) in earlier {
                let Some((_, efficiency)) = goods.iter().find(|(id, _)| *id == good) else {
                    continue;
                };
                if *efficiency <= 0.0 {
                    continue;
                }
                if *sat > 0.0 {
                    units += sat / efficiency;
                }
                // A worse pile, including a meal already set aside, is left once this good is made.
                for &(other, other_eff) in goods {
                    if other == good || other_eff <= 0.0 || other_eff >= *efficiency {
                        continue;
                    }
                    let stock = free.get(&other).copied().unwrap_or(0.0)
                        + meal.get(&other).copied().unwrap_or(0.0)
                        + saved.get(&other).copied().unwrap_or(0.0);
                    if stock > 0.0 {
                        units += stock * other_eff / efficiency;
                    }
                }
            }
            if units <= 0.0 {
                continue;
            }
            let Some((index, per)) = best_output_line(lines, factuals, good) else {
                continue;
            };
            if per <= 0.0 {
                continue;
            }
            later_runs[index] += units / per;
        }
    }
}

/// # Desire Goods Eaten
///
/// Units each good in `gaps` takes when the job can make it.
///
/// `lines` and `factuals` say which goods a line outputs. Each gap spends
/// its missing satisfaction on the highest-efficiency target the job makes.
/// Returns those units by good.
fn desire_goods_eaten(
    lines: &[JobLine],
    factuals: &Factuals,
    gaps: &[(Vec<(usize, f64)>, f64)],
) -> HashMap<usize, f64> {
    let mut eaten = HashMap::new();
    for (goods, sat) in gaps {
        if !sat.is_finite() || *sat <= 0.0 {
            continue;
        }
        let mut best_good = None;
        let mut best_eff = 0.0;
        for &(good, efficiency) in goods {
            if !efficiency.is_finite() || efficiency <= best_eff {
                continue;
            }
            if !line_makes_good(lines, factuals, good) {
                continue;
            }
            best_good = Some(good);
            best_eff = efficiency;
        }
        if let Some(good) = best_good {
            *eaten.entry(good).or_insert(0.0) += sat / best_eff;
        }
    }
    eaten
}

/// # Add Cover Inputs
///
/// Reserves material a specialty cover destroys.
///
/// `lines` and `factuals` supply the job. `later` is the later cover.
/// `earlier` is the basic gaps. `free` is working stock. Time and skills
/// are skipped. Output reserved in `basic` counts, and so does `free`,
/// except the units [`desire_goods_eaten`] says those gaps will eat. The
/// rest is added on the line that makes the most of the input per unit of
/// time. A basic specialty does the same, and a supplier reserved that way
/// is visited again, up to one pass per line. Nothing is returned.
fn add_cover_inputs(
    lines: &[JobLine],
    factuals: &Factuals,
    basic: &mut [f64],
    later: &[f64],
    earlier: &[(Vec<(usize, f64)>, f64)],
    free: &HashMap<usize, f64>,
) {
    let mut spare: HashMap<usize, f64> = HashMap::new();
    for (&good, &units) in free {
        if good == TIME || units <= 0.0 {
            continue;
        }
        spare.insert(good, units);
    }
    for (index, &runs) in basic.iter().enumerate() {
        if runs <= 0.0 {
            continue;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        for output in &process.outputs {
            if output.amount > 0.0 {
                *spare.entry(output.good).or_insert(0.0) += output.amount * runs;
            }
        }
    }
    for (good, units) in desire_goods_eaten(lines, factuals, earlier) {
        if let Some(have) = spare.get_mut(&good) {
            *have = (*have - units).max(0.0);
        }
    }
    for (index, &runs) in later.iter().enumerate() {
        pull_cover_inputs(lines, factuals, index, runs, &mut spare, basic);
    }
    // A supplier reserved for a basic specialty is visited on the next pass.
    for _ in 0..lines.len() {
        let queued = basic.to_vec();
        let before = basic.to_vec();
        for (index, &runs) in queued.iter().enumerate() {
            pull_cover_inputs(lines, factuals, index, runs, &mut spare, basic);
        }
        if basic.iter().zip(&before).all(|(now, was)| (*now - was).abs() <= 1e-12) {
            break;
        }
    }
}

/// # Pull Cover Inputs
///
/// Adds the supplier runs one specialty cover still needs.
///
/// `lines` and `factuals` supply the job. `index` is the consumer.
/// `runs` is that cover. `spare` is stock and output already counted.
/// `into` receives the supplier runs. Time, skills, and a subsistence
/// consumer are skipped. The supplier is the line that makes the most of
/// the input per unit of time, and it is not the consumer. `spare` gains
/// the units just reserved, so the same need is not pulled twice. Nothing
/// is returned.
fn pull_cover_inputs(
    lines: &[JobLine],
    factuals: &Factuals,
    index: usize,
    runs: f64,
    spare: &mut HashMap<usize, f64>,
    into: &mut [f64],
) {
    if runs <= 0.0 {
        return;
    }
    let Some(process) = factuals.get_process(lines[index].process) else {
        return;
    };
    if process.is_subsistence() {
        return;
    }
    for input in process.requirements() {
        if input.good == TIME
            || !input.amount.is_finite()
            || input.amount <= 0.0
            || !matches!(input.input_type, InputType::Destroyed | InputType::Consumed)
            || factuals
                .get_good(input.good)
                .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let Some((supplier, per)) = best_output_line(lines, factuals, input.good) else {
            continue;
        };
        if per <= 0.0 || supplier == index {
            continue;
        }
        let have = spare.get(&input.good).copied().unwrap_or(0.0);
        let still = (input.amount * runs - have).max(0.0);
        if still <= 1e-12 {
            continue;
        }
        into[supplier] += still / per;
        *spare.entry(input.good).or_insert(0.0) += still;
    }
}

/// # Fit Layers
///
/// Shrinks the two covers so they fit in `points`.
///
/// `basic` is the leading desires. `later` is the rest. `meal` marks a
/// later line that makes a basic desire's good. A subsistence line is
/// kept, and so is a specialty whose [`cover_complexity`] is below its
/// own. Those basic runs are funded first. A meal line then spends
/// [`subsistence_floor`], and is scaled to what remains. A basic specialty
/// that spends its own comes next. A later subsistence tier takes what is
/// left, and any other later specialty takes what that tier leaves. An
/// open budget leaves the runs. Nothing is returned.
fn fit_layers(
    lines: &[JobLine],
    factuals: &Factuals,
    basic: &mut [f64],
    later: &mut [f64],
    meal: &[bool],
    points: f64,
) {
    if !points.is_finite() {
        return;
    }
    let rates = subsistence_cover(factuals);
    let floor = subsistence_floor(&rates);
    let count = lines.len();
    let mut protect = vec![false; count];
    let mut discounted = false;
    for (index, line) in lines.iter().enumerate() {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let cover = cover_complexity(process, &rates);
        let own = complexity_spent(process);
        protect[index] = process.is_subsistence() || cover < own;
        if protect[index] && cover < own && basic[index] > 0.0 {
            discounted = true;
        }
    }
    let price = |index: usize, cheap: bool| -> f64 {
        let Some(process) = factuals.get_process(lines[index].process) else {
            return 0.0;
        };
        if cheap {
            if let Some(floor) = floor {
                return floor.min(complexity_spent(process));
            }
        }
        cover_complexity(process, &rates)
    };
    let mut cost_basic = 0.0;
    for index in 0..count {
        if protect[index] && basic[index] > 0.0 {
            cost_basic += price(index, false) * basic[index];
        }
    }
    if cost_basic >= points {
        for index in 0..count {
            if !protect[index] {
                basic[index] = 0.0;
            }
            later[index] = 0.0;
        }
        if !discounted {
            let mut only = vec![0.0; count];
            for index in 0..count {
                if protect[index] {
                    only[index] = basic[index];
                }
            }
            scale_to_complexity(lines, factuals, &mut only, points);
            basic.copy_from_slice(&only);
            return;
        }
        if cost_basic > points && cost_basic > 0.0 {
            let scale = points / cost_basic;
            for index in 0..count {
                if protect[index] {
                    basic[index] *= scale;
                }
            }
        }
        return;
    }
    let mut left = points - cost_basic;
    let mut cost_food = 0.0;
    for index in 0..count {
        if meal[index] && !protect[index] && later[index] > 0.0 {
            cost_food += price(index, true) * later[index];
        }
    }
    if cost_food > left && cost_food > 0.0 {
        let scale = left / cost_food;
        for index in 0..count {
            if !protect[index] {
                basic[index] = 0.0;
            }
            if meal[index] {
                later[index] *= scale;
            } else {
                later[index] = 0.0;
            }
        }
        return;
    }
    left -= cost_food;
    let mut cost_open = 0.0;
    for index in 0..count {
        if !protect[index] && basic[index] > 0.0 {
            cost_open += price(index, false) * basic[index];
        }
    }
    if cost_open > left && cost_open > 0.0 {
        let scale = left / cost_open;
        for index in 0..count {
            if !protect[index] {
                basic[index] *= scale;
                if !meal[index] {
                    later[index] = 0.0;
                }
            } else {
                later[index] = 0.0;
            }
        }
        return;
    }
    left -= cost_open;
    let mut cost_tier = 0.0;
    for index in 0..count {
        if protect[index] && later[index] > 0.0 {
            cost_tier += price(index, false) * later[index];
        }
    }
    if cost_tier > left && cost_tier > 0.0 {
        let scale = left / cost_tier;
        for index in 0..count {
            if protect[index] {
                later[index] *= scale;
            } else if !meal[index] {
                later[index] = 0.0;
            }
        }
        return;
    }
    left -= cost_tier;
    let mut cost_rest = 0.0;
    for index in 0..count {
        if !meal[index] && !protect[index] && later[index] > 0.0 {
            cost_rest += price(index, true) * later[index];
        }
    }
    if cost_rest > left && cost_rest > 0.0 {
        let scale = left / cost_rest;
        for index in 0..count {
            if !meal[index] && !protect[index] {
                later[index] *= scale;
            }
        }
    }
}

/// # Scale To Complexity
///
/// Shrinks `runs` so their complexity fits in `points`.
///
/// `lines` and `factuals` price that complexity. An open budget, or a
/// total already inside it, leaves the runs. Otherwise each run is scaled
/// by `points / total`. Nothing is returned.
fn scale_to_complexity(
    lines: &[JobLine],
    factuals: &Factuals,
    runs: &mut [f64],
    points: f64,
) {
    if !points.is_finite() {
        return;
    }
    let cost = complexity_of_runs(lines, factuals, runs);
    if cost > points && cost > 0.0 {
        let scale = points / cost;
        for n in runs.iter_mut() {
            *n *= scale;
        }
    }
}

/// # Time Spent
///
/// Required time `process` destroys or consumes in one run.
///
/// Optional time is not counted. A process with no such input returns 0.
fn time_spent(process: &Process) -> f64 {
    process
        .inputs
        .iter()
        .filter(|input| {
            !input.is_optional()
                && input.good == TIME
                && matches!(input.input_type, InputType::Destroyed | InputType::Consumed)
        })
        .map(|input| input.amount)
        .sum()
}

/// # Line Yield
///
/// The best [`yield_per_time`] among `process` outputs.
///
/// A process with no positive output is 0.
fn line_yield(process: &Process) -> f64 {
    process
        .outputs
        .iter()
        .filter(|output| output.amount > 0.0)
        .map(|output| yield_per_time(process, output.good))
        .fold(0.0_f64, f64::max)
}

/// # Time Ready
///
/// Time this job can still spend.
///
/// Free units of [`TIME`] plus this job's claim. A desire reserve stays out.
fn time_ready(property: &HashMap<usize, PopPRow>, claimed: &HashMap<usize, f64>) -> f64 {
    free_of(property, TIME) + claimed.get(&TIME).copied().unwrap_or(0.0)
}

/// # Feeders Before
///
/// The run order with a maker placed ahead of the line that consumes it.
///
/// `lines` is the job. `order` is the stored line order. `factuals` supplies
/// the processes. The move is the one [`order_producers_first`] makes, on
/// indexes. A pair that feeds both ways stays as it stands. The walk is
/// bounded by the length of the order. Returns that order.
fn feeders_before(lines: &[JobLine], mut order: Vec<usize>, factuals: &Factuals) -> Vec<usize> {
    let passes = order.len();
    for _ in 0..passes {
        let mut moved = false;
        let mut consumer = 0;
        while consumer < order.len() {
            let found = (consumer + 1..order.len()).find(|&producer| {
                line_feeds(&lines[order[producer]], &lines[order[consumer]], factuals)
                    && !line_feeds(&lines[order[consumer]], &lines[order[producer]], factuals)
            });
            if let Some(producer) = found {
                let id = order.remove(producer);
                order.insert(consumer, id);
                moved = true;
            }
            consumer += 1;
        }
        if !moved {
            break;
        }
    }
    order
}

/// # Time Held For Later
///
/// Time `lines[index]` must leave for a later line it feeds.
///
/// `order` is the run order. A line after `index` in that order counts
/// when [`line_feeds`] is true and its [`line_yield`] is higher. The time
/// is that line's target times its required time. `factuals` supplies both
/// processes. A missing process adds nothing.
fn time_held_for_later(
    lines: &[JobLine],
    order: &[usize],
    index: usize,
    factuals: &Factuals,
) -> f64 {
    let Some(pos) = order.iter().position(|&later| later == index) else {
        return 0.0;
    };
    let mut held = 0.0;
    for &later in order.iter().skip(pos + 1) {
        if !holds_time_for(&lines[index], &lines[later], factuals) {
            continue;
        }
        let Some(process) = factuals.get_process(lines[later].process) else {
            continue;
        };
        let Some(n) = lines[later].target else {
            continue;
        };
        if n > 0.0 {
            held += time_spent(process) * n;
        }
    }
    held
}

/// # Runs For Leave
///
/// Runs of `process` that put `leave` on hand.
///
/// `property` and `claimed` are the stock already there. Each output the
/// leave names counts the gap above that stock. The largest gap wins.
/// Time and a fixed output add nothing. Stock that already covers the
/// leave returns 0. Returns that count.
fn runs_for_leave(
    process: &Process,
    property: &HashMap<usize, PopPRow>,
    claimed: &HashMap<usize, f64>,
    leave: &HashMap<usize, f64>,
) -> f64 {
    let mut runs = 0.0_f64;
    for output in &process.outputs {
        if output.fixed || output.amount <= 0.0 || output.good == TIME {
            continue;
        }
        let need = leave.get(&output.good).copied().unwrap_or(0.0).max(0.0);
        if need <= 0.0 {
            continue;
        }
        let have = free_of(property, output.good)
            + claimed.get(&output.good).copied().unwrap_or(0.0).max(0.0);
        let gap = (need - have).max(0.0);
        if gap <= 0.0 {
            continue;
        }
        runs = runs.max(gap / output.amount);
    }
    runs
}

/// # Points Held For Later
///
/// Complexity points `lines[index]` leaves for a later line.
///
/// `order` is the run order. `rates` is [`subsistence_cover`]. `factuals`
/// supplies the processes. Each later line keeps [`runs_point_cost`] of
/// its cover. A later line that pays, and that another line feeds, also
/// keeps its target above that cover, at [`complexity_spent`]. A missing
/// process adds nothing. Returns that hold.
fn points_held_for_later(
    lines: &[JobLine],
    order: &[usize],
    index: usize,
    factuals: &Factuals,
    rates: &HashMap<usize, f64>,
) -> f64 {
    let Some(pos) = order.iter().position(|&later| later == index) else {
        return 0.0;
    };
    let mut held = 0.0;
    for &later in order.iter().skip(pos + 1) {
        let Some(process) = factuals.get_process(lines[later].process) else {
            continue;
        };
        let Some(n) = lines[later].target else {
            continue;
        };
        if n <= 0.0 {
            continue;
        }
        let cover = lines[later].cover.max(0.0);
        let cover_runs = n.min(cover);
        if cover_runs > 0.0 {
            held += runs_point_cost(process, rates, cover_runs, cover, lines[later].cover_cheap);
        }
        // The meal is already counted. The rest of a fed paying target stays too.
        if lines[later].pays && is_line_fed(lines, later, factuals) {
            let surplus = (n - cover_runs).max(0.0);
            if surplus > 0.0 {
                held += surplus * complexity_spent(process);
            }
        }
    }
    held
}

/// # Is Line Fed
///
/// True when another line makes a required input of `lines[index]`.
///
/// `factuals` supplies the processes. A pair that feeds both ways does not
/// count. A missing process is false.
fn is_line_fed(lines: &[JobLine], index: usize, factuals: &Factuals) -> bool {
    lines.iter().enumerate().any(|(other, _)| {
        other != index
            && line_feeds(&lines[other], &lines[index], factuals)
            && !line_feeds(&lines[index], &lines[other], factuals)
    })
}

/// # Feeding Runs For Later
///
/// Runs of `lines[index]` a later paying line is still short.
///
/// `order` is the run order. `property` and `claimed` are the stock already
/// on hand. `rates` is [`subsistence_cover`]. `points` is the budget still
/// free, and the consumer's target stops at the runs that budget can start.
/// The count is the largest gap among goods this line makes, and it stops
/// at this line's target. Time and skills add nothing. A line that does
/// not pay adds nothing. Returns that count.
fn feeding_runs_for_later(
    lines: &[JobLine],
    order: &[usize],
    index: usize,
    factuals: &Factuals,
    property: &HashMap<usize, PopPRow>,
    claimed: &HashMap<usize, f64>,
    rates: &HashMap<usize, f64>,
    points: f64,
) -> f64 {
    let Some(pos) = order.iter().position(|&later| later == index) else {
        return 0.0;
    };
    let Some(made) = factuals.get_process(lines[index].process) else {
        return 0.0;
    };
    let mut need: HashMap<usize, f64> = HashMap::new();
    for &later in order.iter().skip(pos + 1) {
        if !lines[later].pays || !is_line_fed(lines, later, factuals) {
            continue;
        }
        let Some(target) = lines[later].target.filter(|n| n.is_finite() && *n > 0.0) else {
            continue;
        };
        let Some(used) = factuals.get_process(lines[later].process) else {
            continue;
        };
        let affordable = if points.is_finite() {
            runs_in_points(used, rates, points, lines[later].cover, lines[later].cover_cheap)
                .min(target)
        } else {
            target
        };
        if affordable <= 0.0 {
            continue;
        }
        for input in used.requirements() {
            if input.good == TIME || input.amount <= 0.0 {
                continue;
            }
            if factuals
                .get_good(input.good)
                .is_some_and(|good| good.is_skill())
            {
                continue;
            }
            if !made
                .outputs
                .iter()
                .any(|output| output.good == input.good && output.amount > 0.0)
            {
                continue;
            }
            *need.entry(input.good).or_insert(0.0) += input.amount * affordable;
        }
    }
    let mut runs = 0.0_f64;
    for (good, needed) in need {
        let have = free_of(property, good) + claimed.get(&good).copied().unwrap_or(0.0).max(0.0);
        let gap = (needed - have).max(0.0);
        if gap <= 0.0 {
            continue;
        }
        let Some(per) = made
            .outputs
            .iter()
            .find(|output| output.good == good && output.amount > 0.0)
            .map(|output| output.amount)
        else {
            continue;
        };
        runs = runs.max(gap / per);
    }
    match lines[index].target {
        Some(n) if n > 0.0 => runs.min(n.max(0.0)),
        Some(_) => 0.0,
        None => runs,
    }
}

/// # Complexity Held For Targets
///
/// Complexity points `lines[index]` leaves for a later line, using `targets`.
///
/// `order` is the morning order. `factuals` supplies processes. The lines
/// that count are the ones [`complexity_held_for_later`] would hold for,
/// and the points are that line's target times its complexity.
fn complexity_held_for_targets(
    lines: &[JobLine],
    order: &[usize],
    index: usize,
    targets: &[f64],
    factuals: &Factuals,
) -> f64 {
    let Some(pos) = order.iter().position(|&later| later == index) else {
        return 0.0;
    };
    let mut held = 0.0;
    for &later in order.iter().skip(pos + 1) {
        if !holds_time_for(&lines[index], &lines[later], factuals) {
            continue;
        }
        let Some(process) = factuals.get_process(lines[later].process) else {
            continue;
        };
        let n = targets.get(later).copied().unwrap_or(0.0);
        if n > 0.0 {
            held += complexity_spent(process) * n;
        }
    }
    held
}

/// # Line Feeds
///
/// True when `producer` outputs a required input of `consumer`.
///
/// `factuals` supplies both processes. A missing process is false.
fn line_feeds(producer: &JobLine, consumer: &JobLine, factuals: &Factuals) -> bool {
    let Some(made) = factuals.get_process(producer.process) else {
        return false;
    };
    let Some(used) = factuals.get_process(consumer.process) else {
        return false;
    };
    for input in used.requirements() {
        if made
            .outputs
            .iter()
            .any(|output| output.good == input.good && output.amount > 0.0)
        {
            return true;
        }
    }
    false
}

/// # Order Producers First
///
/// Moves a line to just before a later line when it makes that line's input.
///
/// `lines` is the front group. `factuals` supplies processes. A pair that
/// feeds both ways is left as it stands. The walk is bounded by the length
/// of the group, so a cycle cannot spin.
fn order_producers_first(lines: &mut Vec<JobLine>, factuals: &Factuals) {
    let passes = lines.len();
    for _ in 0..passes {
        let mut moved = false;
        let mut consumer = 0;
        while consumer < lines.len() {
            let found = (consumer + 1..lines.len()).find(|&producer| {
                line_feeds(&lines[producer], &lines[consumer], factuals)
                    && !line_feeds(&lines[consumer], &lines[producer], factuals)
            });
            if let Some(producer) = found {
                let line = lines.remove(producer);
                lines.insert(consumer, line);
                moved = true;
            }
            consumer += 1;
        }
        if !moved {
            break;
        }
    }
}

/// # Feed Bands
///
/// How many makers in this job sit ahead of each line.
///
/// `lines` is the job. `factuals` supplies the processes. A line whose
/// output is a required input of another sits one band ahead of that
/// consumer. A pair that feeds both ways stays in one band. The walk is
/// bounded by the length of the job. Returns one band per line. Upstream
/// is 0.
fn feed_bands(lines: &[JobLine], factuals: &Factuals) -> Vec<usize> {
    let mut band = vec![0usize; lines.len()];
    for _ in 0..lines.len() {
        let mut changed = false;
        for consumer in 0..lines.len() {
            for producer in 0..lines.len() {
                if producer == consumer {
                    continue;
                }
                if line_feeds(&lines[producer], &lines[consumer], factuals)
                    && !line_feeds(&lines[consumer], &lines[producer], factuals)
                {
                    let next = band[producer].saturating_add(1);
                    if next > band[consumer] {
                        band[consumer] = next;
                        changed = true;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
    band
}

/// # Priority Order
///
/// Line indexes, upstream bands first, then the higher trade score.
///
/// `lines` is the job. `factuals` supplies the processes. `history` prices
/// [`trade_score`]. [`feed_bands`] is the first key. Inside a band the
/// higher score is first. A tie keeps the lower index. The morning buys in
/// this order. Returns those indexes.
fn priority_order(lines: &[JobLine], factuals: &Factuals, history: &MarketHistory) -> Vec<usize> {
    let bands = feed_bands(lines, factuals);
    let mut order: Vec<usize> = (0..lines.len()).collect();
    order.sort_by(|&left, &right| {
        bands[left].cmp(&bands[right]).then_with(|| {
            let score = |index: usize| {
                factuals
                    .get_process(lines[index].process)
                    .map(|process| trade_score(process, history))
                    .filter(|score| score.is_finite())
                    .unwrap_or(f64::MIN)
            };
            score(right).total_cmp(&score(left)).then(left.cmp(&right))
        })
    });
    order
}

/// # Order Job Lines
///
/// Stores `lines` in [`priority_order`].
///
/// The iteration on each line stays with that line. Nothing is returned.
fn order_job_lines(lines: &mut Vec<JobLine>, factuals: &Factuals, history: &MarketHistory) {
    let order = priority_order(lines, factuals, history);
    let mut slots: Vec<Option<JobLine>> = std::mem::take(lines).into_iter().map(Some).collect();
    for index in order {
        if let Some(line) = slots[index].take() {
            lines.push(line);
        }
    }
}

/// # March Target
///
/// One night's step from `current` toward `ideal`.
///
/// The step is at most [`PLAN_MARCH`] times the larger of `current` and 1.
/// The result does not pass `ideal`. A negative result is `0`.
fn march_target(current: f64, ideal: f64) -> f64 {
    let gap = ideal - current;
    let limit = PLAN_MARCH * current.max(1.0);
    (current + gap.clamp(-limit, limit)).max(0.0)
}

/// One run's product and its return on the cost of that run.
struct RunWorth {
    amv: f64,
    roi: f64,
}

/// # Chosen Inputs
///
/// Optional goods `lines[index]` draws.
///
/// `factuals` supplies the process. `history` prices the run. Each optional
/// is judged on its own, against the same run with no optionals. It stays
/// when [`input_earns`] says the return holds. A missing process draws
/// nothing. The ids are sorted.
fn chosen_inputs(
    lines: &[JobLine],
    index: usize,
    factuals: &Factuals,
    history: &MarketHistory,
) -> Vec<usize> {
    let Some(process) = factuals.get_process(lines[index].process) else {
        return Vec::new();
    };
    let off = run_worth(process, None, lines, index, factuals, history);
    let mut kept = Vec::new();
    for input in process.optional_inputs() {
        if input.amount <= 0.0 {
            continue;
        }
        let on = run_worth(process, Some(input.good), lines, index, factuals, history);
        if input_earns(&off, &on) {
            kept.push(input.good);
        }
    }
    kept.sort_unstable();
    kept.dedup();
    kept
}

/// # Input Earns
///
/// Whether `on` is worth taking over `off`.
///
/// Both the rate and the product holding, or better, keeps it. A lower rate
/// keeps it when the product rises by at least the same fraction the rate
/// fell, and the run without it still earns a positive return. A product
/// that does not rise is dropped. A rate that is not a real number, other
/// than an unbounded return, is dropped.
fn input_earns(off: &RunWorth, on: &RunWorth) -> bool {
    if !roi_usable(on.roi) {
        return false;
    }
    let rate_holds = on.roi >= off.roi;
    let product_holds = on.amv + 1e-9 >= off.amv;
    if rate_holds && product_holds {
        return true;
    }
    if on.amv <= off.amv || !off.roi.is_finite() || off.roi <= 0.0 || off.amv <= 0.0 {
        return false;
    }
    let drop = (off.roi - on.roi) / off.roi;
    let gain = (on.amv - off.amv) / off.amv;
    gain.is_finite() && drop.is_finite() && gain + 1e-9 >= drop && gain > 0.0
}

/// # ROI Usable
///
/// True when `roi` is a real number or an unbounded return.
fn roi_usable(roi: f64) -> bool {
    roi.is_finite() || roi == f64::INFINITY
}

/// # Run Worth
///
/// Product and return for one iteration of `process`.
///
/// `enabled` is the one optional good included, or none. `lines` and `index`
/// price an output at a later line's product when that use beats holding the
/// good. `factuals` and `history` price inputs and outputs. A good that is
/// not consumed is costed at its decay. The return is product minus cost,
/// divided by cost. No cost and a positive product is an unbounded return.
fn run_worth(
    process: &Process,
    enabled: Option<usize>,
    lines: &[JobLine],
    index: usize,
    factuals: &Factuals,
    history: &MarketHistory,
) -> RunWorth {
    let (input_bonus, throughput_bonus, output_bonus, extras) = run_mods(process, enabled);
    let input_mod = input_bonus * throughput_bonus;
    let output_mod = output_bonus * throughput_bonus;
    let mut cost = 0.0;
    let mut made: HashMap<usize, f64> = HashMap::new();
    for input in process.requirements() {
        let take = if input.fixed {
            input.amount
        } else {
            input.amount * input_mod
        };
        if take <= 0.0 {
            continue;
        }
        if matches!(input.input_type, InputType::Destroyed | InputType::Consumed) {
            cost += take * input_holding(history, input.good);
        }
        if matches!(input.input_type, InputType::Consumed) {
            add_decay_yield(factuals, input.good, take, &mut made);
        }
    }
    if let Some(good) = enabled {
        if let Some(input) = process
            .optional_inputs()
            .into_iter()
            .find(|input| input.good == good)
        {
            match input.input_type {
                InputType::Destroyed | InputType::Consumed => {
                    cost += input.amount * input_holding(history, input.good);
                    if matches!(input.input_type, InputType::Consumed) {
                        add_decay_yield(factuals, input.good, input.amount, &mut made);
                    }
                }
                InputType::Capital => {
                    let decay = factuals
                        .get_good(input.good)
                        .map(|row| row.decay_rate)
                        .unwrap_or(0.0)
                        .clamp(0.0, 1.0);
                    cost += input.amount * input_holding(history, input.good) * decay;
                }
                InputType::Factor => {}
            }
        }
    }
    for output in &process.outputs {
        if output.amount <= 0.0 {
            continue;
        }
        let units = if output.fixed {
            output.amount
        } else {
            output.amount * output_mod
        };
        *made.entry(output.good).or_insert(0.0) += units;
    }
    for (good, amount) in extras {
        if amount > 0.0 {
            *made.entry(good).or_insert(0.0) += amount;
        }
    }
    let mut amv = 0.0;
    for (good, units) in &made {
        if *units <= 0.0 {
            continue;
        }
        let held = history.holding_per_unit(*good);
        let down = downstream_per_unit(lines, index, *good, factuals, history);
        amv += units * held.max(down);
    }
    let roi = if cost > 1e-12 {
        (amv - cost) / cost
    } else if amv > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };
    RunWorth { amv, roi }
}

/// # Run Mods
///
/// Bonuses and extra outputs from the one optional `enabled`.
///
/// `None` leaves every bonus at 1 and adds no extra output. The input bonus
/// is clamped to `0..=1`.
fn run_mods(process: &Process, enabled: Option<usize>) -> (f64, f64, f64, Vec<(usize, f64)>) {
    let mut input_bonus = 1.0;
    let mut throughput_bonus = 1.0;
    let mut output_bonus = 1.0;
    let mut extras = Vec::new();
    let Some(good) = enabled else {
        return (input_bonus, throughput_bonus, output_bonus, extras);
    };
    for input in process.optional_inputs() {
        if input.good != good {
            continue;
        }
        if let Some(effects) = input.optional_effects() {
            for effect in effects {
                match effect {
                    InputEffect::Throughput(value) => throughput_bonus += value,
                    InputEffect::Input(value) => input_bonus -= value,
                    InputEffect::Output(value) => output_bonus += value,
                    InputEffect::ExtraOutput(id, amount) => extras.push((*id, *amount)),
                    InputEffect::BirthRate(_) | InputEffect::MortalityRate(_, _) => {}
                }
            }
        }
        break;
    }
    input_bonus = input_bonus.clamp(0.0, 1.0);
    (input_bonus, throughput_bonus, output_bonus, extras)
}

/// # Add Decay Yield
///
/// Adds the decay products of `take` units of `good` into `made`.
///
/// A missing good adds nothing. A non-positive rate adds nothing.
fn add_decay_yield(factuals: &Factuals, good: usize, take: f64, made: &mut HashMap<usize, f64>) {
    let Some(row) = factuals.get_good(good) else {
        return;
    };
    for (&other, &rate) in &row.decay_result {
        if rate > 0.0 {
            *made.entry(other).or_insert(0.0) += take * rate;
        }
    }
}

/// # Downstream Per Unit
///
/// Holding one unit of `good` is worth when another line on `lines` consumes it.
///
/// `index` is the line being scored and is skipped. The worth is that line's
/// own output holding divided by the input amount. The best such line wins.
/// No consumer returns 0.
fn downstream_per_unit(
    lines: &[JobLine],
    index: usize,
    good: usize,
    factuals: &Factuals,
    history: &MarketHistory,
) -> f64 {
    let mut best = 0.0;
    for (other, line) in lines.iter().enumerate() {
        if other == index {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        for input in process.requirements() {
            if input.good != good || input.amount <= 0.0 {
                continue;
            }
            let per = direct_output_holding(process, history) / input.amount;
            if per > best {
                best = per;
            }
        }
    }
    best
}

/// # Direct Output Holding
///
/// Holding of one run of `process` with no optional bonus.
///
/// `history` prices each output. A non-positive amount adds nothing.
fn direct_output_holding(process: &Process, history: &MarketHistory) -> f64 {
    let mut sum = 0.0;
    for output in &process.outputs {
        if output.amount > 0.0 {
            sum += output.amount * history.holding_per_unit(output.good);
        }
    }
    sum
}

/// # Trade Score
///
/// Holding gained by running `process` once, on `history`'s board.
///
/// Output holding is added. [`destroyed_input_holding`] and
/// [`complexity_holding`] are subtracted. Returns the net.
pub fn trade_score(process: &Process, history: &MarketHistory) -> f64 {
    direct_output_holding(process, history)
        - destroyed_input_holding(process, history)
        - complexity_holding(process)
}

/// # Break Even AMV
///
/// The stored AMV of `good` at which one run of `process` scores zero.
///
/// `history` prices every other good. `salability` scales a positive ask
/// through [`amv_scale`], the same way [`MarketHistory::holding_per_unit`]
/// does. Output of `good` raises the score as the ask rises. A required
/// input of `good` lowers it. Optional inputs, factors, and capital stay
/// out, matching [`trade_score`]. A process that does not gain when this
/// ask rises returns `None`. A process that already pays at every positive
/// ask returns 0. Returns that ask.
pub(crate) fn break_even_amv(
    process: &Process,
    good: usize,
    history: &MarketHistory,
    salability: f64,
) -> Option<f64> {
    let scale = amv_scale(salability);
    if scale <= 0.0 {
        return None;
    }
    let mut net = 0.0;
    let mut rest = -complexity_holding(process);
    for output in &process.outputs {
        if output.amount <= 0.0 {
            continue;
        }
        if output.good == good {
            net += output.amount;
        } else {
            rest += output.amount * history.holding_per_unit(output.good);
        }
    }
    for input in &process.inputs {
        if input.is_optional() {
            continue;
        }
        if !matches!(input.input_type, InputType::Destroyed | InputType::Consumed) {
            continue;
        }
        if input.good == good {
            net -= input.amount;
        } else {
            rest -= input.amount * input_holding(history, input.good);
        }
    }
    // A higher ask does not make this run pay.
    if net <= 0.0 {
        return None;
    }
    if rest >= 0.0 {
        return Some(0.0);
    }
    let amv = (-rest / net) / scale;
    amv.is_finite().then_some(amv.max(0.0))
}

/// # Trade Rate
///
/// [`trade_score`] of one run divided by the absolute input holding.
///
/// `process` is the recipe. `history` prices it. The gain has already
/// subtracted the complexity cost. Spending inputs worth -100 and making
/// outputs worth 100, at complexity 1, returns 1.99: the run gains 199 on
/// a base of 100. A base of 0, or a rate that is not finite, returns `None`.
pub fn trade_rate(process: &Process, history: &MarketHistory) -> Option<f64> {
    let base = destroyed_input_holding(process, history);
    if !base.is_finite() || base == 0.0 {
        return None;
    }
    let rate = trade_score(process, history) / base.abs();
    rate.is_finite().then_some(rate)
}

/// # Profit Pull
///
/// Share of the unfunded ceiling a positive trade rate will try to run.
///
/// `rate` is [`trade_rate`]. Zero and a rate that is not positive return 0.
/// A positive rate returns `rate / (1 + rate)`. A thin profit takes a small
/// share of the gap. A large one takes nearly all of it. Returns that share.
fn profit_pull(rate: f64) -> f64 {
    if !rate.is_finite() || rate <= 0.0 {
        0.0
    } else {
        rate / (1.0 + rate)
    }
}

/// # Pushed Runs
///
/// Runs of `process` the pile will fund, including a currency input the purse buys.
///
/// `ceiling` is the time cap. `hand` is working stock plus input hold.
/// The stocked count is what [`funded_runs`] can fund without that
/// purchase. [`profit_pull`] of the gap above it is the aim, and that aim
/// may pay other tender for the short currency. A rate that is not
/// positive returns the stocked count. Returns the larger count.
fn pushed_runs(
    process: &Process,
    ceiling: f64,
    hand: &HashMap<usize, f64>,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
) -> f64 {
    let stocked = funded_runs(process, ceiling, hand, factuals, history, currencies, false);
    let Some(rate) = trade_rate(process, history) else {
        return stocked;
    };
    let pull = profit_pull(rate);
    if pull <= 0.0 || !ceiling.is_finite() {
        return stocked;
    }
    let aim = stocked + (ceiling - stocked).max(0.0) * pull;
    if aim <= stocked + 1e-9 {
        return stocked;
    }
    stocked.max(funded_runs(
        process, aim, hand, factuals, history, currencies, true,
    ))
}

/// # Destroyed Input Holding
///
/// Holding a run of `process` spends on inputs it destroys or consumes.
///
/// `history` prices each input through [`input_holding`]. An optional
/// input, a factor, and capital stay out. A non-positive amount still
/// counts. Returns that holding.
fn destroyed_input_holding(process: &Process, history: &MarketHistory) -> f64 {
    let mut sum = 0.0;
    for input in &process.inputs {
        if input.is_optional() {
            continue;
        }
        if !matches!(input.input_type, InputType::Destroyed | InputType::Consumed) {
            continue;
        }
        sum += input.amount * input_holding(history, input.good);
    }
    sum
}

/// # Input Holding
///
/// Holding one unit of `good` costs in a recipe.
///
/// Every good uses [`MarketHistory::holding_per_unit`], including one
/// that does not survive the night. Returns that holding.
fn input_holding(history: &MarketHistory, good: usize) -> f64 {
    history.holding_per_unit(good)
}

/// # Complexity Holding
///
/// Cost of one run's complexity, in the same figure as holding.
///
/// `process` supplies [`complexity_spent`]. That many points are subtracted
/// once per run. A process that spends nothing costs nothing. Returns that cost.
fn complexity_holding(process: &Process) -> f64 {
    complexity_spent(process)
}

/// # Paced Surplus
///
/// Extra runs left after rot and output still on hand pull `raw` back
/// toward the output just made.
///
/// `process` supplies the outputs. `property` is the pop's rows.
/// `factuals` tells a skill output from the product. `raw` is the
/// input-limited surplus. A skill output is skipped. An output with
/// `amount <= 0`, or a missing row, does not change `raw`.
///
/// `produced > 0` sets the room from `(lost + kept) / produced`. `kept` is
/// `quantity` clamped to `0..=produced`. At [`SURPLUS_ROT_SHARE`] the room
/// is one, so the extra stays at that output's runs. A smaller share raises
/// the room, up to twice the runs when nothing rotted and nothing of that
/// output is left on hand. A larger share lowers it by
/// `SURPLUS_ROT_SHARE / share`. `produced == 0` and `lost > 0` leaves one
/// run. The tightest output wins. Returns that many runs, never negative.
fn paced_surplus(
    process: &Process,
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
    raw: f64,
) -> f64 {
    if raw <= 0.0 {
        return 0.0;
    }
    let mut paced = raw;
    for output in &process.outputs {
        if output.amount <= 0.0 {
            continue;
        }
        // A skill is practice, not the product whose stock paces the line.
        if factuals
            .get_good(output.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let Some(row) = property.get(&output.good) else {
            continue;
        };
        if row.produced > 0.0 {
            let kept = row.quantity.clamp(0.0, row.produced);
            let share = (row.lost + kept) / row.produced;
            let last = row.produced / output.amount;
            let room = if share > SURPLUS_ROT_SHARE {
                SURPLUS_ROT_SHARE / share
            } else {
                1.0 + (SURPLUS_ROT_SHARE - share) / SURPLUS_ROT_SHARE
            };
            paced = paced.min(last * room);
        } else if row.lost > 0.0 {
            paced = paced.min(1.0);
        }
    }
    paced.max(0.0)
}

/// # Pay Per Time
///
/// `score` earned per unit of time on the run.
///
/// `time` is the required time. A positive time divides the score. No
/// time leaves the score as it is. Returns that rate.
fn pay_per_time(score: f64, time: f64) -> f64 {
    if time > 0.0 { score / time } else { score }
}

/// # Line Rate
///
/// [`pay_per_time`] of one line, when that line pays.
///
/// `scores[index]` is the [`trade_score`]. `lines` and `factuals` supply
/// the time. A missing score, a score that is not finite, and a score at
/// or below zero return `None`.
fn line_rate(
    lines: &[JobLine],
    index: usize,
    scores: &[Option<f64>],
    factuals: &Factuals,
) -> Option<f64> {
    let score = scores.get(index).copied().flatten()?;
    if !score.is_finite() || score <= 0.0 {
        return None;
    }
    let time = lines
        .get(index)
        .and_then(|line| factuals.get_process(line.process))
        .map(time_spent)
        .unwrap_or(0.0);
    Some(pay_per_time(score, time))
}

/// # Ranked Pay
///
/// [`pay_per_time`] of one line, for ordering a weaker line first.
///
/// A missing or non-finite score is [`f64::NEG_INFINITY`]. A score at or
/// below zero still ranks, so a loss gives its time up before a gain.
fn ranked_pay(lines: &[JobLine], index: usize, scores: &[Option<f64>], factuals: &Factuals) -> f64 {
    let Some(score) = scores.get(index).copied().flatten() else {
        return f64::NEG_INFINITY;
    };
    if !score.is_finite() {
        return f64::NEG_INFINITY;
    }
    let time = lines
        .get(index)
        .and_then(|line| factuals.get_process(line.process))
        .map(time_spent)
        .unwrap_or(0.0);
    pay_per_time(score, time)
}

/// # Best Paying Line
///
/// Index of the strongest positive [`pay_per_time`] in `scores`.
///
/// `lines` and `factuals` supply the time on each run. A missing score,
/// a score that is not finite, and a score at or below zero are skipped.
/// A tie keeps the earlier line. Returns `None` when no line qualifies.
fn best_paying_line(
    lines: &[JobLine],
    scores: &[Option<f64>],
    factuals: &Factuals,
) -> Option<usize> {
    let mut found: Option<(usize, f64)> = None;
    for index in 0..scores.len() {
        let Some(rate) = line_rate(lines, index, scores, factuals) else {
            continue;
        };
        if found.is_none_or(|(_, best)| rate > best) {
            found = Some((index, rate));
        }
    }
    found.map(|(index, _)| index)
}

/// # Add Required
///
/// Puts the required inputs of `runs` back into `remaining`.
///
/// `process` supplies the inputs. Factors and optional inputs stay out.
/// An input with `amount <= 0`, or `runs <= 0`, adds nothing.
fn add_required(remaining: &mut HashMap<usize, f64>, process: &Process, runs: f64) {
    if runs <= 0.0 {
        return;
    }
    for input in process.requirements() {
        if input.amount <= 0.0 {
            continue;
        }
        *remaining.entry(input.good).or_insert(0.0) += input.amount * runs;
    }
}

/// # Take Required
///
/// Removes the required inputs of `runs` from `remaining`.
///
/// `process` supplies the inputs. Factors and optional inputs stay out.
/// A good that is used up is dropped. The pool does not go negative.
fn take_required(remaining: &mut HashMap<usize, f64>, process: &Process, runs: f64) {
    if runs <= 0.0 {
        return;
    }
    for input in process.requirements() {
        if input.amount <= 0.0 {
            continue;
        }
        let have = remaining.get(&input.good).copied().unwrap_or(0.0);
        let left = (have - input.amount * runs).max(0.0);
        if left > 0.0 {
            remaining.insert(input.good, left);
        } else {
            remaining.remove(&input.good);
        }
    }
}

/// # Donor Runs Spent
///
/// How many of the donor's offered runs the gain actually used.
///
/// `donor` and `best` are the two processes. `pool` is the input pool
/// before the offer went back. `give` is the runs offered. `gained` is
/// the extra runs the strongest line takes.
///
/// Each required input of `donor` counts the part of `best`'s new need
/// that `pool` could not cover, capped at the amount `give` put back.
/// The donor loses the largest of those run counts, and not more than
/// `give`. Returns 0 when the gain used none of the offer.
fn donor_runs_spent(
    donor: &Process,
    best: &Process,
    pool: &HashMap<usize, f64>,
    give: f64,
    gained: f64,
) -> f64 {
    let mut runs_lost: f64 = 0.0;
    for input in donor.requirements() {
        if input.amount <= 0.0 {
            continue;
        }
        let gifted = input.amount * give;
        let need: f64 = best
            .requirements()
            .into_iter()
            .filter(|required| required.good == input.good)
            .map(|required| required.amount)
            .sum::<f64>()
            * gained;
        let had = pool.get(&input.good).copied().unwrap_or(0.0);
        let from_gift = (need - had).max(0.0).min(gifted);
        runs_lost = runs_lost.max(from_gift / input.amount);
    }
    runs_lost.min(give).max(0.0)
}

/// # Nudge Surplus
///
/// Moves a fraction of a weaker line's surplus onto the strongest line.
///
/// `lines` and `scores` are this job's processes and their [`trade_score`]s.
/// `extras` is the surplus already taken, above cover. `remaining` is the
/// input pool those runs have not spent. `property` paces the strongest
/// line through [`paced_surplus`]. `factuals` supplies the processes.
///
/// [`best_paying_line`] picks the destination. Each other line with surplus
/// and a strictly lower [`pay_per_time`] offers [`PROFIT_NUDGE`] of that
/// surplus, weakest first. The offer goes back into `remaining`. The destination
/// takes only the extra runs its pace still allows. [`donor_runs_spent`]
/// is how many of the donor's runs that gain used, and the donor loses
/// those. An offer the destination cannot run is put back.
fn nudge_surplus(
    lines: &[JobLine],
    scores: &[Option<f64>],
    extras: &mut [f64],
    remaining: &mut HashMap<usize, f64>,
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
) {
    let Some(best) = best_paying_line(lines, scores, factuals) else {
        return;
    };
    let Some(best_rate) = line_rate(lines, best, scores, factuals) else {
        return;
    };
    let Some(best_process) = factuals.get_process(lines[best].process) else {
        return;
    };
    let mut donors: Vec<usize> = (0..lines.len())
        .filter(|&index| {
            index != best
                && extras.get(index).is_some_and(|extra| *extra > 0.0)
                && line_rate(lines, index, scores, factuals).is_some_and(|rate| rate < best_rate)
        })
        .collect();
    // The weakest offer lands first, while the destination still has room.
    donors.sort_by(|&left, &right| {
        let left_rate = line_rate(lines, left, scores, factuals).unwrap_or(0.0);
        let right_rate = line_rate(lines, right, scores, factuals).unwrap_or(0.0);
        left_rate
            .partial_cmp(&right_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.cmp(&right))
    });
    for donor in donors {
        let Some(donor_process) = factuals.get_process(lines[donor].process) else {
            continue;
        };
        let give = extras[donor] * PROFIT_NUDGE;
        if !(give > 0.0) {
            continue;
        }
        let pool = remaining.clone();
        let before = surplus_iterations(best_process, remaining);
        add_required(remaining, donor_process, give);
        let enabled = (surplus_iterations(best_process, remaining) - before).max(0.0);
        let room = paced_surplus(best_process, property, factuals, extras[best] + enabled);
        let gained = (room - extras[best]).max(0.0).min(enabled);
        let spent = donor_runs_spent(donor_process, best_process, &pool, give, gained);
        if !(gained > 0.0) || !(spent > 0.0) {
            *remaining = pool;
            continue;
        }
        // The unspent offer stays on the donor. Drop it from the pool.
        *remaining = pool;
        take_required(remaining, best_process, gained);
        extras[donor] = (extras[donor] - spent).max(0.0);
        extras[best] += gained;
    }
}

/// # Last Product Runs
///
/// Runs of `process` that yesterday's product output implies.
///
/// `property` is the pop's rows. `factuals` tells a skill from a product.
/// Each non-skill output with `amount > 0` and `produced > 0` counts
/// `produced / amount`. A skill is skipped. No such output returns 0.
fn last_product_runs(
    process: &Process,
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
) -> f64 {
    let mut last: f64 = 0.0;
    for output in &process.outputs {
        if output.amount <= 0.0 {
            continue;
        }
        if factuals
            .get_good(output.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let Some(row) = property.get(&output.good) else {
            continue;
        };
        if row.produced > 0.0 {
            last = last.max(row.produced / output.amount);
        }
    }
    last
}

/// # Profit Floor
///
/// The fewest runs the strongest paying line is planned at.
///
/// `process`, `property`, and `factuals` are the line and yesterday's rows.
/// The floor is one run, or [`last_product_runs`] when that is larger.
/// Returns that many runs.
fn profit_floor(process: &Process, property: &HashMap<usize, PopPRow>, factuals: &Factuals) -> f64 {
    last_product_runs(process, property, factuals).max(1.0)
}

/// # Feeder Line
///
/// The line, other than `except`, that makes the most of `good` per time.
///
/// `lines` is the job. `factuals` supplies processes. A skill is not a
/// feeder. The highest [`yield_per_time`] wins, and a tie keeps the earlier
/// line. Returns `None` when no line qualifies.
fn feeder_line(
    lines: &[JobLine],
    good: usize,
    except: usize,
    factuals: &Factuals,
) -> Option<usize> {
    if factuals.get_good(good).is_some_and(|row| row.is_skill()) {
        return None;
    }
    let mut found: Option<(usize, f64)> = None;
    for (index, line) in lines.iter().enumerate() {
        if index == except {
            continue;
        }
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let amount = process
            .outputs
            .iter()
            .find(|output| output.good == good)
            .map(|output| output.amount)
            .unwrap_or(0.0);
        if amount <= 0.0 {
            continue;
        }
        let pace = yield_per_time(process, good);
        if pace <= 0.0 {
            continue;
        }
        if found.is_none_or(|(_, best)| pace > best) {
            found = Some((index, pace));
        }
    }
    found.map(|(index, _)| index)
}

/// # Outputs Required Material
///
/// True when `made` outputs a required non-time input of `used`.
///
/// A skill input does not count. `factuals` tags skills.
fn outputs_required_material(made: &Process, used: &Process, factuals: &Factuals) -> bool {
    used.requirements().iter().any(|input| {
        input.good != TIME
            && input.amount > 0.0
            && !factuals
                .get_good(input.good)
                .is_some_and(|good| good.is_skill())
            && made
                .outputs
                .iter()
                .any(|output| output.good == input.good && output.amount > 0.0)
    })
}

/// # Feeder Runs For
///
/// Extra runs each feeder must make so `runs` of the strongest line can start.
///
/// `feeders` is `(line, good, amount, per, pace)`. `had` is stock already
/// in the pool. An input that stock covers adds nothing. Two inputs from
/// the same feeder take the larger run count. Returns line index to runs.
fn feeder_runs_for(
    feeders: &[(usize, usize, f64, f64, f64)],
    had: &HashMap<usize, f64>,
    runs: f64,
) -> HashMap<usize, f64> {
    let mut by_line: HashMap<usize, f64> = HashMap::new();
    if runs <= 0.0 {
        return by_line;
    }
    for &(line, good, amount, per, _) in feeders {
        if per <= 0.0 {
            continue;
        }
        let have = had.get(&good).copied().unwrap_or(0.0);
        let missing = (amount * runs - have).max(0.0);
        let need = missing / per;
        let entry = by_line.entry(line).or_insert(0.0);
        if need > *entry {
            *entry = need;
        }
    }
    by_line
}

/// # Press Run Time
///
/// Time `runs` of the strongest line need, including feeders for what is short.
///
/// `best_time` is that line's time per run. `feeders` and `had` match
/// [`feeder_runs_for`]. A feeder with no time input adds none. Returns
/// the total.
fn press_run_time(
    best_time: f64,
    feeders: &[(usize, usize, f64, f64, f64)],
    had: &HashMap<usize, f64>,
    runs: f64,
) -> f64 {
    if runs <= 0.0 {
        return 0.0;
    }
    let mut cost = best_time * runs;
    let needed = feeder_runs_for(feeders, had, runs);
    for (&line, &need) in &needed {
        let pace = feeders
            .iter()
            .find(|row| row.0 == line)
            .map(|row| row.4)
            .unwrap_or(0.0);
        if pace > 0.0 && need > 0.0 {
            cost += need * pace;
        }
    }
    cost
}

/// # Release Runs
///
/// Takes `runs` off `extras` first, then off `cover`.
///
/// Neither goes below 0. Returns the runs actually taken.
fn release_runs(cover: &mut f64, extras: &mut f64, runs: f64) -> f64 {
    if runs <= 0.0 {
        return 0.0;
    }
    let from_extra = runs.min((*extras).max(0.0));
    *extras -= from_extra;
    let from_cover = (runs - from_extra).min((*cover).max(0.0));
    *cover -= from_cover;
    from_extra + from_cover
}

/// # Press Profit
///
/// Keeps the strongest paying line up at [`profit_floor`].
///
/// `lines` and `scores` are the job. `cover` and `extras` are this night's
/// runs. `sought` is the unmet-buy extra and is not changed. `remaining`
/// is the input pool. `property` is yesterday's rows. `factuals` supplies
/// the processes.
///
/// [`best_paying_line`] picks the line. A line already at the floor is
/// left alone. The shortfall is funded from `remaining`, from one pass of
/// the line that makes a missing input, and from at most [`PROFIT_NUDGE`]
/// of the time on lines that do not make that input. The weakest of those
/// gives time up first, surplus before cover. The feeder's cover grows
/// only for the units still short. The strongest line takes the runs that
/// time can finish, on its surplus. What the pool cannot fund stays unplanned.
fn press_profit(
    lines: &[JobLine],
    scores: &[Option<f64>],
    cover: &mut [f64],
    extras: &mut [f64],
    sought: &[f64],
    remaining: &mut HashMap<usize, f64>,
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
) {
    let Some(best) = best_paying_line(lines, scores, factuals) else {
        return;
    };
    let Some(best_process) = factuals.get_process(lines[best].process) else {
        return;
    };
    let have = cover.get(best).copied().unwrap_or(0.0)
        + extras.get(best).copied().unwrap_or(0.0)
        + sought.get(best).copied().unwrap_or(0.0);
    let floor = profit_floor(best_process, property, factuals);
    if have >= floor {
        return;
    }
    let add = floor - have;

    // One feeder per required material. Time and skills are not fed this way.
    let mut needed: HashMap<usize, f64> = HashMap::new();
    for input in best_process.requirements() {
        if input.good == TIME || input.amount <= 0.0 {
            continue;
        }
        if factuals
            .get_good(input.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        *needed.entry(input.good).or_insert(0.0) += input.amount;
    }
    // `(line, good, amount, per, pace)`.
    let mut feeders: Vec<(usize, usize, f64, f64, f64)> = Vec::new();
    for (good, amount) in needed {
        let Some(feeder) = feeder_line(lines, good, best, factuals) else {
            continue;
        };
        let Some(made) = factuals.get_process(lines[feeder].process) else {
            continue;
        };
        let per = made
            .outputs
            .iter()
            .find(|output| output.good == good)
            .map(|output| output.amount)
            .unwrap_or(0.0);
        if per <= 0.0 {
            continue;
        }
        feeders.push((feeder, good, amount, per, time_spent(made)));
    }

    let mut competitors: Vec<usize> = (0..lines.len())
        .filter(|&index| {
            if index == best {
                return false;
            }
            let Some(made) = factuals.get_process(lines[index].process) else {
                return false;
            };
            !outputs_required_material(made, best_process, factuals)
        })
        .collect();
    // The weakest pay per time gives its time up first.
    competitors.sort_by(|&left, &right| {
        ranked_pay(lines, left, scores, factuals)
            .partial_cmp(&ranked_pay(lines, right, scores, factuals))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.cmp(&right))
    });

    let mut skim_time = 0.0;
    for &index in &competitors {
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let pace = time_spent(process);
        if pace <= 0.0 {
            continue;
        }
        let runs =
            cover.get(index).copied().unwrap_or(0.0) + extras.get(index).copied().unwrap_or(0.0);
        skim_time += PROFIT_NUDGE * runs * pace;
    }

    let pool_time = remaining.get(&TIME).copied().unwrap_or(0.0).max(0.0);
    let had = remaining.clone();
    // Materials a feeder can make are not a limit. Time is priced apart.
    let mut pool = remaining.clone();
    for &(_, good, amount, _, _) in &feeders {
        *pool.entry(good).or_insert(0.0) += amount * add;
    }
    pool.insert(TIME, 1.0e12);
    let material_runs = surplus_iterations(best_process, &pool);
    let best_time = time_spent(best_process);
    let budget = pool_time + skim_time;
    let time_cap = add.min(material_runs.max(0.0));
    let time_runs = if press_run_time(best_time, &feeders, &had, time_cap) <= budget {
        time_cap
    } else {
        let mut lo = 0.0;
        let mut hi = time_cap;
        for _ in 0..48 {
            let mid = (lo + hi) * 0.5;
            if press_run_time(best_time, &feeders, &had, mid) <= budget {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    };
    let funded = time_runs.max(0.0);
    if !(funded > 0.0) {
        return;
    }

    // The feeder covers only the units the pool does not already hold.
    let grow = feeder_runs_for(&feeders, &had, funded);
    for (&line, &need) in &grow {
        if need > 0.0 {
            cover[line] += need;
        }
    }
    let time_needed = (press_run_time(best_time, &feeders, &had, funded) - pool_time).max(0.0);
    let mut left = time_needed;
    for &index in &competitors {
        if left <= 0.0 {
            break;
        }
        let Some(process) = factuals.get_process(lines[index].process) else {
            continue;
        };
        let pace = time_spent(process);
        if pace <= 0.0 {
            continue;
        }
        let runs = cover[index] + extras[index];
        let offer = PROFIT_NUDGE * runs;
        let take = (left / pace).min(offer);
        let taken = release_runs(&mut cover[index], &mut extras[index], take);
        left -= taken * pace;
    }
    extras[best] += funded;

    // Spend stock the strongest line uses now. A feeder's future output stays out.
    for input in best_process.requirements() {
        if input.good == TIME || input.amount <= 0.0 {
            continue;
        }
        let have_input = remaining.get(&input.good).copied().unwrap_or(0.0);
        let used = (input.amount * funded).min(have_input.max(0.0));
        let left_input = have_input - used;
        if left_input > 0.0 {
            remaining.insert(input.good, left_input);
        } else {
            remaining.remove(&input.good);
        }
    }
    let time_use = press_run_time(best_time, &feeders, &had, funded).min(pool_time);
    let time_left = (pool_time - time_use).max(0.0);
    if time_left > 0.0 {
        remaining.insert(TIME, time_left);
    } else {
        remaining.remove(&TIME);
    }
}

/// # Pace Sale
///
/// Surplus runs that follow what sold, per line.
///
/// `property` is the pop's rows, `produced` included. `factuals` supplies
/// processes and decay. `history` is the card. The pile and the units sold
/// come from [`Job::note_sale`].
///
/// A line with no product, and a product this morning did not make, is
/// `None`, so the march stays. Every other line that makes the good shares
/// [`sale_runs`]. Returns that surplus.
fn pace_sale(
    job: &Job,
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
    history: &MarketHistory,
) -> Vec<Option<f64>> {
    let mut surplus = vec![None; job.lines.len()];
    let mut decided: HashMap<usize, Option<f64>> = HashMap::new();
    for (index, line) in job.lines.iter().enumerate() {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let Some((good, amount)) = sale_product(process, factuals) else {
            continue;
        };
        if amount <= 0.0 {
            continue;
        }
        let made = property
            .get(&good)
            .map(|row| row.produced.max(0.0))
            .unwrap_or(0.0);
        if made <= 0.0 {
            continue;
        }
        let runs = if let Some(done) = decided.get(&good) {
            *done
        } else {
            let next = sale_runs(job, good, amount, factuals, history);
            decided.insert(good, next);
            next
        };
        surplus[index] = runs;
    }
    surplus
}

/// # Sale Runs
///
/// Surplus runs of `good` above the cover, from what sold.
///
/// `amount` is units of `good` one run makes. `factuals` supplies decay.
/// `history` is the card. The pile and the sale come from [`Job::note_sale`].
/// A pile under one unit returns `None`. A card [`card_covers_cost`] rejects
/// returns zero runs. The offer is the listing from [`Job::note_listed`]
/// when that good was stored, and the pile when it was not. An offer under
/// one unit returns `None`. A card above [`lowest_make_cost`] that sold
/// that offer returns `None`. Any other accepted card returns the units
/// sold divided by `amount`. A missing cost counts as zero.
fn sale_runs(
    job: &Job,
    good: usize,
    amount: f64,
    factuals: &Factuals,
    history: &MarketHistory,
) -> Option<f64> {
    let pile = job.sale_pile.get(&good).copied().unwrap_or(0.0).max(0.0);
    if pile < 1.0 {
        return None;
    }
    let sold = job.sale_sold.get(&good).copied().unwrap_or(0.0).max(0.0);
    let cost = lowest_make_cost(job, good, factuals, history).unwrap_or(0.0);
    let card = history.price(good);
    if !card_covers_cost(cost, card, listed_decay(factuals, good)) {
        return Some(0.0);
    }
    let offer = job
        .sale_listed
        .get(&good)
        .copied()
        .map(|units| units.max(0.0))
        .unwrap_or(pile);
    if offer < 1.0 {
        return None;
    }
    let sold_out = sold + 1e-9 >= offer;
    if card > cost && sold_out {
        return None;
    }
    Some((sold / amount).max(0.0))
}

/// # Lowest Make Cost
///
/// The smallest finite [`make_cost_per_unit`] of `good` on this job.
///
/// `factuals` supplies the processes. `history` prices the inputs. A line
/// that does not make `good`, and a non-finite cost, are skipped. No
/// remaining cost returns `None`.
fn lowest_make_cost(
    job: &Job,
    good: usize,
    factuals: &Factuals,
    history: &MarketHistory,
) -> Option<f64> {
    let mut best: Option<f64> = None;
    for line in &job.lines {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        let Some(cost) = make_cost_per_unit(process, good, history) else {
            continue;
        };
        if !cost.is_finite() {
            continue;
        }
        best = Some(best.map(|had| had.min(cost)).unwrap_or(cost));
    }
    best
}

/// # Sale Product
///
/// The product output [`pace_sale`] measures, and its amount per run.
///
/// Skill outputs are skipped. The largest amount wins, and a tie keeps
/// the lower good id. A process with no product output returns `None`.
fn sale_product(process: &Process, factuals: &Factuals) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for output in &process.outputs {
        if output.amount <= 0.0 {
            continue;
        }
        if factuals
            .get_good(output.good)
            .is_some_and(|good| good.is_skill())
        {
            continue;
        }
        let replace = match best {
            None => true,
            Some((id, amount)) => {
                output.amount > amount || (output.amount == amount && output.good < id)
            }
        };
        if replace {
            best = Some((output.good, output.amount));
        }
    }
    best
}

/// # Surplus Iterations
///
/// How many extra runs `process` can take from `remaining` inputs.
///
/// Required inputs limit it to the minimum of `remaining / amount`.
/// Factors and optional inputs do not. A process with no required input
/// returns one run. Returns `0` when a required input is missing.
fn surplus_iterations(process: &Process, remaining: &HashMap<usize, f64>) -> f64 {
    let requirements = process.requirements();
    if requirements.is_empty() {
        return 1.0;
    }
    let mut iterations = f64::INFINITY;
    for input in &requirements {
        if input.amount <= 0.0 {
            continue;
        }
        let have = remaining.get(&input.good).copied().unwrap_or(0.0);
        iterations = iterations.min(have / input.amount);
    }
    if iterations.is_finite() {
        iterations.max(0.0)
    } else {
        1.0
    }
}

/// # Process Goods
///
/// Input and output good ids of `process`, or `None` when the world has
/// no such process.
fn process_goods(factuals: &Factuals, process: usize) -> Option<HashSet<usize>> {
    let process = factuals.get_process(process)?;
    let mut goods = HashSet::new();
    for input in &process.inputs {
        goods.insert(input.good);
    }
    for output in &process.outputs {
        goods.insert(output.good);
    }
    Some(goods)
}

/// # Survives Night
///
/// True when some of `good` is still there after decay.
///
/// A good missing from `factuals` does not decay. Decay of 1 or more
/// leaves nothing, so that good is not bought and does not count as an
/// input to a desire.
fn survives_night(factuals: &Factuals, good: usize) -> bool {
    let decay = factuals
        .get_good(good)
        .map(|row| row.decay_rate)
        .unwrap_or(0.0);
    (1.0 - decay) > 0.0
}

/// # Night Durability
///
/// Share of `good` still on hand after decay.
///
/// `None` when the good decays completely, so nothing bought today would
/// remain. A good missing from `factuals` does not decay, and this returns 1.
fn night_durability(factuals: &Factuals, good: usize) -> Option<f64> {
    if !survives_night(factuals, good) {
        return None;
    }
    let decay = factuals
        .get_good(good)
        .map(|row| row.decay_rate)
        .unwrap_or(0.0);
    let durability = (1.0 - decay).clamp(0.0, 1.0);
    if durability <= 0.0 {
        None
    } else {
        Some(durability)
    }
}

/// # Next Stock
///
/// Stock of `good` the next morning's claim can still use.
///
/// Free stock is what is on hand and not reserved. A desire's reserve is
/// left out. When `capital` is set, [`PopPRow::used`] is added, because
/// that pile returns at decay.
fn next_stock(property: &HashMap<usize, PopPRow>, good: usize, capital: bool) -> f64 {
    let Some(row) = property.get(&good) else {
        return 0.0;
    };
    let free = row.available().max(0.0);
    if capital {
        free + row.used.max(0.0)
    } else {
        free
    }
}

/// # Free Of
///
/// Units of `good` that are on hand and not reserved.
///
/// Missing rows and a non-positive available amount are `0`.
fn free_of(property: &HashMap<usize, PopPRow>, good: usize) -> f64 {
    property
        .get(&good)
        .map(|row| row.available().max(0.0))
        .unwrap_or(0.0)
}

/// # Held
///
/// Quantity of `good` on hand, reserve included.
///
/// A missing row is `0`. Used for factors, where any held amount covers the run.
fn held(property: &HashMap<usize, PopPRow>, good: usize) -> f64 {
    property
        .get(&good)
        .map(|row| row.quantity.max(0.0))
        .unwrap_or(0.0)
}

/// # Claim
///
/// Moves up to `amount` of free `good` into `reserved` and `claimed`.
///
/// Does nothing when `amount` is not positive or nothing is free. The take
/// cannot pass the free stock.
fn claim(
    property: &mut HashMap<usize, PopPRow>,
    claimed: &mut HashMap<usize, f64>,
    good: usize,
    amount: f64,
) {
    if amount <= 0.0 {
        return;
    }
    let Some(row) = property.get_mut(&good) else {
        return;
    };
    let take = amount.min(row.available().max(0.0));
    if take <= 0.0 {
        return;
    }
    row.reserved += take;
    *claimed.entry(good).or_insert(0.0) += take;
}

/// # Inputs Bound
///
/// True when the map holds a positive amount of some non-factor input.
///
/// An unlimited run with nothing bounding it would not stop. Callers skip
/// that run instead of passing an open target.
fn inputs_bound(process: &Process, inputs: &HashMap<usize, f64>) -> bool {
    process
        .requirements()
        .iter()
        .chain(process.optional_inputs().iter())
        .any(|input| inputs.get(&input.good).copied().unwrap_or(0.0) > 0.0 && input.amount > 0.0)
}

#[cfg(test)]
mod job {
    use std::collections::{HashMap, HashSet};

    use crate::game::actor::Actor;
    use crate::game::config::TIME_PER_LABOR;
    use crate::game::craft::Craft;
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag};
    use crate::game::job::{Job, JobLine};
    use crate::game::market::MarketHistory;
    use crate::game::pop::Pop;
    use crate::game::pop_property::PopPRow;
    use crate::game::process::{
        InputEffect, InputType, Process, ProcessEffect, ProcessInput, ProcessOutput,
    };

    fn good(id: usize, decay_rate: f64) -> Good {
        Good {
            id,
            name: format!("good {id}"),
            class: None,
            decay_rate,
            decay_result: HashMap::new(),
            mass: 0.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: Vec::new(),
        }
    }

    fn skill_good(id: usize) -> Good {
        let mut row = good(id, 0.1);
        row.tags.insert(GoodTag::Skill);
        row.tags.insert(GoodTag::Untradeable);
        row
    }

    /// Bake that spends 1 grain and optionally 1 of skill 40.
    ///
    /// This helper returns 1.1 of that skill per 1 fed, and a fixed 0.01 a run.
    fn practiced_bake() -> Process {
        Process::new(7, "bake", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_input(
                ProcessInput::new(40, 1.0, true, InputType::Destroyed, true)
                    .with_optional(InputEffect::ExtraOutput(40, 1.1)),
            )
            .with_output(ProcessOutput::new(2, 1.0, true))
            .with_output(ProcessOutput::new(40, 0.01, true))
    }

    fn bake() -> Process {
        Process::new(7, "bake", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true))
    }

    #[test]
    fn craft_zero_is_not_a_craft() {
        let idle = Job::none();
        let farmers = Job::new(4, vec![]);
        assert!(!idle.has_craft(0));
        assert!(!farmers.has_craft(0));
        assert!(farmers.has_craft(4));
        assert!(farmers.is_same_craft(&Job::new(4, vec![])));
        assert!(!farmers.is_same_craft(&Job::new(5, vec![])));
        assert!(!idle.is_same_craft(&Job::none()));
    }

    #[test]
    fn ensure_lines_adds_missing_processes_and_keeps_the_rest() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(2.0), vec![4])]);

        job.ensure_lines(&[7, 9, 7]);

        assert_eq!(job.lines.len(), 2);
        assert_eq!(job.lines[0].process, 7);
        assert_eq!(job.lines[0].target, Some(2.0));
        assert_eq!(job.lines[0].inputs, vec![4]);
        assert_eq!(job.lines[1].process, 9);
        assert_eq!(job.lines[1].target, Some(0.0));
        assert!(job.lines[1].inputs.is_empty());
    }

    #[test]
    fn complexity_cost_uses_the_line_processes() {
        let craft = Craft::new(1, "subsistence")
            .with_process(1)
            .with_complexity_modifier(0.4);
        let matched = Job::new(1, vec![JobLine::new(1, Some(0.0), vec![])]);
        let extra = Job::new(
            1,
            vec![
                JobLine::new(1, Some(0.0), vec![]),
                JobLine::new(2, Some(0.0), vec![]),
            ],
        );

        assert!((matched.complexity_cost(&craft, 0.1) - 0.4).abs() < 1e-12);
        assert!((extra.complexity_cost(&craft, 0.1) - 0.5).abs() < 1e-12);
    }

    /// An exact clothier spends half a point. Three extra processes at the
    /// default distance of 0.1 bring that to 0.8. A missing craft spends one.
    #[test]
    fn craft_point_scale_uses_the_craft_cost() {
        let factuals = Factuals::new().with_craft(
            Craft::new(8, "clothier")
                .with_process(39)
                .with_process(40)
                .with_process(41)
                .with_process(42)
                .with_complexity_modifier(0.5),
        );
        let matched = Job::new(
            8,
            vec![
                JobLine::new(39, Some(0.0), vec![]),
                JobLine::new(40, Some(0.0), vec![]),
                JobLine::new(41, Some(0.0), vec![]),
                JobLine::new(42, Some(0.0), vec![]),
            ],
        );
        let household = Job::new(
            8,
            vec![
                JobLine::new(39, Some(0.0), vec![]),
                JobLine::new(40, Some(0.0), vec![]),
                JobLine::new(41, Some(0.0), vec![]),
                JobLine::new(42, Some(0.0), vec![]),
                JobLine::new(29, Some(0.0), vec![]),
                JobLine::new(30, Some(0.0), vec![]),
                JobLine::new(31, Some(0.0), vec![]),
            ],
        );
        let missing = Job::new(0, vec![JobLine::new(39, Some(0.0), vec![])]);

        assert!((matched.craft_point_scale(&factuals) - 0.5).abs() < 1e-12);
        assert!((household.craft_point_scale(&factuals) - 0.8).abs() < 1e-12);
        assert!((missing.craft_point_scale(&factuals) - 1.0).abs() < 1e-12);
    }

    /// Two clothes runs pull two fabric, two thread, and two cotton.
    #[test]
    fn add_cover_inputs_chains_a_basic_specialty() {
        let cotton = Process::new(39, "cotton", 0).with_output(ProcessOutput::new(39, 1.0, false));
        let thread = Process::new(40, "thread", 0)
            .with_input(ProcessInput::new(39, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(40, 1.0, false));
        let fabric = Process::new(41, "fabric", 0)
            .with_input(ProcessInput::new(40, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(41, 1.0, false));
        let clothes = Process::new(42, "clothes", 0)
            .with_input(ProcessInput::new(41, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(42, 1.0, false));
        let factuals = Factuals::new()
            .with_process(cotton)
            .with_process(thread)
            .with_process(fabric)
            .with_process(clothes);
        let lines = vec![
            JobLine::new(39, None, vec![]),
            JobLine::new(40, None, vec![]),
            JobLine::new(41, None, vec![]),
            JobLine::new(42, None, vec![]),
        ];
        let mut basic = vec![0.0, 0.0, 0.0, 2.0];
        super::add_cover_inputs(
            &lines,
            &factuals,
            &mut basic,
            &[0.0, 0.0, 0.0, 0.0],
            &[],
            &HashMap::new(),
        );
        assert!((basic[0] - 2.0).abs() < 1e-9, "cotton {}", basic[0]);
        assert!((basic[1] - 2.0).abs() < 1e-9, "thread {}", basic[1]);
        assert!((basic[2] - 2.0).abs() < 1e-9, "fabric {}", basic[2]);
        assert!((basic[3] - 2.0).abs() < 1e-9, "clothes {}", basic[3]);
    }

    #[test]
    fn complexity_cost_counts_a_repeated_process_once() {
        let craft = Craft::new(1, "subsistence")
            .with_process(1)
            .with_complexity_modifier(0.4);
        let doubled = Job::new(
            1,
            vec![
                JobLine::new(2, Some(0.0), vec![]),
                JobLine::new(2, Some(0.0), vec![]),
            ],
        );

        // Missing process 1 costs half of 0.1. Process 2 is one extra process.
        assert!((doubled.complexity_cost(&craft, 0.1) - 0.55).abs() < 1e-12);
    }

    #[test]
    fn plan_stores_the_complexity_cost_of_the_targets() {
        let mut job = Job::new(1, vec![JobLine::new(7, None, vec![])]);
        let factuals = Factuals::new().with_process(bake());

        // Four grain finishes the four cover runs.
        job.plan(
            &HashMap::from([(2, 3.0)]),
            &HashMap::from([(1, PopPRow::new(4.0))]),
            &factuals,
            &MarketHistory::new(),
            0.4,
            true,
        );

        // 0.4 * 1 * (1 - 0) * 4 iterations. The gap is 3, plus one unit.
        assert_eq!(job.lines[0].target, Some(4.0));
        assert!((job.plan_cost - 1.6).abs() < 1e-12);
    }

    #[test]
    fn plan_cost_subtracts_goods_shared_with_another_line() {
        let mill = Process::new(3, "mill", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let shared = Process::new(7, "bake", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let mut job = Job::new(
            1,
            vec![JobLine::new(3, None, vec![]), JobLine::new(7, None, vec![])],
        );
        let factuals = Factuals::new().with_process(mill).with_process(shared);

        // Two grain finishes each line's two runs. Each line sees that pile.
        job.plan(
            &HashMap::from([(2, 1.0), (4, 1.0)]),
            &HashMap::from([(1, PopPRow::new(2.0))]),
            &factuals,
            &MarketHistory::new(),
            1.0,
            true,
        );

        // Each line shares one of two goods: 1 * (1 - 0.5 / 2) * 2, twice.
        // The gap is 1, plus one unit.
        assert_eq!(job.lines[0].target, Some(2.0));
        assert_eq!(job.lines[1].target, Some(2.0));
        assert!((job.plan_cost - 3.0).abs() < 1e-12);
    }

    #[test]
    fn plan_cost_stays_light_for_a_subsistence_process() {
        let forage = Process::new(31, "forage", 0)
            .with_output(ProcessOutput::new(7, 2.0, true))
            .with_complexity(0.25);
        let mut job = Job::new(1, vec![JobLine::new(31, Some(1.0), vec![])]);
        let factuals = Factuals::new().with_process(forage);

        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            true,
        );

        // 1 * 0.25 * (1 - 0) * 1.
        assert_eq!(job.lines[0].target, Some(1.0));
        assert!((job.plan_cost - 0.25).abs() < 1e-12);
    }

    #[test]
    fn plan_cost_counts_overlap_on_a_resting_line() {
        let running = Process::new(3, "mill", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let resting = Process::new(7, "bake", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let mut job = Job::new(
            1,
            vec![JobLine::new(3, None, vec![]), JobLine::new(7, None, vec![])],
        );
        let factuals = Factuals::new().with_process(running).with_process(resting);
        let mut history = MarketHistory::new();
        history.prices.insert(4, 0.0);

        // Two grain finishes the mill's two runs. The bake has no cover.
        job.plan(
            &HashMap::from([(2, 1.0)]),
            &HashMap::from([(1, PopPRow::new(2.0))]),
            &factuals,
            &history,
            1.0,
            true,
        );

        // The gap is 1, plus one unit. The resting line still shares one of two goods.
        assert_eq!(job.lines[0].target, Some(2.0));
        assert_eq!(job.lines[1].target, Some(0.0));
        assert!((job.plan_cost - 1.5).abs() < 1e-12);
    }

    #[test]
    fn plan_sets_iterations_from_the_desire_gap() {
        let mut job = Job::new(1, vec![JobLine::new(7, None, vec![])]);
        let factuals = Factuals::new().with_process(bake());

        // The pop already subtracted stock. Three bread are still short, plus one unit.
        // Four grain finishes those runs.
        job.plan(
            &HashMap::from([(2, 3.0)]),
            &HashMap::from([(1, PopPRow::new(4.0))]),
            &factuals,
            &MarketHistory::new(),
            1.0,
            true,
        );

        assert_eq!(job.lines[0].target, Some(4.0));
    }

    #[test]
    fn plan_adds_one_output_unit_above_the_gap() {
        let make = Process::new(1, "make grain", 0).with_output(ProcessOutput::new(1, 2.0, true));
        let mut job = Job::new(1, vec![JobLine::new(1, Some(0.0), vec![])]);
        let factuals = Factuals::new().with_process(make);
        let mut history = MarketHistory::new();
        history.prices.insert(1, 0.0);

        job.plan(
            &HashMap::from([(1, 10.0)]),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            true,
        );

        // 10 grain short, one extra unit, two per run.
        assert_eq!(job.lines[0].target, Some(5.5));
    }

    #[test]
    fn plan_surplus_runs_the_better_line_on_leftover_inputs() {
        let bake = Process::new(7, "bake", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let weave = Process::new(8, "weave", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(3, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(1.0), vec![]),
                JobLine::new(8, Some(1.0), vec![]),
            ],
        );
        let factuals = Factuals::new().with_process(bake).with_process(weave);
        let mut history = MarketHistory::new();
        history.prices.insert(2, 4.0);
        history.prices.insert(4, 2.0);
        history.prices.insert(1, 0.0);
        history.prices.insert(3, 0.0);
        let property = HashMap::from([(1, PopPRow::new(3.0)), (3, PopPRow::new(5.0))]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // Both pay. Each steps once toward the inputs it can still run.
        assert_eq!(job.lines[0].process, 7);
        assert_eq!(job.lines[0].target, Some(1.25));
        assert_eq!(job.lines[1].process, 8);
        assert_eq!(job.lines[1].target, Some(1.25));
    }

    /// The mill is listed first and takes the spare time. Cloth pays more.
    /// Its pace can absorb 0.4 of the mill's offer. It is still under
    /// yesterday's output, so the floor takes another quarter of the mill.
    #[test]
    fn plan_nudges_surplus_onto_the_stronger_line() {
        let mill = Process::new(7, "mill", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let weave = Process::new(8, "weave", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(10.0), vec![]),
                JobLine::new(8, Some(0.0), vec![]),
            ],
        );
        let factuals = Factuals::new().with_process(mill).with_process(weave);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(2, 2.0);
        history.salability.insert(2, 1.0);
        history.prices.insert(4, 6.0);
        history.salability.insert(4, 1.0);
        let mut cloth = PopPRow::new(4.0);
        cloth.produced = 4.0;
        let property = HashMap::from([(0, PopPRow::new(10.0)), (4, cloth)]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // Ten time would all sit on the mill. Cloth kept its whole output, so
        // its pace allows 0.4 of the offer. The floor then funds 2.4 more
        // from a quarter of the mill. The mill steps from 10 toward 7.2.
        // Cloth steps a quarter of the way off 0.
        assert_eq!(job.lines[0].process, 7);
        assert_eq!(job.lines[1].process, 8);
        let mill_runs = job.lines[0].target.unwrap_or(0.0);
        let weave_runs = job.lines[1].target.unwrap_or(0.0);
        assert!((mill_runs - 7.5).abs() < 1e-9);
        assert!((weave_runs - 0.25).abs() < 1e-9);
    }

    /// Cloth's pace refuses the mill's surplus offer. The floor still takes
    /// a quarter of the mill, because cloth is under yesterday's ten runs.
    #[test]
    fn plan_keeps_surplus_the_stronger_line_cannot_run() {
        let weave = Process::new(8, "weave", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let mill = Process::new(7, "mill", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(8, Some(1.0), vec![]),
                JobLine::new(7, Some(9.0), vec![]),
            ],
        );
        let factuals = Factuals::new().with_process(weave).with_process(mill);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(2, 2.0);
        history.salability.insert(2, 1.0);
        history.prices.insert(4, 6.0);
        history.salability.insert(4, 1.0);
        let mut cloth = PopPRow::new(10.0);
        cloth.produced = 10.0;
        let property = HashMap::from([(0, PopPRow::new(10.0)), (4, cloth)]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        assert_eq!(job.lines[0].process, 8);
        assert_eq!(job.lines[1].process, 7);
        let weave_runs = job.lines[0].target.unwrap_or(0.0);
        let mill_runs = job.lines[1].target.unwrap_or(0.0);
        assert!((weave_runs - 1.25).abs() < 1e-9);
        assert!((mill_runs - 6.75).abs() < 1e-9);
    }

    /// Gold pays and has no meal. The forage cover is holding the morning.
    /// One gold run is planned, and forage gives up that time.
    #[test]
    fn plan_presses_a_paying_time_line_up_off_zero() {
        let gold = Process::new(4, "make gold", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let forage = Process::new(31, "subsistence forage", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                50.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(7, 4.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(4, Some(0.0), vec![]),
                JobLine::new(31, Some(4.0), vec![]),
            ],
        );
        let factuals = Factuals::new().with_process(gold).with_process(forage);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(4, 5.0);
        history.salability.insert(4, 1.0);
        history.prices.insert(7, 0.0);

        job.plan(
            &HashMap::from([(7, 15.0)]),
            &HashMap::from([(0, PopPRow::new(200.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        let gold_line = job.lines.iter().find(|line| line.process == 4).unwrap();
        let wood_line = job.lines.iter().find(|line| line.process == 31).unwrap();
        assert!((gold_line.target.unwrap_or(0.0) - 0.25).abs() < 1e-9);
        assert!((wood_line.target.unwrap_or(0.0) - 3.8).abs() < 1e-9);
    }

    /// Forage earns more per run. Gold earns more per unit of time, so the
    /// spare time steps onto gold.
    #[test]
    fn plan_ranks_surplus_by_pay_per_time() {
        let forage = Process::new(31, "subsistence forage", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                50.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(7, 4.0, true));
        let gold = Process::new(4, "make gold", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(31, Some(4.0), vec![]),
                JobLine::new(4, Some(0.0), vec![]),
            ],
        );
        let factuals = Factuals::new().with_process(forage).with_process(gold);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(7, 2.0);
        history.salability.insert(7, 1.0);
        history.prices.insert(4, 5.0);
        history.salability.insert(4, 1.0);

        job.plan(
            &HashMap::new(),
            &HashMap::from([(0, PopPRow::new(200.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        let wood_line = job.lines.iter().find(|line| line.process == 31).unwrap();
        let gold_line = job.lines.iter().find(|line| line.process == 4).unwrap();
        assert!((wood_line.target.unwrap_or(0.0) - 3.0).abs() < 1e-9);
        assert!((gold_line.target.unwrap_or(0.0) - 0.25).abs() < 1e-9);
    }

    /// Bread pays and the grain is not on hand. The farm grows to feed one
    /// run. Forage, which bread does not eat, gives up the time.
    #[test]
    fn plan_grows_a_feeder_for_the_stronger_line() {
        let bread = Process::new(3, "make bread", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let farm = Process::new(29, "subsistence farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                50.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(1, 4.0, true));
        let forage = Process::new(31, "subsistence forage", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                50.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(7, 4.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(3, Some(0.0), vec![]),
                JobLine::new(29, Some(0.0), vec![]),
                JobLine::new(31, Some(4.0), vec![]),
            ],
        );
        let factuals = Factuals::new()
            .with_process(bread)
            .with_process(farm)
            .with_process(forage);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(1, 0.0);
        history.prices.insert(2, 4.0);
        history.salability.insert(2, 1.0);
        history.prices.insert(7, 0.0);

        job.plan(
            &HashMap::from([(7, 15.0)]),
            &HashMap::from([(0, PopPRow::new(200.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        let bread_line = job.lines.iter().find(|line| line.process == 3).unwrap();
        let farm_line = job.lines.iter().find(|line| line.process == 29).unwrap();
        let wood_line = job.lines.iter().find(|line| line.process == 31).unwrap();
        // One bread run needs a quarter of a farm run. Forage gives up that
        // time, 14.5, and stays on the rest of its cover.
        assert!((bread_line.target.unwrap_or(0.0) - 0.25).abs() < 1e-9);
        assert!((farm_line.target.unwrap_or(0.0) - 0.25).abs() < 1e-9);
        assert!((wood_line.target.unwrap_or(0.0) - 3.71).abs() < 1e-9);
    }

    #[test]
    fn plan_surplus_adds_the_grain_the_cover_leaves() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(0.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 4.0);
        history.prices.insert(1, 0.0);
        let mut bread = PopPRow::new(0.0);
        bread.produced = 10.0;
        bread.lost = 1.0;
        let property = HashMap::from([(1, PopPRow::new(5.0)), (2, bread)]);

        // Cover snaps to 3: two short plus one unit. Two grain remain, and this night steps toward that surplus.
        job.plan(
            &HashMap::from([(2, 2.0)]),
            &property,
            &factuals,
            &history,
            1.0,
            true,
        );

        assert_eq!(job.lines[0].target, Some(3.25));
    }

    #[test]
    fn plan_surplus_pulls_back_when_own_rot_passes_a_tenth() {
        let mut line = JobLine::new(7, Some(15.0), vec![]);
        line.cover = 3.0;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 4.0);
        history.prices.insert(1, 0.0);
        let mut bread = PopPRow::new(0.0);
        bread.produced = 10.0;
        bread.lost = 2.0;
        let property = HashMap::from([(1, PopPRow::new(20.0)), (2, bread)]);

        // Cover snaps to 3: two short plus one unit. Twice the tenth leaves a surplus of 5.
        // The stored cover is 3, so the old surplus of 12 steps to 9, and 3 + 9 stays at 12.
        job.plan(
            &HashMap::from([(2, 2.0)]),
            &property,
            &factuals,
            &history,
            1.0,
            true,
        );

        assert_eq!(job.lines[0].target, Some(12.0));
    }

    #[test]
    fn plan_surplus_keeps_one_run_when_the_output_only_rotted() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(0.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 4.0);
        history.prices.insert(1, 0.0);
        let mut bread = PopPRow::new(0.0);
        bread.lost = 1.0;
        let property = HashMap::from([(1, PopPRow::new(5.0)), (2, bread)]);

        // Cover snaps to 3: two short plus one unit. Nothing was made, so the surplus ideal is one run.
        job.plan(
            &HashMap::from([(2, 2.0)]),
            &property,
            &factuals,
            &history,
            1.0,
            true,
        );

        assert_eq!(job.lines[0].target, Some(3.25));
    }

    #[test]
    fn plan_surplus_doubles_last_output_when_none_rotted() {
        let make = Process::new(1, "make grain", 0)
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 6.0, true));
        let mut job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        let factuals = Factuals::new().with_process(make);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        let mut grain = PopPRow::new(0.0);
        grain.produced = 6.0;
        let property = HashMap::from([(0, PopPRow::new(512.0)), (1, grain)]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // 512 time would fund 1024 runs. Nothing rotted and nothing is left
        // on hand, so the ideal doubles the one run and the line steps toward 2.
        assert_eq!(job.lines[0].target, Some(1.25));
    }

    #[test]
    fn plan_surplus_pulls_back_when_the_output_is_still_on_hand() {
        let make = Process::new(1, "make grain", 0)
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 6.0, true));
        let mut job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        let factuals = Factuals::new().with_process(make);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        let mut grain = PopPRow::new(6.0);
        grain.produced = 6.0;
        let property = HashMap::from([(0, PopPRow::new(512.0)), (1, grain)]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // The whole output is still on hand, so the pace leaves a tenth of
        // the one run. This is the paying line, so the floor keeps that run.
        assert_eq!(job.lines[0].target, Some(1.0));
    }

    #[test]
    fn plan_snap_does_not_stack_an_unpaid_target_on_the_cover() {
        let mut line = JobLine::new(7, Some(12.5), vec![]);
        line.cover = 0.0;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        history.prices.insert(1, 0.0);

        // 13 bread short, plus one unit. Fourteen grain can finish that cover.
        // The line does not pay, so the snap raises 12.5 to it and does not add them.
        let property = HashMap::from([(1, PopPRow::new(14.0))]);
        job.plan(
            &HashMap::from([(2, 13.0)]),
            &property,
            &factuals,
            &history,
            1.0,
            true,
        );

        assert_eq!(job.lines[0].target, Some(14.0));
        assert!((job.lines[0].cover - 14.0).abs() < 1e-12);
    }

    /// The line is the only maker and does not pay. A snap still writes the
    /// cover, then steps toward the market's unmet buys.
    #[test]
    fn plan_snap_steps_sought_on_an_unpaid_best_maker() {
        let mut line = JobLine::new(7, Some(10.0), vec![]);
        line.cover = 10.0;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        history.prices.insert(1, 0.0);
        history.sought.insert(2, 40.0);

        // Eleven cover runs. Forty sought runs are taken because this bake
        // is the only maker. Fifty-one grain finish both. The snap steps
        // a quarter of the way from 10.
        let property = HashMap::from([(1, PopPRow::new(51.0))]);
        job.plan(
            &HashMap::from([(2, 10.0)]),
            &property,
            &factuals,
            &history,
            1.0,
            true,
        );

        assert_eq!(job.lines[0].target, Some(12.5));
        assert!((job.lines[0].cover - 11.0).abs() < 1e-12);
    }

    /// The stored target is the cover, so the old surplus is zero. Sought
    /// still steps from that whole line.
    #[test]
    fn plan_snap_steps_sought_from_the_whole_paying_line() {
        let mut line = JobLine::new(7, Some(3.0), vec![]);
        line.cover = 3.0;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 4.0);
        history.prices.insert(1, 0.0);
        history.sought.insert(2, 40.0);
        let mut bread = PopPRow::new(0.0);
        bread.produced = 10.0;
        bread.lost = 10.0;

        // Cover snaps to 3. The rotten pile paces surplus to one run.
        // Forty-three grain can finish the cover, that run, and the sought bread.
        job.plan(
            &HashMap::from([(2, 2.0)]),
            &HashMap::from([(1, PopPRow::new(43.0)), (2, bread)]),
            &factuals,
            &history,
            1.0,
            true,
        );

        // Surplus steps from 0 to 0.25. Sought then steps a quarter of 3.25.
        assert_eq!(job.lines[0].target, Some(4.0625));
        assert!((job.lines[0].cover - 3.0).abs() < 1e-12);
    }

    /// The market still wants the bread. The line is the only maker, so the
    /// order is taken even though the run does not pay. Fifty-one grain
    /// finish the cover and that order, and 48 steps to 51.
    #[test]
    fn plan_sought_on_the_only_maker_is_taken_at_a_loss() {
        let mut line = JobLine::new(7, Some(48.0), vec![]);
        line.cover = 11.0;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        history.prices.insert(1, 0.0);
        history.sought.insert(2, 40.0);

        // Gap 10 plus one unit is a cover of 11. Sought 40 is taken.
        // Fifty-one grain finish both, so 48 steps to 51.
        job.plan(
            &HashMap::from([(2, 10.0)]),
            &HashMap::from([(1, PopPRow::new(51.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(51.0));
    }

    /// Forage is this job's only wood line, and a faster wood process is known.
    /// The desire gap still lands on forage. The market's wood does not.
    #[test]
    fn plan_sought_on_a_worse_process_stays_inside_its_cover() {
        let forage = Process::new(31, "subsistence forage", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(7, 2.0, true));
        let saw = Process::new(7, "make wood", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 8.0, true));
        let mut line = JobLine::new(31, Some(10.0), vec![]);
        line.cover = 5.5;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new().with_process(forage).with_process(saw);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(7, 0.0);
        history.sought.insert(7, 100.0);

        // Gap 10 plus one unit is 5.5 forage runs. Wood and time are priced
        // at 0, so the line does not pay and the market's wood is not taken.
        // The line is at 10 and steps toward that cover.
        job.plan(
            &HashMap::from([(7, 10.0)]),
            &HashMap::from([(0, PopPRow::new(200.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(7.5));
        assert!((job.lines[0].cover - 5.5).abs() < 1e-12);
    }

    /// Forage costs more per wood than the board, and a faster wood process
    /// is known. Sought wood stays on that faster process.
    #[test]
    fn plan_sought_skips_a_worse_line_that_is_cheaper_to_buy() {
        let forage = Process::new(31, "subsistence forage", 0)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(7, 2.0, true));
        let saw = Process::new(7, "make wood", 0)
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 8.0, true));
        let mut line = JobLine::new(31, Some(2.0), vec![]);
        line.cover = 5.5;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new()
            .with_process(forage)
            .with_process(saw)
            .with_good(good(0, 1.0))
            .with_good(good(7, 0.0));
        let mut history = MarketHistory::new();
        history.prices.insert(7, 1.0);
        history.salability.insert(7, 1.0);
        history.sought.insert(7, 100.0);

        // Ten time for two wood costs 5. The board holds wood at 1, so sought
        // adds nothing. No wood is on hand, so the meal stays. A thousand time
        // can finish the 5.5 cover runs. The line is at 2 and takes the step.
        job.plan(
            &HashMap::from([(7, 10.0)]),
            &HashMap::from([(0, PopPRow::new(1000.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(2.5));
    }

    /// Forage is the fastest wood process known, and making it loses.
    /// The market's unmet wood is still taken, up to the time on hand.
    /// The meal stays underneath that order.
    #[test]
    fn plan_sought_on_the_fastest_line_is_taken_at_a_loss() {
        let forage = Process::new(31, "subsistence forage", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(7, 2.0, true));
        let mut line = JobLine::new(31, Some(10.0), vec![]);
        line.cover = 5.5;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new()
            .with_process(forage)
            .with_good(good(0, 1.0))
            .with_good(good(7, 0.0));
        let mut history = MarketHistory::new();
        history.prices.insert(7, 1.0);
        history.salability.insert(7, 1.0);
        history.sought.insert(7, 100.0);

        // Cover is 5.5 and uses 55 time. Sixty-five time remain, which
        // finishes 6.5 of the sought runs. The line is at 10 and steps to 12.
        job.plan(
            &HashMap::from([(7, 10.0)]),
            &HashMap::from([(0, PopPRow::new(120.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(12.0));
    }

    /// Tools do not pay, and forage has already taken the morning. The
    /// unmet tool order takes its time back out of that surplus.
    #[test]
    fn plan_fills_sought_on_a_losing_line_from_surplus_time() {
        let forage = Process::new(31, "subsistence forage", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(7, 1.0, true));
        let tools = Process::new(9, "make tools", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(9, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(31, Some(10.0), vec![]),
                JobLine::new(9, Some(4.0), vec![]),
            ],
        );
        let factuals = Factuals::new().with_process(forage).with_process(tools);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(7, 4.0);
        history.salability.insert(7, 1.0);
        history.prices.insert(9, 0.0);
        history.sought.insert(9, 10.0);

        job.plan(
            &HashMap::new(),
            &HashMap::from([(0, PopPRow::new(100.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        let wood_line = job.lines.iter().find(|line| line.process == 31).unwrap();
        let tool_line = job.lines.iter().find(|line| line.process == 9).unwrap();
        assert!((wood_line.target.unwrap_or(0.0) - 8.0).abs() < 1e-9);
        assert!((tool_line.target.unwrap_or(0.0) - 5.0).abs() < 1e-9);
    }

    /// Forage costs nothing while time is free, so the card is not cheaper.
    /// Gold is the best gold line and it pays, so the wood meal is bought.
    #[test]
    fn buys_a_worse_staple_when_another_line_is_the_specialty() {
        let forage = Process::new(31, "subsistence forage", 0)
            .with_input(ProcessInput::new(
                0,
                50.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(7, 4.0, true));
        let gold = Process::new(4, "make gold", 0)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let saw = Process::new(7, "make wood", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 8.0, true));
        let lines = vec![
            JobLine::new(31, Some(0.0), vec![]),
            JobLine::new(4, Some(0.0), vec![]),
        ];
        let factuals = Factuals::new()
            .with_process(forage)
            .with_process(gold)
            .with_process(saw);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(7, 1.0);
        history.salability.insert(7, 1.0);
        history.prices.insert(4, 5.0);
        history.salability.insert(4, 1.0);

        let buys = super::buys_cheaper_than_making(
            &lines,
            &HashMap::from([(7, 4.0)]),
            &HashMap::from([(7, 4.0)]),
            &factuals,
            &history,
        );

        assert!((buys.get(&7).copied().unwrap_or(0.0) - 4.0).abs() < 1e-9);
    }

    /// The same forage, with nothing else that pays. Time is free, so the
    /// meal stays a make.
    #[test]
    fn keeps_a_worse_staple_when_nothing_else_pays() {
        let forage = Process::new(31, "subsistence forage", 0)
            .with_input(ProcessInput::new(
                0,
                50.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(7, 4.0, true));
        let saw = Process::new(7, "make wood", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 8.0, true));
        let lines = vec![JobLine::new(31, Some(0.0), vec![])];
        let factuals = Factuals::new().with_process(forage).with_process(saw);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(7, 1.0);
        history.salability.insert(7, 1.0);

        let buys = super::buys_cheaper_than_making(
            &lines,
            &HashMap::from([(7, 4.0)]),
            &HashMap::from([(7, 4.0)]),
            &factuals,
            &history,
        );

        assert!(buys.is_empty());
    }

    /// Forty jewelry runs are on the line. Four gold and the time on hand
    /// finish four of them, so the aim stops there.
    #[test]
    fn plan_stops_a_cover_at_the_runs_stock_can_finish() {
        let jewelry = Process::new(6, "make jewelry", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(4, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(6, 1.0, true));
        let mut job = Job::new(1, vec![JobLine::new(6, Some(40.0), vec![])]);
        let factuals = Factuals::new()
            .with_process(jewelry)
            .with_good(good(0, 1.0))
            .with_good(good(4, 0.0))
            .with_good(good(6, 0.0));
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(4, 0.0);
        history.prices.insert(6, 0.0);
        let property = HashMap::from([(0, PopPRow::new(1000.0)), (4, PopPRow::new(4.0))]);

        // Nineteen jewelry plus one unit is 20 runs. Four gold finishes four,
        // so the stored target is those four runs.
        job.plan(
            &HashMap::from([(6, 19.0)]),
            &property,
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(4.0));
    }

    /// The same four gold are not on hand. The shopping list holds them,
    /// so the morning still plans the four runs that buy will feed.
    #[test]
    fn plan_keeps_the_runs_a_planned_buy_can_feed() {
        let jewelry = Process::new(6, "make jewelry", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(4, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(6, 1.0, true));
        let mut job = Job::new(1, vec![JobLine::new(6, Some(40.0), vec![])]);
        job.shopping.insert(4, 4.0);
        let factuals = Factuals::new()
            .with_process(jewelry)
            .with_good(good(0, 1.0))
            .with_good(good(4, 0.0))
            .with_good(good(6, 0.0));
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(4, 0.0);
        history.prices.insert(6, 0.0);
        let property = HashMap::from([(0, PopPRow::new(1000.0))]);

        job.plan(
            &HashMap::from([(6, 19.0)]),
            &property,
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(4.0));
    }

    /// The water row was made empty. The line pays and the old target would
    /// step up. It stays, because that product was not made.
    #[test]
    fn plan_does_not_raise_a_product_that_was_not_made() {
        let process = Process::new(2, "make water", 0)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 6.0, true));
        let mut job = Job::new(1, vec![JobLine::new(2, Some(8.0), vec![])]);
        let factuals = Factuals::new().with_process(process);
        let mut water = PopPRow::new(0.0);
        water.produced = 0.0;
        let property = HashMap::from([(0, PopPRow::new(10_000.0)), (2, water)]);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(2, 2.0);
        history.salability.insert(2, 1.0);
        job.note_sale(&HashMap::new(), &HashMap::new());

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        assert_eq!(job.lines[0].target, Some(8.0));
    }

    /// The jeweler has stood down and holds no gold. Bids are waiting and
    /// nobody is offering, so the plan keeps one run and shops the gold.
    #[test]
    fn plan_restarts_one_jewelry_run_and_shops_the_gold() {
        let jewelry = Process::new(6, "make jewelry", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(4, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(6, 1.0, true));
        let mut job = Job::new(1, vec![JobLine::new(6, Some(0.0), vec![])]);
        let factuals = Factuals::new()
            .with_process(jewelry)
            .with_good(good(4, 0.0));
        let mut history = MarketHistory::new();
        history.sought.insert(6, 8.0);

        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(1.0));
        assert!((job.shopping.get(&4).copied().unwrap_or(0.0) - 1.0).abs() < 1e-9);
    }

    /// The same empty line stays down when the book already has an offer,
    /// and when nobody is bidding.
    #[test]
    fn plan_does_not_restart_jewelry_without_an_empty_bid() {
        let jewelry = Process::new(6, "make jewelry", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(4, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(6, 1.0, true));
        let factuals = Factuals::new()
            .with_process(jewelry)
            .with_good(good(4, 0.0));
        let mut offered = MarketHistory::new();
        offered.sought.insert(6, 8.0);
        offered.mass_offered.insert(6, 3.0);
        let mut quiet = MarketHistory::new();
        quiet.mass_offered.insert(6, 0.0);

        let mut listing = Job::new(1, vec![JobLine::new(6, Some(0.0), vec![])]);
        listing.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &offered,
            1.0,
            false,
        );
        let mut unsought = Job::new(1, vec![JobLine::new(6, Some(0.0), vec![])]);
        unsought.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &quiet,
            1.0,
            false,
        );

        assert_eq!(listing.lines[0].target, Some(0.0));
        assert!(listing.shopping.is_empty());
        assert_eq!(unsought.lines[0].target, Some(0.0));
        assert!(unsought.shopping.is_empty());
    }

    /// Nothing was made and the stored target is under one run. The empty
    /// offer still comes back to one run.
    #[test]
    fn plan_keeps_the_one_run_when_nothing_was_made() {
        let jewelry = Process::new(6, "make jewelry", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(4, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(6, 1.0, true));
        let mut job = Job::new(1, vec![JobLine::new(6, Some(0.4), vec![])]);
        let factuals = Factuals::new()
            .with_process(jewelry)
            .with_good(good(4, 0.0))
            .with_good(good(6, 0.0));
        let mut made = PopPRow::new(0.0);
        made.produced = 0.0;
        let mut history = MarketHistory::new();
        history.sought.insert(6, 8.0);
        job.note_sale(&HashMap::new(), &HashMap::new());

        job.plan(
            &HashMap::new(),
            &HashMap::from([(6, made)]),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(1.0));
    }

    /// Pots have stood down. One pot run raises one clay run and one coal run.
    #[test]
    fn plan_restarts_one_pot_run_on_the_digs() {
        let clay = Process::new(26, "make clay", 0)
            .with_input(ProcessInput::new(0, 3.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(26, 3.0, true));
        let coal = Process::new(23, "make coal", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(23, 5.0, true));
        let pots = Process::new(27, "make pots", 0)
            .with_input(ProcessInput::new(0, 3.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(
                26,
                1.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_input(ProcessInput::new(
                23,
                1.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(27, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(27, Some(0.0), vec![]),
                JobLine::new(26, Some(0.0), vec![]),
                JobLine::new(23, Some(0.0), vec![]),
            ],
        );
        let factuals = Factuals::new()
            .with_process(clay)
            .with_process(coal)
            .with_process(pots);
        let mut history = MarketHistory::new();
        history.sought.insert(27, 12.0);

        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        let target = |process: usize| {
            job.lines
                .iter()
                .find(|line| line.process == process)
                .and_then(|line| line.target)
        };
        assert_eq!(target(27), Some(1.0));
        assert_eq!(target(26), Some(1.0));
        assert_eq!(target(23), Some(1.0));
        assert!(job.shopping.is_empty());
    }

    /// Jewelry holds at 1, so the line pays. Four gold fund four surplus runs
    /// and leave nothing for the market's order. The line is already at four.
    #[test]
    fn plan_sought_stops_at_the_input_on_hand() {
        let jewelry = Process::new(6, "make jewelry", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(4, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(6, 1.0, true));
        let mut job = Job::new(1, vec![JobLine::new(6, Some(4.0), vec![])]);
        let factuals = Factuals::new()
            .with_process(jewelry)
            .with_good(good(0, 1.0))
            .with_good(good(4, 0.0))
            .with_good(good(6, 0.0));
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(4, 0.0);
        history.prices.insert(6, 1.0);
        history.salability.insert(6, 1.0);
        history.sought.insert(6, 100.0);
        let property = HashMap::from([(0, PopPRow::new(1000.0)), (4, PopPRow::new(4.0))]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        assert_eq!(job.lines[0].target, Some(4.0));
    }

    /// Grain funds the meal and none of the bread the market still wants.
    /// The sale left the march, and the bake pays, so the line takes one
    /// step toward that order. The next morning buys the grain.
    #[test]
    fn plan_steps_toward_sought_the_morning_cannot_fund() {
        let bread = Process::new(3, "make bread", 0)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 3.0, true));
        let mut job = Job::new(1, vec![JobLine::new(3, Some(10.0), vec![])]);
        let factuals = Factuals::new()
            .with_process(bread)
            .with_good(good(0, 1.0))
            .with_good(good(1, 0.0))
            .with_good(good(3, 0.3));
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.1);
        history.prices.insert(1, 1.0);
        history.prices.insert(3, 2.0);
        history.salability.insert(0, 1.0);
        history.salability.insert(1, 1.0);
        history.salability.insert(3, 1.0);
        history.sought.insert(3, 30.0);
        let mut made = PopPRow::new(0.0);
        made.produced = 30.0;
        let property = HashMap::from([
            (0, PopPRow::new(1_000.0)),
            (1, PopPRow::new(10.0)),
            (3, made),
        ]);
        job.note_sale(&HashMap::new(), &HashMap::new());

        job.plan(
            &HashMap::from([(3, 29.0)]),
            &property,
            &factuals,
            &history,
            1.0,
            false,
        );

        // Meal is 10 runs. The 10 sought runs were cut. One march from 10 toward 20.
        assert!((job.lines[0].target.unwrap_or(0.0) - 12.5).abs() < 1e-9);
    }

    /// The bread line is listed first and no grain is on hand. The farm
    /// still runs first and makes that grain, so the bread cover is taken.
    #[test]
    fn plan_bakes_from_grain_the_farm_makes_this_morning() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(1, 4.0, true));
        let bread = Process::new(7, "bake", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(0.0), vec![]),
                JobLine::new(29, Some(0.0), vec![]),
            ],
        );
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(bread)
            .with_good(good(0, 1.0))
            .with_good(good(1, 0.0))
            .with_good(good(2, 0.0));
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(1, 0.0);
        history.prices.insert(2, 0.0);

        // Three bread plus one unit is four runs, and four grain. The farm's
        // extra unit makes five grain. A hundred time finishes the farm.
        job.plan(
            &HashMap::from([(2, 3.0)]),
            &HashMap::from([(0, PopPRow::new(100.0))]),
            &factuals,
            &history,
            1.0,
            true,
        );

        assert_eq!(job.lines[0].process, 29);
        assert_eq!(job.lines[0].target, Some(1.25));
        assert_eq!(job.lines[1].process, 7);
        assert_eq!(job.lines[1].target, Some(4.0));
    }

    /// A hundred bread runs would hold the whole morning and leave the farm
    /// with no time. The stored runs are a morning both lines still finish.
    #[test]
    fn plan_keeps_a_cover_the_later_line_can_still_feed() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(
                0,
                50.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(1, 4.0, true));
        let bread = Process::new(7, "bake", 0)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(29, Some(0.0), vec![]),
                JobLine::new(7, Some(0.0), vec![]),
            ],
        );
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(bread)
            .with_good(good(0, 1.0))
            .with_good(good(1, 0.0))
            .with_good(good(2, 0.0));
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(1, 0.0);
        history.prices.insert(2, 0.0);

        job.plan(
            &HashMap::from([(2, 99.0)]),
            &HashMap::from([(0, PopPRow::new(200.0))]),
            &factuals,
            &history,
            1.0,
            true,
        );

        let farm_runs = job
            .lines
            .iter()
            .find(|line| line.process == 29)
            .and_then(|line| line.target)
            .unwrap_or(0.0);
        let bread_runs = job
            .lines
            .iter()
            .find(|line| line.process == 7)
            .and_then(|line| line.target)
            .unwrap_or(0.0);
        assert!(farm_runs > 0.0);
        assert!(bread_runs > 1.0);
        assert!(bread_runs < 100.0);
        assert!(farm_runs * 4.0 + 1e-4 >= bread_runs);
        assert!(farm_runs * 50.0 + bread_runs * 2.0 <= 200.0 + 1e-4);
    }

    /// Jewelry used 30 gold and none is still sought. The gold line steps
    /// toward those runs. A filled sale alone would step toward zero.
    #[test]
    fn plan_steps_toward_an_input_downstream_production_used() {
        let gold = Process::new(4, "make gold", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 3.0, true));
        let jewelry = Process::new(6, "make jewelry", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(4, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(6, 1.0, true));
        let mut job = Job::new(1, vec![JobLine::new(4, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(gold).with_process(jewelry);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(4, 10.0);
        history.produced.insert(6, 30.0);

        job.plan(
            &HashMap::new(),
            &HashMap::from([(0, PopPRow::new(10.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        // Thirty jewelry took thirty gold, ten runs of this line. Ten time
        // finishes those runs. The line is at 4 and steps toward 10.
        assert_eq!(job.lines[0].target, Some(5.0));
    }

    #[test]
    fn plan_snap_does_not_stack_a_paying_target_on_the_cover() {
        let mut line = JobLine::new(7, Some(4.0), vec![]);
        line.cover = 0.0;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 4.0);
        history.prices.insert(1, 0.0);
        let property = HashMap::from([(1, PopPRow::new(5.0))]);

        // Cover snaps to 3. Two grain remain, so the extra is 2 and the ideal
        // is 5. Stored cover is still 0, so 4 marches to 5 instead of 3 + 4.
        job.plan(
            &HashMap::from([(2, 2.0)]),
            &property,
            &factuals,
            &history,
            1.0,
            true,
        );

        assert_eq!(job.lines[0].target, Some(5.0));
        assert!((job.lines[0].cover - 3.0).abs() < 1e-12);
    }

    #[test]
    fn plan_prices_time_at_its_holding() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let mut job = Job::new(1, vec![JobLine::new(29, Some(12.5), vec![])]);
        let factuals = Factuals::new()
            .with_good(good(0, 1.0))
            .with_good(good(1, 0.12))
            .with_process(farm);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.1);
        history.prices.insert(1, 1.0);
        history.salability.insert(0, 1.0);
        history.salability.insert(1, 1.0);
        let property = HashMap::from([(0, PopPRow::new(1000.0))]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // Time's board price is 0.1, so ten time costs 1 and two grain pay 2.
        // The run pays. A thousand time funds 100 runs, and 12.5 steps toward that.
        assert!(job.lines[0].pays);
        assert_eq!(job.lines[0].target, Some(15.625));
    }

    #[test]
    fn plan_keeps_a_line_when_the_output_is_not_worth_holding() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        let property = HashMap::from([(1, PopPRow::new(5.0))]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // Worth nothing, and it is not covering, so the 4 runs step toward zero.
        assert_eq!(job.lines[0].target, Some(3.0));
        assert!((job.plan_cost - 3.0).abs() < 1e-12);
    }

    /// No cover, and the bread is worthless. No grain is on hand, so the
    /// running line goes to 0. A target of 0 stays at 0.
    #[test]
    fn plan_stands_an_unpaid_line_down_when_the_cover_is_gone() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);

        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        // No grain is on hand, so the run does not stay above what the morning finishes.
        assert_eq!(job.lines[0].target, Some(0.0));

        job.lines[0].target = Some(0.0);
        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(0.0));
    }

    #[test]
    fn plan_shifts_an_unpaid_line_to_the_back() {
        let bake = Process::new(7, "bake", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let weave = Process::new(8, "weave", 0)
            .with_input(ProcessInput::new(3, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(4.0), vec![]),
                JobLine::new(8, Some(1.0), vec![]),
            ],
        );
        let factuals = Factuals::new().with_process(bake).with_process(weave);
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        history.prices.insert(1, 0.0);
        history.prices.insert(4, 4.0);
        history.prices.insert(3, 0.0);
        let property = HashMap::from([(3, PopPRow::new(5.0))]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // Bread is not worth holding. No grain is on hand, so its target is 0
        // and it follows the cloth.
        assert_eq!(job.lines[0].process, 8);
        assert_eq!(job.lines[0].target, Some(1.25));
        assert_eq!(job.lines[1].process, 7);
        assert_eq!(job.lines[1].target, Some(0.0));
    }

    #[test]
    fn plan_gives_the_cover_to_the_higher_yield() {
        let slow = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let fast = Process::new(1, "make grain", 0)
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 6.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(29, Some(0.0), vec![]),
                JobLine::new(1, Some(0.0), vec![]),
            ],
        );
        let factuals = Factuals::new().with_process(slow).with_process(fast);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(1, 0.0);

        job.plan(
            &HashMap::from([(1, 12.0)]),
            &HashMap::from([(0, PopPRow::new(100.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        // 12 grain short, plus one unit, six per run. A hundred time can finish
        // that. The fast line steps from 0. The slow line has no cover and follows it.
        assert_eq!(job.lines[0].process, 1);
        assert_eq!(job.lines[0].target, Some(0.25));
        assert_eq!(job.lines[1].process, 29);
        assert_eq!(job.lines[1].target, Some(0.0));
    }

    #[test]
    fn plan_puts_a_covering_line_back_ahead_of_a_paying_line() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let gold = Process::new(4, "make gold", 0).with_output(ProcessOutput::new(4, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(29, Some(0.0), vec![]),
                JobLine::new(4, Some(1.0), vec![]),
            ],
        );
        // The farm was sent back on an earlier night.
        job.lines.swap(0, 1);
        let factuals = Factuals::new().with_process(farm).with_process(gold);
        let mut history = MarketHistory::new();
        history.prices.insert(0, 0.0);
        history.prices.insert(1, 0.0);
        history.prices.insert(4, 4.0);

        job.plan(
            &HashMap::from([(1, 2.0)]),
            &HashMap::from([(0, PopPRow::new(100.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].process, 29);
        assert_eq!(job.lines[1].process, 4);

        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].process, 4);
        assert_eq!(job.lines[1].process, 29);
    }

    #[test]
    fn plan_moves_a_feeder_in_front_of_its_consumer() {
        let water =
            Process::new(30, "subsistence water", 0).with_output(ProcessOutput::new(2, 2.0, true));
        let forage =
            Process::new(31, "subsistence forage", 0).with_output(ProcessOutput::new(7, 2.0, true));
        let cabins = Process::new(8, "make cabins", 0)
            .with_input(ProcessInput::new(7, 3.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(8, 1.0, true));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(30, Some(0.0), vec![]),
                JobLine::new(31, Some(0.0), vec![]),
                JobLine::new(8, Some(1.0), vec![]),
            ],
        );
        // Water was sent back. Forage sits after the cabin line it feeds.
        job.lines.rotate_right(1);
        let factuals = Factuals::new()
            .with_process(water)
            .with_process(forage)
            .with_process(cabins);
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        history.prices.insert(7, 0.0);
        history.prices.insert(8, 0.0);

        // Two cabin runs take six wood. That stock keeps the cabin line covering.
        job.plan(
            &HashMap::from([(2, 2.0), (8, 1.0)]),
            &HashMap::from([(7, PopPRow::new(6.0))]),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].process, 30);
        assert_eq!(job.lines[1].process, 31);
        assert_eq!(job.lines[2].process, 8);
    }

    #[test]
    fn plan_runs_a_line_when_the_craft_is_none() {
        let mut job = Job::new(0, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());

        // Ten grain finishes the ten cover runs.
        job.plan(
            &HashMap::from([(2, 9.0)]),
            &HashMap::from([(1, PopPRow::new(10.0))]),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );

        // 9 bread short, plus one unit. The line is already at 4, so this night steps toward 10.
        assert_eq!(job.lines[0].target, Some(5.0));
    }

    #[test]
    fn no_lines_skips_plan_reserve_produce_and_buys() {
        let mut job = Job::new(4, vec![]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(5.0))]);

        job.plan(
            &HashMap::from([(2, 9.0)]),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );
        job.reserve(&mut property, &factuals);
        let effects = job.produce(&mut property, &factuals);

        assert!(job.lines.is_empty());
        assert!(job.plan_cost.abs() < 1e-12);
        assert_eq!(property[&1].quantity, 5.0);
        assert_eq!(property[&1].reserved, 0.0);
        assert!(effects.is_empty());
        assert!(job.buy_orders(Actor::Pop(1)).is_empty());
    }

    #[test]
    fn craft_zero_still_reserves_produces_and_buys() {
        let mut job = Job::new(0, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(1.0))]);

        job.reserve(&mut property, &factuals);

        assert_eq!(property[&1].reserved, 1.0);
        let orders = job.buy_orders(Actor::Pop(1));
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 1);
        assert_eq!(orders[0].target_amount, 4.0);

        let effects = job.produce(&mut property, &factuals);
        assert!(effects.is_empty());
        assert_eq!(property[&1].quantity, 0.0);
        assert_eq!(property[&2].quantity, 1.0);
        assert_eq!(property[&2].fresh, 1.0);
        assert_eq!(property[&2].produced, 1.0);
    }

    #[test]
    fn plan_keeps_a_line_whose_process_is_missing() {
        let mut job = Job::new(1, vec![JobLine::new(99, Some(4.0), vec![])]);

        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &Factuals::new(),
            &MarketHistory::new(),
            1.0,
            false,
        );

        // No process, so the line does not pay. The 4 steps toward a cover of 0.
        assert_eq!(job.lines[0].target, Some(3.0));
        assert!(job.plan_cost.abs() < 1e-12);
    }

    #[test]
    fn reserve_claims_free_stock_and_records_the_shortfall() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(1.0))]);

        job.reserve(&mut property, &factuals);

        assert_eq!(property[&1].quantity, 1.0);
        assert_eq!(property[&1].reserved, 1.0);
        assert_eq!(job.shopping[&1], 4.0);
    }

    #[test]
    fn buy_orders_are_the_ceiled_next_run() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(1.0))]);
        job.reserve(&mut property, &factuals);

        let orders = job.buy_orders(Actor::Pop(1));

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 1);
        assert_eq!(orders[0].target_amount, 4.0);
        job.note_purchase(1, 1.0);
        assert_eq!(job.shopping[&1], 3.0);
        assert_eq!(job.buy_orders(Actor::Pop(1))[0].target_amount, 3.0);
        job.note_purchase(1, 10.0);
        assert!(job.shopping.get(&1).is_none());
        assert!(job.buy_orders(Actor::Pop(1)).is_empty());
    }

    #[test]
    fn buy_orders_ceil_a_fractional_next_run() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(0.4), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::new();
        job.reserve(&mut property, &factuals);

        assert_eq!(job.shopping[&1], 0.4);
        assert_eq!(job.buy_orders(Actor::Pop(1))[0].target_amount, 1.0);
    }

    /// Prices at salability 1, so holding matches the card.
    fn board(prices: &[(usize, f64)]) -> MarketHistory {
        let mut history = MarketHistory::new();
        for &(id, price) in prices {
            history.prices.insert(id, price);
            history.salability.insert(id, 1.0);
        }
        history
    }

    /// One time and one grain make one bread.
    ///
    /// Complexity stays under 1 so a few units of time still fund the runs
    /// these tests count. A complexity of 1 would spend the day's points
    /// on a fraction of a run.
    fn timed_bake() -> Process {
        Process::new(1, "bake", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true))
    }

    #[test]
    fn plan_purchases_shops_the_grain_the_time_ceiling_still_needs() {
        let factuals = Factuals::new().with_process(timed_bake());
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(8.0)), (1, PopPRow::new(3.0))]);
        let history = board(&[(0, 1.0), (1, 1.0), (2, 5.0)]);

        let hold = job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert_eq!(job.lines[0].target, Some(8.0));
        assert!((job.shopping_of(1) - 5.0).abs() < 1e-9);
        assert!((hold.get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
        assert_eq!(job.shopping_of(0), 0.0);
    }

    /// # Plan Purchases Writes The Aim The Same Morning
    ///
    /// The bake's time ceiling is eight runs. A line with no target yet
    /// takes that aim and buys the five grain it lacks. A stored zero closes
    /// a quarter of the gap and lands on two. A stored four lands on five.
    /// The turn length does not change that step. The grain those runs use
    /// is held, and only the grain still missing is shopped.
    #[test]
    fn plan_purchases_writes_the_aim_the_same_morning() {
        let mut factuals = Factuals::new().with_process(timed_bake());
        factuals.config.pop.turn_length = 12.0;
        let property = HashMap::from([(0, PopPRow::new(8.0)), (1, PopPRow::new(3.0))]);
        let history = board(&[(0, 1.0), (1, 1.0), (2, 5.0)]);

        let mut fresh = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        fresh.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);
        assert_eq!(fresh.lines[0].target, Some(8.0));
        assert!((fresh.shopping_of(1) - 5.0).abs() < 1e-9);

        let mut resting = Job::new(1, vec![JobLine::new(1, Some(0.0), vec![])]);
        let hold = resting.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);
        assert!((resting.lines[0].target.unwrap_or(0.0) - 2.0).abs() < 1e-9);
        assert!(resting.shopping_of(1).abs() < 1e-9);
        assert!((hold.get(&1).copied().unwrap_or(0.0) - 2.0).abs() < 1e-9);

        let mut mid = Job::new(1, vec![JobLine::new(1, Some(4.0), vec![])]);
        let hold = mid.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);
        assert!((mid.lines[0].target.unwrap_or(0.0) - 5.0).abs() < 1e-9);
        assert!((mid.shopping_of(1) - 2.0).abs() < 1e-9);
        assert!((hold.get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
    }

    /// # Plan Purchases Writes A Zero Aim The Same Morning
    ///
    /// The line cannot run, so the aim is zero. A stored four closes a
    /// quarter of that gap and lands on three. A stored half run steps
    /// under one run, so it sits at zero. The turn length does not change
    /// that step.
    #[test]
    fn plan_purchases_writes_a_zero_aim_the_same_morning() {
        let idle = Process::new(1, "idle", 0)
            .with_input(ProcessInput::new(9, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(10, 1.0, true));
        let mut factuals = Factuals::new().with_process(idle);
        factuals.config.pop.turn_length = 12.0;
        let history = board(&[(9, 5.0), (10, 1.0)]);

        let mut job = Job::new(1, vec![JobLine::new(1, Some(4.0), vec![])]);
        job.plan_purchases(&HashMap::new(), &HashMap::new(), &factuals, &history, &[], 0);
        assert!((job.lines[0].target.unwrap_or(0.0) - 3.0).abs() < 1e-9);

        let mut small = Job::new(1, vec![JobLine::new(1, Some(0.5), vec![])]);
        small.plan_purchases(&HashMap::new(), &HashMap::new(), &factuals, &history, &[], 0);
        assert_eq!(small.lines[0].target, Some(0.0));
    }

    /// One destroyed input and one variable product.
    ///
    /// The output price clears the input and the complexity, so the score
    /// is positive. Nothing of the input is on hand, and the process takes
    /// no time, so the morning aim is zero.
    fn short_make() -> Process {
        Process::new(1, "make", 0)
            .with_input(ProcessInput::new(9, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(10, 1.0, false))
    }

    /// # Plan Purchases Keeps A Profitable Target When The Input Is Missing
    ///
    /// The stored target is eight and the aim is zero. The product was not
    /// offered, so the target stays at eight. A book that sold every offered
    /// unit stays there too. A currency output left on the book is not that
    /// product, and a remainder under one unit is not a missed sale. The
    /// line keeps eight in each of those cases.
    #[test]
    fn plan_purchases_keeps_a_profitable_target_when_the_input_is_missing() {
        let factuals = Factuals::new().with_process(short_make());
        let history = board(&[(9, 1.0), (10, 10.0)]);

        let mut idle = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        idle.plan_purchases(&HashMap::new(), &HashMap::new(), &factuals, &history, &[], 0);
        assert!((idle.lines[0].target.unwrap_or(0.0) - 8.0).abs() < 1e-9);

        let mut cleared = history.clone();
        cleared.mass_offered.insert(10, 8.0);
        cleared.mass_exchanged.insert(10, 8.0);
        let mut sold = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        sold.plan_purchases(&HashMap::new(), &HashMap::new(), &factuals, &cleared, &[], 0);
        assert!((sold.lines[0].target.unwrap_or(0.0) - 8.0).abs() < 1e-9);

        let mut dust = history.clone();
        dust.mass_offered.insert(10, 8.0);
        dust.mass_exchanged.insert(10, 7.2);
        let mut near = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        near.plan_purchases(&HashMap::new(), &HashMap::new(), &factuals, &dust, &[], 0);
        assert!((near.lines[0].target.unwrap_or(0.0) - 8.0).abs() < 1e-9);

        let mut coin = history.clone();
        coin.currencies = vec![10];
        coin.mass_offered.insert(10, 8.0);
        let mut tender = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        tender.plan_purchases(&HashMap::new(), &HashMap::new(), &factuals, &coin, &[], 0);
        assert!((tender.lines[0].target.unwrap_or(0.0) - 8.0).abs() < 1e-9);
    }

    /// # Plan Purchases Eases A Profitable Target When The Product Is Unsold
    ///
    /// The stored target is eight and the aim is zero. Four of the eight
    /// offered units sold, so the line steps a quarter of the way from eight
    /// toward four and lands on seven. A book that sold nothing steps a
    /// quarter of the way toward zero and lands on six. The missing input
    /// does not take it the rest of the way.
    #[test]
    fn plan_purchases_eases_a_profitable_target_when_the_product_is_unsold() {
        let factuals = Factuals::new().with_process(short_make());
        let mut half = board(&[(9, 1.0), (10, 10.0)]);
        half.mass_offered.insert(10, 8.0);
        half.mass_exchanged.insert(10, 4.0);

        let mut job = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        job.plan_purchases(&HashMap::new(), &HashMap::new(), &factuals, &half, &[], 0);
        assert!((job.lines[0].target.unwrap_or(0.0) - 7.0).abs() < 1e-9);

        let mut none = board(&[(9, 1.0), (10, 10.0)]);
        none.mass_offered.insert(10, 8.0);
        let mut idle = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        idle.plan_purchases(&HashMap::new(), &HashMap::new(), &factuals, &none, &[], 0);
        assert!((idle.lines[0].target.unwrap_or(0.0) - 6.0).abs() < 1e-9);
    }

    /// # Plan Purchases Keeps A Target When The Miss Was Unaffordable
    ///
    /// The stored target is eight and the aim is zero. Eight were offered
    /// and nothing sold. Nobody could buy the product, so the morning
    /// keeps eight.
    #[test]
    fn plan_purchases_keeps_a_target_when_the_miss_was_unaffordable() {
        let factuals = Factuals::new().with_process(short_make());
        let mut none = board(&[(9, 1.0), (10, 10.0)]);
        none.mass_offered.insert(10, 8.0);
        none.unaffordable.insert(10);

        let mut job = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        job.plan_purchases(&HashMap::new(), &HashMap::new(), &factuals, &none, &[], 0);
        assert!((job.lines[0].target.unwrap_or(0.0) - 8.0).abs() < 1e-9);
    }

    /// # Plan Purchases Eases A Profitable Target When The Make Outran The Sale
    ///
    /// The book cleared four, and twelve are still free. A well feeds the
    /// line, so that pile counts. It is past the sale grown by a quarter.
    /// The sold share is four sixteenths of eight, and one morning closes a
    /// quarter of the way there. Five free units are only that growth, a
    /// pile with no offer is not a missed sale, and the same twelve without
    /// a feeder stay at eight.
    #[test]
    fn plan_purchases_eases_a_profitable_target_when_the_make_outran_the_sale() {
        let well = Process::new(2, "well", 0).with_output(ProcessOutput::new(9, 1.0, false));
        let factuals = Factuals::new().with_process(short_make()).with_process(well);
        let mut history = board(&[(9, 1.0), (10, 10.0)]);
        history.mass_offered.insert(10, 4.0);
        history.mass_exchanged.insert(10, 4.0);
        let piled = HashMap::from([(10, PopPRow::new(12.0))]);
        let lines = vec![
            JobLine::new(1, Some(8.0), vec![]),
            JobLine::new(2, Some(0.0), vec![]),
        ];

        let mut job = Job::new(1, lines.clone());
        job.plan_purchases(&piled, &HashMap::new(), &factuals, &history, &[], 0);
        let expected = 8.0 + (2.0 - 8.0) * 0.25;
        let target = job
            .lines
            .iter()
            .find(|line| line.process == 1)
            .and_then(|line| line.target)
            .unwrap_or(0.0);
        assert!((target - expected).abs() < 1e-9, "{target}");

        let close_pile = HashMap::from([(10, PopPRow::new(5.0))]);
        let mut close = Job::new(1, lines.clone());
        close.plan_purchases(&close_pile, &HashMap::new(), &factuals, &history, &[], 0);
        let close_target = close
            .lines
            .iter()
            .find(|line| line.process == 1)
            .and_then(|line| line.target)
            .unwrap_or(0.0);
        assert!((close_target - 8.0).abs() < 1e-9, "{close_target}");

        let unlisted = board(&[(9, 1.0), (10, 10.0)]);
        let mut first = Job::new(1, lines.clone());
        first.plan_purchases(&piled, &HashMap::new(), &factuals, &unlisted, &[], 0);
        let first_target = first
            .lines
            .iter()
            .find(|line| line.process == 1)
            .and_then(|line| line.target)
            .unwrap_or(0.0);
        assert!((first_target - 8.0).abs() < 1e-9, "{first_target}");

        let mut alone = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        alone.plan_purchases(
            &piled,
            &HashMap::new(),
            &Factuals::new().with_process(short_make()),
            &history,
            &[],
            0,
        );
        assert!((alone.lines[0].target.unwrap_or(0.0) - 8.0).abs() < 1e-9);

        // Household growth scales the stored eight to sixteen before the step.
        // The sold share is still a quarter, so the aim is four.
        let mut grown = Job::new(1, lines);
        grown.planned_households = 4.0;
        grown.plan_purchases_for(
            &piled,
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            &[],
            0,
            8.0,
        );
        let grown_target = grown
            .lines
            .iter()
            .find(|line| line.process == 1)
            .and_then(|line| line.target)
            .unwrap_or(0.0);
        let grown_expected = 16.0 + (4.0 - 16.0) * 0.25;
        assert!(
            (grown_target - grown_expected).abs() < 1e-9,
            "grown {grown_target}"
        );
    }

    /// # Plan Purchases Scales A Stored Target When The Households Grow
    ///
    /// The book cleared four and nothing is waiting past that sale. The
    /// stored target is eight and the households double. The morning aim
    /// is lower, so the target stays at the doubled sixteen. A feeder
    /// does not change that scale.
    #[test]
    fn plan_purchases_scales_a_stored_target_when_the_households_grow() {
        let well = Process::new(2, "well", 0).with_output(ProcessOutput::new(9, 1.0, false));
        let factuals = Factuals::new().with_process(short_make()).with_process(well);
        let mut history = board(&[(9, 1.0), (10, 10.0)]);
        history.mass_offered.insert(10, 4.0);
        history.mass_exchanged.insert(10, 4.0);
        let lines = vec![
            JobLine::new(1, Some(8.0), vec![]),
            JobLine::new(2, Some(0.0), vec![]),
        ];

        let mut grown = Job::new(1, lines);
        grown.planned_households = 4.0;
        grown.plan_purchases_for(
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            &[],
            0,
            8.0,
        );
        let grown_target = grown
            .lines
            .iter()
            .find(|line| line.process == 1)
            .and_then(|line| line.target)
            .unwrap_or(0.0);
        assert!((grown_target - 16.0).abs() < 1e-9, "fed {grown_target}");

        let mut alone = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        alone.planned_households = 4.0;
        alone.plan_purchases_for(
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &Factuals::new().with_process(short_make()),
            &history,
            &[],
            0,
            8.0,
        );
        assert!(
            (alone.lines[0].target.unwrap_or(0.0) - 16.0).abs() < 1e-9,
            "alone {}",
            alone.lines[0].target.unwrap_or(0.0)
        );
    }

    /// # Stored Step Aim Keeps A Profitable Aim
    ///
    /// The stored target is eight and the morning aim is forty. The score
    /// is positive and nothing was left unsold, so the aim stays forty and
    /// the step starts at eight. One morning closes a quarter of that gap
    /// and lands on sixteen. Half of an eight-unit offer sold, so the aim
    /// is half the stored target and the step still starts there. The same
    /// sale against a stored sixteen aims at eight. A meal above the stored
    /// target is the aim.
    #[test]
    fn stored_step_aim_keeps_a_profitable_aim() {
        let make = Process::new(1, "make", 0).with_output(ProcessOutput::new(10, 1.0, false));
        let factuals = Factuals::new().with_process(make);
        let history = board(&[(10, 10.0)]);
        let line = JobLine::new(1, Some(8.0), vec![]);
        let (aim, current) = super::stored_step_aim(
            &line,
            8.0,
            40.0,
            0.0,
            &factuals,
            &history,
            &HashMap::new(),
        );
        assert!((current - 8.0).abs() < 1e-9, "{current}");
        assert!((aim - 40.0).abs() < 1e-9, "{aim}");
        let stepped = super::step_plan_target(current, aim, super::PLAN_MARCH);
        assert!((stepped - 16.0).abs() < 1e-9, "{stepped}");

        // Half the offer sold. The aim is half of eight, and the step starts at eight.
        let mut unsold = history.clone();
        unsold.mass_offered.insert(10, 8.0);
        unsold.mass_exchanged.insert(10, 4.0);
        let (cut, cut_current) = super::stored_step_aim(
            &line,
            8.0,
            40.0,
            0.0,
            &factuals,
            &unsold,
            &HashMap::new(),
        );
        assert!((cut_current - 8.0).abs() < 1e-9, "{cut_current}");
        assert!((cut - 4.0).abs() < 1e-9, "{cut}");

        // The same sale against a stored sixteen aims at eight.
        let (grown_cut, grown_current) = super::stored_step_aim(
            &line,
            16.0,
            40.0,
            0.0,
            &factuals,
            &unsold,
            &HashMap::new(),
        );
        assert!((grown_current - 16.0).abs() < 1e-9, "{grown_current}");
        assert!((grown_cut - 8.0).abs() < 1e-9, "{grown_cut}");

        // The meal sits above the stored four. Half of an eight-unit offer sold, so the aim is the meal.
        let (covered, covered_current) = super::stored_step_aim(
            &line,
            4.0,
            40.0,
            6.0,
            &factuals,
            &unsold,
            &HashMap::new(),
        );
        assert!((covered_current - 4.0).abs() < 1e-9, "{covered_current}");
        assert!((covered - 6.0).abs() < 1e-9, "{covered}");
    }

    #[test]
    fn plan_purchases_counts_the_meal_out_of_the_leftover() {
        let factuals = Factuals::new().with_process(timed_bake());
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(8.0)), (1, PopPRow::new(3.0))]);
        let meal = HashMap::from([(1, 2.0)]);
        let history = board(&[(0, 1.0), (1, 1.0), (2, 5.0)]);

        let hold = job.plan_purchases(&property, &meal, &factuals, &history, &[], 0);

        assert_eq!(job.lines[0].target, Some(8.0));
        assert!((hold.get(&1).copied().unwrap_or(0.0) - 1.0).abs() < 1e-9);
        assert!((job.shopping_of(1) - 7.0).abs() < 1e-9);
    }

    /// # Plan Purchases Runs The Only Losing Line
    ///
    /// The score is negative, and this is the only line. Eight time still
    /// funds eight runs. The three grain on hand are held, and the five
    /// the day is short are bought. The line does not count as paying.
    #[test]
    fn plan_purchases_runs_the_only_losing_line_up_to_its_time() {
        let factuals = Factuals::new().with_process(timed_bake());
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(8.0)), (1, PopPRow::new(3.0))]);
        let history = board(&[(0, 1.0), (1, 5.0), (2, 1.0)]);

        let hold = job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert!(!job.lines[0].pays);
        assert_eq!(job.lines[0].target, Some(8.0));
        assert!((job.shopping_of(1) - 5.0).abs() < 1e-9);
        assert!((hold.get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
    }

    /// # Plan Purchases Gives The Day To The Line That Pays
    ///
    /// The farm does not pay, and it would take the whole day. The bake
    /// keeps that day as its target and takes the time its grain can
    /// already run. The farm keeps the time that grain leaves. The higher
    /// score is stored first. Produce bakes the grain on hand and the farm
    /// spends what remains.
    #[test]
    fn plan_purchases_gives_the_day_to_the_line_that_pays() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(timed_bake());
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(8.0)), (1, PopPRow::new(3.0))]);
        let history = board(&[(0, 1.0), (1, 1.0), (2, 5.0), (3, 1.0)]);

        let hold = job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert_eq!(job.lines[0].process, 1);
        assert_eq!(job.lines[1].process, 2);
        assert!(job.lines[0].pays);
        assert!(!job.lines[1].pays);
        assert_eq!(job.lines[0].target, Some(8.0));
        assert_eq!(job.lines[1].target, Some(1.25));
        assert!((job.shopping_of(1) - 5.0).abs() < 1e-9);
        assert!((hold.get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);

        let mut stock = property;
        job.produce(&mut stock, &factuals);

        assert!((stock[&2].produced - 3.0).abs() < 1e-9);
        assert!((stock[&3].produced - 1.25).abs() < 1e-9);
    }

    /// # Plan Purchases Keeps The Basic Gap
    ///
    /// The purse has no coin, and the bake does not feed the short good.
    /// The farm keeps the one run that covers that gap. The bake keeps the
    /// time that run leaves.
    #[test]
    fn plan_purchases_keeps_the_basic_gap_beside_the_line_that_pays() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(timed_bake());
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(8.0)), (1, PopPRow::new(3.0))]);
        let history = board(&[(0, 1.0), (1, 1.0), (2, 5.0), (3, 1.0)]);
        let gaps = [(vec![(3, 1.0)], 1.0)];

        let hold = job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &gaps, gaps.len());

        // The bake scores higher, so the morning stores it ahead of the farm.
        assert_eq!(job.lines[0].process, 1);
        assert_eq!(job.lines[1].process, 2);
        assert_eq!(job.lines[0].target, Some(4.0));
        assert_eq!(job.lines[1].target, Some(1.0));
        assert!((job.shopping_of(1) - 1.0).abs() < 1e-9);
        assert!((hold.get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
    }

    /// # Plan Purchases Keeps The Basic Gap When The Purse Is Open
    ///
    /// A whole coin is on hand, and the bake pays. The short good is still
    /// the farm's output, so the farm keeps the one run that covers it.
    /// The bake keeps the time that run leaves and buys the grain the coin pays.
    #[test]
    fn plan_purchases_keeps_the_basic_gap_when_the_purse_is_open() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(timed_bake());
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([
            (0, PopPRow::new(8.0)),
            (1, PopPRow::new(3.0)),
            (4, PopPRow::new(2.0)),
        ]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 5.0), (3, 1.0), (4, 1.0)]);
        history.currencies = vec![4];
        let gaps = [(vec![(3, 1.0)], 1.0)];

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &gaps, gaps.len());

        // The bake scores higher, so the morning stores it ahead of the farm.
        assert_eq!(job.lines[0].process, 1);
        assert_eq!(job.lines[1].process, 2);
        assert!(job.lines[0].pays);
        assert!(!job.lines[1].pays);
        assert_eq!(job.lines[0].target, Some(4.0));
        assert_eq!(job.lines[1].target, Some(1.0));
        assert_eq!(job.lines[1].idle_days, 0);
        assert!((job.shopping_of(1) - 1.0).abs() < 1e-9);
        assert!((job.unlisted_of(4) - 1.0).abs() < 1e-6);
    }

    /// # Plan Purchases Shares A Losing Day
    ///
    /// Both lines lose. Sixteen time is four runs, and the spare day is
    /// split, so each line aims at two. A sought bid does not pull that
    /// share onto one line. The lines do not count as paying.
    #[test]
    fn plan_purchases_shares_the_spare_day_when_every_line_loses() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let forage = Process::new(3, "forage", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 1.0, true));
        let factuals = Factuals::new().with_process(farm).with_process(forage);
        let property = HashMap::from([(0, PopPRow::new(16.0))]);
        let mut history = board(&[(0, 1.0), (3, 0.01), (7, 0.01)]);
        let mut job = Job::new(
            1,
            vec![JobLine::new(2, None, vec![]), JobLine::new(3, None, vec![])],
        );

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert!(!job.lines[0].pays);
        assert!(!job.lines[1].pays);
        assert!((job.lines[0].target.unwrap_or(0.0) - 2.0).abs() < 1e-9);
        assert!((job.lines[1].target.unwrap_or(0.0) - 2.0).abs() < 1e-9);

        history.sought.insert(3, 10.0);
        let mut sought = Job::new(
            1,
            vec![JobLine::new(2, None, vec![]), JobLine::new(3, None, vec![])],
        );
        sought.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);
        assert!((sought.lines[0].target.unwrap_or(0.0) - 2.0).abs() < 1e-9);
        assert!((sought.lines[1].target.unwrap_or(0.0) - 2.0).abs() < 1e-9);
    }

    /// # Plan Purchases Gives Every Losing Line A Point Share
    ///
    /// Sixteen time funds a quarter point. The farm spends a quarter point
    /// a run and the buckets spend one. Each line gets half the points, so
    /// the farm aims at half a run and the buckets at an eighth. The farm
    /// is subsistence and still keeps that aim.
    #[test]
    fn plan_purchases_gives_every_losing_line_a_point_share() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let buckets = Process::new(4, "buckets", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(8, 1.0, true));
        let factuals = Factuals::new().with_process(farm).with_process(buckets);
        let mut job = Job::new(
            1,
            vec![JobLine::new(2, None, vec![]), JobLine::new(4, None, vec![])],
        );
        let property = HashMap::from([(0, PopPRow::new(16.0))]);
        let history = board(&[(0, 1.0), (3, 0.01), (8, 0.01)]);

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert!(!job.lines[0].pays);
        assert!(!job.lines[1].pays);
        assert!((job.lines[0].target.unwrap_or(0.0) - 0.5).abs() < 1e-9, "farm");
        assert!(
            (job.lines[1].target.unwrap_or(0.0) - 0.125).abs() < 1e-9,
            "buckets {}",
            job.lines[1].target.unwrap_or(0.0)
        );
    }

    /// # Plan Purchases Adds Runs For A Sought Output
    ///
    /// Both lines lose. The farm's gap holds one run, and the spare day is
    /// split across the two lines. A sought bid does not move that share.
    #[test]
    fn plan_purchases_adds_runs_for_a_sought_output() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let forage = Process::new(3, "forage", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 1.0, true));
        let factuals = Factuals::new().with_process(farm).with_process(forage);
        let property = HashMap::from([(0, PopPRow::new(16.0))]);
        let gaps = [(vec![(3, 1.0)], 1.0)];
        let mut quiet = board(&[(0, 1.0), (3, 0.01), (7, 0.01)]);
        let mut job = Job::new(
            1,
            vec![JobLine::new(2, None, vec![]), JobLine::new(3, None, vec![])],
        );
        job.plan_purchases(&property, &HashMap::new(), &factuals, &quiet, &gaps, gaps.len());
        assert!((job.lines[0].target.unwrap_or(0.0) - 2.5).abs() < 1e-9);
        assert!((job.lines[1].target.unwrap_or(0.0) - 1.5).abs() < 1e-9);

        quiet.sought.insert(3, 10.0);
        let mut job = Job::new(
            1,
            vec![JobLine::new(2, None, vec![]), JobLine::new(3, None, vec![])],
        );
        job.plan_purchases(&property, &HashMap::new(), &factuals, &quiet, &gaps, gaps.len());
        assert!((job.lines[0].target.unwrap_or(0.0) - 2.5).abs() < 1e-9);
        assert!((job.lines[1].target.unwrap_or(0.0) - 1.5).abs() < 1e-9);
    }

    /// # Plan Purchases Moves Subsistence Surplus Onto The Sought Good
    ///
    /// The farm pays and would take three runs. Water does not pay and
    /// keeps the one run its gap holds. Ten units of water were still
    /// sought. The farm gives up a quarter of the runs above that hold.
    /// Water is already running, so it adds a quarter of its own run.
    /// Time left after that add goes back onto the farm.
    #[test]
    fn plan_purchases_moves_subsistence_surplus_onto_the_sought_good() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let water = Process::new(3, "water", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(farm).with_process(water);
        let property = HashMap::from([(0, PopPRow::new(16.0))]);
        let gaps = [(vec![(2, 1.0)], 1.0)];
        let mut history = board(&[(0, 1.0), (2, 0.01), (3, 10.0)]);
        let mut quiet = Job::new(
            1,
            vec![JobLine::new(2, None, vec![]), JobLine::new(3, None, vec![])],
        );
        quiet.plan_purchases(&property, &HashMap::new(), &factuals, &history, &gaps, gaps.len());
        assert_eq!(quiet.lines[0].target, Some(3.0));
        assert_eq!(quiet.lines[1].target, Some(1.0));

        history.sought.insert(2, 10.0);
        let mut job = Job::new(
            1,
            vec![JobLine::new(2, None, vec![]), JobLine::new(3, None, vec![])],
        );
        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &gaps, gaps.len());
        assert!((job.lines[0].target.unwrap_or(0.0) - 2.75).abs() < 1e-9);
        assert!((job.lines[1].target.unwrap_or(0.0) - 1.25).abs() < 1e-9);
    }

    /// # Plan Purchases Starts A Sought Subsistence Line
    ///
    /// The farm pays and takes the day. Water does not pay and has no gap,
    /// so it leaves the same morning. Two units of water were still
    /// sought. The farm gives up a quarter of its runs. Water at zero
    /// starts that bid, and only the freed run of time is there to take.
    #[test]
    fn plan_purchases_starts_a_sought_subsistence_line_that_was_at_zero() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let water = Process::new(3, "water", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let mut factuals = Factuals::new().with_process(farm).with_process(water);
        factuals.config.pop.turn_length = 4.0;
        let property = HashMap::from([(0, PopPRow::new(16.0))]);
        let mut history = board(&[(0, 1.0), (2, 0.01), (3, 10.0)]);
        let mut quiet = Job::new(
            1,
            vec![JobLine::new(2, None, vec![]), JobLine::new(3, None, vec![])],
        );
        quiet.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);
        assert_eq!(quiet.lines.len(), 1);
        assert_eq!(quiet.lines[0].process, 2);
        assert_eq!(quiet.lines[0].target, Some(4.0));

        history.sought.insert(2, 2.0);
        let mut job = Job::new(
            1,
            vec![JobLine::new(2, None, vec![]), JobLine::new(3, None, vec![])],
        );
        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);
        assert_eq!(job.lines[0].target, Some(3.0));
        assert_eq!(job.lines[1].target, Some(1.0));
    }

    /// # Plan Purchases Keeps A Currency Input
    ///
    /// Grain is a tender and the bake takes one per run. Three on hand fund
    /// three runs. Those three stay in working stock and off the sell book,
    /// and the line does not buy more grain.
    #[test]
    fn plan_purchases_holds_a_currency_input() {
        let factuals = Factuals::new().with_process(timed_bake());
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(8.0)), (1, PopPRow::new(3.0))]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 5.0)]);
        history.currencies = vec![1];

        let hold = job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert!((job.lines[0].target.unwrap_or(0.0) - 3.0).abs() < 1e-6);
        assert_eq!(job.shopping_of(1), 0.0);
        assert!(hold.get(&1).copied().unwrap_or(0.0) < 1e-9);
        assert!((job.unlisted_of(1) - 3.0).abs() < 1e-6);
    }

    /// # Plan Purchases Buys Grain With Wood When Gold Is Not Offered
    ///
    /// Grain is a tender and gold was not offered. The bake has wood and no
    /// grain, so it buys the grain the wood can pay for and keeps that wood
    /// off the sell book.
    #[test]
    fn plan_purchases_buys_grain_with_wood_when_gold_is_not_offered() {
        let factuals = Factuals::new()
            .with_process(timed_bake())
            .with_good(good(7, 0.0));
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(4.0)), (7, PopPRow::new(3.0))]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 5.0), (4, 1.0), (7, 1.0)]);
        history.currencies = vec![1, 4];

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        let target = job.lines[0].target.unwrap_or(0.0);
        assert!(target > 2.0, "target {target}");
        assert!(job.shopping_of(1) >= 2.0, "grain {}", job.shopping_of(1));
        assert!(job.unlisted_of(7) >= 2.0, "wood {}", job.unlisted_of(7));
    }

    /// # Plan Purchases Buys Grain With Wood When Gold Was Offered
    ///
    /// Gold was offered. The bake has wood and no grain, so other goods still
    /// pay for the grain and that wood stays off the sell book.
    #[test]
    fn plan_purchases_buys_grain_with_wood_when_gold_was_offered() {
        let factuals = Factuals::new()
            .with_process(timed_bake())
            .with_good(good(7, 0.0));
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(4.0)), (7, PopPRow::new(3.0))]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 5.0), (4, 1.0), (7, 1.0)]);
        history.currencies = vec![1, 4];
        history.mass_offered.insert(4, 1.0);

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        let target = job.lines[0].target.unwrap_or(0.0);
        assert!(target > 2.0, "target {target}");
        assert!(job.shopping_of(1) >= 2.0, "grain {}", job.shopping_of(1));
        assert!(job.unlisted_of(7) >= 2.0, "wood {}", job.unlisted_of(7));
    }

    /// # Plan Purchases Buys More Grain When The Bread Pays More
    ///
    /// Gold was offered and the bake holds gold, not grain. A loss still
    /// aims at the eight runs the time funds, and it does not spend the
    /// gold on grain. A thin profit buys a few runs. A large profit buys
    /// more of the time ceiling, and the gold that pays stays off the sell book.
    #[test]
    fn plan_purchases_buys_more_grain_when_the_bread_pays_more() {
        let factuals = Factuals::new().with_process(timed_bake());
        let bake_at = |bread: f64| {
            let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
            let property = HashMap::from([(0, PopPRow::new(8.0)), (4, PopPRow::new(6.0))]);
            let mut history = board(&[(0, 1.0), (1, 1.0), (2, bread), (4, 1.0)]);
            history.currencies = vec![1, 4];
            history.mass_offered.insert(4, 1.0);
            job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);
            job
        };

        let loss = bake_at(1.0);
        let thin = bake_at(3.0);
        let rich = bake_at(20.0);
        let thin_runs = thin.lines[0].target.unwrap_or(0.0);
        let rich_runs = rich.lines[0].target.unwrap_or(0.0);

        assert!(
            (loss.lines[0].target.unwrap_or(0.0) - 8.0).abs() < 1e-6,
            "loss {}",
            loss.lines[0].target.unwrap_or(0.0)
        );
        assert_eq!(loss.shopping_of(1), 0.0);
        assert!(thin_runs > 0.5, "thin {thin_runs}");
        assert!(
            rich_runs > thin_runs + 2.0,
            "thin {thin_runs} rich {rich_runs}"
        );
        assert!(rich.shopping_of(1) >= 1.0, "grain {}", rich.shopping_of(1));
        assert!(rich.unlisted_of(4) >= 1.0, "gold {}", rich.unlisted_of(4));
        assert!(thin.unlisted_of(4) + 1e-6 < rich.unlisted_of(4));
    }

    /// # Fit Shopping To Purse Buys The Runs The Coin Can Fund
    ///
    /// Five grain has to stay in the recipe and pay for the water. The list
    /// stops at the runs that still leave that grain.
    #[test]
    fn fit_shopping_to_purse_buys_the_runs_the_coin_can_fund() {
        let bake = Process::new(1, "bake", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(3, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(bake);
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(8.0)), (1, PopPRow::new(5.0))]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 5.0), (3, 1.0)]);
        history.currencies = vec![1];

        let hold = job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);
        let hand = HashMap::from([(0, 8.0), (1, 5.0)]);
        assert!(hold.get(&1).copied().unwrap_or(0.0) < 1e-9);
        assert!(
            (job.unlisted_of(1) - 4.0).abs() < 1.01,
            "grain {}",
            job.unlisted_of(1)
        );
        job.fit_shopping_to_purse(&hand, &factuals, &history, &[1]);

        assert!((job.shopping_of(3) - 2.0).abs() < 1e-6, "water {}", job.shopping_of(3));
        assert_eq!(job.shopping_of(1), 0.0);
    }

    /// # Plan Purchases Keeps A Profitable Line When The Book Missed
    ///
    /// The bread was offered and did not trade. The score is still positive,
    /// so the line keeps the day and shops the grain. A side output that was
    /// not on the book does not change that.
    #[test]
    fn plan_purchases_keeps_a_profitable_line_when_the_book_missed() {
        let bake = timed_bake().with_output(ProcessOutput::new(9, 0.01, true));
        let factuals = Factuals::new().with_process(bake);
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(8.0)), (1, PopPRow::new(3.0))]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 5.0)]);
        history.mass_offered.insert(2, 10.0);

        let hold = job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert!(job.lines[0].pays);
        assert_eq!(job.lines[0].target, Some(8.0));
        assert!((job.shopping_of(1) - 5.0).abs() < 1e-9);
        assert!((hold.get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
    }

    /// # Plan Purchases Runs An Unsold Losing Line
    ///
    /// The bread was offered and did not trade, and the score is negative.
    /// Nothing else pays, so the line still takes the eight runs the time
    /// funds and buys the grain those runs are short.
    #[test]
    fn plan_purchases_runs_an_unsold_losing_line_when_nothing_else_pays() {
        let factuals = Factuals::new().with_process(timed_bake());
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(8.0)), (1, PopPRow::new(3.0))]);
        let mut history = board(&[(0, 1.0), (1, 5.0), (2, 1.0)]);
        history.mass_offered.insert(2, 10.0);

        let hold = job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert!(!job.lines[0].pays);
        assert_eq!(job.lines[0].target, Some(8.0));
        assert!((job.shopping_of(1) - 5.0).abs() < 1e-9);
        assert!((hold.get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
    }

    /// # Plan Purchases Keeps A Line That Sold Enough
    ///
    /// The score is negative. Four of the ten offered units traded, so the
    /// line keeps the day and shops the grain. Complexity stays at 1 so the
    /// sale can mark it. Eight labor-days fund the eight runs.
    #[test]
    fn plan_purchases_keeps_a_line_that_sold_enough() {
        let factuals = Factuals::new().with_process(timed_bake().with_complexity(1.0));
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(512.0)), (1, PopPRow::new(3.0))]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 1.0)]);
        history.mass_offered.insert(2, 10.0);
        history.mass_exchanged.insert(2, 4.0);

        let hold = job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert!(job.lines[0].pays);
        assert_eq!(job.lines[0].target, Some(8.0));
        assert!((job.shopping_of(1) - 5.0).abs() < 1e-9);
        assert!((hold.get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
    }

    /// # Plan Purchases Gives The Day To The Specialty
    ///
    /// Subsistence farm scores above zero, and so does the mill. The mill
    /// was offered and did not trade. The farm drops its mark and the mill
    /// keeps the day. The pile is eight labor-days, so the mill's one point
    /// per run still fits eight runs, and those runs spend the points.
    #[test]
    fn plan_purchases_gives_the_day_to_the_specialty_when_subsistence_also_scores() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 1.0, true));
        let mill = Process::new(1, "mill", 0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let factuals = Factuals::new().with_process(farm).with_process(mill);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(512.0))]);
        let mut history = board(&[(0, 1.0), (1, 5.0)]);
        history.mass_offered.insert(1, 10.0);

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        // The mill scores higher, so the morning stores it ahead of the farm.
        assert_eq!(job.lines[0].process, 1);
        assert_eq!(job.lines[1].process, 2);
        assert!(job.lines[0].pays);
        assert!(!job.lines[1].pays);
        assert_eq!(job.lines[0].target, Some(8.0));
        assert_eq!(job.lines[1].target, Some(0.0));
    }

    /// # Plan Purchases Keeps Subsistence When The Specialty Does Not Pay
    ///
    /// The mill's score is negative and its bread did not trade. The farm's
    /// score is positive, so the farm keeps the day. The mill sits at zero
    /// and leaves the same morning. The farm spends 0.0625 a run, so the
    /// two runs the time allows also spend the day's points.
    #[test]
    fn plan_purchases_keeps_subsistence_when_the_specialty_does_not_pay() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.0625)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 1.0, true));
        let mill = Process::new(1, "mill", 0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let mut factuals = Factuals::new().with_process(farm).with_process(mill);
        factuals.config.pop.turn_length = 12.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(8.0))]);
        let mut history = board(&[(0, 1.0), (1, 5.0), (2, 0.5)]);
        history.mass_offered.insert(2, 10.0);

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert!(job.lines[0].pays);
        assert_eq!(job.lines.len(), 1);
        assert_eq!(job.lines[0].process, 2);
        assert_eq!(job.lines[0].target, Some(2.0));
    }

    /// One destroyed input the pile does not hold, so the line cannot run.
    ///
    /// `id` is the process. `output` is the good it makes. The score is that
    /// good's price minus the shared input's price.
    fn losing(id: usize, output: usize) -> Process {
        Process::new(id, "lose", 0)
            .with_input(ProcessInput::new(9, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(output, 1.0, true))
    }

    /// # Plan Purchases Drops One Idle Line A Day
    ///
    /// Three lines sit at zero and none of them pay. The turn is one morning.
    /// The first plan drops the worst score. The next drops the worse of
    /// what remains. The last line stays, and its count keeps growing.
    #[test]
    fn plan_purchases_drops_one_idle_line_a_day_and_keeps_the_last() {
        let mut factuals = Factuals::new()
            .with_process(losing(1, 10))
            .with_process(losing(2, 11))
            .with_process(losing(3, 12));
        factuals.config.pop.turn_length = 1.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, None, vec![]),
                JobLine::new(2, None, vec![]),
                JobLine::new(3, None, vec![]),
            ],
        );
        let history = board(&[(9, 5.0), (10, 4.0), (11, 1.0), (12, 2.0)]);
        let property = HashMap::new();

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert_eq!(
            job.lines.iter().map(|line| line.process).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(job.lines[0].idle_days, 1);
        assert_eq!(job.lines[1].idle_days, 1);

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert_eq!(job.lines.len(), 1);
        assert_eq!(job.lines[0].process, 1);
        assert_eq!(job.lines[0].idle_days, 2);

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert_eq!(job.lines.len(), 1);
        assert_eq!(job.lines[0].process, 1);
        assert_eq!(job.lines[0].target, Some(0.0));
        assert_eq!(job.lines[0].idle_days, 3);
    }

    /// # Plan Purchases Drops The Lower Process On A Score Tie
    ///
    /// Both lines score the same and have sat at zero for a turn. The lower
    /// process id is the one that leaves.
    #[test]
    fn plan_purchases_drops_the_lower_process_when_the_scores_tie() {
        let mut factuals = Factuals::new()
            .with_process(losing(5, 10))
            .with_process(losing(1, 11));
        factuals.config.pop.turn_length = 1.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(5, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let history = board(&[(9, 5.0), (10, 2.0), (11, 2.0)]);

        job.plan_purchases(&HashMap::new(), &HashMap::new(), &factuals, &history, &[], 0);

        assert_eq!(job.lines.len(), 1);
        assert_eq!(job.lines[0].process, 5);
        assert_eq!(job.lines[0].idle_days, 1);
    }

    /// # Plan Purchases Drops The Longer Idle Streak
    ///
    /// Two lines have already sat out. A third line is new. One morning
    /// drops the worse of the longer streaks. The new line stays, and its
    /// count starts at one.
    #[test]
    fn plan_purchases_drops_the_longer_idle_streak() {
        let mut factuals = Factuals::new()
            .with_process(losing(1, 10))
            .with_process(losing(2, 11))
            .with_process(losing(3, 12));
        factuals.config.pop.turn_length = 12.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, None, vec![]),
                JobLine::new(2, None, vec![]),
            ],
        );
        job.lines[0].idle_days = 2;
        job.lines[1].idle_days = 2;
        let history = board(&[(9, 5.0), (10, 4.0), (11, 1.0), (12, 2.0)]);
        let property = HashMap::new();

        job.lines.push(JobLine::new(3, None, vec![]));
        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert_eq!(
            job.lines
                .iter()
                .map(|line| (line.process, line.idle_days))
                .collect::<Vec<_>>(),
            vec![(1, 3), (3, 1)]
        );
    }

    /// # Plan Purchases Keeps A Positive Score At Zero
    ///
    /// The farm scores above zero and the mill takes the day, so the farm's
    /// target stays at zero. A one-morning turn does not drop it.
    #[test]
    fn plan_purchases_keeps_a_positive_score_at_zero() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 1.0, true));
        let mill = Process::new(1, "mill", 0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let mut factuals = Factuals::new().with_process(farm).with_process(mill);
        factuals.config.pop.turn_length = 1.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(8.0))]);
        let mut history = board(&[(0, 1.0), (1, 5.0)]);
        history.mass_offered.insert(1, 10.0);

        for _ in 0..3 {
            job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);
        }

        assert_eq!(job.lines.len(), 2);
        // The mill scores higher, so the morning stores it ahead of the farm.
        assert_eq!(job.lines[0].process, 1);
        assert!(job.lines[0].pays);
        assert_eq!(job.lines[1].process, 2);
        assert_eq!(job.lines[1].target, Some(0.0));
        assert_eq!(job.lines[1].idle_days, 0);
    }

    /// # Plan Purchases Keeps A Sold Line At Zero
    ///
    /// Clay's score is negative and it already sold, so it still pays.
    /// Coal scores higher, so it is stored first and buys first. Coal keeps
    /// the eight runs and clay's target stays at zero. A one-morning turn
    /// does not drop clay.
    #[test]
    fn plan_purchases_keeps_a_sold_line_at_zero() {
        let clay = Process::new(1, "clay", 0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let coal = Process::new(2, "coal", 0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let mut factuals = Factuals::new().with_process(clay).with_process(coal);
        factuals.config.pop.turn_length = 1.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, None, vec![]),
                JobLine::new(2, None, vec![]),
            ],
        );
        // Eight labor-days fund the eight complexity-1 runs coal buys first.
        let property = HashMap::from([(0, PopPRow::new(512.0))]);
        let mut history = board(&[(0, 1.0), (3, 0.5), (4, 5.0)]);
        history.mass_offered.insert(3, 10.0);
        history.mass_exchanged.insert(3, 4.0);

        for _ in 0..3 {
            job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);
        }

        assert_eq!(job.lines.len(), 2);
        assert_eq!(job.lines[0].process, 2);
        assert_eq!(job.lines[1].process, 1);
        assert_eq!(job.lines[0].target, Some(8.0));
        assert_eq!(job.lines[1].target, Some(0.0));
        assert_eq!(job.lines[0].idle_days, 0);
        assert_eq!(job.lines[1].idle_days, 0);
        assert!(job.lines[0].pays);
        assert!(job.lines[1].pays);
    }

    /// # Plan Purchases Keeps A Reserved Line That Cannot Run
    ///
    /// The farm is the line that would feed the gap, and it is missing its
    /// capital, so the target stays at zero. The mill cannot run and does
    /// not feed, so it leaves the same morning. The farm's count stays at
    /// zero.
    #[test]
    fn plan_purchases_keeps_a_reserved_line_that_cannot_run() {
        let farm = Process::new(2, "farm", 0)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(6, 1.0, true, InputType::Capital, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let mill = Process::new(1, "mill", 0)
            .with_input(ProcessInput::new(8, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let mut factuals = Factuals::new().with_process(farm).with_process(mill);
        factuals.config.pop.turn_length = 12.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(8.0))]);
        let history = board(&[(0, 1.0), (2, 0.5), (3, 1.0), (8, 2.0)]);
        let gaps = [(vec![(3, 1.0)], 1.0)];

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &gaps, gaps.len());

        assert_eq!(job.lines.len(), 1);
        assert_eq!(job.lines[0].process, 2);
        assert_eq!(job.lines[0].target, Some(0.0));
        assert_eq!(job.lines[0].idle_days, 0);
    }

    /// # Plan Purchases Gives The Day To The Higher Score
    ///
    /// Both lines pay and neither feeds the other. Coal scores higher, so
    /// it is stored first and buys first. It keeps the eight runs. Clay is
    /// next and the time is gone.
    #[test]
    fn plan_purchases_gives_the_day_to_the_higher_score() {
        let clay = Process::new(1, "clay", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let coal = Process::new(2, "coal", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let factuals = Factuals::new().with_process(clay).with_process(coal);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, None, vec![]),
                JobLine::new(2, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(8.0))]);
        let history = board(&[(0, 1.0), (3, 2.0), (4, 5.0)]);

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert_eq!(job.lines[0].process, 2);
        assert_eq!(job.lines[1].process, 1);
        assert!(job.lines[0].pays);
        assert!(job.lines[1].pays);
        assert_eq!(job.lines[0].target, Some(8.0));
        assert_eq!(job.lines[1].target, Some(0.0));
    }

    /// # Plan Purchases Moves The Day When The Higher Score Flips
    ///
    /// Clay and coal do not feed each other. The higher score is stored
    /// first and buys first, so it keeps the eight runs. Swapping which
    /// output is worth more swaps who is stored first and who keeps them.
    #[test]
    fn plan_purchases_moves_the_day_when_the_higher_score_flips() {
        let clay = Process::new(1, "clay", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let coal = Process::new(2, "coal", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let factuals = Factuals::new().with_process(clay).with_process(coal);
        let property = HashMap::from([(0, PopPRow::new(8.0))]);
        let mut coal_first = Job::new(
            1,
            vec![
                JobLine::new(1, None, vec![]),
                JobLine::new(2, None, vec![]),
            ],
        );
        coal_first.plan_purchases(
            &property,
            &HashMap::new(),
            &factuals,
            &board(&[(0, 1.0), (3, 2.0), (4, 5.0)]),
            &[],
            0,
        );
        let mut clay_first = Job::new(
            1,
            vec![
                JobLine::new(1, None, vec![]),
                JobLine::new(2, None, vec![]),
            ],
        );
        clay_first.plan_purchases(
            &property,
            &HashMap::new(),
            &factuals,
            &board(&[(0, 1.0), (3, 9.0), (4, 2.0)]),
            &[],
            0,
        );

        assert_eq!(coal_first.lines[0].process, 2);
        assert_eq!(coal_first.lines[1].process, 1);
        assert_eq!(clay_first.lines[0].process, 1);
        assert_eq!(clay_first.lines[1].process, 2);
        assert_eq!(coal_first.lines[0].target, Some(8.0));
        assert_eq!(coal_first.lines[1].target, Some(0.0));
        assert_eq!(clay_first.lines[0].target, Some(8.0));
        assert_eq!(clay_first.lines[1].target, Some(0.0));
    }

    /// # Plan Purchases Runs The Grain Line Before The Bake
    ///
    /// Baking consumes grain, so the grain line is stored first and buys
    /// first even though bread scores higher. Grain keeps the eight runs.
    /// The bake is next and the time is gone.
    #[test]
    fn plan_purchases_runs_the_grain_line_before_the_bake() {
        let grain = Process::new(1, "grain", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 1.0, true));
        let bake = Process::new(2, "bake", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(grain).with_process(bake);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(8.0))]);
        let history = board(&[(0, 1.0), (1, 2.0), (2, 10.0)]);

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert_eq!(job.lines[0].process, 1);
        assert_eq!(job.lines[1].process, 2);
        assert_eq!(job.lines[0].target, Some(8.0));
        assert_eq!(job.lines[1].target, Some(0.0));
    }

    /// # Plan Purchases Keeps A Paying Need
    ///
    /// Both lines pay, and cotton scores higher. No specialty pays, so both
    /// short desires are held. Each desire is one run of its own line.
    /// Twelve time covers those two runs and one run left over. Cotton is
    /// stored first and buys first, so it keeps that spare run. The farm
    /// keeps the one run its desire already held.
    #[test]
    fn plan_purchases_keeps_a_paying_need_ahead_of_the_higher_score() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let cotton = Process::new(4, "cotton", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(5, 1.0, true));
        let factuals = Factuals::new().with_process(farm).with_process(cotton);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(4, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(12.0))]);
        let history = board(&[(0, 1.0), (3, 6.0), (5, 9.0)]);
        let gaps = [(vec![(3, 1.0)], 1.0), (vec![(5, 1.0)], 1.0)];

        job.plan_purchases(
            &property,
            &HashMap::new(),
            &factuals,
            &history,
            &gaps,
            1,
        );

        assert_eq!(job.lines[0].process, 4);
        assert_eq!(job.lines[1].process, 2);
        assert!(job.lines[0].pays);
        assert!(job.lines[1].pays);
        assert!((job.lines[0].target.unwrap_or(0.0) - 2.0).abs() < 1e-9);
        assert!((job.lines[1].target.unwrap_or(0.0) - 1.0).abs() < 1e-9);
    }

    /// # Plan Purchases Leaves A Later Need When A Specialty Pays
    ///
    /// The mill pays and is not subsistence. The basic gap is one farm run.
    /// The later gap is forage. The mill keeps the time that farm run leaves.
    /// Forage sits at zero and leaves the same morning.
    #[test]
    fn plan_purchases_leaves_a_later_need_when_a_specialty_pays() {
        let farm = Process::new(3, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let forage = Process::new(4, "forage", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 1.0, true));
        let mill = Process::new(1, "mill", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let mut factuals = Factuals::new()
            .with_process(farm)
            .with_process(forage)
            .with_process(mill);
        factuals.config.pop.turn_length = 12.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(3, None, vec![]),
                JobLine::new(4, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(512.0))]);
        let history = board(&[(0, 1.0), (2, 5.0), (3, 6.0), (7, 0.5)]);
        let gaps = [(vec![(3, 1.0)], 1.0), (vec![(7, 1.0)], 1.0)];

        job.plan_purchases(
            &property,
            &HashMap::new(),
            &factuals,
            &history,
            &gaps,
            1,
        );

        assert_eq!(job.lines.len(), 2);
        // The mill scores higher, so the morning stores it ahead of the farm.
        assert_eq!(job.lines[0].process, 1);
        assert!(job.lines[0].pays);
        assert!(job.lines[0].target.unwrap_or(0.0) > 1.0);
        assert_eq!(job.lines[1].process, 3);
        assert!(!job.lines[1].pays);
        assert_eq!(job.lines[1].target, Some(1.0));
    }

    /// # Plan Purchases Keeps A Later Tier Of A Basic Good
    ///
    /// The mill pays. The basic gap is one farm run, and the next gap asks
    /// for that same good, so the farm keeps both. Forage asks for a
    /// different good, sits at zero, and leaves the same morning.
    #[test]
    fn plan_purchases_keeps_a_later_tier_of_a_basic_good() {
        let farm = Process::new(3, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let forage = Process::new(4, "forage", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 1.0, true));
        let mill = Process::new(1, "mill", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let mut factuals = Factuals::new()
            .with_process(farm)
            .with_process(forage)
            .with_process(mill);
        factuals.config.pop.turn_length = 12.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(3, None, vec![]),
                JobLine::new(4, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(512.0))]);
        let history = board(&[(0, 1.0), (2, 5.0), (3, 6.0), (7, 0.5)]);
        let gaps = [
            (vec![(3, 1.0)], 1.0),
            (vec![(3, 1.0)], 1.0),
            (vec![(7, 1.0)], 1.0),
        ];

        job.plan_purchases(
            &property,
            &HashMap::new(),
            &factuals,
            &history,
            &gaps,
            1,
        );

        assert_eq!(job.lines.len(), 2);
        // The mill scores higher, so the morning stores it ahead of the farm.
        assert_eq!(job.lines[0].process, 1);
        assert!(job.lines[0].target.unwrap_or(0.0) > 1.0);
        assert_eq!(job.lines[1].process, 3);
        assert_eq!(job.lines[1].target, Some(2.0));
    }

    /// # Plan Purchases Bakes The Common Meal Beside Grain
    ///
    /// Basic food stays on the farm. Common bread is a later desire the
    /// farm cannot make, so the bread line keeps that meal and the bread
    /// the basic desire will eat first. The water line keeps the drinking
    /// runs and the water those bread runs destroy. Four points are enough
    /// at the subsistence floor.
    #[test]
    fn plan_purchases_bakes_the_common_meal_beside_grain() {
        let farm = Process::new(29, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 12.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 3.0, false));
        let water = Process::new(30, "water", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 12.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 3.0, false));
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(2, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 3.0, false));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(water)
            .with_process(bread);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(29, None, vec![]),
                JobLine::new(30, None, vec![]),
                JobLine::new(3, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(4.0 * TIME_PER_LABOR))]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 1.0), (3, 5.0)]);
        // Grain is tender, and none is on hand, so bread is not funded past the meal.
        history.currencies = vec![1];
        let gaps = [
            (vec![(1, 1.0), (3, 2.0)], 6.0),
            (vec![(2, 1.0)], 6.0),
            (vec![(3, 1.0)], 6.0),
        ];

        job.plan_purchases(
            &property,
            &HashMap::new(),
            &factuals,
            &history,
            &gaps,
            2,
        );

        let farm_runs = job.lines[0].target.unwrap_or(0.0);
        let water_runs = job.lines[1].target.unwrap_or(0.0);
        let bread_runs = job.lines[2].target.unwrap_or(0.0);
        assert!((farm_runs - 2.0).abs() < 1e-6, "farm {farm_runs}");
        assert!((water_runs - 3.0).abs() < 1e-6, "water {water_runs}");
        assert!(bread_runs + 1e-6 >= 3.0, "bread {bread_runs}");
        assert!(job.lines[2].cover_cheap);
    }

    /// # Plan Purchases Bakes Bread The Grain Pile Will Not Fill
    ///
    /// Six grain already covers the basic food gap. Bread is still the meal,
    /// because it is eaten first, so the bread line keeps that grain's
    /// satisfaction and the common meal. The water line keeps the drinking
    /// runs and that water.
    #[test]
    fn plan_purchases_bakes_bread_the_grain_pile_will_not_fill() {
        let farm = Process::new(29, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 12.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 3.0, false));
        let water = Process::new(30, "water", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 12.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 3.0, false));
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(2, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 3.0, false));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(water)
            .with_process(bread);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(29, None, vec![]),
                JobLine::new(30, None, vec![]),
                JobLine::new(3, None, vec![]),
            ],
        );
        let property = HashMap::from([
            (0, PopPRow::new(4.0 * TIME_PER_LABOR)),
            (1, PopPRow::new(6.0)),
        ]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 1.0), (3, 5.0)]);
        history.currencies = vec![1];
        let gaps = [
            (vec![(1, 1.0), (3, 2.0)], 0.0),
            (vec![(2, 1.0)], 6.0),
            (vec![(3, 1.0)], 6.0),
        ];

        job.plan_purchases(
            &property,
            &HashMap::new(),
            &factuals,
            &history,
            &gaps,
            2,
        );

        let water_runs = job.lines[1].target.unwrap_or(0.0);
        let bread_runs = job.lines[2].target.unwrap_or(0.0);
        assert!(water_runs + 1e-6 >= 3.0, "water {water_runs}");
        assert!(bread_runs + 1e-6 >= 3.0, "bread {bread_runs}");
        assert!(job.lines[2].cover_cheap);
    }

    /// # Plan Purchases Bakes Bread The Saved Grain Will Not Fill
    ///
    /// Six grain already set aside covers the basic food gap, and it is not
    /// in working stock. Bread is still the meal, because it is eaten first,
    /// so the bread line keeps that grain's satisfaction and the common meal.
    #[test]
    fn plan_purchases_bakes_bread_the_saved_grain_will_not_fill() {
        let farm = Process::new(29, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 12.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 3.0, false));
        let water = Process::new(30, "water", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 12.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 3.0, false));
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(2, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 3.0, false));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(water)
            .with_process(bread);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(29, None, vec![]),
                JobLine::new(30, None, vec![]),
                JobLine::new(3, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(4.0 * TIME_PER_LABOR))]);
        let saved = HashMap::from([(1, 6.0)]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 1.0), (3, 5.0)]);
        history.currencies = vec![1];
        let gaps = [
            (vec![(1, 1.0), (3, 2.0)], 0.0),
            (vec![(2, 1.0)], 6.0),
            (vec![(3, 1.0)], 6.0),
        ];

        job.plan_purchases_for(
            &property,
            &HashMap::new(),
            &saved,
            &factuals,
            &history,
            &gaps,
            2,
            0.0,
        );

        let water_runs = job.lines[1].target.unwrap_or(0.0);
        let bread_runs = job.lines[2].target.unwrap_or(0.0);
        assert!(water_runs + 1e-6 >= 3.0, "water {water_runs}");
        assert!(bread_runs + 1e-6 >= 3.0, "bread {bread_runs}");
        assert!(job.lines[2].cover_cheap);
    }

    /// # Plan Purchases Holds Time For The Line That Makes A Missing Input
    ///
    /// Grain is the tender. Water costs more than that grain can pay, so the
    /// bake funds no run. The water line is subsistence, and it is the only
    /// maker of the missing water, so it keeps the four runs the grain can
    /// still feed. The farm is not that maker, so it keeps the time the
    /// water runs leave. Those runs take the time, so the bake holds no
    /// extra tender and the grain stays on the book. Water and the farm
    /// spend little, so those reserved runs fit the day's points. The bake
    /// stays at complexity 1, which keeps it the specialty.
    #[test]
    fn plan_purchases_holds_time_for_the_line_that_makes_a_missing_input() {
        let farm = Process::new(3, "farm", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(9, 1.0, true));
        let water = Process::new(2, "water", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let bake = Process::new(1, "bake", 0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(3, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(water)
            .with_process(bake);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(3, None, vec![]),
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(20.0)), (1, PopPRow::new(4.0))]);
        let mut history = board(&[(0, 1.0), (1, 1.0), (2, 200.0), (3, 100.0), (9, 1.0)]);
        history.currencies = vec![1];

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        // Water feeds the bake, and it outscores the farm, so it is stored first.
        assert_eq!(job.lines[0].process, 2);
        assert_eq!(job.lines[1].process, 3);
        assert_eq!(job.lines[2].process, 1);
        assert!(!job.lines[0].pays);
        assert!(!job.lines[1].pays);
        assert!(job.lines[2].pays);
        assert_eq!(job.lines[0].target, Some(4.0));
        assert_eq!(job.lines[1].target, Some(1.0));
        assert!(job.lines[2].target.unwrap_or(1.0) < 1e-6);
        assert!(job.unlisted_of(1).abs() < 1e-6, "grain {}", job.unlisted_of(1));
        assert_eq!(job.shopping_of(1), 0.0);
        assert_eq!(job.shopping_of(3), 0.0);
    }

    /// # Craft Input Keep Parks The Run The Stock Can Finish
    ///
    /// Four grain and ten water finish four bakes. Time stays in working
    /// stock. The water above those four runs stays there too.
    #[test]
    fn craft_input_keep_parks_the_run_the_stock_can_finish() {
        let factuals = Factuals::new().with_process(timed_bake().with_input(
            ProcessInput::new(3, 1.0, true, InputType::Destroyed, false),
        ));
        let mut job = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        let property = HashMap::from([
            (0, PopPRow::new(8.0)),
            (1, PopPRow::new(4.0)),
            (3, PopPRow::new(10.0)),
        ]);

        let keep = job.craft_input_keep(&property, &factuals);

        assert!((keep.get(&1).copied().unwrap_or(0.0) - 4.0).abs() < 1e-9);
        assert!((keep.get(&3).copied().unwrap_or(0.0) - 4.0).abs() < 1e-9);
        assert!(keep.get(&0).is_none());

        job.lines[0].pays = false;
        let none = job.craft_input_keep(&property, &factuals);
        assert!(none.is_empty());
    }

    #[test]
    fn plan_purchases_writes_zero_when_the_process_is_missing() {
        let mut job = Job::new(1, vec![JobLine::new(4, Some(1.0), vec![])]);

        let hold = job.plan_purchases(
            &HashMap::new(),
            &HashMap::new(),
            &Factuals::new(),
            &MarketHistory::new(),
            &[],
            0,
        );

        assert_eq!(job.lines[0].target, Some(0.0));
        assert!(hold.is_empty());
    }

    #[test]
    fn reserve_buys_the_next_run_after_decay_and_ignores_fresh() {
        let mut row = PopPRow::new(4.0);
        row.fresh = 4.0;
        let factuals = Factuals::new().with_process(bake()).with_good(good(1, 0.5));
        let mut covered = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        covered.reserve(&mut HashMap::from([(1, row)]), &factuals);
        // Held today's 4, all of it fresh. The next 4 still decays by half, so the buy is 8.
        assert_eq!(covered.shopping[&1], 8.0);
        assert_eq!(covered.buy_orders(Actor::Pop(1))[0].target_amount, 8.0);

        let mut bare = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        bare.reserve(&mut HashMap::new(), &factuals);
        // Need 1, durability 0.5, nothing kept. 1 / 0.5 = 2, already whole.
        assert_eq!(bare.shopping[&1], 2.0);

        let mut fraction = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        let slow = Factuals::new().with_process(bake()).with_good(good(1, 0.1));
        fraction.reserve(&mut HashMap::new(), &slow);
        // 1 / 0.9 rounds up to 2.
        assert!((fraction.shopping[&1] - (1.0 / 0.9)).abs() < 1e-12);
        assert_eq!(fraction.buy_orders(Actor::Pop(1))[0].target_amount, 2.0);
    }

    /// Ten time in, two grain out. On an empty card that score is negative.
    /// Make-grain spends half a time for six grain and pays.
    #[test]
    fn reserve_shops_one_output_when_the_line_does_not_pay() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let mill = Process::new(1, "make grain", 0)
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 6.0, true));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(mill)
            .with_good(good(1, 0.0));
        let mut unpaid = Job::new(1, vec![JobLine::new(29, Some(12.5), vec![])]);
        unpaid.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );
        assert!(!unpaid.lines[0].pays);
        unpaid.reserve(&mut HashMap::new(), &factuals);
        // Decay 0, nothing on hand. One grain, already whole.
        // The time input is a separate, larger buy.
        assert_eq!(unpaid.shopping[&1], 1.0);
        let grain = unpaid
            .buy_orders(Actor::Pop(1))
            .into_iter()
            .find(|order| order.target == 1)
            .expect("grain buy");
        assert_eq!(grain.target_amount, 1.0);
        // Nothing is short, so 12.5 steps toward a cover of 0.
        assert_eq!(unpaid.lines[0].target, Some(9.375));

        let mut paying = Job::new(1, vec![JobLine::new(1, Some(10.0), vec![])]);
        paying.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );
        assert!(paying.lines[0].pays);
        paying.reserve(&mut HashMap::new(), &factuals);
        assert!(paying.shopping.get(&1).is_none());
    }

    #[test]
    fn reserve_shops_the_output_up_through_decay_and_stops_when_stock_covers_it() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let factuals = Factuals::new().with_process(farm).with_good(good(1, 0.5));
        let mut bare = Job::new(1, vec![JobLine::new(29, Some(1.0), vec![])]);
        bare.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );
        bare.reserve(&mut HashMap::new(), &factuals);
        // 1 / 0.5, nothing kept.
        assert_eq!(bare.shopping[&1], 2.0);

        let mut short = Job::new(1, vec![JobLine::new(29, Some(1.0), vec![])]);
        short.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );
        short.reserve(&mut HashMap::from([(1, PopPRow::new(1.0))]), &factuals);
        // 2 - 1 on hand.
        assert_eq!(short.shopping[&1], 1.0);

        let mut covered = Job::new(1, vec![JobLine::new(29, Some(1.0), vec![])]);
        covered.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );
        covered.reserve(&mut HashMap::from([(1, PopPRow::new(2.0))]), &factuals);
        assert!(covered.shopping.get(&1).is_none());
    }

    #[test]
    fn reserve_keeps_a_larger_input_buy_than_the_output_unit() {
        let cycle = Process::new(28, "make time", 0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(0, 1.0, true));
        let factuals = Factuals::new().with_process(cycle).with_good(good(0, 0.0));
        let mut job = Job::new(1, vec![JobLine::new(28, Some(4.0), vec![])]);
        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );
        job.reserve(&mut HashMap::new(), &factuals);
        // The score is zero, so 4 steps to 3. The output bid is 1. The input buy stays larger.
        assert_eq!(job.shopping[&0], 3.0);
    }

    #[test]
    fn reserve_does_not_shop_an_input_that_decays_completely() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_good(good(0, 1.0))
            .with_good(good(1, 0.0));
        let mut job = Job::new(1, vec![JobLine::new(29, Some(2.0), vec![])]);

        job.reserve(&mut HashMap::new(), &factuals);

        // Time is gone by morning, so it is not a buy. The line still pays,
        // so the grain output is not shopped either.
        assert!(job.shopping.get(&0).is_none());
        assert!(job.shopping.get(&1).is_none());
        assert_eq!(job.feeds_of(0), vec![]);
    }

    #[test]
    fn reserve_shops_one_output_once_when_two_lines_make_it() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let factuals = Factuals::new().with_process(farm).with_good(good(1, 0.0));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(29, Some(1.0), vec![]),
                JobLine::new(29, Some(1.0), vec![]),
            ],
        );
        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );
        job.reserve(&mut HashMap::new(), &factuals);
        assert_eq!(job.shopping[&1], 1.0);
    }

    #[test]
    fn reserve_replaces_decayed_capital_and_skips_a_pile_that_covers_tomorrow() {
        let capital = Process::new(8, "with a pan", 0)
            .with_input(ProcessInput::new(6, 1.0, true, InputType::Capital, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new()
            .with_process(capital)
            .with_good(good(6, 0.5));
        let mut job = Job::new(1, vec![JobLine::new(8, Some(4.0), vec![])]);
        job.reserve(&mut HashMap::from([(6, PopPRow::new(4.0))]), &factuals);
        // The 4 comes back, then half rots, so another 4 is bought. 4 / 0.5 - 4 = 4.
        assert_eq!(job.shopping[&6], 4.0);

        let mut stocked = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        stocked.reserve(
            &mut HashMap::from([(1, PopPRow::new(10.0))]),
            &Factuals::new().with_process(bake()),
        );
        assert!(stocked.shopping.is_empty());
    }

    /// Forage wants the whole morning. Cabins are later, use less time per
    /// unit, and take forage's wood, so five time stays for them.
    #[test]
    fn produce_leaves_time_for_a_later_higher_yield_line() {
        let forage = Process::new(31, "subsistence forage", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(
                0,
                10.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(7, 2.0, true));
        let cabins = Process::new(8, "make cabins", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(7, 3.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(8, 1.0, true));
        let factuals = Factuals::new().with_process(forage).with_process(cabins);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(31, Some(10.0), vec![]),
                JobLine::new(8, Some(4.0), vec![]),
            ],
        );
        let mut property = HashMap::from([(0, PopPRow::new(30.0))]);
        job.reserve(&mut property, &factuals);

        job.produce(&mut property, &factuals);

        // 30 time, of which cabins keep 2. Forage runs 2.8 and makes 5.6 wood.
        // That wood finishes 1.866... cabins. The stored targets stay put.
        assert!((property[&7].produced - 5.6).abs() < 1e-9);
        assert!((property[&8].produced - (5.6 / 3.0)).abs() < 1e-9);
        assert_eq!(job.lines[0].target, Some(10.0));
        assert_eq!(job.lines[1].target, Some(4.0));
    }

    #[test]
    fn produce_turns_grain_into_bread() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        let factuals =
            Factuals::new().with_process(bake().with_effect(ProcessEffect::Culture(2.0)));
        let mut property = HashMap::from([(1, PopPRow::new(1.0))]);
        job.reserve(&mut property, &factuals);

        let effects = job.produce(&mut property, &factuals);

        assert_eq!(effects, vec![ProcessEffect::Culture(2.0)]);
        assert_eq!(property[&1].quantity, 0.0);
        assert_eq!(property[&1].reserved, 0.0);
        assert_eq!(property[&2].quantity, 1.0);
        assert_eq!(property[&2].fresh, 1.0);
        assert_eq!(property[&2].produced, 1.0);
        assert!(job.claimed.get(&1).is_none());
    }

    /// Two time and one grain make one bread. Each input is credited with that
    /// whole bread, on top of the units it lost.
    #[test]
    fn produce_credits_each_destroyed_input_with_the_whole_output() {
        let process = Process::new(1, "bake", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(process);
        let mut job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        let mut property = HashMap::from([(0, PopPRow::new(2.0)), (1, PopPRow::new(1.0))]);
        job.reserve(&mut property, &factuals);

        job.produce(&mut property, &factuals);

        let time = job.process_credit().get(&0).expect("time");
        let grain = job.process_credit().get(&1).expect("grain");
        assert!((time.units - 2.0).abs() < 1e-12);
        assert!((time.outputs[&2] - 1.0).abs() < 1e-12);
        assert!((grain.units - 1.0).abs() < 1e-12);
        assert!((grain.outputs[&2] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn produce_spends_into_the_kept_pile() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(5.0))]);
        job.leave(&HashMap::from([(1, 3.0)]));

        job.reserve(&mut property, &factuals);

        // Five on hand, three kept. The line claims the other two and shops
        // the next four without counting the pile as stock.
        assert_eq!(property[&1].reserved, 2.0);
        assert!((job.shopping[&1] - 4.0).abs() < 1e-9);

        job.produce(&mut property, &factuals);
        assert_eq!(property[&1].quantity, 1.0);
        assert_eq!(property[&2].quantity, 4.0);

        job.settle_shopping(&property, &factuals);
        assert!((job.shopping[&1] - 4.0).abs() < 1e-9);
    }

    #[test]
    fn produce_spends_the_morning_make_before_the_kept_pile() {
        let maker = Process::new(1, "make grain", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 2.0, true));
        // The bake spends little, so five runs still fit in the ten time on hand.
        let factuals = Factuals::new()
            .with_process(maker)
            .with_process(bake().with_complexity(0.01));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, Some(1.0), vec![]),
                JobLine::new(7, Some(5.0), vec![]),
            ],
        );
        let mut property = HashMap::from([(0, PopPRow::new(10.0)), (1, PopPRow::new(3.0))]);
        // The pile is larger than the dawn stock. Today's make is baked
        // with it, instead of filling the pile first.
        job.leave(&HashMap::from([(1, 4.0)]));

        job.reserve(&mut property, &factuals);
        job.produce(&mut property, &factuals);

        assert_eq!(property[&1].quantity, 0.0);
        assert_eq!(property[&2].quantity, 5.0);
        assert_eq!(property[&2].produced, 5.0);
    }

    /// Three grain are reserved and the job did not claim them. The bake
    /// takes the other two. The reserve, and a kept pile under it, stay.
    #[test]
    fn produce_leaves_a_desire_reserve() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(5.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(5.0).with_reserve(3.0))]);
        job.leave(&HashMap::from([(1, 4.0)]));

        job.produce(&mut property, &factuals);

        assert_eq!(property[&1].quantity, 3.0);
        assert_eq!(property[&1].reserved, 3.0);
        assert_eq!(property[&2].quantity, 2.0);
        assert_eq!(property[&2].produced, 2.0);
    }

    #[test]
    fn unlimited_line_uses_the_stock_on_hand() {
        let mut job = Job::new(1, vec![JobLine::new(7, None, vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(3.0))]);

        job.reserve(&mut property, &factuals);
        assert!(job.shopping.is_empty());
        assert_eq!(property[&1].reserved, 3.0);

        job.produce(&mut property, &factuals);
        assert_eq!(property[&1].quantity, 0.0);
        assert_eq!(property[&2].quantity, 3.0);
        assert_eq!(property[&2].produced, 3.0);
    }

    #[test]
    fn reserve_keeps_a_held_factor_and_shops_a_missing_one() {
        let process = Process::new(8, "bake in an oven", 0)
            .with_input(ProcessInput::new(5, 1.0, true, InputType::Factor, false))
            .with_input(ProcessInput::new(6, 1.0, true, InputType::Factor, true))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(process);
        let mut job = Job::new(1, vec![JobLine::new(8, Some(1.0), vec![])]);
        let mut property = HashMap::from([(5, PopPRow::new(2.0))]);

        job.reserve(&mut property, &factuals);

        assert_eq!(property[&5].reserved, 2.0);
        assert_eq!(job.shopping[&1], 1.0);
        assert!(job.shopping.get(&6).is_none());
        job.produce(&mut property, &factuals);
        assert_eq!(property[&5].reserved, 2.0);
        assert_eq!(property[&5].quantity, 2.0);
    }

    #[test]
    fn missing_required_factor_does_not_lock_other_inputs() {
        let process = Process::new(8, "bake in an oven", 0)
            .with_input(ProcessInput::new(5, 1.0, true, InputType::Factor, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(process);
        let mut job = Job::new(1, vec![JobLine::new(8, Some(1.0), vec![])]);
        let mut property = HashMap::from([(1, PopPRow::new(5.0))]);

        job.reserve(&mut property, &factuals);

        assert_eq!(property[&1].reserved, 0.0);
        assert_eq!(job.shopping[&5], 1.0);
        assert!(job.shopping.get(&1).is_none());
    }

    #[test]
    fn listed_optional_is_shopped_and_an_unlisted_one_is_not() {
        let process = Process::new(9, "season", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(4, 1.0, true, InputType::Destroyed, true))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(process);

        let mut plain = Job::new(1, vec![JobLine::new(9, Some(1.0), vec![])]);
        plain.reserve(&mut HashMap::new(), &factuals);
        assert_eq!(plain.shopping[&1], 1.0);
        assert!(plain.shopping.get(&4).is_none());

        let mut seasoned = Job::new(1, vec![JobLine::new(9, Some(1.0), vec![4])]);
        seasoned.reserve(&mut HashMap::new(), &factuals);
        assert_eq!(seasoned.shopping[&1], 1.0);
        assert_eq!(seasoned.shopping[&4], 1.0);
    }

    /// The line does not list the tool, so the tool is neither shopped nor
    /// fed. Listing it shops one when the rack is empty, and one run with
    /// the tool turns two product into three.
    #[test]
    fn listed_capital_is_fed_and_an_unlisted_one_is_not() {
        let process = Process::new(11, "with a tool", 0)
            .with_input(ProcessInput::new(
                1,
                1.0,
                false,
                InputType::Destroyed,
                false,
            ))
            .with_input(
                ProcessInput::new(9, 1.0, false, InputType::Capital, true)
                    .with_optional(InputEffect::Output(0.5)),
            )
            .with_output(ProcessOutput::new(2, 2.0, false));
        let factuals = Factuals::new()
            .with_process(process)
            .with_good(good(1, 0.0))
            .with_good(good(9, 0.0))
            .with_good(good(2, 0.0));

        let mut bare = Job::new(1, vec![JobLine::new(11, Some(1.0), vec![])]);
        bare.reserve(&mut HashMap::new(), &factuals);
        assert_eq!(bare.shopping[&1], 1.0);
        assert!(bare.shopping.get(&9).is_none());

        let mut skipped = Job::new(1, vec![JobLine::new(11, Some(1.0), vec![])]);
        let mut held = HashMap::from([(1, PopPRow::new(1.0)), (9, PopPRow::new(1.0))]);
        skipped.reserve(&mut held, &factuals);
        skipped.produce(&mut held, &factuals);
        assert_eq!(held[&9].quantity, 1.0);
        assert_eq!(held[&9].used, 0.0);
        assert!((held[&2].quantity - 2.0).abs() < 1e-12);

        let mut job = Job::new(1, vec![JobLine::new(11, Some(1.0), vec![9])]);
        let mut property = HashMap::from([(1, PopPRow::new(1.0)), (9, PopPRow::new(1.0))]);
        job.reserve(&mut property, &factuals);
        job.produce(&mut property, &factuals);

        assert_eq!(property[&9].quantity, 0.0);
        assert_eq!(property[&9].used, 1.0);
        assert!((property[&2].quantity - 3.0).abs() < 1e-12);
    }

    /// The tool's decay is its cost. Half again the product raises both the
    /// return and the product, so the line turns the tool on.
    #[test]
    fn plan_lists_a_tool_whose_return_holds() {
        let make = Process::new(1, "make", 0)
            .with_input(ProcessInput::new(
                0,
                0.5,
                false,
                InputType::Destroyed,
                false,
            ))
            .with_input(
                ProcessInput::new(9, 1.0, false, InputType::Capital, true)
                    .with_optional(InputEffect::Output(0.5)),
            )
            .with_output(ProcessOutput::new(2, 8.0, false));
        let factuals = Factuals::new()
            .with_process(make)
            .with_good(good(0, 1.0))
            .with_good(good(9, 0.1));
        let mut job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);

        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );

        assert_eq!(job.lines[0].inputs, vec![9]);
    }

    /// The optional adds cost and no product. The line leaves it off.
    #[test]
    fn plan_drops_an_optional_that_does_not_raise_the_product() {
        let make = Process::new(1, "make", 0)
            .with_input(ProcessInput::new(
                0,
                5.0,
                false,
                InputType::Destroyed,
                false,
            ))
            .with_input(ProcessInput::new(4, 3.0, false, InputType::Destroyed, true))
            .with_output(ProcessOutput::new(2, 20.0, false));
        let factuals = Factuals::new()
            .with_process(make)
            .with_good(good(0, 1.0))
            .with_good(good(4, 0.0));
        let mut job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![4])]);

        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            false,
        );

        assert!(job.lines[0].inputs.is_empty());
    }

    /// The rate falls by less than the product rises, so the optional stays.
    /// A smaller product rise, against the same cost, turns it off.
    /// Time is on the card at holding 1. The product and the optional stay
    /// at the default holding.
    #[test]
    fn plan_keeps_an_optional_when_the_product_gain_covers_the_rate_loss() {
        let paid = Process::new(1, "paid", 0)
            .with_input(ProcessInput::new(
                0,
                5.0,
                false,
                InputType::Destroyed,
                false,
            ))
            .with_input(
                ProcessInput::new(4, 3.0, false, InputType::Destroyed, true)
                    .with_optional(InputEffect::Output(0.2)),
            )
            .with_output(ProcessOutput::new(2, 20.0, false));
        let thin = Process::new(1, "thin", 0)
            .with_input(ProcessInput::new(
                0,
                5.0,
                false,
                InputType::Destroyed,
                false,
            ))
            .with_input(
                ProcessInput::new(4, 3.0, false, InputType::Destroyed, true)
                    .with_optional(InputEffect::Output(0.05)),
            )
            .with_output(ProcessOutput::new(2, 20.0, false));
        let mut history = MarketHistory::new();
        history.prices.insert(0, 1.0);
        history.salability.insert(0, 1.0);
        let factuals = Factuals::new()
            .with_process(paid)
            .with_good(good(0, 1.0))
            .with_good(good(4, 0.0));
        let mut job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );
        assert_eq!(job.lines[0].inputs, vec![4]);

        let factuals = Factuals::new()
            .with_process(thin)
            .with_good(good(0, 1.0))
            .with_good(good(4, 0.0));
        let mut job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );
        assert!(job.lines[0].inputs.is_empty());
    }

    /// Alone, the optional's cost drops a losing run further, so it stays off.
    /// A later line that turns the product into more holding makes the same
    /// optional pay, and the line turns it on. Time is on the card at
    /// holding 1. The products stay at the default holding.
    #[test]
    fn plan_counts_a_later_line_in_the_products_worth() {
        let upstream = Process::new(1, "upstream", 0)
            .with_input(ProcessInput::new(
                0,
                50.0,
                false,
                InputType::Destroyed,
                false,
            ))
            .with_input(
                ProcessInput::new(4, 80.0, false, InputType::Destroyed, true)
                    .with_optional(InputEffect::Output(0.5)),
            )
            .with_output(ProcessOutput::new(1, 10.0, false));
        let later = Process::new(2, "later", 0)
            .with_input(ProcessInput::new(
                1,
                1.0,
                false,
                InputType::Destroyed,
                false,
            ))
            .with_output(ProcessOutput::new(2, 20.0, false));
        let mut history = MarketHistory::new();
        history.prices.insert(0, 1.0);
        history.salability.insert(0, 1.0);
        let goods = Factuals::new()
            .with_process(upstream.clone())
            .with_good(good(0, 1.0))
            .with_good(good(4, 0.0));
        let mut alone = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        alone.plan(
            &HashMap::new(),
            &HashMap::new(),
            &goods,
            &history,
            1.0,
            false,
        );
        assert!(alone.lines[0].inputs.is_empty());

        let factuals = Factuals::new()
            .with_process(upstream)
            .with_process(later)
            .with_good(good(0, 1.0))
            .with_good(good(4, 0.0));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, Some(1.0), vec![]),
                JobLine::new(2, Some(1.0), vec![]),
            ],
        );
        job.plan(
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );
        let feeder = job
            .lines
            .iter()
            .find(|line| line.process == 1)
            .expect("upstream");
        assert_eq!(feeder.inputs, vec![4]);
    }

    #[test]
    fn produce_sets_capital_aside_as_used() {
        let process = Process::new(10, "grind", 0)
            .with_input(ProcessInput::new(6, 1.0, true, InputType::Capital, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(process);
        let mut job = Job::new(1, vec![JobLine::new(10, Some(1.0), vec![])]);
        let mut property = HashMap::from([(6, PopPRow::new(1.0)), (1, PopPRow::new(1.0))]);
        job.reserve(&mut property, &factuals);

        job.produce(&mut property, &factuals);

        assert_eq!(property[&6].quantity, 0.0);
        assert_eq!(property[&6].used, 1.0);
        assert_eq!(property[&6].spent, 0.0);
        assert_eq!(property[&6].reserved, 0.0);
        assert_eq!(property[&1].quantity, 0.0);
        assert_eq!(property[&1].spent, 1.0);
        assert_eq!(property[&2].quantity, 1.0);
        assert_eq!(property[&2].spent, 0.0);
    }

    #[test]
    fn reset_day_clears_shopping_and_keeps_the_target() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(2.0), vec![])]);
        job.shopping.insert(1, 5.0);
        job.claimed.insert(1, 1.0);

        job.reset_day();

        assert_eq!(job.lines[0].target, Some(2.0));
        assert!(job.shopping.is_empty());
        assert!(job.claimed.is_empty());
    }

    /// Grain made this morning is bread's input. The morning buy was the
    /// whole next run, because the grain was not on hand yet.
    #[test]
    fn settle_shopping_drops_an_input_this_morning_made() {
        let grow = Process::new(1, "make grain", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 6.0, true));
        let bread = Process::new(7, "make bread", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 0.2, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new()
            .with_process(grow)
            .with_process(bread)
            .with_good(good(0, 1.0))
            .with_good(good(1, 0.0));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, Some(2.0), vec![]),
                JobLine::new(7, Some(4.0), vec![]),
            ],
        );
        let mut property = HashMap::from([(0, PopPRow::new(100.0))]);

        job.reserve(&mut property, &factuals);
        assert_eq!(job.shopping[&1], 4.0);

        job.produce(&mut property, &factuals);
        // 12 grain made, 4 spent on bread, 8 left.
        assert_eq!(property[&1].quantity, 8.0);
        job.settle_shopping(&property, &factuals);
        assert!(job.shopping.get(&1).is_none());
    }

    /// The grain on hand is reserved for a desire, and no line here makes
    /// grain. The next bread run is bought.
    #[test]
    fn settle_shopping_still_buys_stock_a_desire_is_holding() {
        let factuals = Factuals::new().with_process(bake()).with_good(good(1, 0.0));
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let row = PopPRow::new(4.0).with_reserve(4.0);

        job.settle_shopping(&HashMap::from([(1, row)]), &factuals);

        assert_eq!(job.shopping[&1], 4.0);
    }

    /// The grain line runs before bread and makes 6. Bread needs 4. Nothing
    /// is bought, and a make of 2 leaves a buy of 2.
    #[test]
    fn settle_shopping_counts_what_an_earlier_line_will_make() {
        let grow = Process::new(1, "make grain", 0).with_output(ProcessOutput::new(1, 6.0, true));
        let bread = Process::new(7, "make bread", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new()
            .with_process(grow)
            .with_process(bread.clone())
            .with_good(good(1, 0.0));
        let mut covered = Job::new(
            1,
            vec![
                JobLine::new(1, Some(1.0), vec![]),
                JobLine::new(7, Some(4.0), vec![]),
            ],
        );
        covered.settle_shopping(&HashMap::new(), &factuals);
        assert!(covered.shopping.get(&1).is_none());

        let short = Process::new(1, "make grain", 0).with_output(ProcessOutput::new(1, 2.0, true));
        let factuals = Factuals::new()
            .with_process(short)
            .with_process(bread)
            .with_good(good(1, 0.0));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, Some(1.0), vec![]),
                JobLine::new(7, Some(4.0), vec![]),
            ],
        );
        job.settle_shopping(&HashMap::new(), &factuals);
        assert_eq!(job.shopping[&1], 2.0);
    }

    /// The pan is in `used` after produce. Counting only free stock would
    /// shop a second pan. Half of the returned pile rots, so one replacement
    /// is still bought.
    #[test]
    fn settle_shopping_counts_capital_produce_set_aside() {
        let capital = Process::new(8, "with a pan", 0)
            .with_input(ProcessInput::new(6, 1.0, true, InputType::Capital, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new()
            .with_process(capital)
            .with_good(good(6, 0.5));
        let mut job = Job::new(1, vec![JobLine::new(8, Some(4.0), vec![])]);
        let mut property = HashMap::from([(6, PopPRow::new(4.0))]);
        job.reserve(&mut property, &factuals);
        job.produce(&mut property, &factuals);
        assert_eq!(property[&6].used, 4.0);
        assert_eq!(property[&6].quantity, 0.0);

        job.settle_shopping(&property, &factuals);

        // 4 / 0.5 - 4 = 4.
        assert_eq!(job.shopping[&6], 4.0);
    }

    /// Six on hand, two lines that each need four, no decay. The first line
    /// uses four of the six. The second buys the two it is still short.
    #[test]
    fn settle_shopping_counts_shared_leftover_once() {
        let eat = Process::new(7, "eat", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(eat).with_good(good(1, 0.0));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(4.0), vec![]),
                JobLine::new(7, Some(4.0), vec![]),
            ],
        );

        job.settle_shopping(&HashMap::from([(1, PopPRow::new(6.0))]), &factuals);

        assert_eq!(job.shopping[&1], 2.0);
    }

    /// Need 4, half rots, 2 already free. 4 / 0.5 - 2 = 6.
    #[test]
    fn settle_shopping_buys_the_decay_the_leftover_does_not_cover() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake()).with_good(good(1, 0.5));

        job.settle_shopping(&HashMap::from([(1, PopPRow::new(2.0))]), &factuals);

        assert_eq!(job.shopping[&1], 6.0);
    }

    #[test]
    fn settle_shopping_skips_inputs_when_a_factor_is_missing() {
        let process = Process::new(8, "bake in an oven", 0)
            .with_input(ProcessInput::new(5, 1.0, true, InputType::Factor, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(process);
        let mut job = Job::new(1, vec![JobLine::new(8, Some(4.0), vec![])]);

        job.settle_shopping(&HashMap::from([(1, PopPRow::new(10.0))]), &factuals);

        assert_eq!(job.shopping[&5], 1.0);
        assert!(job.shopping.get(&1).is_none());
    }

    /// Only optional inputs tagged skill are attached. A required skill and a
    /// plain optional stay off the list. Repeats collapse, and the ids sort.
    #[test]
    fn attach_skills_keeps_optional_skill_inputs() {
        let practice = Process::new(7, "bake", 0)
            .with_input(ProcessInput::new(
                41,
                1.0,
                true,
                InputType::Destroyed,
                false,
            ))
            .with_input(ProcessInput::new(2, 1.0, true, InputType::Destroyed, true))
            .with_input(
                ProcessInput::new(40, 1.0, true, InputType::Destroyed, true)
                    .with_optional(InputEffect::ExtraOutput(40, 1.1)),
            )
            .with_input(
                ProcessInput::new(28, 1.0, true, InputType::Destroyed, true)
                    .with_optional(InputEffect::ExtraOutput(28, 1.1)),
            );
        let again = Process::new(8, "again", 0).with_input(
            ProcessInput::new(40, 1.0, true, InputType::Destroyed, true)
                .with_optional(InputEffect::ExtraOutput(40, 1.1)),
        );
        let factuals = Factuals::new()
            .with_process(practice)
            .with_process(again)
            .with_good(skill_good(41))
            .with_good(good(2, 0.0))
            .with_good(skill_good(40))
            .with_good(skill_good(28));
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(1.0), vec![2, 40]),
                JobLine::new(8, Some(1.0), vec![]),
                JobLine::new(99, Some(1.0), vec![]),
            ],
        );

        job.attach_skills(&factuals);

        assert_eq!(job.skills, vec![28, 40]);
    }

    /// Two runs and one skill on hand. The line lists the skill, so it is fed
    /// once: 1.1 comes back, and each run adds 0.01. The grain is spent and
    /// credited. The returned skill is not. The bread is made. The pile is all
    /// fresh, so the night takes none of it.
    #[test]
    fn produce_feeds_a_skill_once_and_returns_the_bonus() {
        let mut skill = skill_good(40);
        skill.decay_rate = 0.05;
        let factuals = Factuals::new()
            .with_process(practiced_bake())
            .with_good(good(1, 0.0))
            .with_good(skill);
        let mut job = Job::new(1, vec![JobLine::new(7, Some(2.0), vec![40])]);
        let mut property = HashMap::from([(1, PopPRow::new(2.0)), (40, PopPRow::new(1.0))]);
        job.reserve(&mut property, &factuals);
        assert!(job.shopping.get(&40).is_none());

        job.produce(&mut property, &factuals);

        assert_eq!(property[&1].quantity, 0.0);
        assert_eq!(property[&2].quantity, 2.0);
        assert!((property[&40].quantity - 1.12).abs() < 1e-12);
        // The skill came back. Grain was used up, so only grain is credited.
        assert!(job.process_credit().get(&40).is_none());
        let grain = job.process_credit().get(&1).expect("grain");
        assert!((grain.units - 2.0).abs() < 1e-12);
        assert_eq!(property[&40].reserved, 0.0);
        // The one unit on hand was fed, so the whole pile is fresh.
        assert!((property[&40].fresh - property[&40].quantity).abs() < 1e-12);

        let mut pop = Pop::new(1);
        pop.property.insert(40, property[&40].clone());
        pop.decay_goods(&factuals);
        assert!(pop.property[&40].lost.abs() < 1e-12);
        assert!((pop.property[&40].quantity - 1.12).abs() < 1e-12);
    }

    /// Half a skill, then two lines of one run. The second line feeds what
    /// the first returned. This bake ends at 0.626, all of it fresh, so the
    /// night takes none of it.
    #[test]
    fn produce_feeds_returned_skill_on_the_next_line() {
        let mut skill = skill_good(40);
        skill.decay_rate = 0.05;
        let factuals = Factuals::new()
            .with_process(practiced_bake())
            .with_good(good(1, 0.0))
            .with_good(skill);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(1.0), vec![40]),
                JobLine::new(7, Some(1.0), vec![40]),
            ],
        );
        let mut property = HashMap::from([(1, PopPRow::new(2.0)), (40, PopPRow::new(0.5))]);
        job.reserve(&mut property, &factuals);
        assert!(job.shopping.get(&40).is_none());

        job.produce(&mut property, &factuals);

        assert_eq!(property[&1].quantity, 0.0);
        assert_eq!(property[&2].quantity, 2.0);
        assert!((property[&40].quantity - 0.626).abs() < 1e-12);
        assert!((property[&40].fresh - property[&40].quantity).abs() < 1e-12);
        assert_eq!(property[&40].reserved, 0.0);

        let mut pop = Pop::new(1);
        pop.property.insert(40, property[&40].clone());
        pop.decay_goods(&factuals);
        assert!(pop.property[&40].lost.abs() < 1e-12);
        assert!((pop.property[&40].quantity - 0.626).abs() < 1e-12);
    }

    /// No skill on hand. One run of this bake still makes its fixed 0.01,
    /// and that output is fresh, so the night takes none of it.
    #[test]
    fn produce_makes_fresh_skill_with_none_on_hand() {
        let mut skill = skill_good(40);
        skill.decay_rate = 0.05;
        let factuals = Factuals::new()
            .with_process(practiced_bake())
            .with_good(good(1, 0.0))
            .with_good(skill);
        let mut job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        let mut property = HashMap::from([(1, PopPRow::new(1.0))]);
        job.reserve(&mut property, &factuals);

        job.produce(&mut property, &factuals);

        assert_eq!(property[&1].quantity, 0.0);
        assert_eq!(property[&2].quantity, 1.0);
        assert!((property[&40].quantity - 0.01).abs() < 1e-12);
        assert!((property[&40].fresh - 0.01).abs() < 1e-12);

        let mut pop = Pop::new(1);
        pop.property.insert(40, property[&40].clone());
        pop.decay_goods(&factuals);
        assert!(pop.property[&40].lost.abs() < 1e-12);
        assert!((pop.property[&40].quantity - 0.01).abs() < 1e-12);
    }

    /// Three on hand and a quota of two. Two are fed and come back fresh.
    /// The one left over stays aged and decays.
    #[test]
    fn produce_refreshes_the_skill_it_used() {
        let mut skill = skill_good(40);
        skill.decay_rate = 0.05;
        let factuals = Factuals::new()
            .with_process(practiced_bake())
            .with_good(good(1, 0.0))
            .with_good(skill);
        let mut job = Job::new(1, vec![JobLine::new(7, Some(2.0), vec![40])]);
        let mut property = HashMap::from([(1, PopPRow::new(2.0)), (40, PopPRow::new(3.0))]);
        job.reserve(&mut property, &factuals);

        job.produce(&mut property, &factuals);

        // Fed 2, returned 2.2, plus 0.02 flat. One aged unit remains.
        assert!((property[&40].quantity - 3.22).abs() < 1e-12);
        assert!((property[&40].fresh - 2.22).abs() < 1e-12);

        let mut pop = Pop::new(1);
        pop.property.insert(40, property[&40].clone());
        pop.decay_goods(&factuals);
        assert!((pop.property[&40].lost - 0.05).abs() < 1e-12);
        assert!((pop.property[&40].quantity - 3.17).abs() < 1e-12);
    }

    /// The line lists the skill and does not pay. Three on hand and a quota
    /// of two claims two, not all three. The product is shopped. The skill
    /// is not.
    #[test]
    fn reserve_claims_a_skill_and_does_not_shop_it() {
        let factuals = Factuals::new()
            .with_process(practiced_bake())
            .with_good(good(1, 0.0))
            .with_good(good(2, 0.0))
            .with_good(skill_good(40));
        let mut job = Job::new(1, vec![JobLine::new(7, Some(2.0), vec![40])]);
        job.lines[0].pays = false;
        let mut property = HashMap::from([(40, PopPRow::new(3.0))]);

        job.reserve(&mut property, &factuals);

        assert!((property[&40].reserved - 2.0).abs() < 1e-12);
        assert!(job.shopping.get(&40).is_none());
        assert_eq!(job.shopping[&2], 1.0);
    }

    /// One run that spends inputs worth -100 and makes outputs worth 100.
    /// The gain is 200, and the rate on that base of 100 is 2.
    #[test]
    fn trade_rate_counts_a_negative_input_as_a_gain() {
        let process = Process::new(1, "purge", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let history = priced(&[(1, -100.0), (2, 100.0)]);

        assert!((super::trade_score(&process, &history) - 199.0).abs() < 1e-12);
        let rate = super::trade_rate(&process, &history).expect("rate");
        assert!((rate - 199.0 / 100.0).abs() < 1e-12);
    }

    /// The rate is the one-run gain over the absolute input holding.
    /// Capital and an optional input stay out of both figures. A recipe
    /// with no destroyed input has no rate.
    #[test]
    fn trade_rate_is_the_gain_over_the_absolute_input() {
        let doubled = Process::new(1, "double", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let doubled_rate = super::trade_rate(&doubled, &priced(&[(1, 50.0), (2, 100.0)]))
            .expect("rate");
        assert!((doubled_rate - 49.0 / 50.0).abs() < 1e-12);

        let even = Process::new(1, "even", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let even_rate =
            super::trade_rate(&even, &priced(&[(1, 100.0), (2, 100.0)])).expect("rate");
        assert!((even_rate - (-1.0 / 100.0)).abs() < 1e-12);

        let loss = Process::new(1, "loss", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let loss_rate =
            super::trade_rate(&loss, &priced(&[(1, 100.0), (2, 40.0)])).expect("rate");
        assert!((loss_rate - (-61.0 / 100.0)).abs() < 1e-12);

        let free = Process::new(1, "free", 0).with_output(ProcessOutput::new(2, 1.0, true));
        assert_eq!(super::trade_rate(&free, &priced(&[(2, 100.0)])), None);

        let with_capital = Process::new(1, "double", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(
                9,
                1000.0,
                true,
                InputType::Capital,
                false,
            ))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let capital_rate = super::trade_rate(
            &with_capital,
            &priced(&[(1, 50.0), (2, 100.0), (9, 100.0)]),
        )
        .expect("rate");
        assert!((capital_rate - 49.0 / 50.0).abs() < 1e-12);

        let with_optional = Process::new(1, "double", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(
                8,
                1000.0,
                true,
                InputType::Destroyed,
                true,
            ))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let optional_rate = super::trade_rate(
            &with_optional,
            &priced(&[(1, 50.0), (2, 100.0), (8, 100.0)]),
        )
        .expect("rate");
        assert!((optional_rate - 49.0 / 50.0).abs() < 1e-12);

        let consumed = Process::new(1, "eaten", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Consumed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let consumed_rate =
            super::trade_rate(&consumed, &priced(&[(1, 50.0), (2, 100.0)])).expect("rate");
        assert!((consumed_rate - 49.0 / 50.0).abs() < 1e-12);
    }

    /// # Produce Stops When The Complexity Points Run Out
    ///
    /// One labor-day of time funds one point. A complexity of 1 spends that
    /// point on one run, so a target of four finishes one.
    #[test]
    fn produce_stops_when_the_complexity_points_run_out() {
        let process = Process::new(1, "stamp", 0)
            .with_complexity(1.0)
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(process);
        let mut job = Job::new(1, vec![JobLine::new(1, Some(4.0), vec![])]);
        let mut property = HashMap::from([(0, PopPRow::new(TIME_PER_LABOR))]);

        job.produce(&mut property, &factuals);

        assert!((property[&2].produced - 1.0).abs() < 1e-9);
    }

    /// # Produce Lets The Lighter Process Run More
    ///
    /// The same point buys four runs at complexity 0.25 and one run at
    /// complexity 1.
    #[test]
    fn produce_lets_the_lighter_process_run_more() {
        let light = Process::new(1, "forage", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(2, 1.0, true));
        let heavy = Process::new(2, "mill", 0)
            .with_complexity(1.0)
            .with_output(ProcessOutput::new(3, 1.0, true));
        let light_facts = Factuals::new().with_process(light);
        let heavy_facts = Factuals::new().with_process(heavy);
        let mut light_job = Job::new(1, vec![JobLine::new(1, Some(10.0), vec![])]);
        let mut heavy_job = Job::new(1, vec![JobLine::new(2, Some(10.0), vec![])]);
        let mut light_stock = HashMap::from([(0, PopPRow::new(TIME_PER_LABOR))]);
        let mut heavy_stock = HashMap::from([(0, PopPRow::new(TIME_PER_LABOR))]);

        light_job.produce(&mut light_stock, &light_facts);
        heavy_job.produce(&mut heavy_stock, &heavy_facts);

        assert!((light_stock[&2].produced - 4.0).abs() < 1e-9);
        assert!((heavy_stock[&3].produced - 1.0).abs() < 1e-9);
    }

    /// # Produce Fits Fewer Runs Of The Heavier Line
    ///
    /// Four points. The light line runs first and spends two of them on
    /// eight runs. The heavy line gets the two points left, so it runs
    /// twice, not eight.
    #[test]
    fn produce_fits_fewer_runs_of_the_heavier_line() {
        let light = Process::new(1, "forage", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(2, 1.0, true));
        let heavy = Process::new(2, "mill", 0)
            .with_complexity(1.0)
            .with_output(ProcessOutput::new(3, 1.0, true));
        let factuals = Factuals::new().with_process(light).with_process(heavy);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, Some(8.0), vec![]),
                JobLine::new(2, Some(8.0), vec![]),
            ],
        );
        let mut property = HashMap::from([(0, PopPRow::new(4.0 * TIME_PER_LABOR))]);

        job.produce(&mut property, &factuals);

        assert!((property[&2].produced - 8.0).abs() < 1e-9);
        assert!((property[&3].produced - 2.0).abs() < 1e-9);
    }

    /// # Produce Keeps A Later Cover
    ///
    /// Two points. The mill runs first and would take both. The farm's
    /// cover is two runs, so half a point stays and the farm finishes.
    #[test]
    fn produce_keeps_a_later_cover() {
        let mill = Process::new(1, "mill", 0)
            .with_complexity(1.0)
            .with_output(ProcessOutput::new(2, 1.0, true));
        let farm = Process::new(29, "farm", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(1, 1.0, false));
        let factuals = Factuals::new().with_process(mill).with_process(farm);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, Some(4.0), vec![]),
                JobLine::new(29, Some(2.0), vec![]),
            ],
        );
        job.lines[1].pays = false;
        job.lines[1].cover = 2.0;
        let mut property = HashMap::from([(0, PopPRow::new(2.0 * TIME_PER_LABOR))]);

        job.produce(&mut property, &factuals);

        assert!((property[&2].produced - 1.5).abs() < 1e-9);
        assert!((property[&1].produced - 2.0).abs() < 1e-9);
    }

    /// # Produce Runs The Feeder First
    ///
    /// Bread is paying and listed first, and the pile has no grain. The
    /// farm still runs before it, so bread is made. One point is not enough
    /// for a full bread run after the farm's cover.
    #[test]
    fn produce_runs_the_feeder_first() {
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, false));
        let farm = Process::new(29, "farm", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(1, 1.0, false));
        let factuals = Factuals::new().with_process(bread).with_process(farm);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(3, Some(1.0), vec![]),
                JobLine::new(29, Some(1.0), vec![]),
            ],
        );
        job.lines[1].pays = false;
        job.lines[1].cover = 1.0;
        let mut property = HashMap::from([(0, PopPRow::new(TIME_PER_LABOR))]);

        job.produce(&mut property, &factuals);

        assert!((property[&1].produced - 1.0).abs() < 1e-9);
        assert!(property[&3].produced > 0.5);
    }

    /// # Produce Keeps A Paying Bake When Its Cover Is Already Met
    ///
    /// Six points. Cotton is first, pays, and does not feed the bake. Water
    /// does. Bread's cover is already zero and it still pays, so the four
    /// runs stay reserved. Cotton cannot spend them. Water still makes the
    /// water those runs are short, and bread is baked.
    #[test]
    fn produce_keeps_a_paying_bake_when_its_cover_is_already_met() {
        let cotton = Process::new(32, "cotton", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(4, 6.0, true));
        let water = Process::new(30, "water", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(5, 3.0, true));
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(5, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(6, 12.0, false));
        let factuals = Factuals::new()
            .with_process(cotton)
            .with_process(water)
            .with_process(bread);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(32, Some(40.0), vec![]),
                JobLine::new(30, Some(30.0), vec![]),
                JobLine::new(3, Some(4.0), vec![]),
            ],
        );
        job.lines[2].cover = 0.0;
        let mut property = HashMap::from([(0, PopPRow::new(6.0 * TIME_PER_LABOR))]);

        job.produce(&mut property, &factuals);

        // Two points left after the hold: eight cotton runs, not forty.
        assert!((property[&4].produced - 48.0).abs() < 1e-6);
        // The dip makes the four water the bake is short, then the bake runs.
        assert!((property[&5].produced - 4.0).abs() < 1e-6);
        assert!(property[&6].produced > 36.0);
    }

    /// # Produce Makes The Drink Before The Later Hold
    ///
    /// Four points, all kept for a paying bake. The water line's target
    /// would make the drink and the bake's input. Those drink runs are
    /// funded first, so six water stay and the bake still runs.
    #[test]
    fn produce_makes_the_drink_before_the_later_hold() {
        let water = Process::new(30, "water", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(2, 3.0, false));
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(2, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, false));
        let factuals = Factuals::new().with_process(water).with_process(bread);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(30, Some(20.0), vec![]),
                JobLine::new(3, Some(4.0), vec![]),
            ],
        );
        let mut property = HashMap::from([(0, PopPRow::new(4.0 * TIME_PER_LABOR))]);
        let leave = HashMap::from([(2, 6.0)]);

        job.produce_keeping(&mut property, &factuals, &leave);

        assert!((property[&2].produced - 10.0).abs() < 1e-6);
        assert!((property[&2].quantity - 6.833333).abs() < 1e-4);
        assert!((property[&3].produced - 3.166667).abs() < 1e-4);
    }

    /// # Produce Spends The Subsistence Floor On A Cheap Cover
    ///
    /// One point. Bread's cover is four runs and the flag is set, so those
    /// runs spend the farm's 0.25 and all four finish. The farm is loaded
    /// only so that floor exists.
    #[test]
    fn produce_spends_the_subsistence_floor_on_a_cheap_cover() {
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_output(ProcessOutput::new(3, 1.0, false));
        let farm = Process::new(29, "farm", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(1, 3.0, false));
        let factuals = Factuals::new().with_process(bread).with_process(farm);
        let mut job = Job::new(1, vec![JobLine::new(3, Some(4.0), vec![])]);
        job.lines[0].cover = 4.0;
        job.lines[0].cover_cheap = true;
        let mut property = HashMap::from([(0, PopPRow::new(TIME_PER_LABOR))]);

        job.produce(&mut property, &factuals);

        assert!((property[&3].produced - 4.0).abs() < 1e-9);
    }

    /// # Trade Score Charges More For The Heavier Process
    ///
    /// Both recipes make the same output and spend nothing else. The
    /// complexity-1 run scores 0.75 below the complexity-0.25 run.
    #[test]
    fn trade_score_charges_more_for_the_heavier_process() {
        let light = Process::new(1, "forage", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(2, 1.0, true));
        let heavy = Process::new(2, "mill", 0)
            .with_complexity(1.0)
            .with_output(ProcessOutput::new(2, 1.0, true));
        let history = priced(&[(2, 10.0)]);

        let light_score = super::trade_score(&light, &history);
        let heavy_score = super::trade_score(&heavy, &history);
        assert!((light_score - 9.75).abs() < 1e-12);
        assert!((heavy_score - 9.0).abs() < 1e-12);
    }

    /// # Plan Purchases Caps A Paying Line At Its Complexity Points
    ///
    /// Four labor-days of time would fund 256 runs at one time each. The
    /// line's complexity is 1, so the target stops at four runs.
    #[test]
    fn plan_purchases_caps_a_paying_line_at_its_complexity_points() {
        let process = Process::new(1, "bake", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let factuals = Factuals::new().with_process(process);
        let mut job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        let property = HashMap::from([(0, PopPRow::new(4.0 * TIME_PER_LABOR))]);
        let history = board(&[(0, 1.0), (2, 5.0)]);

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        assert!((job.lines[0].target.expect("target") - 4.0).abs() < 1e-9);
    }

    /// # Plan Purchases Keeps A Replacement Need
    ///
    /// The mill makes the farm's good, so the two runs that cover that gap
    /// spend the farm's complexity. Four pots runs do not fit in the two
    /// points beside that cover, so pots is cut to one and a half and the
    /// mill keeps both runs. The farm's output is variable, which is what
    /// makes the mill a replacement.
    #[test]
    fn plan_purchases_keeps_a_replacement_need_when_another_specialty_does_not_fit() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 1.0, false));
        let mill = Process::new(1, "mill", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 2.0, false));
        let pots = Process::new(5, "pots", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 1.0, false));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_process(mill)
            .with_process(pots);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
                JobLine::new(5, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(2.0 * TIME_PER_LABOR))]);
        let history = board(&[(0, 1.0), (1, 5.0), (7, 5.0)]);
        let gaps = [(vec![(1, 1.0)], 4.0), (vec![(7, 1.0)], 4.0)];

        job.plan_purchases(
            &property,
            &HashMap::new(),
            &factuals,
            &history,
            &gaps,
            gaps.len(),
        );

        // Mill outscores pots, and pots outscore the farm.
        assert_eq!(job.lines[0].process, 1);
        assert_eq!(job.lines[1].process, 5);
        assert_eq!(job.lines[2].process, 2);
        let mill_runs = job.lines[0].target.unwrap_or(0.0);
        let pot_runs = job.lines[1].target.unwrap_or(0.0);
        let farm_runs = job.lines[2].target.unwrap_or(1.0);
        assert!((mill_runs - 2.0).abs() < 1e-9, "mill {mill_runs}");
        assert!((pot_runs - 1.5).abs() < 1e-9, "pots {pot_runs}");
        assert!(farm_runs.abs() < 1e-9, "farm {farm_runs}");
    }

    /// # Plan Purchases Spends The Cover Rate On The Need
    ///
    /// The mill's two need runs spend the farm's 0.25. Two points leave one
    /// and a half, and that remainder buys one and a half more mill runs at
    /// the mill's own complexity. The target is three and a half.
    #[test]
    fn plan_purchases_buys_more_of_a_replacement_with_the_points_the_cover_leaves() {
        let farm = Process::new(2, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 1.0, false));
        let mill = Process::new(1, "mill", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 2.0, false));
        let factuals = Factuals::new().with_process(farm).with_process(mill);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(2, None, vec![]),
                JobLine::new(1, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(2.0 * TIME_PER_LABOR))]);
        let history = board(&[(0, 1.0), (1, 5.0)]);
        let gaps = [(vec![(1, 1.0)], 4.0)];

        job.plan_purchases(
            &property,
            &HashMap::new(),
            &factuals,
            &history,
            &gaps,
            gaps.len(),
        );

        // The mill scores higher, so the morning stores it ahead of the farm.
        assert_eq!(job.lines[0].process, 1);
        let mill_runs = job.lines[0].target.unwrap_or(0.0);
        assert!((mill_runs - 3.5).abs() < 1e-9, "mill {mill_runs}");
    }

    /// Six water a run, ten runs on the line, and time enough to repeat them.
    fn water_job(target: f64, made: f64, lost: f64) -> (Job, Factuals, HashMap<usize, PopPRow>) {
        let process = Process::new(2, "make water", 0)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 6.0, true));
        let factuals = Factuals::new().with_process(process);
        let job = Job::new(1, vec![JobLine::new(2, Some(target), vec![])]);
        let mut water = PopPRow::new(made);
        water.produced = made;
        water.lost = lost;
        let property = HashMap::from([(0, PopPRow::new(10_000.0)), (2, water)]);
        (job, factuals, property)
    }

    fn water_card(price: f64) -> MarketHistory {
        priced(&[(0, 0.0), (2, price)])
    }

    /// Nothing sold, and the card is above a zero cost. Surplus aims at nothing.
    /// Ten runs step once. They do not drop to zero in that night.
    #[test]
    fn plan_aims_an_unsold_surplus_at_nothing() {
        let (mut job, factuals, property) = water_job(10.0, 60.0, 0.0);
        job.note_sale(&HashMap::from([(2, 1_000.0)]), &HashMap::new());

        job.plan(
            &HashMap::new(),
            &property,
            &factuals,
            &water_card(2.0),
            1.0,
            false,
        );

        assert!((job.lines[0].target.unwrap_or(0.0) - 7.5).abs() < 1e-9);
    }

    /// # Plan Purchases Reserves Inputs For An Unmade Desire
    ///
    /// Nothing on the job makes bread. Ten households send half the pile,
    /// and three bread runs need six grain, six water, and 1.2 wood. Those
    /// runs are held. The lines do not pay, so the spare points are split
    /// across the three lines on top of that hold.
    #[test]
    fn plan_purchases_reserves_inputs_for_an_unmade_desire() {
        let farm = Process::new(29, "farm", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(1, 3.0, false));
        let water = Process::new(30, "water", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(2, 3.0, false));
        let forage = Process::new(31, "forage", 0)
            .with_complexity(0.25)
            .with_output(ProcessOutput::new(7, 3.0, false));
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(2, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(7, 0.2, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 3.0, false));
        let mut factuals = Factuals::new()
            .with_process(farm)
            .with_process(water)
            .with_process(forage)
            .with_process(bread);
        factuals.config.pop.turn_length = 4.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(29, None, vec![]),
                JobLine::new(30, None, vec![]),
                JobLine::new(31, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(8.0 * TIME_PER_LABOR))]);
        let gaps = [(vec![(3, 1.0)], 1.0)];
        // The outputs are priced under the complexity, so the lines do not pay.
        let history = priced(&[(1, 0.01), (2, 0.01), (7, 0.01)]);

        job.plan_purchases_for(
            &property,
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            &gaps,
            0,
            10.0,
        );

        let farm_runs = job.lines[0].target.unwrap_or(0.0);
        let water_runs = job.lines[1].target.unwrap_or(0.0);
        let forage_runs = job.lines[2].target.unwrap_or(0.0);
        // Stake 2 plus 46/5 from the spare points. Forage's stake is 2/5.
        assert!((farm_runs - 56.0 / 5.0).abs() < 1e-6, "{farm_runs}");
        assert!((water_runs - 56.0 / 5.0).abs() < 1e-6, "{water_runs}");
        assert!((forage_runs - 48.0 / 5.0).abs() < 1e-6, "{forage_runs}");
    }

    /// A long turn still steps from the scaled target.
    ///
    /// The stored target is forty and the household count doubles, so the
    /// step starts at eighty. One morning closes a quarter of the gap
    /// toward the aim a new line would write. The turn length does not
    /// change that step.
    #[test]
    fn plan_writes_the_aim_when_the_turn_is_long() {
        let (mut job, mut factuals, property) = water_job(40.0, 0.0, 0.0);
        factuals.config.pop.turn_length = 1_000_000.0;
        job.planned_households = 4.0;
        job.plan_purchases_for(
            &property,
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &water_card(2.0),
            &[],
            0,
            8.0,
        );
        let (mut fresh, fresh_factuals, fresh_property) = water_job(0.0, 0.0, 0.0);
        fresh.lines[0].target = None;
        let mut fresh_factuals = fresh_factuals;
        fresh_factuals.config.pop.turn_length = 1_000_000.0;
        fresh.plan_purchases_for(
            &fresh_property,
            &HashMap::new(),
            &HashMap::new(),
            &fresh_factuals,
            &water_card(2.0),
            &[],
            0,
            8.0,
        );
        let target = job.lines[0].target.unwrap_or(0.0);
        let aim = fresh.lines[0].target.unwrap_or(0.0);
        let scaled = 40.0 * (8.0 / 4.0);
        let expected = scaled + (aim - scaled) * 0.25;
        assert!(
            (target - expected).abs() < 1e-6,
            "stored {target} aim {aim} expected {expected}"
        );
        assert!(
            (aim - scaled).abs() > 1.0,
            "aim {aim} sits on the scaled target"
        );
        assert!((job.planned_households - 8.0).abs() < 1e-9);
    }

    /// # Plan Purchases Bakes Extra For An Unmade Desire
    ///
    /// Nothing on the job makes cloth. The card for that gap is forty.
    /// Bread sells for ten a run, so the bill is four bread runs. The purse
    /// is empty and the grain pile is empty, so those four runs stay on the
    /// bread line and the farm holds the grain they eat.
    #[test]
    fn plan_purchases_bakes_extra_for_an_unmade_desire() {
        let farm = Process::new(29, "farm", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 4.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 1.0, false));
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 2.0, false));
        let factuals = Factuals::new().with_process(farm).with_process(bread);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(29, None, vec![]),
                JobLine::new(3, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(8.0 * TIME_PER_LABOR))]);
        let mut history = board(&[(0, 0.1), (1, 1.0), (3, 5.0), (8, 2.0), (9, 1.0)]);
        history.currencies = vec![9];
        let gaps = [(vec![(8, 1.0)], 20.0)];

        job.plan_purchases(&property, &HashMap::new(), &factuals, &history, &gaps, 0);

        let farm_runs = job.lines[0].target.unwrap_or(0.0);
        let bread_runs = job.lines[1].target.unwrap_or(0.0);
        assert!((bread_runs - 4.0).abs() < 1e-6, "bread {bread_runs}");
        assert!((farm_runs - 4.0).abs() < 1e-6, "farm {farm_runs}");
    }

    /// # Sought Sale Runs Skip A Fixed Output
    ///
    /// The byproduct is fixed and sought. It adds no runs. Twenty-four of
    /// the variable output is two runs. Naming that output as tender
    /// leaves none.
    #[test]
    fn sought_sale_runs_skips_a_fixed_output() {
        let process = Process::new(3, "bread", 0)
            .with_output(ProcessOutput::new(3, 12.0, false))
            .with_output(ProcessOutput::new(31, 0.01, true));
        let factuals = Factuals::new();
        let mut history = MarketHistory::new();
        history.sought.insert(31, 100.0);

        assert_eq!(
            super::sought_sale_runs(&process, &factuals, &history),
            0.0
        );

        history.sought.insert(3, 24.0);
        assert!(
            (super::sought_sale_runs(&process, &factuals, &history) - 2.0).abs() < 1e-9
        );

        history.currencies = vec![3];
        assert_eq!(
            super::sought_sale_runs(&process, &factuals, &history),
            0.0
        );
    }

    /// # Plan Purchases Steps Toward A Sought Sale
    ///
    /// Four points are free. The book still wants a hundred and twenty
    /// loaves, ten runs, and the points fund four of them. The stored
    /// target is two, so one morning closes a quarter of the gap and
    /// lands on two and a half. With nothing sought, the empty purse
    /// keeps the stored two. Eight offered and none exchanged still
    /// steps toward the meal.
    #[test]
    fn plan_purchases_steps_toward_a_sought_sale() {
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 12.0, false));
        let factuals = Factuals::new().with_process(bread);
        let property = HashMap::from([(0, PopPRow::new(4.0 * TIME_PER_LABOR))]);
        let mut history = board(&[(0, 0.1), (1, 1.0), (3, 5.0), (9, 1.0)]);
        history.currencies = vec![9];
        history.sought.insert(3, 120.0);
        let mut job = Job::new(1, vec![JobLine::new(3, Some(2.0), vec![])]);
        job.planned_households = 1.0;

        job.plan_purchases_for(
            &property,
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            &[],
            0,
            1.0,
        );

        assert!(
            (job.lines[0].target.unwrap_or(0.0) - 2.5).abs() < 1e-6,
            "sought {}",
            job.lines[0].target.unwrap_or(0.0)
        );

        history.sought.remove(&3);
        let mut quiet = Job::new(1, vec![JobLine::new(3, Some(2.0), vec![])]);
        quiet.planned_households = 1.0;
        quiet.plan_purchases_for(
            &property,
            &HashMap::new(),
            &HashMap::new(),
            &factuals,
            &history,
            &[],
            0,
            1.0,
        );
        assert!(
            (quiet.lines[0].target.unwrap_or(0.0) - 2.0).abs() < 1e-6,
            "quiet {}",
            quiet.lines[0].target.unwrap_or(0.0)
        );

        let grain = Process::new(1, "grain", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(10, 1.0, false));
        let grain_factuals = Factuals::new().with_process(grain);
        let mut unsold = board(&[(0, 0.1), (10, 10.0)]);
        unsold.mass_offered.insert(10, 8.0);
        unsold.mass_exchanged.insert(10, 0.0);
        unsold.sought.insert(10, 40.0);
        let mut glut = Job::new(1, vec![JobLine::new(1, Some(8.0), vec![])]);
        glut.planned_households = 1.0;
        glut.plan_purchases_for(
            &property,
            &HashMap::new(),
            &HashMap::new(),
            &grain_factuals,
            &unsold,
            &[],
            0,
            1.0,
        );
        assert!(
            (glut.lines[0].target.unwrap_or(0.0) - 6.0).abs() < 1e-6,
            "unsold {}",
            glut.lines[0].target.unwrap_or(0.0)
        );
    }

    /// # Plan Purchases Leaves The Bake When The Purse Covers The Gap
    ///
    /// The same cloth gap costs forty. Forty coins are already on hand, so
    /// the bread aim matches the plan with no gap at all.
    #[test]
    fn plan_purchases_skips_the_sale_when_the_purse_covers_the_gap() {
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 2.0, false));
        let factuals = Factuals::new().with_process(bread);
        let property = HashMap::from([
            (0, PopPRow::new(8.0 * TIME_PER_LABOR)),
            (9, PopPRow::new(40.0)),
        ]);
        let mut history = board(&[(0, 0.1), (3, 5.0), (8, 2.0), (9, 1.0)]);
        history.currencies = vec![9];
        let gaps = [(vec![(8, 1.0)], 20.0)];

        let mut covered = Job::new(1, vec![JobLine::new(3, None, vec![])]);
        covered.plan_purchases(&property, &HashMap::new(), &factuals, &history, &gaps, 0);
        let mut quiet = Job::new(1, vec![JobLine::new(3, None, vec![])]);
        quiet.plan_purchases(&property, &HashMap::new(), &factuals, &history, &[], 0);

        let with_gap = covered.lines[0].target.unwrap_or(0.0);
        let no_gap = quiet.lines[0].target.unwrap_or(0.0);
        assert!(
            (with_gap - no_gap).abs() < 1e-6,
            "gap {with_gap} no gap {no_gap}"
        );
    }

    /// # Plan Purchases Moves A Later Meal Onto The Sale
    ///
    /// Cotton's basic four runs stay. Its later four are the points the
    /// cloth bill needs. Bread takes one of those points and cotton keeps
    /// the basic meal.
    #[test]
    fn plan_purchases_moves_a_later_meal_onto_the_sale() {
        let cotton = Process::new(4, "cotton", 0)
            .with_complexity(0.25)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(5, 1.0, false));
        let bread = Process::new(3, "bread", 0)
            .with_complexity(1.0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 2.0, false));
        let mut factuals = Factuals::new().with_process(cotton).with_process(bread);
        factuals.config.pop.turn_length = 4.0;
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(4, None, vec![]),
                JobLine::new(3, None, vec![]),
            ],
        );
        let property = HashMap::from([(0, PopPRow::new(2.0 * TIME_PER_LABOR))]);
        let mut history = board(&[(0, 0.1), (3, 5.0), (5, 0.1), (8, 2.0), (9, 1.0)]);
        history.currencies = vec![9];
        let gaps = [
            (vec![(5, 1.0)], 4.0),
            (vec![(5, 1.0)], 4.0),
            (vec![(8, 1.0)], 20.0),
        ];

        job.plan_purchases(
            &property,
            &HashMap::new(),
            &factuals,
            &history,
            &gaps,
            1,
        );

        // Bread scores higher, so the morning stores it ahead of cotton.
        assert_eq!(job.lines[0].process, 3);
        assert_eq!(job.lines[1].process, 4);
        let bread_runs = job.lines[0].target.unwrap_or(0.0);
        let cotton_runs = job.lines[1].target.unwrap_or(0.0);
        assert!((cotton_runs - 4.0).abs() < 1e-6, "cotton {cotton_runs}");
        assert!((bread_runs - 1.0).abs() < 1e-6, "bread {bread_runs}");
    }

    /// Twelve sold, six a run. The card is above cost and the pile did not
    /// sell out, so the surplus is two runs. A cover of five water is one
    /// more run. The line starts just above that aim and the step lands on it.
    #[test]
    fn plan_aims_surplus_at_the_units_sold() {
        let (mut job, factuals, property) = water_job(3.2, 60.0, 0.0);
        job.note_sale(&HashMap::from([(2, 1_000.0)]), &HashMap::from([(2, 12.0)]));

        job.plan(
            &HashMap::from([(2, 5.0)]),
            &property,
            &factuals,
            &water_card(2.0),
            1.0,
            false,
        );

        assert!((job.lines[0].target.unwrap_or(0.0) - 3.0).abs() < 1e-9);
    }

    /// Time holds at 6, so one water costs 2. The card is 2. Six sold is one run.
    /// The line starts just above that run and the step lands on it.
    #[test]
    fn plan_aims_at_sales_when_the_card_is_at_cost() {
        let (mut job, factuals, property) = water_job(1.2, 60.0, 0.0);
        job.note_sale(&HashMap::from([(2, 100.0)]), &HashMap::from([(2, 6.0)]));

        job.plan(
            &HashMap::new(),
            &property,
            &factuals,
            &priced(&[(0, 6.0), (2, 2.0)]),
            1.0,
            false,
        );

        assert!((job.lines[0].target.unwrap_or(0.0) - 1.0).abs() < 1e-9);
    }

    /// Water decays by half. Cost is 2 and the floor is 1. A card of 1 still sells
    /// what sold, and a card of 0.5 aims the surplus at nothing.
    #[test]
    fn plan_follows_sales_down_to_the_decay_floor() {
        let (mut job, mut factuals, property) = water_job(2.5, 60.0, 0.0);
        factuals = factuals.with_good(good(2, 0.5));
        job.note_sale(&HashMap::from([(2, 40.0)]), &HashMap::from([(2, 6.0)]));
        job.plan(
            &HashMap::from([(2, 5.0)]),
            &property,
            &factuals,
            &priced(&[(0, 6.0), (2, 1.0)]),
            1.0,
            false,
        );
        assert!((job.lines[0].target.unwrap_or(0.0) - 2.0).abs() < 1e-9);

        job.note_sale(&HashMap::from([(2, 40.0)]), &HashMap::from([(2, 6.0)]));
        job.plan(
            &HashMap::from([(2, 5.0)]),
            &property,
            &factuals,
            &priced(&[(0, 6.0), (2, 0.5)]),
            1.0,
            false,
        );
        // The surplus is gone. One step from the two runs leaves the cover.
        assert!((job.lines[0].target.unwrap_or(0.0) - 1.5).abs() < 1e-9);
    }

    /// The pile sold out above cost, so the sale leaves the march.
    /// Ten runs were just made and the line pays, so the target stays at 10.
    /// A sale write would have been five runs or zero.
    #[test]
    fn plan_leaves_the_march_when_the_pile_sells_out_above_cost() {
        let (mut job, factuals, property) = water_job(10.0, 60.0, 0.0);
        job.note_sale(&HashMap::from([(2, 30.0)]), &HashMap::from([(2, 30.0)]));

        job.plan(
            &HashMap::new(),
            &property,
            &factuals,
            &water_card(2.0),
            1.0,
            false,
        );

        assert!((job.lines[0].target.unwrap_or(0.0) - 10.0).abs() < 1e-9);
    }

    /// Three units were listed and all three sold. Thirty remain in the pile.
    /// The card is above cost, so the sale leaves the march.
    #[test]
    fn plan_leaves_the_march_when_the_listed_offer_sells_out() {
        let (mut job, factuals, property) = water_job(10.0, 60.0, 0.0);
        job.note_sale(&HashMap::from([(2, 30.0)]), &HashMap::from([(2, 3.0)]));
        job.note_listed(2, 3.0);

        job.plan(
            &HashMap::new(),
            &property,
            &factuals,
            &water_card(2.0),
            1.0,
            false,
        );

        assert!((job.lines[0].target.unwrap_or(0.0) - 10.0).abs() < 1e-9);
    }

    /// One of three listed units sold. The surplus is that one unit.
    /// The line starts just above it, and the step lands on that unit.
    #[test]
    fn plan_aims_surplus_at_the_part_of_the_listing_that_sold() {
        let (mut job, factuals, property) = water_job(0.4, 60.0, 0.0);
        job.note_sale(&HashMap::from([(2, 30.0)]), &HashMap::from([(2, 1.0)]));
        job.note_listed(2, 3.0);

        job.plan(
            &HashMap::new(),
            &property,
            &factuals,
            &water_card(2.0),
            1.0,
            false,
        );

        assert!((job.lines[0].target.unwrap_or(0.0) - 1.0 / 6.0).abs() < 1e-9);
    }

    /// A stored listing of zero was not offered. The pile remains.
    /// The card is above cost, so the sale leaves the march.
    #[test]
    fn plan_leaves_the_march_when_nothing_was_listed() {
        let (mut job, factuals, property) = water_job(10.0, 60.0, 0.0);
        job.note_sale(&HashMap::from([(2, 30.0)]), &HashMap::new());
        job.note_listed(2, 0.0);

        job.plan(
            &HashMap::new(),
            &property,
            &factuals,
            &water_card(2.0),
            1.0,
            false,
        );

        assert!((job.lines[0].target.unwrap_or(0.0) - 10.0).abs() < 1e-9);
    }

    /// The card is under the floor. An empty listing still aims the surplus
    /// at nothing, because the pile was large enough to refuse.
    /// Ten runs take one step toward that aim.
    #[test]
    fn plan_zeros_an_unlisted_surplus_under_the_floor() {
        let (mut job, mut factuals, property) = water_job(10.0, 60.0, 0.0);
        factuals = factuals.with_good(good(2, 0.5));
        job.note_sale(&HashMap::from([(2, 40.0)]), &HashMap::new());
        job.note_listed(2, 0.0);

        job.plan(
            &HashMap::new(),
            &property,
            &factuals,
            &priced(&[(0, 6.0), (2, 0.5)]),
            1.0,
            false,
        );

        assert!((job.lines[0].target.unwrap_or(0.0) - 7.5).abs() < 1e-9);
    }

    /// Nothing was offered, so the sale leaves the march.
    /// The same paying line stays at the ten runs it just made.
    #[test]
    fn plan_leaves_the_march_when_the_pile_was_not_offered() {
        let (mut job, factuals, property) = water_job(10.0, 60.0, 0.0);
        job.note_sale(&HashMap::new(), &HashMap::new());

        job.plan(
            &HashMap::new(),
            &property,
            &factuals,
            &water_card(2.0),
            1.0,
            false,
        );

        assert!((job.lines[0].target.unwrap_or(0.0) - 10.0).abs() < 1e-9);
    }

    /// The morning made none. An unsold pile under the floor does not zero the line.
    #[test]
    fn plan_leaves_the_march_when_nothing_was_made() {
        let (mut job, factuals, property) = water_job(10.0, 0.0, 0.0);
        job.note_sale(&HashMap::from([(2, 30.0)]), &HashMap::new());

        job.plan(
            &HashMap::new(),
            &property,
            &factuals,
            &priced(&[(0, 5.0), (2, 0.0)]),
            1.0,
            false,
        );

        assert!((job.lines[0].target.unwrap_or(0.0) - 7.5).abs() < 1e-9);
    }

    /// # Priced
    ///
    /// A card where each listed good has that AMV and salability 1.
    ///
    /// A positive AMV is then the holding. Returns the snapshot.
    fn priced(cards: &[(usize, f64)]) -> MarketHistory {
        let mut history = MarketHistory::new();
        for &(good, amv) in cards {
            history.prices.insert(good, amv);
            history.salability.insert(good, 1.0);
        }
        history
    }
}
