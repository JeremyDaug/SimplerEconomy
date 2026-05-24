
use std::collections::{HashMap, HashSet};

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
            .filter(|input| matches!(input.input_output, InputType::Factor))
            .cloned()
            .collect()
    }

    /// # Requirements
    /// 
    /// Gets the required inputs of the process. 
    /// Excludes Factors.
    pub fn requirements(&self) -> Vec<ProcessInput> {
        self.inputs.iter()
            .filter(|input| !matches!(input.input_output, InputType::Factor) && !input.is_optional())
            .cloned()
            .collect()
    }

    /// # Optional Inputs
    /// 
    /// Gets the optional inputs of the process, excluding factors.
    pub fn optional_inputs(&self) -> Vec<ProcessInput> {
        self.inputs.iter()
            .filter(|input| input.is_optional() && !matches!(input.input_output, InputType::Factor))
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
        // --- 1. Factor check + factor-only multipliers ---
        let mut factor_input_mult: f64 = 1.0;
        let mut factor_output_mult: f64 = 1.0;
        let mut factor_throughput_mult: f64 = 1.0;

        for factor in self.factors() {
            if !factor.is_optional() && !inputs.contains_key(&factor.good) {
                return ProcessResult::empty();
            }
            if let Some(effects) = factor.optional_effects() {
                for effect in effects {
                    match effect {
                        InputEffect::Throughput(v) => factor_throughput_mult += v,
                        InputEffect::Input(v) => factor_input_mult -= v,
                        InputEffect::Output(v) => factor_output_mult += v,
                        _ => {}
                    }
                }
            }
        }
        factor_input_mult = factor_input_mult.max(0.0).min(1.0);

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

                let coverage = if base_max_iters.is_infinite() {
                    1.0
                } else {
                    (support / base_max_iters).min(1.0)
                };

                if let Some(effects) = opt.optional_effects() {
                    for effect in effects {
                        match effect {
                            InputEffect::Throughput(v) => opt_throughput_bonus += v * coverage,
                            InputEffect::Input(v) => opt_input_bonus += v * coverage,
                            InputEffect::Output(v) => opt_output_bonus += v * coverage,
                            InputEffect::ExtraOutput(good_id, amt) => {
                                *bonus_extra_outputs.entry(*good_id).or_insert(0.0) += amt * coverage;
                            }
                            InputEffect::Growth(v) => {
                                bonus_effects.push(ProcessEffect::Growth(v * coverage));
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
            let itype = &inp.input_output;

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
            let itype = &opt.input_output;

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
    pub input_output: InputType,
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
    pub fn new(good: usize, amount: f64, fixed: bool, input_output: InputType, optional: bool) -> Self {
        ProcessInput {
            good,
            amount,
            fixed,
            input_output,
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