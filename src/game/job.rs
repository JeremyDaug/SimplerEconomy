//! A pop's cottage work: the processes it can run and the day's plan.
//!
//! The stock is the pop's. The job does not set a price and does not sell.

use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::craft::Craft;
use crate::game::factuals::Factuals;
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
    /// Units to buy for the next run, recorded by [`Job::claim_one`].
    ///
    /// Produce does not clear it. The next [`Job::reset_day`] does.
    shopping: HashMap<usize, f64>,
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
        }
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
    /// Drops the morning's claim book and the shopping list.
    ///
    /// Line targets stay, so last night's plan is what the next reserve runs.
    /// Does not touch pop property. [`crate::game::pop::Pop::reset_day`]
    /// zeros `reserved` on the rows, which releases the claim itself.
    pub fn reset_day(&mut self) {
        self.claimed.clear();
        self.shopping.clear();
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
    /// The first line that outputs a short good takes that good. Its cover
    /// iterations are the largest `(gap + 1) / output.amount` across the
    /// short outputs it takes. The extra unit is one more of that output
    /// than the gap. A missing process is no cover. That cover is a
    /// floor under the line.
    ///
    /// Every line with a positive [`trade_score`] adds its own surplus on
    /// that floor. The extra iterations start from [`surplus_iterations`]
    /// on the inputs the cover leaves, after earlier lines have taken
    /// theirs, then [`paced_surplus`] pulls that run toward the output
    /// just made. The ideal is the cover plus that extra.
    /// [`march_target`] moves the stored target one step toward it.
    ///
    /// A line that does not pay does not step down. A cover above its
    /// current target can still raise it. When it is not covering a short
    /// good, the line moves to the back of the list and keeps the target
    /// it had. `snap_cover` is a population change or a new or removed
    /// desire: the new cover is written whole. The surplus above the old
    /// cover steps only on a line that pays.
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

        // Cover. Walk the lines in order. The first line that outputs a short
        // good takes it, at the largest (gap + 1) / output.amount. The extra
        // unit is one more of that output than the gap. A later line does
        // not take that good again. A missing process stores 0.
        let mut assigned = HashSet::new();
        let mut cover_targets = Vec::with_capacity(self.lines.len());
        for line in &self.lines {
            let Some(process) = factuals.get_process(line.process) else {
                cover_targets.push(0.0);
                continue;
            };
            let mut iterations = 0.0_f64;
            for output in &process.outputs {
                if output.amount <= 0.0 {
                    continue;
                }
                let gap = cover.get(&output.good).copied().unwrap_or(0.0);
                if gap <= 0.0 || assigned.contains(&output.good) {
                    continue;
                }
                iterations = iterations.max((gap + 1.0) / output.amount);
                assigned.insert(output.good);
            }
            cover_targets.push(iterations);
        }

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

        // Step toward the ideal. A line that pays aims at its cover plus the
        // extra. A line that does not pay does not step down. Its cover is
        // still a floor, so a short good can raise it. A snap writes the new
        // cover whole. The surplus above the old cover steps only on a line
        // that pays. On a line that does not, that surplus stays.
        for (index, line) in self.lines.iter_mut().enumerate() {
            let new_cover = cover_targets[index];
            let current = line.target.unwrap_or(0.0);
            let pays = scores[index].is_some_and(|score| score > 0.0);
            let target = if snap_cover {
                let old_surplus = (current - line.cover).max(0.0);
                if pays {
                    new_cover + march_target(old_surplus, extras[index])
                } else {
                    new_cover + old_surplus
                }
            } else if pays {
                march_target(current, new_cover + extras[index])
            } else if new_cover > current {
                march_target(current, new_cover)
            } else {
                current
            };
            if snap_cover {
                line.cover = new_cover;
            }
            line.target = Some(target);
        }

        // Priority. A line that does not pay and is not covering this night
        // moves to the back. Paying lines and feeding lines keep their order.
        let lines = std::mem::take(&mut self.lines);
        let mut next = Vec::with_capacity(lines.len());
        let mut back = Vec::new();
        for (index, line) in lines.into_iter().enumerate() {
            let pays = scores[index].is_some_and(|score| score > 0.0);
            if pays || cover_targets[index] > 0.0 {
                next.push(line);
            } else {
                back.push(line);
            }
        }
        next.extend(back);
        self.lines = next;
        // Complexity cost of the targets just written.
        self.plan_cost = self.plan_complexity_cost(factuals, modifier);
    }

    /// # Reserve
    ///
    /// Claims inputs the pop already holds and records a buy for the next run.
    ///
    /// `property` is the pop's rows. Desires have already reserved, so this
    /// takes only [`PopPRow::available`]. `factuals` supplies processes.
    ///
    /// A required factor that is missing shops one unit, and that line's
    /// other inputs stay free. An optional factor is claimed when held and
    /// is not shopped. Other optional inputs are claimed and shopped only
    /// when the line lists them. `None` claims what is free and does not
    /// shop an open-ended amount. `Some(0.0)` skips the line.
    pub fn reserve(&mut self, property: &mut HashMap<usize, PopPRow>, factuals: &Factuals) {
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
    /// also claimed. A planned line stops at its own quota.
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
        for line in &lines {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            let target = match line.target {
                Some(n) if n > 0.0 => Some(n),
                None => None,
                Some(_) => continue,
            };
            // Pass this line's inputs only, capped at its quota when it has one.
            let inputs = self.inputs_for(process, line, property);
            if target.is_none() && !inputs_bound(process, &inputs) {
                continue;
            }
            let result = process.do_process(&inputs, target, factuals);
            self.apply_result(property, &result);
            effects.extend(result.effects);
        }
        // Factors stay reserved through the market. Everything else can be sold.
        let factors = self.factor_goods(factuals);
        self.release_claims(property, &factors);
        effects
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
                let slot = self.shopping.entry(factor.good).or_insert(0.0);
                *slot = (*slot).max(1.0);
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
    /// `Some(n)` needs `input.amount * n` today. The buy is that same need
    /// divided by durability (`1 - decay`), minus stock that will still be
    /// on hand: leftover free stock for a destroyed or consumed input, or
    /// the whole free pile for capital, which comes back at decay. Fresh
    /// units sit in that pile and are not spared. A good missing from
    /// `factuals` does not decay. A good that decays completely shops only
    /// today's gap, because nothing bought today is left tomorrow.
    fn claim_one(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        input: &ProcessInput,
        iterations: Option<f64>,
        factuals: &Factuals,
    ) {
        let good = input.good;
        let free = free_of(property, good);
        match iterations {
            None => claim(property, &mut self.claimed, good, free),
            Some(n) => {
                let need = input.amount * n;
                let take = free.min(need);
                claim(property, &mut self.claimed, good, take);
                let decay = factuals
                    .get_good(good)
                    .map(|row| row.decay_rate)
                    .unwrap_or(0.0);
                let durability = (1.0 - decay).clamp(0.0, 1.0);
                // Capital comes back at decay. Spent destroyed and consumed stock does not.
                let kept = if matches!(input.input_type, InputType::Capital) {
                    free
                } else {
                    free - take
                };
                // A total loss cannot be stocked overnight.
                let raw = if durability > 0.0 {
                    need / durability - kept
                } else {
                    need - take
                };
                if raw > 0.0 {
                    *self.shopping.entry(good).or_insert(0.0) += raw;
                }
            }
        }
    }

    /// # Inputs For
    ///
    /// The input map for one run of this line.
    ///
    /// Required inputs and listed optionals contribute the claimed amount,
    /// capped at this line's quota when it has one. A held factor is entered
    /// so the process sees it; the amount is not spent.
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
            let claimed_amount = self.claimed.get(&input.good).copied().unwrap_or(0.0);
            let amount = match line.target {
                Some(n) if n > 0.0 => claimed_amount.min(input.amount * n),
                _ => claimed_amount,
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
    /// Removes `amount` of `good` from quantity, reserved, and the claim.
    ///
    /// When `capital` is set, those units are added to `used` so decay can
    /// return them. The take is capped at the quantity on hand.
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
        let take = if let Some(row) = property.get_mut(&good) {
            let take = amount.min(row.quantity.max(0.0));
            row.quantity -= take;
            row.reserved = (row.reserved - take).max(0.0);
            if capital {
                row.used += take;
            }
            take
        } else {
            0.0
        };
        if let Some(left) = self.claimed.get_mut(&good) {
            *left = (*left - take).max(0.0);
        }
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
/// Factors, optional inputs, and capital stay out. Returns the net.
fn trade_score(process: &Process, history: &MarketHistory) -> f64 {
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
        score -= input.amount * history.holding_per_unit(input.good);
    }
    score
}

/// # Paced Surplus
///
/// Extra runs left after rot pulls `raw` back toward the output just made.
///
/// `process` supplies the outputs. `property` is the pop's rows. `raw` is
/// the input-limited surplus. An output with `amount <= 0`, or a missing
/// row, does not change `raw`.
///
/// `produced > 0` sets the room from `lost / produced`. At
/// [`SURPLUS_ROT_SHARE`] the room is one, so the extra stays at that
/// output's runs. Less rot raises the room, up to twice the runs when
/// nothing rotted. More rot lowers it by `SURPLUS_ROT_SHARE / share`.
/// `produced == 0` and `lost > 0` leaves one run. The tightest output
/// wins. Returns that many runs, never negative.
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
            let share = row.lost / row.produced;
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
        let mut line = JobLine::new(7, Some(12.0), vec![]);
        line.cover = 0.0;
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
        // The old surplus of 12 steps to 9, and 3 + 9 stays at 12.
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
        let mut grain = PopPRow::new(6.0);
        grain.produced = 6.0;
        let property = HashMap::from([(0, PopPRow::new(512.0)), (1, grain)]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // 512 time would fund 1024 runs. Nothing rotted, so the ideal doubles the one run, and the line steps toward 2.
        assert_eq!(job.lines[0].target, Some(1.25));
    }

    #[test]
    fn plan_keeps_a_line_when_the_output_is_not_worth_holding() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        let property = HashMap::from([(1, PopPRow::new(5.0))]);

        job.plan(&HashMap::new(), &property, &factuals, &history, 1.0, false);

        // Worth nothing, and it is not covering, so the 4 runs stay.
        assert_eq!(job.lines[0].target, Some(4.0));
        assert!((job.plan_cost - 4.0).abs() < 1e-12);
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

        // Bread is not worth holding, so its 4 runs stay and it follows the cloth.
        assert_eq!(job.lines[0].process, 8);
        assert_eq!(job.lines[0].target, Some(1.25));
        assert_eq!(job.lines[1].process, 7);
        assert_eq!(job.lines[1].target, Some(4.0));
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

        // No process, so the line does not pay. The 4 runs stay.
        assert_eq!(job.lines[0].target, Some(4.0));
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
}
