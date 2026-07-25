
use std::collections::{HashMap, HashSet};

use crate::game::factuals::Factuals;

pub use crate::game::effects::ProcessEffect;

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
        debug_assert!(!self.inputs.iter().any(|i| i.good == input.good), 
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

    /// # Is Valid
    /// 
    /// A helper function that checks if a process is valid, or more accurately if 
    /// it's invalid.
    /// 
    /// It checks for repeated goods as inputs, or if it has factors that bring input 
    /// down to zero but have no fixed required inputs to keep the process from going 
    /// infinitely.
    pub fn is_valid(&self) -> bool {
        // check for repeated goods as inputs.
        let mut seen = HashSet::new();
        for input in &self.inputs {
            if !seen.insert(input.good) {
                return false;
            }
        }
        // check for factors that reduce input to 0 without fixed required inputs.
        if !self.requirements().iter().any(|input| input.fixed) {
            // add up our input reduction factors.
            let mut input_mult: f64 = 1.0;
            for factor in self.factors() {
                if let Some(effects) = factor.optional_effects() {
                    for effect in effects {
                        match effect {
                            InputEffect::Input(v) => input_mult -= v,
                            _ => {}
                        }
                    }
                }
            }
            // if the input multiplier is 0, then we can have an infinite process.
            if input_mult <= 0.0 {
                return false;
            }
        }
        // if there are required inputs that are fixed, we don't need to work about 
        // infinite runs.
        true
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
    /// Factors and capital are never consumed or destroyed, just used. They are not 
    /// added to changed or used.
    /// 
    /// Due to the way the logic works out, doing a process that can reach over 100% 
    /// input reduction is lossy. It's not smart enough to conserve bonuses and ensure
    /// maximum benefit. So, when you make a process be aware that a greater than 100% 
    /// input bonus will end up being lost. Either cap input efficiency at 100%, or
    /// break up that process into variants to keep it under the cap.
    /// 
    /// ## Functional Logic
    /// 
    /// 1. Check and record Factors, as they don't scale or get consumed anyway and a 
    /// missing required factor stops the whole process.
    /// 2. Work on optional inputs next, getting any bonuses and effects they have.
    /// 3. With all bonuses calculated, check how many iterations can be done with 
    /// required inputs. Shifting goods from optional inputs to required as needed.
    /// 4. Only do this for as many as we can guarantee the current coverage.
    /// 5. Collect results adn return to step 2 until we can't make any more iterations.
    /// 
    /// ## On End and Failure
    /// 
    /// Regardless of success or failure, the ProcessResult includes the last required 
    /// good(s) that ran out, giving a hint to show what would be needed to do more 
    /// iterations. Useful for both totally new processes, and processes that didn't 
    /// reach their target.
    pub fn do_process(
        &self,
        inputs: &HashMap<usize, f64>,
        target: Option<f64>,
        factuals: &Factuals,
    ) -> ProcessResult {
        debug_assert!(target.is_none() || target.unwrap() > 0.0, "Target must be greater than 0 if provided.");

        // first, check factors and get bonuses.
        let bonuses = match self.check_factors(inputs) {
            Some(bonuses) => bonuses,
            None => { // if missing a required factor, return an empty result.
                let mut result = ProcessResult::empty();
                for factor in self.factors() {
                    if inputs.get(&factor.good).unwrap_or(&0.0) <= &0.0 {
                        result.missing_goods.push(factor.good);
                    }
                }
                return result;
            },
        };

        // loop through legs of the process until we get 0 iterations in return.
        let mut working_inputs = inputs.clone();
        let mut working_target = target.unwrap_or(f64::INFINITY);
        let mut result_acc = ProcessResult::empty();
        loop {
            let result = self.do_process_leg(&working_inputs, Some(working_target), 
                (bonuses.0, bonuses.1, bonuses.2), factuals);
            if result.iterations <= 0.0 {
                // we ran out of inputs, so find which inputs
                for req in self.requirements() {
                    if *working_inputs.get(&req.good).unwrap_or(&0.0) <= 0.0 {
                        result_acc.missing_goods.push(req.good);
                    }
                }
                break;
            }
            working_target -= result.iterations;
            result_acc.iterations += result.iterations;
            // add the results to the accumulator, and subtract used inputs from the working inputs for the next leg.
            for (good, change) in &result.changes {
                *result_acc.changes.entry(*good).or_insert(0.0) += *change;
                if *change < 0.0 {
                    *working_inputs.entry(*good).or_insert(0.0) += *change;
                }
            }
            for (good, used) in &result.used_inputs {
                *result_acc.used_inputs.entry(*good).or_insert(0.0) += *used;
                *working_inputs.entry(*good).or_insert(0.0) -= *used;
            }
            // add effects, consolidating into singular effects.
            let mut effects = vec![];
            for effect in &result.effects {
                let mut added = false;
                for existing in &mut result_acc.effects {
                    if let Some(new_effect) = existing.add(effect) {
                        *existing = new_effect;
                        added = true;
                        break;
                    }
                }
                if !added {
                    effects.push(effect.clone());
                }
            }
            result_acc.effects.extend(effects);
            // After adding up everything, break out if we reached the target.
            if working_target <= 0.0 {
                break;
            }
        }

        result_acc
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
        let other_effects = vec![];

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
        debug_assert!(target.is_none() || target.unwrap() > 0.0, "Target must be greater than 0 if provided.");
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
        input_bonus = input_bonus.max(0.0).min(1.0);
        // println!("Input Bonus {}, Throughput Bonus {}, Output Bonus {}", input_bonus, throughput_bonus, output_bonus);

        // next, using our guaranteed input reduction, find how many iterations we can do
        // with required goods.
        let final_input_mod = input_bonus * throughput_bonus;

        for required in self.requirements() {
            let available = *inputs.get(&required.good).unwrap_or(&0.0);
            // println!("Required good {}: available {}, required per iter {}, final mod {}", required.good, available, required.amount, final_input_mod);
            let effective_cost = if required.fixed {
                required.amount
            } else {
                required.amount * final_input_mod 
            };
            if effective_cost > 0.0 {
                let iters = available / effective_cost;
                shortest = shortest.min(iters);
            }
            // println!("Effective cost {}, iters {}", effective_cost, shortest);
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
            missing_goods: Vec::new(),
        };

        // record all input effects.
        for input in self.inputs.iter() {
            // get how much we are moving.
            let change = if input.is_optional() || input.fixed {
                input.amount * shortest
            } else {
                input.amount * final_input_mod * shortest
            }.min(*inputs.get(&input.good).unwrap_or(&0.0));
            // println!("Processing input good {}, change {}, type {:?}", input.good, change, input.input_type);
            if change <= 0.0 { continue; }
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
                    } else { assert!(false, "Good '{}' not found!", input.good)}
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
    /// The number of iterations completed in total.
    pub iterations: f64,
    /// The changes to the goods input, negative means destroyed, positive means created.
    pub changes: HashMap<usize, f64>,
    /// Inputs that were used, but not consumed, always peositive value.
    pub used_inputs: HashMap<usize, f64>,
    /// Any addititional effects produced by the process.
    pub effects: Vec<ProcessEffect>,
    /// The goods that caused us to stop.
    /// 
    /// Should only be empty if the process ends by running out of goods, not because
    /// of reaching it's target.
    pub missing_goods: Vec<usize>,
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
            missing_goods: Vec::new(),
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
        debug_assert!(amount > 0.0, "Input amount must be greater than 0.");
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

#[cfg(test)]
mod processes {
    use crate::game::{factuals::Factuals, good::Good, process::{InputEffect, InputType, Process, ProcessEffect, ProcessInput, ProcessOutput}};
    use std::{collections::HashMap};
    // --- Minimal helpers to keep tests readable ---

    static REQ_GOOD: usize = 10;
    static OUT_GOOD: usize = 20;
    static OPTIN_GOOD: usize = 30;
    static OPTIN_EFFECT: InputEffect = InputEffect::Input(0.2);
    static OPTOUT_GOOD: usize = 31;
    static OPTOUT_EFFECT: InputEffect = InputEffect::Output(0.3);
    static OPTTHROUGH_GOOD: usize = 32;
    static OPTTHROUGH_EFFECT: InputEffect = InputEffect::Throughput(0.25);
    static FIXED_GOOD: usize = 40;
    static FACTOR_GOOD: usize = 99;
    static CONSUMED_GOOD: usize = 100;
    static CAPITAL_GOOD: usize = 50;
    static DECAY_OUTPUT: usize = 200;

    fn make_factuals(goods: Vec<Good>) -> Factuals {
        let mut factuals = Factuals::new();
        for g in goods {
            factuals.goods.insert(g.id, g);
        }
        factuals
    }

    fn make_input(good: usize, amount: f64, fixed: bool, itype: InputType) -> ProcessInput {
        ProcessInput::new(good, amount, fixed, itype, false)
    }

    fn make_optional_input(
        good: usize,
        amount: f64,
        fixed: bool,
        itype: InputType,
        effects: Vec<InputEffect>,
    ) -> ProcessInput {
        let mut inp = ProcessInput::new(good, amount, fixed, itype, true);
        for e in effects {
            inp = inp.with_optional(e);
        }
        inp
    }

    fn make_process() -> Process {
        Process::new(0, "Test", 0)
            .with_input(make_input(REQ_GOOD, 1.0, false, InputType::Destroyed))
            .with_output(ProcessOutput::new(OUT_GOOD, 1.0, false))
    }

    fn make_good(id: usize, name: &str, decay_result: HashMap<usize, f64>) -> Good {
        Good {
            id,
            name: name.to_string(),
            class: None,
            tags: Default::default(),
            decay_rate: 0.0,
            decay_result,
            categories: vec![],
            // add any other fields your Good actually has
        }
    }

    mod check_factor {
        use super::*;

        #[test]
        fn test_without_factors() {
            let process = make_process();

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);
            available.insert(FACTOR_GOOD, 1.0);

            // Initial check, no factors, returns some with 1.0 in all parts.
            let result = process.check_factors(&available);
            if let Some((input, throughput, output, _)) = result {
                assert_eq!(input, 1.0, "Input Incorrect.");
                assert_eq!(throughput, 1.0, "Throughput Incorrect.");
                assert_eq!(output, 1.0, "Output Incorrect.");
            } else {
                assert!(false, "Did not return Correct value.");
            }
        }

        #[test]
        fn test_with_required_factors() {
            let process = make_process()
                .with_input(make_input(FACTOR_GOOD, 1.0, false, InputType::Factor));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);

            // Initial check, no factors, returns None
            let result = process.check_factors(&available);
            if let Some(_) = result {
                assert!(false, "Returned Some when it shouldn't have.");
            } else {
                assert!(true, "Did not return Correct value.");
            }

            // Include factors, expect output.
            available.insert(FACTOR_GOOD, 1.0);

            let result = process.check_factors(&available);
            if let Some((input, throughput, output, _)) = result {
                assert_eq!(input, 1.0, "Input Incorrect.");
                assert_eq!(throughput, 1.0, "Throughput Incorrect.");
                assert_eq!(output, 1.0, "Output Incorrect.");
            } else {
                assert!(false, "Did not return Correct value.");
            }
        }

        #[test]
        fn test_with_optional_factors() {
            let process = make_process()
                .with_input(make_optional_input(OPTIN_GOOD, 1.0, false, 
                    InputType::Factor, vec![OPTIN_EFFECT.clone()]))
                .with_input(make_optional_input(OPTTHROUGH_GOOD, 1.0, false, 
                    InputType::Factor, vec![OPTTHROUGH_EFFECT.clone()]))
                .with_input(make_optional_input(OPTOUT_GOOD, 1.0, false, 
                    InputType::Factor, vec![OPTOUT_EFFECT.clone()]));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);

            // Initial check, no factors, returns baseline.
            let result = process.check_factors(&available);
            if let Some((input, throughput, output, _)) = result {
                assert_eq!(input, 1.0, "Input Incorrect.");
                assert_eq!(throughput, 1.0, "Throughput Incorrect.");
                assert_eq!(output, 1.0, "Output Incorrect.");
            } else {
                assert!(false, "Did not return Correct value.");
            }

            // Include factors, expect output.
            available.insert(OPTIN_GOOD, 1.0);

            let result = process.check_factors(&available);
            if let Some((input, throughput, output, _)) = result {
                assert_eq!(input, 0.8, "Input Incorrect.");
                assert_eq!(throughput, 1.0, "Throughput Incorrect.");
                assert_eq!(output, 1.0, "Output Incorrect.");
            } else {
                assert!(false, "Did not return Correct value.");
            }

            available.insert(OPTTHROUGH_GOOD, 1.0);

            let result = process.check_factors(&available);
            if let Some((input, throughput, output, _)) = result {
                assert_eq!(input, 0.8, "Input Incorrect.");
                assert_eq!(throughput, 1.25, "Throughput Incorrect.");
                assert_eq!(output, 1.0, "Output Incorrect.");
            } else {
                assert!(false, "Did not return Correct value.");
            }
            
            available.insert(OPTOUT_GOOD, 1.0);

            let result = process.check_factors(&available);
            if let Some((input, throughput, output, _)) = result {
                assert_eq!(input, 0.8, "Input Incorrect.");
                assert_eq!(throughput, 1.25, "Throughput Incorrect.");
                assert_eq!(output, 1.3, "Output Incorrect.");
            } else {
                assert!(false, "Did not return Correct value.");
            }
        }
    }

    mod do_process_leg_should {
        use super::*;

        #[test]
        fn return_empty_result_correctly() {
            let process = make_process();

            let available = HashMap::new();

            let factuals = make_factuals(vec![]);
            let result = process.do_process_leg(&available, None, (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 0.0);
            assert_eq!(result.changes.len(), 0);
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
        }

        #[test]
        fn return_complete_success_on_simple_process() {
            let process = make_process();

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);

            let factuals = make_factuals(vec![]);
            let result = process.do_process_leg(&available, None, (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 2);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
        }

        #[test]
        fn return_complete_success_with_capital() {
            let process = make_process()
                .with_input(make_input(CAPITAL_GOOD, 1.0, true, InputType::Capital));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);
            available.insert(CAPITAL_GOOD, 100.0);

            let factuals = make_factuals(vec![]);
            let result = process.do_process_leg(&available, None, (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 2);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 1);
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&100.0));
            assert_eq!(result.effects.len(), 0);
        }

        #[test]
        fn return_complete_success_with_consumed_input() {
            let process = make_process()
                .with_input(make_input(CONSUMED_GOOD, 1.0, false, InputType::Consumed));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);
            available.insert(CONSUMED_GOOD, 100.0);

            let factuals = make_factuals(vec![make_good(CONSUMED_GOOD, 
                "Consumed", vec![(DECAY_OUTPUT, 1.0)].into_iter().collect())]);
            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 4);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.changes.get(&CONSUMED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
        }

        #[test]
        fn return_partial_success() {
            let process = make_process()
                .with_input(make_input(CAPITAL_GOOD, 1.0, false, InputType::Destroyed));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 50.0);
            available.insert(CAPITAL_GOOD, 100.0);

            let factuals = make_factuals(vec![]);
            let result = process.do_process_leg(&available, None, (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 3);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&CAPITAL_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
        }

        #[test]
        fn return_success_with_optional_input_but_none_given() {
            let process = make_process()
                .with_input(make_optional_input(OPTIN_GOOD, 1.0, false, 
                    InputType::Destroyed, vec![OPTIN_EFFECT.clone()]));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);

            let factuals = make_factuals(vec![]);
            let result = process.do_process_leg(&available, None, (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 2);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
        }

        #[test]
        fn correctly_include_factor_bonuses_and_fixed_input() {
            let process = make_process()
                .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed))
                .with_output(ProcessOutput::new(FACTOR_GOOD, 1.0, true));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);
            available.insert(FIXED_GOOD, 100.0);

            let factuals = make_factuals(vec![]);

            // Input Bonus
            let result = process.do_process_leg(&available, None, 
                (0.5, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 4);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);

            // Throughput Bonus
            let result = process.do_process_leg(&available, None, 
                (1.0, 2.0, 1.0), &factuals);

            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 4);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);

            // Output Bonus
            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 2.0), &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 4);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&200.0));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);

            // Overlapping Bonii
            let result = process.do_process_leg(&available, None, 
                (0.5, 4.0, 2.0), &factuals);

            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 4);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&400.0));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
        }

        #[test]
        fn correctly_use_optional_goods_and_bonuses() {
            let og_process = make_process()
                .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed))
                .with_output(ProcessOutput::new(FACTOR_GOOD, 1.0, true));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);
            available.insert(FIXED_GOOD, 100.0);
            available.insert(OPTIN_GOOD, 100.0);
            available.insert(OPTTHROUGH_GOOD, 100.0);
            available.insert(OPTOUT_GOOD, 100.0);

            let factuals = make_factuals(vec![]);

            // Input Bonus
            let process = og_process.clone()
                .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTIN_EFFECT.clone()]));
            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 5);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-80.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);

            // Throughput Bonus
            let process = og_process.clone()
                .with_input(make_optional_input(OPTTHROUGH_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTTHROUGH_EFFECT.clone()]));
            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 80.0);
            assert_eq!(result.changes.len(), 5);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-80.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-80.0));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&80.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);

            // Output Bonus
            let process = og_process.clone()
                .with_input(make_optional_input(OPTOUT_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTOUT_EFFECT.clone()]));
            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 5);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&130.0));
            assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);

            // overlapping Bonii
            let process = og_process.clone()
                .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTIN_EFFECT.clone()]))
                .with_input(make_optional_input(OPTTHROUGH_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTTHROUGH_EFFECT.clone()]))
                .with_input(make_optional_input(OPTOUT_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTOUT_EFFECT.clone()]));
            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);
            
            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 7);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&162.5));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
        }
    
        #[test]
        fn process_other_effects_correctly() {
            let process = make_process()
                .with_input(make_optional_input(CONSUMED_GOOD, 1.0, false, 
                    InputType::Destroyed, vec![InputEffect::ExtraOutput(DECAY_OUTPUT, 1.0)]))
                .with_input(make_optional_input(FACTOR_GOOD, 1.0, false, 
                    InputType::Destroyed, vec![InputEffect::Growth(0.5)]))
                .with_effect(ProcessEffect::Research(100.0));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);
            available.insert(CONSUMED_GOOD, 100.0);
            available.insert(FACTOR_GOOD, 100.0);

            let factuals = make_factuals(vec![]);
            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 5);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&CONSUMED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 2);
            assert!(result.effects.contains(&ProcessEffect::Research(10000.0)));
            assert!(result.effects.contains(&ProcessEffect::Growth(50.0)));
        }

        #[test]
        fn multistep_process_test() {
            let process = make_process()
                .with_input(make_input(CAPITAL_GOOD, 1.0, true, InputType::Capital))
                .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed))
                .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTIN_EFFECT.clone()]))
                .with_input(make_optional_input(OPTTHROUGH_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTTHROUGH_EFFECT.clone()]))
                .with_input(make_optional_input(OPTOUT_GOOD, 1.0, true, 
                    InputType::Consumed, vec![OPTOUT_EFFECT.clone()]))
                .with_output(ProcessOutput::new(FACTOR_GOOD, 1.0, true));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 150.0);
            available.insert(CAPITAL_GOOD, 140.0);
            available.insert(FIXED_GOOD, 190.0);
            available.insert(OPTIN_GOOD, 30.0);
            available.insert(OPTTHROUGH_GOOD, 50.0);
            available.insert(OPTOUT_GOOD, 80.0);

            let factuals = make_factuals(vec![make_good(OPTOUT_GOOD, "Consumed", 
                vec![(DECAY_OUTPUT, 1.0)].into_iter().collect())]);

            // First Pass
            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);
            
            assert_eq!(result.iterations, 30.0);
            assert_eq!(result.changes.len(), 8);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&48.75));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&30.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&30.0));
            assert_eq!(result.used_inputs.len(), 1);
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&30.0));
            assert_eq!(result.effects.len(), 0);

            // second pass, remove stuff
            for (good, change) in &result.changes {
                *available.entry(*good).or_insert(0.0) += *change;
            }
            if let Some(capital_used) = result.used_inputs.get(&CAPITAL_GOOD) {
                *available.entry(CAPITAL_GOOD).or_insert(0.0) -= *capital_used;
            }

            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);
            
            assert_eq!(result.iterations, 20.0);
            assert_eq!(result.changes.len(), 7);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-25.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-20.0));
            assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-20.0));
            assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-20.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&32.5));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&20.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&20.0));
            assert_eq!(result.used_inputs.len(), 1);
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&20.0));
            assert_eq!(result.effects.len(), 0);

            // THird Pass, output bonus good next
            for (good, change) in &result.changes {
                *available.entry(*good).or_insert(0.0) += *change;
            }
            if let Some(capital_used) = result.used_inputs.get(&CAPITAL_GOOD) {
                *available.entry(CAPITAL_GOOD).or_insert(0.0) -= *capital_used;
            }

            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);
            
            assert_eq!(result.iterations, 30.0);
            assert_eq!(result.changes.len(), 6);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&39.0));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&30.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&30.0));
            assert_eq!(result.used_inputs.len(), 1);
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&30.0));
            assert_eq!(result.effects.len(), 0);

            // Fourth Pass, no bonuses left, one last one.
            for (good, change) in &result.changes {
                *available.entry(*good).or_insert(0.0) += *change;
            }
            if let Some(capital_used) = result.used_inputs.get(&CAPITAL_GOOD) {
                *available.entry(CAPITAL_GOOD).or_insert(0.0) -= *capital_used;
            }

            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);
            
            assert_eq!(result.iterations, 60.0);
            assert_eq!(result.changes.len(), 4);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-60.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-60.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&60.0));
            assert_eq!(result.changes.get(&FACTOR_GOOD), Some(&60.0));
            assert_eq!(result.used_inputs.len(), 1);
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&60.0));
            assert_eq!(result.effects.len(), 0);

            // Last Pass, should return 0 iterations.
            for (good, change) in &result.changes {
                *available.entry(*good).or_insert(0.0) += *change;
            }
            if let Some(capital_used) = result.used_inputs.get(&CAPITAL_GOOD) {
                *available.entry(CAPITAL_GOOD).or_insert(0.0) -= *capital_used;
            }

            let result = process.do_process_leg(&available, None, 
                (1.0, 1.0, 1.0), &factuals);

            assert_eq!(result.iterations, 0.0);
            assert_eq!(result.changes.len(), 0);
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);

            // Lastly, check that our available goods have been correctly updated for sanity reasons.

            assert_eq!(available.get(&REQ_GOOD), Some(&5.0));
            assert_eq!(available.get(&FIXED_GOOD), Some(&50.0));
            assert_eq!(available.get(&OPTIN_GOOD), Some(&0.0));
            assert_eq!(available.get(&OPTTHROUGH_GOOD), Some(&0.0));
            assert_eq!(available.get(&OPTOUT_GOOD), Some(&0.0));
            assert_eq!(available.get(&OUT_GOOD), Some(&180.25));
            assert_eq!(available.get(&FACTOR_GOOD), Some(&140.0));
            assert_eq!(available.get(&DECAY_OUTPUT), Some(&80.0));
        }
    }

    mod do_process_should {
        use super::*;

        #[test]
        fn basic_process_and_target_plus_capital_and_fixed_good_check() {
            let process = make_process();

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);

            let factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert!(result.used_inputs.is_empty()); // confirm no capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&REQ_GOOD));

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert!(result.used_inputs.is_empty()); // confirm no capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 0);

            // Repeat, but with Capital good.
            let process = process
                .with_input(make_input(CAPITAL_GOOD, 1.0, true, InputType::Capital));

            available.insert(CAPITAL_GOOD, 100.0);

            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 1); // confirm capital used
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&100.0)); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 2);
            assert!(result.missing_goods.contains(&REQ_GOOD));
            assert!(result.missing_goods.contains(&CAPITAL_GOOD));

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 1); // confirm capital used
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&50.0)); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 0);

            // === Include Fixed Good
            let process = process
                .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed));

            available.insert(FIXED_GOOD, 100.0);

            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 3); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 1); // confirm capital used
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&100.0)); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 3);
            assert!(result.missing_goods.contains(&REQ_GOOD));
            assert!(result.missing_goods.contains(&FIXED_GOOD));
            assert!(result.missing_goods.contains(&CAPITAL_GOOD));

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 3); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 1); // confirm capital used
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&50.0)); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 0);
        }

        #[test]
        fn process_with_factor() {
            // === Incluide Factor (and check it's exclusion cause failure)
            let process = make_process()
                .with_input(make_input(FACTOR_GOOD, 1.0, true, InputType::Factor));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);

            let factuals = make_factuals(vec![]);

            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 0.0);
            assert_eq!(result.changes.len(), 0); // only req and out should be changed
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&FACTOR_GOOD), "Factor input should be missing");

            // Actually add the factor in.
            available.insert(FACTOR_GOOD, 1.0);

            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&REQ_GOOD), "Required Good should be missing");

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 0);
        }

        #[test]
        fn process_with_consumed_good() {
            let process = make_process()
                .with_input(make_input(CONSUMED_GOOD, 1.0, true, InputType::Consumed));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);
            available.insert(CONSUMED_GOOD, 100.0);

            let factuals = make_factuals(vec![
                make_good(CONSUMED_GOOD, "Consumed", HashMap::from([(DECAY_OUTPUT, 1.0)])),
            ]);
            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 4); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&CONSUMED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 4); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&CONSUMED_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
        }

        #[test]
        fn process_with_optional_with_no_optionals_given() {
            let process = make_process()
                .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTIN_EFFECT.clone()]));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);
                
            let factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&REQ_GOOD), "Missing required good.");

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0);
            assert_eq!(result.changes.len(), 2); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 0);
        }

        #[test]
        fn insufficient_inputs_reduce_output() {
            let process = make_process()
                .with_input(make_input(CAPITAL_GOOD, 1.0, true, InputType::Capital))
                .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed));
            let mut available = HashMap::new();

            available.insert(REQ_GOOD, 100.0);
            available.insert(CAPITAL_GOOD, 30.0);
            available.insert(FIXED_GOOD, 130.0);

            let factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 30.0);
            assert_eq!(result.changes.len(), 3); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&30.0));
            assert_eq!(result.used_inputs.len(), 1); // confirm capital used
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&30.0)); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&CAPITAL_GOOD), "Missing required good.");

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 30.0);
            assert_eq!(result.changes.len(), 3); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-30.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&30.0));
            assert_eq!(result.used_inputs.len(), 1); // confirm capital used
            assert_eq!(result.used_inputs.get(&CAPITAL_GOOD), Some(&30.0)); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&CAPITAL_GOOD), "Missing required good.");
        }

        #[test]
        fn optional_input_gives_proportional_bonus() {
            let process = make_process()
                .with_input(make_optional_input(OPTIN_GOOD, 1.0, true, 
                    InputType::Destroyed, vec![OPTIN_EFFECT.clone()]));
            let mut available = HashMap::new();

            available.insert(REQ_GOOD, 100.0);
            available.insert(OPTIN_GOOD, 50.0); // only half the optional provided

            let factuals = make_factuals(vec![]);
            let result = process.do_process(&available, None, &factuals);

            // Check Results
            assert_eq!(result.iterations, 110.0); // should be a 10% boost from the optional
            assert_eq!(result.changes.len(), 3); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&110.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&REQ_GOOD), "Missing required good.");

            let result = process.do_process(&available, Some(50.0), &factuals);

            // Check Results
            assert_eq!(result.iterations, 50.0); // should be a 10% boost from the optional
            assert_eq!(result.changes.len(), 3); // only req and out should be changed
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-40.0));
            assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-50.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&50.0));
            assert_eq!(result.used_inputs.len(), 0); // confirm capital used
            assert!(result.effects.is_empty()); // confirm no stray effects.
            assert_eq!(result.missing_goods.len(), 0);
        }

        #[test]
        fn efficiency_modifiers_alone_and_stacked() {
            // extra goods for testing purposes
            let throughput_factor = 33;
            let output_factor = 34;
            let factor_throughput_bonus = 0.25;
            let factor_output_bonus = 0.2;
            // Base process (no bonuses)
            let base = make_process()
                .with_input(make_input(FIXED_GOOD, 1.0, true, InputType::Destroyed))
                .with_input(make_optional_input(OPTIN_GOOD, 1.0, false, InputType::Destroyed, vec![OPTIN_EFFECT.clone()]))
                .with_input(make_optional_input(OPTOUT_GOOD, 1.0, false, InputType::Destroyed, vec![OPTOUT_EFFECT.clone()]))
                .with_input(make_optional_input(OPTTHROUGH_GOOD, 1.0, false, InputType::Destroyed, vec![OPTTHROUGH_EFFECT.clone()]))

                .with_input(make_optional_input(FACTOR_GOOD, 1.0, false, InputType::Factor, vec![InputEffect::Input(0.2)]))
                .with_input(make_optional_input(throughput_factor, 1.0, false, InputType::Factor, vec![InputEffect::Throughput(factor_throughput_bonus)]))
                .with_input(make_optional_input(output_factor, 1.0, false, InputType::Factor, vec![InputEffect::Output(factor_output_bonus)]))

                .with_output(ProcessOutput::new(DECAY_OUTPUT, 1.0, true));

            let mut available = HashMap::new();
            available.insert(REQ_GOOD, 100.0);
            available.insert(FIXED_GOOD, 100.0);

            let factuals = make_factuals(vec![]);
            
            // baseline, no bonuses
            let result = base.do_process(&available, None, &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 4);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
            assert_eq!(result.missing_goods.len(), 2);
            assert!(result.missing_goods.contains(&REQ_GOOD));
            assert!(result.missing_goods.contains(&FIXED_GOOD));

            // input bonus factor
            available.insert(FACTOR_GOOD, 1.0);
            let result = base.do_process(&available, None, &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-80.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&100.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&FIXED_GOOD));

            // plus throughput factor
            available.insert(throughput_factor, 1.0);
            let result = base.do_process(&available, None, &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 4);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&125.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
            assert_eq!(result.missing_goods.len(), 2);
            assert!(result.missing_goods.contains(&REQ_GOOD));
            assert!(result.missing_goods.contains(&FIXED_GOOD));

            // plus output factor
            available.insert(output_factor, 1.0);
            let result = base.do_process(&available, None, &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 4);
            assert_eq!(result.changes.get(&REQ_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&150.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
            assert_eq!(result.missing_goods.len(), 2);
            assert!(result.missing_goods.contains(&REQ_GOOD));
            assert!(result.missing_goods.contains(&FIXED_GOOD));

            // plus input bonus optional
            available.insert(OPTIN_GOOD, 100.0);
            let result = base.do_process(&available, None, &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 5);
            assert!((result.changes.get(&REQ_GOOD).unwrap() + 75.0).abs() < 0.01);
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&150.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&FIXED_GOOD));

            // plus throughput optional
            available.insert(OPTTHROUGH_GOOD, 100.0);
            let result = base.do_process(&available, None, &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 6);
            assert!((result.changes.get(&REQ_GOOD).unwrap() + 90.0).abs() < 0.01);
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert!((result.changes.get(&OUT_GOOD).unwrap() -180.0).abs() < 0.01);
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&FIXED_GOOD));

            // plus output optional
            available.insert(OPTOUT_GOOD, 100.0);
            let result = base.do_process(&available, None, &factuals);

            assert_eq!(result.iterations, 100.0);
            assert_eq!(result.changes.len(), 7);
            assert!((result.changes.get(&REQ_GOOD).unwrap() + 90.0).abs() < 0.01);
            assert_eq!(result.changes.get(&FIXED_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTIN_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTTHROUGH_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&OPTOUT_GOOD), Some(&-100.0));
            assert_eq!(result.changes.get(&DECAY_OUTPUT), Some(&100.0));
            assert_eq!(result.changes.get(&OUT_GOOD), Some(&225.0));
            assert_eq!(result.used_inputs.len(), 0);
            assert_eq!(result.effects.len(), 0);
            assert_eq!(result.missing_goods.len(), 1);
            assert!(result.missing_goods.contains(&FIXED_GOOD));
        }
    }
}
