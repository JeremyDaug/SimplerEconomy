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
use crate::game::process::{InputType, Process, ProcessEffect, ProcessResult};

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
/// Firms hav their own, altered, complexity costs to their production 
/// lines. Job complexity cost grows much faster with the complexity of 
/// the job, and gains reduced integration benefits. Complexity cost looks
/// something like:
/// 
/// sum(craft_modifier * (process.complexity^2 - (k * process_overlap_percent)) * j * iterations of line) 
/// 
/// The general idea being that 'subsistence' or simple processes are 
/// lighter while more complex processes are harder. It's best to think
/// of the entire pop trying to do an apropriate share of each process
/// as though they were each independent of each other.
/// 
/// Additionally, the craft can give a complexity cost modifier,
/// and if the job diverges, it pays a penalty on.
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
    /// Units this job added to `PopPRow.reserved` this morning.
    ///
    /// Produce spends this claim and leaves desire reserves alone.
    claimed: HashMap<usize, f64>,
    /// Input shortfall recorded at reserve and bought at the market.
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
    /// [`Job::plan`] writes `Some`, including `Some(0.0)` when the line should rest.
    pub target: Option<f64>,
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
            claimed: HashMap::new(),
            shopping: HashMap::new(),
        }
    }

    /// # Complexity Cost
    ///
    /// The cost of this job's lines against `baseline`.
    ///
    /// `weight` is the full distance of one extra process. Collects the
    /// process id of each line and returns [`Craft::complexity_cost`].
    pub fn complexity_cost(&self, baseline: &Craft, weight: f64) -> f64 {
        let processes: Vec<usize> = self.lines.iter().map(|line| line.process).collect();
        baseline.complexity_cost(&processes, weight)
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
    /// Plan reworks the current job lines in a way that is more profitable
    /// to the pop. That means maximizing AMV produced by the work, ensuring the 
    /// pop is satisfied, and feeding it's own input needs (roughly in this order).
    /// 
    /// It also means avoiding unprotifable lines, reducing volatility, reducing
    /// decay, and avoiding pop starvation.
    /// 
    /// TODO: this function will need to be reworked to match the comments above. 
    /// More details on this are to be worked out.
    pub fn plan(
        &mut self,
        wanted: &HashMap<usize, f64>,
        on_hand: &HashMap<usize, f64>,
        factuals: &Factuals,
        history: &MarketHistory,
    ) {
        if self.lines.is_empty() {
            return;
        }
        for line in &mut self.lines {
            let Some(process) = factuals.get_process(line.process) else {
                // A line whose process is not in the world does not run.
                line.target = Some(0.0);
                continue;
            };
            line.target = Some(target_iterations(process, wanted, on_hand, history));
        }
    }

    /// # Reserve
    ///
    /// Claims inputs the pop already holds and records the rest to buy.
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
            self.claim_inputs(property, process, line, iterations);
        }
    }

    /// # Produce
    ///
    /// Runs each line up to its target and writes the result onto `property`.
    ///
    /// Returns the process effects. Negative changes leave `quantity` and
    /// the claim. Positive changes are new stock and `process_output`, so
    /// decay spares them today. Capital moves from `quantity` into `used`.
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
    /// The amount is the shortfall floored to a whole unit. A shortfall
    /// under one unit is skipped. These are buys: the job does not offer
    /// stock for sale.
    pub fn buy_orders(&self, origin: Actor) -> Vec<MarketOrder> {
        if self.lines.is_empty() {
            return Vec::new();
        }
        let mut goods: Vec<usize> = self.shopping.keys().copied().collect();
        goods.sort_unstable();
        let mut orders = Vec::new();
        for good in goods {
            let amount = self.shopping[&good].floor();
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
    /// and shops the shortfall.
    fn claim_inputs(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        process: &Process,
        line: &JobLine,
        iterations: Option<f64>,
    ) {
        for input in process.requirements() {
            self.claim_one(property, input.good, input.amount, iterations);
        }
        for input in process.optional_inputs() {
            if line.inputs.contains(&input.good) {
                self.claim_one(property, input.good, input.amount, iterations);
            }
        }
    }

    /// # Claim One
    ///
    /// Claims `good` for one input and records any shortfall.
    ///
    /// `per_run` is the amount for one iteration. `iterations` `None` takes
    /// the free stock only. `Some(n)` needs `per_run * n`.
    fn claim_one(
        &mut self,
        property: &mut HashMap<usize, PopPRow>,
        good: usize,
        per_run: f64,
        iterations: Option<f64>,
    ) {
        let free = free_of(property, good);
        match iterations {
            None => claim(property, &mut self.claimed, good, free),
            Some(n) => {
                let need = per_run * n;
                let take = free.min(need);
                claim(property, &mut self.claimed, good, take);
                let short = need - take;
                if short > 0.0 {
                    *self.shopping.entry(good).or_insert(0.0) += short;
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
    /// A negative change is input spent. A positive change is new output.
    /// `used_inputs` is capital set aside for the day.
    fn apply_result(&mut self, property: &mut HashMap<usize, PopPRow>, result: &ProcessResult) {
        for (&good, &change) in &result.changes {
            if change < 0.0 {
                self.spend(property, good, -change, false);
            } else if change > 0.0 {
                // New today. decay_goods reads process_output and skips this portion.
                let row = property.entry(good).or_insert_with(|| PopPRow::new(0.0));
                row.quantity += change;
                row.process_output += change;
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

/// # Target Iterations
///
/// How many times `process` should run for this want and this board.
///
/// The gap for an output is wanted units minus units on hand, floored at
/// zero. Iterations are the largest `gap / output.amount` across outputs.
/// With no gap, the result is `1` when any output's holding value is
/// positive, and `0` when every output is worth nothing to hold.
fn target_iterations(
    process: &Process,
    wanted: &HashMap<usize, f64>,
    on_hand: &HashMap<usize, f64>,
    history: &MarketHistory,
) -> f64 {
    let mut iterations = 0.0_f64;
    let mut worth_holding = false;
    for output in &process.outputs {
        if output.amount <= 0.0 {
            continue;
        }
        let gap = (wanted.get(&output.good).copied().unwrap_or(0.0)
            - on_hand.get(&output.good).copied().unwrap_or(0.0))
        .max(0.0);
        if gap > 0.0 {
            iterations = iterations.max(gap / output.amount);
        }
        if history.holding_per_unit(output.good) > 0.0 {
            worth_holding = true;
        }
    }
    if iterations > 0.0 {
        iterations
    } else if worth_holding {
        1.0
    } else {
        0.0
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
    use std::collections::HashMap;

    use crate::game::actor::Actor;
    use crate::game::craft::Craft;
    use crate::game::factuals::Factuals;
    use crate::game::job::{Job, JobLine};
    use crate::game::market::MarketHistory;
    use crate::game::pop_property::PopPRow;
    use crate::game::process::{InputType, Process, ProcessEffect, ProcessInput, ProcessOutput};

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
    fn plan_sets_iterations_from_the_desire_gap() {
        let mut job = Job::new(1, vec![JobLine::new(7, None, vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let wanted = HashMap::from([(2, 4.0)]);
        let on_hand = HashMap::from([(2, 1.0)]);

        job.plan(&wanted, &on_hand, &factuals, &MarketHistory::new());

        assert_eq!(job.lines[0].target, Some(3.0));
    }

    #[test]
    fn plan_makes_one_batch_when_the_output_is_worth_holding() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(0.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        // No gap. A default board prices bread at 1, so one batch is worth making.
        job.plan(&HashMap::new(), &HashMap::from([(2, 5.0)]), &factuals, &MarketHistory::new());

        assert_eq!(job.lines[0].target, Some(1.0));
    }

    #[test]
    fn plan_idles_when_the_output_is_not_worth_holding() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);

        job.plan(&HashMap::new(), &HashMap::from([(2, 5.0)]), &factuals, &history);

        assert_eq!(job.lines[0].target, Some(0.0));
    }

    #[test]
    fn plan_runs_a_line_when_the_craft_is_none() {
        let mut job = Job::new(0, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());

        job.plan(&HashMap::from([(2, 9.0)]), &HashMap::new(), &factuals, &MarketHistory::new());

        // 9 bread wanted, 1 per batch, nothing on hand.
        assert_eq!(job.lines[0].target, Some(9.0));
    }

    #[test]
    fn no_lines_skips_plan_reserve_produce_and_buys() {
        let mut job = Job::new(4, vec![]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(5.0))]);

        job.plan(&HashMap::from([(2, 9.0)]), &HashMap::new(), &factuals, &MarketHistory::new());
        job.reserve(&mut property, &factuals);
        let effects = job.produce(&mut property, &factuals);

        assert!(job.lines.is_empty());
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
        assert_eq!(orders[0].target_amount, 3.0);

        let effects = job.produce(&mut property, &factuals);
        assert!(effects.is_empty());
        assert_eq!(property[&1].quantity, 0.0);
        assert_eq!(property[&2].quantity, 1.0);
        assert_eq!(property[&2].process_output, 1.0);
    }

    #[test]
    fn plan_stops_a_line_whose_process_is_missing() {
        let mut job = Job::new(1, vec![JobLine::new(99, Some(4.0), vec![])]);

        job.plan(&HashMap::new(), &HashMap::new(), &Factuals::new(), &MarketHistory::new());

        assert_eq!(job.lines[0].target, Some(0.0));
    }

    #[test]
    fn reserve_claims_free_stock_and_records_the_shortfall() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(1.0))]);

        job.reserve(&mut property, &factuals);

        assert_eq!(property[&1].quantity, 1.0);
        assert_eq!(property[&1].reserved, 1.0);
        assert_eq!(job.shopping[&1], 3.0);
    }

    #[test]
    fn buy_orders_are_the_floored_shortfall() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::from([(1, PopPRow::new(1.0))]);
        job.reserve(&mut property, &factuals);

        let orders = job.buy_orders(Actor::Pop(1));

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 1);
        assert_eq!(orders[0].target_amount, 3.0);
        job.note_purchase(1, 1.0);
        assert_eq!(job.shopping[&1], 2.0);
        assert_eq!(job.buy_orders(Actor::Pop(1))[0].target_amount, 2.0);
        job.note_purchase(1, 10.0);
        assert!(job.shopping.get(&1).is_none());
        assert!(job.buy_orders(Actor::Pop(1)).is_empty());
    }

    #[test]
    fn buy_orders_skip_a_shortfall_under_one_unit() {
        let mut job = Job::new(1, vec![JobLine::new(7, Some(0.4), vec![])]);
        let factuals = Factuals::new().with_process(bake());
        let mut property = HashMap::new();
        job.reserve(&mut property, &factuals);

        assert_eq!(job.shopping[&1], 0.4);
        assert!(job.buy_orders(Actor::Pop(1)).is_empty());
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
        assert_eq!(property[&2].process_output, 1.0);
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
