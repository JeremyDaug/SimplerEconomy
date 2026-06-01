
use std::{collections::{HashMap, HashSet}, io::ErrorKind::Other};

use crate::game::factuals::Factuals;

/// # Process
/// 
/// Proccesses are how one set of goods is transformed into another set of goods.
/// 
/// It has a list of inputs and separate list of outputs to keep things simple.
/// 
/// ## Current Logical Restrictions
/// 
/// 1. No process can have more than one of the same good as an input, this keeps 
/// things simple, allowing us to skip dealing with balancing inputs.
#[derive(Debug, Clone)]
pub struct Process {
    /// The Unique Id of the Process.
    pub id: usize,
    /// Name of the process, should be unique.
    pub name: String,
    /// The Inputs of the process.
    pub inputs: Vec<ProcessInput>,
    /// The outputs of the process.
    pub outputs: Vec<ProcessOutput>,
    /// Effects created by this process on top of good outputs.
    pub effects: Vec<ProcessEffect>,
    /// The technology that unlockes the process.
    pub tech_source: usize,
}

impl Process {
    /// # New 
    /// 
    /// Create a new process with the given id, name, and technology source.
    /// Inputs, outputs, and effects start empty.
    pub fn new(id: usize, name: impl Into<String>, tech_source: usize) -> Self {
        Process {
            id,
            name: name.into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            effects: Vec::new(),
            tech_source,
        }
    }

    /// # With Input
    /// 
    /// Add an input good to the process definition.
    /// 
    /// ## Asserts
    /// 
    /// Does not allow for repeated goods as inputs.
    pub fn with_input(mut self, input: ProcessInput) -> Self {
        assert!(!self.inputs.iter().any(|i| i.good == input.good), 
            "Process cannot have more than one of the same good as an input.");
        self.inputs.push(input);
        self
    }

    /// # With Output
    /// 
    /// Add an output good to the process definition.
    pub fn with_output(mut self, output: ProcessOutput) -> Self {
        self.outputs.push(output);
        self
    }
    
    /// # With Effect
    /// 
    /// Add an extra effect this process produces when executed.
    pub fn with_effect(mut self, effect: ProcessEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// # Factors
    /// 
    /// Gets the factor inputs of the process.
    pub fn factors(&self) -> Vec<ProcessInput> {
        self.inputs.iter()
            .filter(|input| matches!(input.input_type, InputType::Factor))
            .cloned()
            .collect()
    }

    /// # Requirements
    /// 
    /// Gets the required inputs of the process. 
    /// Excludes Factors.
    pub fn requirements(&self) -> Vec<ProcessInput> {
        self.inputs.iter()
            .filter(|input| !matches!(input.input_type, InputType::Factor) && !input.is_optional())
            .cloned()
            .collect()
    }

    /// # Optional Inputs
    /// 
    /// Gets the optional inputs of the process, excluding factors.
    pub fn optional_inputs(&self) -> Vec<ProcessInput> {
        self.inputs.iter()
            .filter(|input| input.is_optional() && !matches!(input.input_type, InputType::Factor))
            .cloned()
            .collect()
    }

    /// # Has Requirements
    /// 
    /// Gets the required input goods of the process for quick validity checking.
    pub fn has_requirements(&self) -> HashMap<usize, f64> {
        self.requirements().iter().map(|input| (input.good, input.amount)).collect()
    }

    /// # Has Factors
    /// 
    /// Gets a summary of factors needed for the process, a bool attached to them to define them as
    /// required or optional. (true is optional)
    pub fn has_factors(&self) -> HashMap<usize, bool> {
        self.factors().iter().map(|input| (input.good, input.is_optional())).collect()
    }

    /// # Get Input
    /// 
    /// Helper which gets an input of a specific good. Returns None if no input
    /// uses that good.
    pub fn get_input(&self, good: usize) -> Option<&ProcessInput> {
        self.inputs.iter().find(|x| x.good == good)
    }

    /// # Do Process
    /// 
    /// Given Inputs, an optional target, and the factuals of the world, attempt to do 
    /// as many iterations as possible, up to the given target.
    /// 
    /// Target is always scaled with fixed inputs, not variable inputs, so throughput
    /// bonuses do allow for overshooting the target.
    /// 
    /// ## Additional Notes and rules
    /// 
    /// Fixed inputs and optional inputs never gain bonuses with throughput or input 
    /// bonuses to keep wierd scaling interactions from occurring.
    /// 
    /// Factors and capital are never consumed or destroyed, just used and recorded in 
    /// the output.
    /// 
    /// ## Functional Logic
    /// 
    /// 1. Check and record Factors, as they don't scale or get consumed anyway and a 
    /// missing required factor stops the whole process.
    /// 2. Work on optional inputs next, getting any bonuses and effects they have.
    /// 3. With all bonuses calculated, check how many iterations can be done with 
    /// required inputs. Shifting goods from optional inputs to required as needed.
    pub fn do_process(
        &self,
        inputs: &HashMap<usize, f64>,
        target: Option<f64>,
        factuals: &Factuals,
    ) -> ProcessResult {
        let mut factor_input_mult = 0.0;
        let mut factor_throughput_mult;
        let mut factor_output_mult;
        if let Some((input, throughput, output, _other)) = self.check_factors(inputs) {
            factor_input_mult = input;
            factor_throughput_mult = throughput;
            factor_output_mult = output;
        } else {
            return ProcessResult::empty();
        }

        // --- 2. Base max iters using ONLY factors (no optional bonuses) ---
        let mut base_max_iters = target.unwrap_or(f64::INFINITY);
        for req in self.requirements() {  // non-optional inputs only
            let base = req.amount;
            let effective = if req.fixed {
                base
            } else {
                base * factor_input_mult * factor_throughput_mult
            };
            if effective > 0.0 {
                let avail = inputs.get(&req.good).copied().unwrap_or(0.0);
                base_max_iters = base_max_iters.min(avail / effective);
            }
        }
        if base_max_iters <= 0.0 {
            return ProcessResult::empty();
        }

        // --- 3. Optional support + proportional bonuses ---
        let mut opt_input_bonus: f64 = 0.0;
        let mut opt_output_bonus: f64 = 0.0;
        let mut opt_throughput_bonus: f64 = 0.0;
        let mut bonus_extra_outputs: HashMap<usize, f64> = HashMap::new();
        let mut bonus_effects: Vec<ProcessEffect> = Vec::new();

        let mut optional_support: f64 = f64::INFINITY;
        for opt in self.optional_inputs() {
            let avail = inputs.get(&opt.good).copied().unwrap_or(0.0);
            if avail > 0.0 && opt.amount > 0.0 {
                let support = avail / opt.amount;
                optional_support = optional_support.min(support);

                // NO COVERAGE MULTIPLIER for multipliers anymore!
                // Full bonus strength applies only to the boosted_iters slice
                if let Some(effects) = opt.optional_effects() {
                    for effect in effects {
                        match effect {
                            InputEffect::Throughput(v) => opt_throughput_bonus += v,
                            InputEffect::Input(v) => opt_input_bonus += v,
                            InputEffect::Output(v) => opt_output_bonus += v,
                            InputEffect::ExtraOutput(good_id, amt) => {
                                // ExtraOutput / Growth still get scaled by how many boosted iters they actually support
                                *bonus_extra_outputs.entry(*good_id).or_insert(0.0) += amt * support.min(base_max_iters);
                            }
                            InputEffect::Growth(v) => {
                                bonus_effects.push(ProcessEffect::Growth(v * support.min(base_max_iters)));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        let final_input_mult = (factor_input_mult - opt_input_bonus).max(0.0).min(1.0);
        let final_output_mult = factor_output_mult + opt_output_bonus;
        let final_throughput_mult = factor_throughput_mult + opt_throughput_bonus;

        // --- 4. Boosted iters (limited by optional support) ---
        let boosted_iters = base_max_iters.min(optional_support).min(target.unwrap_or(f64::INFINITY));
        let boosted_iters = boosted_iters.max(0.0);

        // --- 5. Normal iters from any leftover required goods ---
        let mut normal_iters = 0.0;
        if boosted_iters < base_max_iters {
            let mut remaining_max = target.unwrap_or(f64::INFINITY) - boosted_iters;
            if remaining_max > 0.0 {
                for req in self.requirements() {
                    let base = req.amount;
                    let effective_base = if req.fixed {
                        base
                    } else {
                        base * factor_input_mult * factor_throughput_mult
                    };
                    if effective_base > 0.0 {
                        let consumed_boosted = if req.fixed {
                            base * boosted_iters
                        } else {
                            base * final_input_mult * final_throughput_mult * boosted_iters
                        };
                        let avail = inputs.get(&req.good).copied().unwrap_or(0.0);
                        let remaining = (avail - consumed_boosted).max(0.0);
                        let this_normal = remaining / effective_base;
                        remaining_max = remaining_max.min(this_normal);
                    }
                }
                normal_iters = remaining_max.max(0.0);
            }
        }

        let completed = boosted_iters + normal_iters;
        if completed <= 0.0 {
            return ProcessResult::empty();
        }

        // --- 6. Build result ---
        let mut changes: HashMap<usize, f64> = HashMap::new();
        let mut used_inputs: HashMap<usize, f64> = HashMap::new();
        let mut effects = bonus_effects;

        // process-level effects (scaled by total completed)
        for eff in &self.effects {
            let scaled = match eff {
                ProcessEffect::Research(v) => ProcessEffect::Research(v * completed),
                ProcessEffect::Culture(v) => ProcessEffect::Culture(v * completed),
                ProcessEffect::Faith(v) => ProcessEffect::Faith(v * completed),
                ProcessEffect::Authority(v) => ProcessEffect::Authority(v * completed),
                ProcessEffect::Legitimacy(v) => ProcessEffect::Legitimacy(v * completed),
                ProcessEffect::Growth(v) => ProcessEffect::Growth(v * completed),
            };
            effects.push(scaled);
        }

        // Required inputs
        for inp in &self.inputs {
            if inp.is_optional() { continue; }
            let gid = inp.good;
            let base = inp.amount;
            let is_fixed = inp.fixed;
            let itype = &inp.input_type;

            let eff_boosted = if is_fixed { base } else { base * final_input_mult * final_throughput_mult };
            let amt_boosted = eff_boosted * boosted_iters;

            let eff_normal = if is_fixed { base } else { base * factor_input_mult * factor_throughput_mult };
            let amt_normal = eff_normal * normal_iters;

            let amount_this_run = amt_boosted + amt_normal;

            match itype {
                InputType::Factor => {}
                InputType::Capital => { *used_inputs.entry(gid).or_insert(0.0) += amount_this_run; }
                InputType::Destroyed => { *changes.entry(gid).or_insert(0.0) -= amount_this_run; }
                InputType::Consumed => {
                    *changes.entry(gid).or_insert(0.0) -= amount_this_run;
                    if let Some(good) = factuals.goods.get(&gid) {
                        for (&decay_gid, &decay_share) in &good.decay_result {
                            let produced = amount_this_run * decay_share;
                            if produced > 0.0 {
                                *changes.entry(decay_gid).or_insert(0.0) += produced;
                            }
                        }
                    }
                }
            }
        }

        // Optional inputs — only consumed in the boosted portion
        for opt in self.optional_inputs() {
            let gid = opt.good;
            let base = opt.amount;
            let is_fixed = opt.fixed;
            let itype = &opt.input_type;

            let eff_boosted = if is_fixed { base } else { base * final_input_mult * final_throughput_mult };
            let amount_boosted = eff_boosted * boosted_iters;

            let avail = inputs.get(&gid).copied().unwrap_or(0.0);
            let amount_consumed = amount_boosted.min(avail);

            if amount_consumed <= 0.0 { continue; }

            match itype {
                InputType::Factor => {}
                InputType::Capital => { *used_inputs.entry(gid).or_insert(0.0) += amount_consumed; }
                InputType::Destroyed => { *changes.entry(gid).or_insert(0.0) -= amount_consumed; }
                InputType::Consumed => {
                    *changes.entry(gid).or_insert(0.0) -= amount_consumed;
                    if let Some(good) = factuals.goods.get(&gid) {
                        for (&decay_gid, &decay_share) in &good.decay_result {
                            let produced = amount_consumed * decay_share;
                            if produced > 0.0 {
                                *changes.entry(decay_gid).or_insert(0.0) += produced;
                            }
                        }
                    }
                }
            }
        }

        // Outputs
        for outp in &self.outputs {
            let gid = outp.good;
            let base = outp.amount;
            let is_fixed = outp.fixed;

            let eff_boosted = if is_fixed { base } else { base * final_output_mult * final_throughput_mult };
            let amt_boosted = eff_boosted * boosted_iters;

            let eff_normal = if is_fixed { base } else { base * factor_output_mult * factor_throughput_mult };
            let amt_normal = eff_normal * normal_iters;

            let produced = amt_boosted + amt_normal;
            *changes.entry(gid).or_insert(0.0) += produced;
        }

        // Extra outputs from optionals (scaled only by boosted portion)
        for (gid, extra_amt) in bonus_extra_outputs {
            *changes.entry(gid).or_insert(0.0) += extra_amt * boosted_iters;
        }

        ProcessResult {
            iterations: completed,
            changes,
            used_inputs,
            effects,
        }
    }

    /// # Check Factors
    /// 
    /// Takes the inputs and returns the bonuses the facotrs give.
    /// 
    /// Result is Some(Input, Throughput, Output).
    /// 
    /// If None is returned, then it is missing a required factor.
    pub(crate) fn check_factors(&self, inputs: &HashMap<usize, f64>) 
    -> Option<(f64, f64, f64, Vec<ProcessEffect>)> {
        let mut input_mult: f64 = 1.0;
        let mut output_mult: f64 = 1.0;
        let mut throughput_mult: f64 = 1.0;
        let mut other_effects = vec![];

        for factor in self.factors() {
            if inputs.contains_key(&factor.good) {
                // if contained, check if it's optional, add optinoal effect if it's there.
                if let Some(effects) = factor.optional_effects() {
                    for effect in effects {
                        match effect {
                            InputEffect::Throughput(v) => throughput_mult += v,
                            InputEffect::Input(v) => input_mult -= v,
                            InputEffect::Output(v) => output_mult += v,
                            InputEffect::ExtraOutput(_, _) |
                            InputEffect::Growth(_) => assert!(false, "Factors cannot include extra output or growth effects."),
                        }
                    }
                }
            } else if !factor.is_optional() {
                // If not contained, and it's not optional, return none.
                return None;
            }
        }
        input_mult = input_mult.max(0.0).min(1.0);

        Some((input_mult, throughput_mult, output_mult, other_effects))
    }

    /// # Do Process Leg
    /// 
    /// A helper which does a single leg of a process. In simplified terms, it takes 
    /// the inputs (minus prior legs), a target it will stop at, and the bonuses 
    /// gained from factuals (we don't recalculate them for each step)
    /// 
    /// This is public for testing purposes, but should not actually be called.
    pub fn do_process_leg(
        &self,
        inputs: &HashMap<usize, f64>,
        target: Option<f64>,
        bonuses: (f64, f64, f64),
        factuals: &Factuals,
    ) -> ProcessResult {
        // copy over our bonuses for modding.
        let (mut input_bonus, mut throughput_bonus, mut output_bonus) = bonuses;
        // Go through optionals, finding the shortest possible and collecting bonuses along
        // the way.
        let mut optional_iters = HashMap::new();
        let mut shortest = target.unwrap_or(f64::INFINITY);
        let mut other_bonuses = vec![];
        for optional in self.optional_inputs() {
            let available = *inputs.get(&optional.good).unwrap_or(&0.0);
            if available > 0.0 { // Skip if none available, otherwise, add and check.
                let iters = available / optional.amount;
                optional_iters.insert(optional.good, iters);
                shortest = shortest.min(iters);
                let effects = optional.optional_and_effects.unwrap();
                for effect in effects {
                    match effect {
                        // Add bonuses for our future calculation needs.
                        InputEffect::Throughput(v) => throughput_bonus += v,
                        InputEffect::Input(v) => input_bonus -= v,
                        InputEffect::Output(v) => output_bonus += v,
                        InputEffect::ExtraOutput(..) |
                        InputEffect::Growth(_) => other_bonuses.push(effect),
                    }
                }
            } // if not available, skip, optional goods don't matter.
        }

        // cap input to ensure no negative input goods.
        input_bonus = input_bonus.min(0.0).max(1.0);
        println!("Input Bonus {}, Throughput Bonus {}, Output Bonus {}", input_bonus, throughput_bonus, output_bonus);

        // next, using our guaranteed input reduction, find how many iterations we can do
        // with required goods.
        let final_input_mod = input_bonus * throughput_bonus;

        for required in self.requirements() {
            let available = *inputs.get(&required.good).unwrap_or(&0.0);
            println!("Required good {}: available {}, required per iter {}, final mod {}", required.good, available, required.amount, final_input_mod);
            let effective_cost = if required.fixed {
                required.amount
            } else {
                required.amount * final_input_mod 
            };
            if effective_cost > 0.0 {
                let iters = available / effective_cost;
                shortest = shortest.min(iters);
            }
            println!("Effective cost {}, iters {}", effective_cost, shortest);
            // emergency shortcut if we hit a shortest of 0.
            if shortest <= 0.0 {
                return ProcessResult::empty();
            }
        }

        // With final target gotten after bonuses, complete up to that shortest step.
        let mut result = ProcessResult {
            iterations: shortest,
            changes: HashMap::new(),
            used_inputs: HashMap::new(),
            effects: Vec::new(),
        };

        // record all input effects.
        for input in self.inputs.iter() {
            // get how much we are moving.
            let change = if input.is_optional() || input.fixed {
                input.amount * shortest
            } else {
                input.amount * final_input_mod * shortest
            };
            // then record the change to where it belongs
            match input.input_type {
                InputType::Factor => {}, // skip factors
                // capital goes to used.
                InputType::Capital => { result.used_inputs.insert(input.good, change); },
                // Destroyed is subtracted from changes.
                InputType::Destroyed => { result.changes.insert(input.good, -change); },
                // Consumed is subtracted and it's decay added to changes
                InputType::Consumed => {
                    result.changes.insert(input.good, -change);
                    // also add decay results to output changes.
                    // we can unwrap here since decay results are guaranteed to exist for consumed goods.
                    if let Some(good) = factuals.goods.get(&input.good) {
                        for (&decay_good, &decay_rate) in &good.decay_result {
                            let decay_amount = change * decay_rate;
                            *result.changes
                                .entry(decay_good)
                                .or_insert(0.0) += decay_amount;
                        }
                    } else { assert!(false, "Good not found!")}
                },
            }
            // lastly, add any extra effects from the input to the process result.
            if let Some(effects) = input.optional_effects() {
                for effect in effects {
                    match effect {
                        InputEffect::ExtraOutput(good_id, amt) => {
                            *result.changes.entry(*good_id)
                                .or_insert(0.0) += amt * shortest;
                        }
                        InputEffect::Growth(v) => {
                            result.effects.push(ProcessEffect::Growth(v * shortest));
                        }
                        _ => {}
                    }
                }
            }
        }

        // now do outputs
        let final_output_mod = output_bonus * throughput_bonus;
        for output in &self.outputs {
            let change = if output.fixed {
                output.amount * shortest
            } else {
                output.amount * final_output_mod * shortest
            };
            *result.changes.entry(output.good).or_insert(0.0) += change;
        }

        // finish with process level bonuses.
        for effect in &self.effects {
            let scaled_effect = match effect {
                ProcessEffect::Research(v) => ProcessEffect::Research(v * shortest),
                ProcessEffect::Culture(v) => ProcessEffect::Culture(v * shortest),
                ProcessEffect::Faith(v) => ProcessEffect::Faith(v * shortest),
                ProcessEffect::Authority(v) => ProcessEffect::Authority(v * shortest),
                ProcessEffect::Legitimacy(v) => ProcessEffect::Legitimacy(v * shortest),
                ProcessEffect::Growth(v) => ProcessEffect::Growth(v * shortest),
            };
            result.effects.push(scaled_effect);
        }

        result
    }
}

/// # Process Result
/// 
/// A helper to return the results of doing a process.
/// 
/// Includes:
/// - Iterations completed
/// - Changes to goods from the process, including outputs and decay results of 
/// destroyed inputs.
/// - Used inputs, including factors and capital, which are not consumed but are still 
/// important to record.
/// - Effects produced by the process, such as research or culture.
#[derive(Debug, Clone)]
pub struct ProcessResult {
    pub iterations: f64,
    pub changes: HashMap<usize, f64>,
    pub used_inputs: HashMap<usize, f64>,
    pub effects: Vec<ProcessEffect>,
}

impl ProcessResult {
    /// # Empty
    /// 
    /// An empty process result, with 0 iterations, no changes, no used inputs, and no effects.
    pub fn empty() -> Self {
        Self {
            iterations: 0.0,
            changes: HashMap::new(),
            used_inputs: HashMap::new(),
            effects: Vec::new(),
        }
    }
}

/// # Process Input
/// 
/// The data for an input good for a process.
#[derive(Debug, Clone)]
pub struct ProcessInput {
    /// The Good for input.
    pub good: usize,
    /// The Amount needed per iteration.
    pub amount: f64,
    /// Whether the input is effected by Throughput or input bonuses.
    pub fixed: bool,
    /// Defines how the input and output of the good works.
    pub input_type: InputType,
    /// Defines the input as optional if this is Some().
    /// Additional Effects can be added to to the vector contained.
    /// 
    /// Optional goods are never effected by input or throughput bonuses.
    optional_and_effects: Option<Vec<InputEffect>>,
}

impl ProcessInput {
    /// # New
    /// 
    /// Create a new process input with the given good, amount, fixed status, input type, and optional status.
    pub fn new(good: usize, amount: f64, fixed: bool, input_type: InputType, optional: bool) -> Self {
        ProcessInput {
            good,
            amount,
            fixed,
            input_type,
            optional_and_effects: if optional { Some(Vec::new()) } else { None },
        }
    }

    /// # With Optional
    /// 
    /// Make this input optional and add the given effect to the vector of effects produced by this input.
    pub fn with_optional(mut self, effect: InputEffect) -> Self {
        if let Some(effects) = &mut self.optional_and_effects {
            effects.push(effect);
        } else {
            self.optional_and_effects = Some(vec![effect]);
        }
        self
    }

    /// # Is Optional
    /// 
    /// Returns true if this input is optional, false otherwise.
    pub fn is_optional(&self) -> bool {
        self.optional_and_effects.is_some()
    }

    /// # Optional Effects
    /// 
    /// Returns the vector of effects produced by this input if it is optional, None otherwise.
    pub fn optional_effects(&self) -> Option<&Vec<InputEffect>> {
        self.optional_and_effects.as_ref()
    }
}

/// # Input Type
/// 
/// Defines how the good interacts with input and it's consumption.
#[derive(Debug, Clone)]
pub enum InputType {
    /// Good Is destroyed, it's decay result does not get added to output.
    Destroyed,
    /// Good is destroyed, but it's decay result is also added to output.
    Consumed,
    /// Good is not destroyed, instead it is just used.
    /// Never produces it's result output from this.
    Capital,
    /// Good is not destroyed and it's amount does not matter. Any amount of this good
    /// covers all processes that could possibly be done.
    /// 
    /// Used for environmental factors typically.
    Factor,
}

/// # Input Effect
/// 
/// Additional Effects produced by a specific input when included in a process.
#[derive(Debug, Clone)]
pub enum InputEffect {
    /// An additive percent increase in both input and output goods produced by the process.
    /// 
    /// Does not effect Fixed Goods.
    Throughput(f64),
    /// An additive precent reduction to the number of input goods needed.
    /// 
    /// Does not effect fixed goods.
    Input(f64),
    /// An additive percent bonuse to all goods of the process.
    Output(f64),
    /// An additional output added to the process on top of all others, when this is
    /// included in the process.
    ExtraOutput(usize, f64),
    /// Additinoal Birth or mortality rate of workers attached to this process.
    Growth(f64),
}

/// # Process Output
/// 
/// The details of a process's outputs.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    /// The good produced by the process.
    pub good: usize,
    /// The Amount of the good produced per iteration of the process completed.
    pub amount: f64,
    /// Whether the output scales with output and throughput bonuses.
    pub fixed: bool,
}

impl ProcessOutput {
    pub fn new(good: usize, amount: f64, fixed: bool) -> Self {
        ProcessOutput { good, amount, fixed }
    }
}

/// # Process Effect
/// 
/// Additional effects which a process produces when done.
#[derive(Debug, Clone)]
pub enum ProcessEffect {
    /// Additional Research points produced by the process.
    /// Goes to the firm doing the process.
    Research(f64),
    /// Additional culture produced by the process.
    /// Goes to the cultures of the workers.
    Culture(f64),
    /// Additional Faith produced by the process.
    /// Goes to the religion of the workers.
    Faith(f64),
    /// Additional Authority produced by the process.
    /// Goes to the player who's territory the process is done in..
    Authority(f64),
    /// Additional Legitimacy produced by the process.
    /// Goes to the player who's territory the process is done in.
    Legitimacy(f64),
    /// Additional birth or mortality rate of the populace within the workers.
    /// Does not scale with processes done, only with size of worker populace.
    Growth(f64),
}