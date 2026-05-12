
use std::collections::{HashMap, HashSet};

use crate::game::factuals::Factuals;

/// # Process
/// 
/// Proccesses are how one set of goods is transformed into another set of goods.
/// 
/// It has a list of inputs and separate list of outputs to keep things simple.
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
    pub fn with_input(mut self, input: ProcessInput) -> Self {
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
    pub fn do_process(&self, inputs: HashMap<usize, f64>, target: Option<f64>, 
    factuals: &Factuals) -> ProcessResult {
        // get copy of inputs for overall good availability checking.
        let mut available = inputs.clone();
        // start with factuals, checking that required factors are available before
        // trying anything.
        // Modifiers are stacks of pairs multiplier followed by iterations it covers.
        let mut input_mult = vec![(1.0, f64::INFINITY)];
        let mut output_mult = vec![(1.0, f64::INFINITY)];
        let mut throughput_mult = vec![(1.0, f64::INFINITY)];
        for factor in self.factors().iter() {
            // If required and not available, then we can't continue.
            if !factor.is_optional() && !inputs.contains_key(&factor.good) {
                return ProcessResult {
                    iterations: 0.0,
                    changes: HashMap::new(),
                    used_inputs: HashMap::new(),
                    effects: Vec::new(),
                };
            }

            // get bonuses and effects from factors.
            if let Some(effects) = factor.optional_effects() {
                for effect in effects {
                    match effect {
                        InputEffect::Throughput(mult) => {
                            let last_mult = throughput_mult.last().unwrap().0;
                            throughput_mult.push((last_mult + mult, f64::INFINITY));
                        },
                        InputEffect::Input(mult) => {
                            let last_mult = input_mult.last().unwrap().0;
                            input_mult.push((last_mult - mult, f64::INFINITY));
                        },
                        InputEffect::Output(mult) => {
                            let last_mult = output_mult.last().unwrap().0;
                            output_mult.push((last_mult + mult, f64::INFINITY));
                        },
                        _ => {},
                    }
                }
            }
        }

        // Next get optionals, maximizing effects and bonuses. We can always scale back 
        // or reduce them later, but we want to know our bonuses as clearly as possible.
        let mut optional_reserves: HashMap<usize, (f64, f64)> = HashMap::new(); // Good, (reserved, iterations covered)
        for optional in self.optional_inputs() {
            if let Some(available) = inputs.get(&optional.good) {
                // If we have some of the optional, use it all to get max bonuses and effects.
                // We can scale back later if we don't have enough required goods to cover the target.
                if *available > 0.0 {
                    if let Some(effects) = optional.optional_effects() {
                        for effect in effects {
                            match effect {
                                InputEffect::Throughput(mult) => {
                                    let last_mult = throughput_mult.last().unwrap().0;
                                    throughput_mult.push((last_mult + mult, f64::INFINITY));
                                },
                                InputEffect::Input(mult) => {
                                    let last_mult = input_mult.last().unwrap().0;
                                    input_mult.push((last_mult - mult, f64::INFINITY));
                                },
                                InputEffect::Output(mult) => {
                                    let last_mult = output_mult.last().unwrap().0;
                                    output_mult.push((last_mult + mult, f64::INFINITY));
                                },
                                _ => {},
                            }
                        }
                    }
                }
            }
        }

        // Lastly, with bonuses calculated, iterate through the required inputs

        return ProcessResult {
            iterations: 0.0,
            changes: HashMap::new(),
            used_inputs: HashMap::new(),
            effects: Vec::new(),
        };
    }
}

pub struct ProcessResult {
    pub iterations: f64,
    pub changes: HashMap<usize, f64>,
    pub used_inputs: HashMap<usize, f64>,
    pub effects: Vec<ProcessEffect>,
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