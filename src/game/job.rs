//! A pop's cottage work: the processes it can run and the day's plan.
//!
//! The stock is the pop's. The job does not set a price and does not sell.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::game::actor::Actor;
use crate::game::craft::Craft;
use crate::game::factuals::Factuals;
use crate::game::good::TIME;
use crate::game::market::MarketHistory;
use crate::game::marketorder::MarketOrder;
use crate::game::pop_property::PopPRow;
use crate::game::process::{InputType, Process, ProcessEffect, ProcessInput, ProcessResult};
use crate::game::util::whole_units_up;

/// Share of this pop's own output where surplus holds.
/// Under it, surplus may grow. Over it, surplus pulls back.
const SURPLUS_ROT_SHARE: f64 = 0.1;

/// Fraction of a line's current target it may move in one night.
///
/// A target below 1 uses 1 as the base, so a resting line can leave 0.
const PLAN_MARCH: f64 = 0.25;

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
/// The disadvantages: It can't play the market, simply accepting at or near market AMV
/// price. It can't do directed research, simply pushing a bit of research everywhere
/// and quickly distributing it to the wider society, reducing it's competative
/// advantage. It can't separate pop consumption from the work fully. It has a higher
/// complexity penalty, as it's not unified. It's also self-competing, if prices are
/// falling, it has no mechanism to manage that beyond cutting back on production and
/// reducing supply.
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
    /// drops it.
    shopping: HashMap<usize, f64>,
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
/// Same shape as a firm's production line. `inputs` lists optional goods
/// this line may draw. Required inputs are always drawn.
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
    /// Optional inputs this line is allowed to draw.
    pub inputs: Vec<usize>,
    /// True when [`Job::plan`] last scored this line above zero.
    ///
    /// Stays true until a plan runs, so the first morning does not shop an output.
    pub pays: bool,
    /// Order this line was added. Set once in [`Self::new`].
    home: u64,
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
            inputs,
            pays: true,
            home: next_home(),
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
            plan_cost: 0.0,
            claimed: HashMap::new(),
            shopping: HashMap::new(),
            input_feeds: Vec::new(),
            kept: HashMap::new(),
        }
    }

    /// # Leave
    ///
    /// Units the morning run must leave in quantity.
    ///
    /// `pile` replaces the previous map. A non-positive amount is dropped.
    /// Reserve claims only the free stock above the pile, produce will not
    /// spend the pile, and settle does not count it as stock for the next
    /// buy. Output made this morning counts toward the pile, and only the
    /// surplus over it can feed a later line.
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
    /// Drops the morning's claim book, the shopping list, and the input links.
    ///
    /// Line targets stay, so last night's plan is what the next reserve runs.
    /// Does not touch pop property. [`crate::game::pop::Pop::reset_day`]
    /// zeros `reserved` on the rows, which releases the claim itself.
    /// The next [`Self::reserve`] writes the input links again.
    pub fn reset_day(&mut self) {
        self.claimed.clear();
        self.shopping.clear();
        self.input_feeds.clear();
        self.kept.clear();
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
    /// That cover is a floor under the line.
    ///
    /// Every line with a positive [`trade_score`] adds its own surplus on
    /// that floor. The extra iterations start from [`surplus_iterations`]
    /// on the inputs the cover leaves, after earlier lines have taken
    /// theirs, then [`paced_surplus`] pulls that run toward the output
    /// just made. Unmet buys of an output, plus units downstream production
    /// used, above stock on hand, add more runs on the best line for that
    /// good. The ideal is the cover plus the surplus and those runs. A line
    /// that does not pay aims at the cover and those runs. A snap still aims there: the new cover is written in whole when
    /// the step has not reached it, and the old target is not added on top.
    /// A line that is already running keeps one run when that aim is
    /// gone. A line at 0 stays at 0.
    ///
    /// [`march_target`] moves the stored target one
    /// step toward its aim. A snap writes a cover above the current target
    /// in whole, and does not add the old target to it. A covering line
    /// returns to the order it was added, ahead of a line that only pays or
    /// only feeds one of those lines. A line that is not covering, and does
    /// not feed one that is, moves to the back. A line that makes an input
    /// runs before the line that consumes it. The
    /// line records that score for [`Self::shop_unpaid_outputs`].
    /// `snap_cover` is a population change or a new or removed desire. A
    /// paying line that already has a stored cover writes the new cover
    /// whole and steps only the surplus above that old cover. A paying
    /// line whose stored cover is still 0 steps its current target toward
    /// the new cover plus the extra.
    ///
    /// An empty line list stores [`Self::plan_cost`] as `0` and returns.
    /// Otherwise [`Self::plan_cost`] is the complexity cost of the targets
    /// just written.
    pub fn plan(
        &mut self,
        cover: &HashMap<usize, f64>,
        property: &HashMap<usize, PopPRow>,
        factuals: &Factuals,
        history: &MarketHistory,
        modifier: f64,
        snap_cover: bool,
    ) {
        if self.lines.is_empty() {
            self.plan_cost = 0.0;
            return;
        }

        // Cover. The best line on the job takes each short good. Then a
        // cover line's made inputs are added, and the assignment is run again.
        let mut cover_units = cover.clone();
        let mut cover_targets = assign_cover(&self.lines, &cover_units, factuals);
        add_input_needs(&self.lines, &cover_targets, &mut cover_units, factuals);
        cover_targets = assign_cover(&self.lines, &cover_units, factuals);
        // Unmet buys the best line can still fill, after stock on hand.
        let sought_extra = sought_runs(&self.lines, &cover_targets, property, factuals, history);

        // Inputs left for a surplus. Copy quantity, then subtract each cover's
        // required inputs times its iterations. What remains can fund a surplus.
        let mut remaining: HashMap<usize, f64> = property
            .iter()
            .filter(|(_, row)| row.quantity > 0.0)
            .map(|(&good, row)| (good, row.quantity))
            .collect();
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
        let mut scores = Vec::with_capacity(self.lines.len());
        for line in &self.lines {
            let score = factuals
                .get_process(line.process)
                .map(|process| trade_score(process, factuals, history));
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
            let extra = paced_surplus(process, property, raw);
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

        // Step toward the ideal. A paying line aims at cover plus extra.
        // An unpaid line aims at the cover. One already running keeps a run.
        for (index, line) in self.lines.iter_mut().enumerate() {
            let new_cover = cover_targets[index];
            let current = line.target.unwrap_or(0.0);
            let pays = scores[index].is_some_and(|score| score > 0.0);
            let ideal = new_cover + extras[index] + sought_extra[index];
            // The next morning's reserve reads this when it shops an output.
            line.pays = pays;
            let target = if snap_cover && pays && line.cover == 0.0 && current > 0.0 {
                // No stored cover yet, so march the current target instead of adding it to the cover.
                march_target(current, ideal)
            } else if snap_cover && pays {
                let old_surplus = (current - line.cover).max(0.0);
                new_cover + march_target(old_surplus, extras[index] + sought_extra[index])
            } else if snap_cover && new_cover > current {
                // The new cover replaces the old target. Unmet buys stay in the aim.
                let aim = new_cover + extras[index] + sought_extra[index];
                march_target(current, aim).max(new_cover)
            } else if pays {
                march_target(current, ideal)
            } else {
                // A line already running keeps one run after its cover and
                // any unmet buy are gone. A line at 0 stays there.
                let aim = if ideal > 0.0 {
                    ideal
                } else if current > 0.0 {
                    1.0
                } else {
                    0.0
                };
                march_target(current, aim)
            };
            if snap_cover {
                line.cover = new_cover;
            }
            line.target = Some(target);
        }

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
    /// is not shopped. Other optional inputs are claimed and shopped only
    /// when the line lists them. `None` claims what is free and does not
    /// shop an open-ended amount. `Some(0.0)` skips the line.
    /// [`Self::shop_unpaid_outputs`] then shops for lines that did not pay.
    /// [`Self::note_input_feeds`] records which surviving inputs make each output.
    pub fn reserve(&mut self, property: &mut HashMap<usize, PopPRow>, factuals: &Factuals) {
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
    fn shop_unpaid_outputs(
        &mut self,
        property: &HashMap<usize, PopPRow>,
        factuals: &Factuals,
    ) {
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
    /// Optional inputs count when the line lists them. Factors and capital
    /// stay out. A good that decays completely stays out. The same input and
    /// output keep the smaller input-per-output. The pop reads the record
    /// through [`Self::feeds_of`].
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

    /// # Produce
    ///
    /// Runs each line up to its target and writes the result onto `property`.
    ///
    /// Returns the process effects. Negative changes leave `quantity` and
    /// the claim. Positive changes are new stock, added to `quantity`,
    /// `fresh`, and `produced`. Capital moves from `quantity` into `used`.
    /// Factor claims stay reserved so the factor is not sold. Other leftover
    /// claims are released. Shopping stays for [`Job::buy_orders`].
    ///
    /// Lines run in order. An unlimited line can spend inputs a later line
    /// also claimed. A planned line stops at its own quota. A line that
    /// spends time leaves enough for a later line it feeds when that line
    /// yields more per unit of time. The pile from [`Self::leave`] stays
    /// in quantity. Stock above that pile, including output an earlier
    /// line just made, is spent.
    pub fn produce(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        factuals: &Factuals,
    ) -> Vec<ProcessEffect> {
        if self.lines.is_empty() {
            return Vec::new();
        }
        let mut effects = Vec::new();
        let lines = self.lines.clone();
        for (index, line) in lines.iter().enumerate() {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let mut running = line.clone();
            match line.target {
                Some(n) if n > 0.0 => {
                    let pace = time_spent(process);
                    if pace > 0.0 {
                        let hold = time_held_for_later(&lines, index, factuals);
                        let room = (time_ready(property, &self.claimed) - hold).max(0.0);
                        let capped = n.min(room / pace);
                        if capped <= 0.0 {
                            continue;
                        }
                        running.target = Some(capped);
                    }
                }
                None => {}
                Some(_) => continue,
            }
            // Pass this line's inputs only, capped at its quota when it has one.
            let inputs = self.inputs_for(process, &running, property);
            if running.target.is_none() && !inputs_bound(process, &inputs) {
                continue;
            }
            let result = process.do_process(&inputs, running.target, factuals);
            self.apply_result(property, &result);
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
    pub fn settle_shopping(
        &mut self,
        property: &HashMap<usize, PopPRow>,
        factuals: &Factuals,
    ) {
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
    /// Required inputs and listed optionals are visited. `iterations` `None`
    /// only marks the free stock as spoken for. `spoken` is stock earlier
    /// lines in this rebuild have already counted. `forthcoming` is output
    /// those lines will make tomorrow morning, and this line's share is
    /// taken out of it.
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
    /// and shops the next run. `factuals` supplies each input's decay.
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
            if line.inputs.contains(&input.good) {
                self.claim_one(property, &input, iterations, factuals);
            }
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
            // Claimed stock, plus free stock above the kept pile. Output an
            // earlier line just made is in that free stock.
            let claimed_amount = self.claimed.get(&input.good).copied().unwrap_or(0.0);
            let unreserved = self.free_above_kept(property, input.good);
            let have = claimed_amount + unreserved;
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
    /// `used_inputs` is capital set aside for the day.
    fn apply_result(&mut self, property: &mut HashMap<usize, PopPRow>, result: &ProcessResult) {
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
    }

    /// # Spend
    ///
    /// Removes `amount` of `good` from quantity.
    ///
    /// The take stops where [`Self::leave`] is holding a pile back, so
    /// quantity does not fall below that pile. Free units above it go first.
    /// Reserved units and the claim are reduced only for what those free
    /// units did not cover, so the pile stays free. When `capital` is set, the units taken are added to
    /// `used` so decay can return them.
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
        // The kept pile stays in quantity. Free units above it go first, so a
        // desire's reserve is left in place.
        let floor = self.kept.get(&good).copied().unwrap_or(0.0).max(0.0);
        let can = (row.quantity.max(0.0) - floor).max(0.0);
        let take = amount.min(can);
        if take <= 0.0 {
            return;
        }
        let free = (row.quantity - row.reserved - floor).max(0.0);
        let from_reserved = (take - free.min(take)).max(0.0);
        row.quantity -= take;
        row.reserved = (row.reserved - from_reserved).max(0.0);
        if capital {
            row.used += take;
        }
        if let Some(left) = self.claimed.get_mut(&good) {
            *left = (*left - from_reserved).max(0.0);
        }
    }

    /// # Free Above Kept
    ///
    /// Free units of `good` this run may take.
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
fn assign_cover(
    lines: &[JobLine],
    cover: &HashMap<usize, f64>,
    factuals: &Factuals,
) -> Vec<f64> {
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
/// Cover units of each good that costs more to make than to buy.
///
/// `lines` is the job. `cover` is the gap in units. `factuals` supplies
/// processes and which goods survive the night. `history` supplies holding.
/// The line is the one [`assign_cover`] would pick after made inputs are
/// added back onto the gap. One unit's make cost is that line's required
/// inputs, divided by its output. Time that does not survive the night is
/// costed at 1. A good that does not survive the night is skipped. A make
/// cost above that good's holding is returned with the gap units. The job
/// still produces the gap.
pub(crate) fn buys_cheaper_than_making(
    lines: &[JobLine],
    cover: &HashMap<usize, f64>,
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
        let Some(cost) = make_cost_per_unit(process, good, factuals, history) else {
            continue;
        };
        if cost > history.holding_per_unit(good) {
            buys.insert(good, gap);
        }
    }
    buys
}

/// # Make Cost Per Unit
///
/// Holding one unit of `good` costs to make with `process`.
///
/// `factuals` and `history` price the required inputs. Optional inputs,
/// factors, and capital stay out. Returns `None` when `process` does not
/// make `good`.
fn make_cost_per_unit(
    process: &Process,
    good: usize,
    factuals: &Factuals,
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
        cost += input.amount * input_holding(factuals, history, input.good);
    }
    if !cost.is_finite() {
        return None;
    }
    Some(cost / output.amount)
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
/// [`input_used`]. The best line for a good takes the whole shortfall above
/// stock when its yield matches the best process in `factuals`. A worse line
/// adds no more than its cover. A worse line whose make cost is above that
/// good's holding adds none: the buy is the cheaper way to cover it. Returns
/// one extra per line.
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
        let costs_more = make_cost_per_unit(process, good, factuals, history)
            .is_some_and(|cost| cost > history.holding_per_unit(good));
        // A worse recipe stays inside the cover. One that is cheaper to buy
        // takes none of this shortfall. The best known recipe takes it all.
        let allow = if pace >= best_known_yield(factuals, good) {
            runs
        } else if costs_more {
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
    if time <= 0.0 {
        amount
    } else {
        amount / time
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

/// # Time Held For Later
///
/// Time `lines[index]` must leave for a later line it feeds.
///
/// A later line counts when [`line_feeds`] is true and its [`line_yield`]
/// is higher. The time is that line's target times its required time.
/// `factuals` supplies both processes. A missing process adds nothing.
fn time_held_for_later(lines: &[JobLine], index: usize, factuals: &Factuals) -> f64 {
    let Some(process) = factuals.get_process(lines[index].process) else {
        return 0.0;
    };
    let mine = line_yield(process);
    let mut held = 0.0;
    for later in lines.iter().skip(index + 1) {
        let Some(later_process) = factuals.get_process(later.process) else {
            continue;
        };
        if line_yield(later_process) <= mine || !line_feeds(&lines[index], later, factuals) {
            continue;
        }
        let Some(n) = later.target else {
            continue;
        };
        if n > 0.0 {
            held += time_spent(later_process) * n;
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

/// # Trade Score
///
/// Holding gained by running `process` once, on `history`'s board.
///
/// Output holding is added. Destroyed and consumed inputs are subtracted.
/// An input that does not survive the night is costed at holding 1, which
/// is AMV 1 and salability 1. Factors, optional inputs, and capital stay
/// out. Returns the net.
fn trade_score(process: &Process, factuals: &Factuals, history: &MarketHistory) -> f64 {
    let mut score = 0.0;
    for output in &process.outputs {
        if output.amount <= 0.0 {
            continue;
        }
        score += output.amount * history.holding_per_unit(output.good);
    }
    for input in &process.inputs {
        if input.is_optional() {
            continue;
        }
        if !matches!(input.input_type, InputType::Destroyed | InputType::Consumed) {
            continue;
        }
        score -= input.amount * input_holding(factuals, history, input.good);
    }
    score
}

/// # Input Holding
///
/// Holding `trade_score` subtracts for one unit of `good`.
///
/// An input that survives the night uses [`MarketHistory::holding_per_unit`].
/// One that does not, such as time, returns 1.
fn input_holding(factuals: &Factuals, history: &MarketHistory, good: usize) -> f64 {
    if survives_night(factuals, good) {
        history.holding_per_unit(good)
    } else {
        1.0
    }
}

/// # Paced Surplus
///
/// Extra runs left after rot and output still on hand pull `raw` back
/// toward the output just made.
///
/// `process` supplies the outputs. `property` is the pop's rows. `raw` is
/// the input-limited surplus. An output with `amount <= 0`, or a missing
/// row, does not change `raw`.
///
/// `produced > 0` sets the room from `(lost + kept) / produced`. `kept` is
/// `quantity` clamped to `0..=produced`. At [`SURPLUS_ROT_SHARE`] the room
/// is one, so the extra stays at that output's runs. A smaller share raises
/// the room, up to twice the runs when nothing rotted and nothing of that
/// output is left on hand. A larger share lowers it by
/// `SURPLUS_ROT_SHARE / share`. `produced == 0` and `lost > 0` leaves one
/// run. The tightest output wins. Returns that many runs, never negative.
fn paced_surplus(process: &Process, property: &HashMap<usize, PopPRow>, raw: f64) -> f64 {
    if raw <= 0.0 {
        return 0.0;
    }
    let mut paced = raw;
    for output in &process.outputs {
        if output.amount <= 0.0 {
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
    use crate::game::craft::Craft;
    use crate::game::factuals::Factuals;
    use crate::game::good::Good;
    use crate::game::job::{Job, JobLine};
    use crate::game::market::MarketHistory;
    use crate::game::pop_property::PopPRow;
    use crate::game::process::{InputType, Process, ProcessEffect, ProcessInput, ProcessOutput};

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

        job.plan(
            &HashMap::from([(2, 3.0)]),
            &HashMap::new(),
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

        job.plan(
            &HashMap::from([(2, 1.0), (4, 1.0)]),
            &HashMap::new(),
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

        job.plan(&HashMap::new(), &HashMap::new(), &factuals, &MarketHistory::new(), 1.0, true);

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

        job.plan(
            &HashMap::from([(2, 1.0)]),
            &HashMap::new(),
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
        job.plan(
            &HashMap::from([(2, 3.0)]),
            &HashMap::new(),
            &factuals,
            &MarketHistory::new(),
            1.0,
            true,
        );

        assert_eq!(job.lines[0].target, Some(4.0));
    }

    #[test]
    fn plan_adds_one_output_unit_above_the_gap() {
        let make = Process::new(1, "make grain", 0)
            .with_output(ProcessOutput::new(1, 2.0, true));
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
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let weave = Process::new(8, "weave", 0)
            .with_input(ProcessInput::new(3, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let mut job = Job::new(
            1,
            vec![JobLine::new(7, Some(1.0), vec![]), JobLine::new(8, Some(1.0), vec![])],
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
        job.plan(&HashMap::from([(2, 2.0)]), &property, &factuals, &history, 1.0, true);

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
        job.plan(&HashMap::from([(2, 2.0)]), &property, &factuals, &history, 1.0, true);

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
        job.plan(&HashMap::from([(2, 2.0)]), &property, &factuals, &history, 1.0, true);

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

        // The whole output is still on hand, so the share is 1 and the room
        // is a tenth of the one run. The line is at 1, so it steps toward 0.1.
        assert_eq!(job.lines[0].target, Some(0.75));
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

        // 13 bread short, plus one unit. The line does not pay, so the snap
        // raises 12.5 to that cover and does not add them.
        job.plan(&HashMap::from([(2, 13.0)]), &HashMap::new(), &factuals, &history, 1.0, true);

        assert_eq!(job.lines[0].target, Some(14.0));
        assert!((job.lines[0].cover - 14.0).abs() < 1e-12);
    }

    /// The stored target is already the cover. A snap still steps toward the
    /// unmet buys instead of writing only the new cover.
    #[test]
    fn plan_snap_keeps_unmet_buys_on_an_unpaid_cover() {
        let mut line = JobLine::new(7, Some(10.0), vec![]);
        line.cover = 10.0;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        history.prices.insert(1, 0.0);
        history.sought.insert(2, 40.0);

        job.plan(&HashMap::from([(2, 10.0)]), &HashMap::new(), &factuals, &history, 1.0, true);

        // Cover is 11. Sought of 40 sits above that. The step from 10 is a quarter of 10.
        assert_eq!(job.lines[0].target, Some(12.5));
        assert!((job.lines[0].cover - 11.0).abs() < 1e-12);
    }

    /// The stored target is already near the unmet buys. A covering line
    /// steps to the whole shortfall, not to one extra cover.
    #[test]
    fn plan_sought_on_a_covering_line_takes_the_whole_shortfall() {
        let mut line = JobLine::new(7, Some(48.0), vec![]);
        line.cover = 11.0;
        let mut job = Job::new(1, vec![line]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        history.prices.insert(1, 0.0);
        history.sought.insert(2, 40.0);

        // Gap 10 plus one unit is a cover of 11. Sought 40 is the whole
        // shortfall. The line does not pay, so 48 steps to 51.
        job.plan(
            &HashMap::from([(2, 10.0)]),
            &HashMap::new(),
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
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 2.0, true));
        let saw = Process::new(7, "make wood", 0)
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

        // Gap 10 plus one unit is 5.5 forage runs. Sought of 100 stays inside
        // that cover, so the aim is 11. The line is at 10 and takes the step.
        job.plan(
            &HashMap::from([(7, 10.0)]),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(11.0));
        assert!((job.lines[0].cover - 5.5).abs() < 1e-12);
    }

    /// Forage costs more per wood than the board, and a faster wood process
    /// is known. Sought wood stays on that faster process.
    #[test]
    fn plan_sought_skips_a_worse_line_that_is_cheaper_to_buy() {
        let forage = Process::new(31, "subsistence forage", 0)
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 2.0, true));
        let saw = Process::new(7, "make wood", 0)
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 8.0, true));
        let mut line = JobLine::new(31, Some(10.0), vec![]);
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

        // Ten time for two wood costs 5. The board holds wood at 1. Cover is
        // 5.5, and sought adds nothing, so 10 steps toward 5.5.
        job.plan(
            &HashMap::from([(7, 10.0)]),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(7.5));
        assert!((job.lines[0].cover - 5.5).abs() < 1e-12);
    }

    /// Forage is the fastest wood process known, so it still takes the wood
    /// the market did not buy even though making costs more than the board.
    #[test]
    fn plan_sought_still_raises_the_only_line_when_buying_is_cheaper() {
        let forage = Process::new(31, "subsistence forage", 0)
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
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

        // Cover is 5.5. Sought of 100 is 50 runs. The line is at 10 and
        // steps a quarter of the way toward 55.5.
        job.plan(
            &HashMap::from([(7, 10.0)]),
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].target, Some(12.5));
    }

    /// Jewelry used 30 gold and none is still sought. The gold line steps
    /// toward those runs. A filled sale alone would step toward zero.
    #[test]
    fn plan_steps_toward_an_input_downstream_production_used() {
        let gold = Process::new(4, "make gold", 0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 3.0, true));
        let jewelry = Process::new(6, "make jewelry", 0)
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
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        // Thirty jewelry took thirty gold, ten runs of this line. No time is
        // on hand, so the surplus is zero. The line is at 4 and steps toward 10.
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
        job.plan(&HashMap::from([(2, 2.0)]), &property, &factuals, &history, 1.0, true);

        assert_eq!(job.lines[0].target, Some(5.0));
        assert!((job.lines[0].cover - 3.0).abs() < 1e-12);
    }

    #[test]
    fn plan_costs_a_decayed_input_at_full_holding() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
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

        // Time's board price is 0.1. A full holding of 1 makes the run a loss,
        // so 12.5 steps toward a cover of 0 instead of growing on the spare time.
        assert_eq!(job.lines[0].target, Some(9.375));
    }

    #[test]
    fn plan_keeps_a_line_when_the_output_is_not_worth_holding() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        let property = HashMap::from([(1, PopPRow::new(5.0))]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // Worth nothing, and it is not covering, so the 4 runs step toward one run.
        assert_eq!(job.lines[0].target, Some(3.0));
        assert!((job.plan_cost - 3.0).abs() < 1e-12);
    }

    /// No cover, and the bread is worthless. One run stays. A target of 0
    /// stays at 0.
    #[test]
    fn plan_keeps_one_run_when_an_unpaid_line_has_no_cover() {
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

        assert_eq!(job.lines[0].target, Some(1.0));

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
            vec![JobLine::new(7, Some(4.0), vec![]), JobLine::new(8, Some(1.0), vec![])],
        );
        let factuals = Factuals::new().with_process(bake).with_process(weave);
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        history.prices.insert(1, 0.0);
        history.prices.insert(4, 4.0);
        history.prices.insert(3, 0.0);
        let property = HashMap::from([(3, PopPRow::new(5.0))]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // Bread is not worth holding, so its 4 runs step toward 0 and it follows the cloth.
        assert_eq!(job.lines[0].process, 8);
        assert_eq!(job.lines[0].target, Some(1.25));
        assert_eq!(job.lines[1].process, 7);
        assert_eq!(job.lines[1].target, Some(3.0));
    }

    #[test]
    fn plan_gives_the_cover_to_the_higher_yield() {
        let slow = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
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
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        // 12 grain short, plus one unit, six per run. The fast line steps from 0.
        // The slow line has no cover and follows it.
        assert_eq!(job.lines[0].process, 1);
        assert_eq!(job.lines[0].target, Some(0.25));
        assert_eq!(job.lines[1].process, 29);
        assert_eq!(job.lines[1].target, Some(0.0));
    }

    #[test]
    fn plan_puts_a_covering_line_back_ahead_of_a_paying_line() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
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
            &HashMap::new(),
            &factuals,
            &history,
            1.0,
            false,
        );

        assert_eq!(job.lines[0].process, 29);
        assert_eq!(job.lines[1].process, 4);

        job.plan(&HashMap::new(), &HashMap::new(), &factuals, &history, 1.0, false);

        assert_eq!(job.lines[0].process, 4);
        assert_eq!(job.lines[1].process, 29);
    }

    #[test]
    fn plan_moves_a_feeder_in_front_of_its_consumer() {
        let water = Process::new(30, "subsistence water", 0)
            .with_output(ProcessOutput::new(2, 2.0, true));
        let forage = Process::new(31, "subsistence forage", 0)
            .with_output(ProcessOutput::new(7, 2.0, true));
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

        job.plan(
            &HashMap::from([(2, 2.0), (8, 1.0)]),
            &HashMap::new(),
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

        job.plan(&HashMap::from([(2, 9.0)]), &HashMap::new(), &factuals, &MarketHistory::new(), 1.0, false);

        // 9 bread short, plus one unit. The line is already at 4, so this night steps toward 10.
        assert_eq!(job.lines[0].target, Some(5.0));
    }

    #[test]
    fn no_lines_skips_plan_reserve_produce_and_buys() {
        let mut job = Job::new(4, vec![]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(5.0))]);

        job.plan(&HashMap::from([(2, 9.0)]), &HashMap::new(), &factuals, &MarketHistory::new(), 1.0, false);
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

        job.plan(&HashMap::new(), &HashMap::new(), &Factuals::new(), &MarketHistory::new(), 1.0, false);

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

    #[test]
    fn reserve_buys_the_next_run_after_decay_and_ignores_fresh() {
        let mut row = PopPRow::new(4.0);
        row.fresh = 4.0;
        let factuals = Factuals::new()
            .with_process(bake())
            .with_good(good(1, 0.5));
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
        let slow = Factuals::new()
            .with_process(bake())
            .with_good(good(1, 0.1));
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
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
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
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_good(good(1, 0.5));
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
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
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
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let factuals = Factuals::new()
            .with_process(farm)
            .with_good(good(1, 0.0));
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
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 2.0, true));
        let cabins = Process::new(8, "make cabins", 0)
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
        let factuals = Factuals::new().with_process(bake().with_effect(ProcessEffect::Culture(2.0)));
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

    #[test]
    fn produce_leaves_the_kept_pile() {
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
        assert_eq!(property[&1].quantity, 3.0);
        assert_eq!(property[&2].quantity, 2.0);

        job.settle_shopping(&property, &factuals);
        assert!((job.shopping[&1] - 4.0).abs() < 1e-9);
    }

    /// Bake claims two grain above the three kept. Brew claimed none. After
    /// bake spends its claim, brew finds no grain it may take, so nothing is
    /// brewed and the pile stays free.
    #[test]
    fn produce_spends_a_claim_from_reserved_above_the_kept_pile() {
        let brew = Process::new(8, "brew", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let factuals = Factuals::new().with_process(bake()).with_process(brew);
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(2.0), vec![]),
                JobLine::new(8, Some(3.0), vec![]),
            ],
        );
        let mut property = HashMap::from([(1, PopPRow::new(5.0))]);
        job.leave(&HashMap::from([(1, 3.0)]));

        job.reserve(&mut property, &factuals);
        assert_eq!(property[&1].reserved, 2.0);

        job.produce(&mut property, &factuals);

        assert_eq!(property[&1].quantity, 3.0);
        assert_eq!(property[&1].reserved, 0.0);
        assert_eq!(property[&2].quantity, 2.0);
        assert!(property.get(&3).is_none_or(|row| row.quantity == 0.0));
        assert!(job.claimed.get(&1).is_none_or(|left| *left == 0.0));
    }

    #[test]
    fn produce_spends_output_above_the_kept_pile() {
        let maker = Process::new(1, "make grain", 0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 2.0, true));
        let factuals = Factuals::new()
            .with_process(maker)
            .with_process(bake());
        let mut job = Job::new(
            1,
            vec![
                JobLine::new(1, Some(1.0), vec![]),
                JobLine::new(7, Some(5.0), vec![]),
            ],
        );
        let mut property = HashMap::from([(0, PopPRow::new(10.0)), (1, PopPRow::new(3.0))]);
        // The pile is larger than the stock on hand. Today's make fills it
        // first. Only the surplus is baked.
        job.leave(&HashMap::from([(1, 4.0)]));

        job.reserve(&mut property, &factuals);
        job.produce(&mut property, &factuals);

        assert_eq!(property[&1].quantity, 4.0);
        assert_eq!(property[&2].quantity, 1.0);
        assert_eq!(property[&2].produced, 1.0);
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
        assert_eq!(property[&6].reserved, 0.0);
        assert_eq!(property[&2].quantity, 1.0);
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
            .with_input(ProcessInput::new(0, 0.5, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 6.0, true));
        let bread = Process::new(7, "make bread", 0)
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
        let factuals = Factuals::new()
            .with_process(bake())
            .with_good(good(1, 0.0));
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let row = PopPRow::new(4.0).with_reserve(4.0);

        job.settle_shopping(&HashMap::from([(1, row)]), &factuals);

        assert_eq!(job.shopping[&1], 4.0);
    }

    /// The grain line runs before bread and makes 6. Bread needs 4. Nothing
    /// is bought, and a make of 2 leaves a buy of 2.
    #[test]
    fn settle_shopping_counts_what_an_earlier_line_will_make() {
        let grow = Process::new(1, "make grain", 0)
            .with_output(ProcessOutput::new(1, 6.0, true));
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

        let short = Process::new(1, "make grain", 0)
            .with_output(ProcessOutput::new(1, 2.0, true));
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
        let factuals = Factuals::new()
            .with_process(eat)
            .with_good(good(1, 0.0));
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
        let factuals = Factuals::new()
            .with_process(bake())
            .with_good(good(1, 0.5));

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
}
