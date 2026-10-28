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
use crate::game::process::{
    InputEffect, InputType, Process, ProcessEffect, ProcessInput, ProcessResult,
};
use crate::game::util::whole_units_up;

/// Share of this pop's own output where surplus holds.
/// Under it, surplus may grow. Over it, surplus pulls back.
const SURPLUS_ROT_SHARE: f64 = 0.1;

/// Fraction of a line's current target it may move in one night.
///
/// A target below 1 uses 1 as the base, so a resting line can leave 0.
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
    /// Optional inputs this line draws. Empty draws none.
    ///
    /// [`Job::plan`] rewrites it from [`chosen_inputs`].
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
            skills: Vec::new(),
            plan_cost: 0.0,
            claimed: HashMap::new(),
            shopping: HashMap::new(),
            input_feeds: Vec::new(),
            kept: HashMap::new(),
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
    /// [`release_stocked_buys`] takes a bought input back off once those
    /// units are in `quantity`. The meal gap stays until then.
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
    /// being planned at nothing.
    /// [`trim_morning_extra`] cuts the extra back when the total would
    /// pass what those lines still finish. The ideal is the cover plus
    /// the surplus and those runs. A line
    /// that does not pay aims at the cover and those runs. A snap still aims there: the new cover is written in whole when
    /// the step has not reached it, and the old target is not added on top.
    /// Once the cover and those runs are gone the line stands down, including
    /// one that is already running. A line at 0 stays at 0.
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
        if self.lines.is_empty() {
            self.plan_cost = 0.0;
            return;
        }

        // Cover. The shortfall stays, including a meal that is cheaper to buy.
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
        cap_to_morning(
            &self.lines,
            &mut cover_targets,
            &stock,
            factuals,
            history,
        );

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

        // Step toward the ideal. A paying line aims at cover plus extra.
        // An unpaid line aims at the cover and the sought runs. Once both are gone it stands down.
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
        self.attach_skills(factuals);
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
            self.apply_result(property, &result, factuals);
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
    /// `used_inputs` is capital set aside for the day. When a fed good's
    /// net did not fall, the claim for [`ProcessResult::spent`] is released
    /// without taking that quantity again. `factuals` distinguishes a skill.
    /// Aged skill that was fed comes back fresh with the output, so a pile
    /// that was fully used has nothing left to decay that night.
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
    }

    /// # Spend
    ///
    /// Removes `amount` of `good` from quantity.
    ///
    /// The take stops where [`Self::leave`] is holding a pile back, so
    /// quantity does not fall below that pile. Free units above it go first.
    /// Reserved units and the claim are reduced only for what that free stock
    /// did not cover. When `capital` is set, the units taken are added to
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
        let free = (row.quantity - row.reserved).max(0.0);
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
/// the good. A worse line that is bought instead keeps its meal gap.
/// Stock offsets only the inputs added on top of that gap, and only once
/// the gap is gone. A good that does not survive the night stays.
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
        let base = meal.get(&good).copied().unwrap_or(0.0);
        let total = cover.get(&good).copied().unwrap_or(0.0);
        let added = (total - base).max(0.0);
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
        cap = cap.max(0.0);
        finished[index] = cap;
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
        } else if trade_score(process, history) <= 0.0
            || is_cheaper_to_buy(process, good, history)
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
    let mut held = 0.0;
    for later in lines.iter().skip(index + 1) {
        if !holds_time_for(&lines[index], later, factuals) {
            continue;
        }
        let Some(process) = factuals.get_process(later.process) else {
            continue;
        };
        let Some(n) = later.target else {
            continue;
        };
        if n > 0.0 {
            held += time_spent(process) * n;
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
        let on = run_worth(
            process,
            Some(input.good),
            lines,
            index,
            factuals,
            history,
        );
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
fn run_mods(
    process: &Process,
    enabled: Option<usize>,
) -> (f64, f64, f64, Vec<(usize, f64)>) {
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
fn add_decay_yield(
    factuals: &Factuals,
    good: usize,
    take: f64,
    made: &mut HashMap<usize, f64>,
) {
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
/// Output holding is added. Destroyed and consumed inputs are subtracted
/// at [`input_holding`]. Factors, optional inputs, and capital stay out.
/// Returns the net.
pub fn trade_score(process: &Process, history: &MarketHistory) -> f64 {
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
        score -= input.amount * input_holding(history, input.good);
    }
    score
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
fn ranked_pay(
    lines: &[JobLine],
    index: usize,
    scores: &[Option<f64>],
    factuals: &Factuals,
) -> f64 {
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
                && line_rate(lines, index, scores, factuals)
                    .is_some_and(|rate| rate < best_rate)
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
fn profit_floor(
    process: &Process,
    property: &HashMap<usize, PopPRow>,
    factuals: &Factuals,
) -> f64 {
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
            && !factuals.get_good(input.good).is_some_and(|good| good.is_skill())
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
        let runs = cover.get(index).copied().unwrap_or(0.0)
            + extras.get(index).copied().unwrap_or(0.0);
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

    /// The mill is listed first and takes the spare time. Cloth pays more.
    /// Its pace can absorb 0.4 of the mill's offer. It is still under
    /// yesterday's output, so the floor takes another quarter of the mill.
    #[test]
    fn plan_nudges_surplus_onto_the_stronger_line() {
        let mill = Process::new(7, "mill", 0)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let weave = Process::new(8, "weave", 0)
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
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let mill = Process::new(7, "mill", 0)
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
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(4, 1.0, true));
        let forage = Process::new(31, "subsistence forage", 0)
            .with_input(ProcessInput::new(0, 50.0, true, InputType::Destroyed, false))
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
            .with_input(ProcessInput::new(0, 50.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 4.0, true));
        let gold = Process::new(4, "make gold", 0)
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
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
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let farm = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(0, 50.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 4.0, true));
        let forage = Process::new(31, "subsistence forage", 0)
            .with_input(ProcessInput::new(0, 50.0, true, InputType::Destroyed, false))
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
        job.plan(&HashMap::from([(2, 13.0)]), &property, &factuals, &history, 1.0, true);

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
        job.plan(&HashMap::from([(2, 10.0)]), &property, &factuals, &history, 1.0, true);

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
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
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
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 1.0, true));
        let tools = Process::new(9, "make tools", 0)
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
            .with_input(ProcessInput::new(0, 50.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 4.0, true));
        let gold = Process::new(4, "make gold", 0)
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
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
            .with_input(ProcessInput::new(0, 50.0, true, InputType::Destroyed, false))
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

        // Nineteen jewelry plus one unit is 20 runs. Four gold finishes four.
        // The line does not pay, so 40 steps toward 4.
        job.plan(&HashMap::from([(6, 19.0)]), &property, &factuals, &history, 1.0, false);

        assert_eq!(job.lines[0].target, Some(30.0));
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

    /// The bread line is listed first and no grain is on hand. The farm
    /// still runs first and makes that grain, so the bread cover is taken.
    #[test]
    fn plan_bakes_from_grain_the_farm_makes_this_morning() {
        let farm = Process::new(29, "subsistence farm", 0)
            .with_input(ProcessInput::new(0, 10.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 4.0, true));
        let bread = Process::new(7, "bake", 0)
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
            .with_input(ProcessInput::new(0, 50.0, true, InputType::Destroyed, false))
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
        job.plan(&HashMap::from([(2, 2.0)]), &property, &factuals, &history, 1.0, true);

        assert_eq!(job.lines[0].target, Some(5.0));
        assert!((job.lines[0].cover - 3.0).abs() < 1e-12);
    }

    #[test]
    fn plan_prices_time_at_its_holding() {
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

    /// No cover, and the bread is worthless. The running line stands down.
    /// A target of 0 stays at 0.
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

        assert_eq!(job.lines[0].target, Some(0.75));

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
            &HashMap::from([(0, PopPRow::new(100.0))]),
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

    /// The line does not list the tool, so the tool is neither shopped nor
    /// fed. Listing it shops one when the rack is empty, and one run with
    /// the tool turns two product into three.
    #[test]
    fn listed_capital_is_fed_and_an_unlisted_one_is_not() {
        let process = Process::new(11, "with a tool", 0)
            .with_input(ProcessInput::new(1, 1.0, false, InputType::Destroyed, false))
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
            .with_input(ProcessInput::new(0, 0.5, false, InputType::Destroyed, false))
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
            .with_input(ProcessInput::new(0, 5.0, false, InputType::Destroyed, false))
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
            .with_input(ProcessInput::new(0, 5.0, false, InputType::Destroyed, false))
            .with_input(
                ProcessInput::new(4, 3.0, false, InputType::Destroyed, true)
                    .with_optional(InputEffect::Output(0.2)),
            )
            .with_output(ProcessOutput::new(2, 20.0, false));
        let thin = Process::new(1, "thin", 0)
            .with_input(ProcessInput::new(0, 5.0, false, InputType::Destroyed, false))
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
            .with_input(ProcessInput::new(0, 50.0, false, InputType::Destroyed, false))
            .with_input(
                ProcessInput::new(4, 80.0, false, InputType::Destroyed, true)
                    .with_optional(InputEffect::Output(0.5)),
            )
            .with_output(ProcessOutput::new(1, 10.0, false));
        let later = Process::new(2, "later", 0)
            .with_input(ProcessInput::new(1, 1.0, false, InputType::Destroyed, false))
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

    /// Only optional inputs tagged skill are attached. A required skill and a
    /// plain optional stay off the list. Repeats collapse, and the ids sort.
    #[test]
    fn attach_skills_keeps_optional_skill_inputs() {
        let practice = Process::new(7, "bake", 0)
            .with_input(ProcessInput::new(41, 1.0, true, InputType::Destroyed, false))
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
    /// once: 1.1 comes back, and each run adds 0.01. The grain is spent. The
    /// bread is made. The pile is all fresh, so the night takes none of it.
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
}
