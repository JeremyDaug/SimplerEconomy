
use std::collections::HashMap;

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
}

impl Process {
    /// # New
    /// 
    /// News up process with a given name and Id.
    pub fn new(id: usize, name: String) -> Self  {
        Process {
            id,
            name,
            inputs: vec![],
            outputs: vec![],
            effects: vec![],
        }
    }

    /// # With Input
    /// 
    /// Fluent input adder.
    pub fn with_input(mut self, input: ProcessInput) -> Self {
        self.inputs.push(input);
        self
    }

    /// # With Outputs
    /// 
    /// Fluent Output Adder
    pub fn with_output(mut self, output: ProcessOutput) -> Self {
        self.outputs.push(output);
        self
    }

    /// # With Effect
    /// 
    /// Fluent Effect Adder.
    pub fn with_effect(mut self, effect: ProcessEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// # Has Inputs
    /// 
    /// Returns all goods needed per iteration.
    pub fn has_inputs(&self) -> Vec<(usize, f64)> {
        let mut acc = HashMap::new();
        for input in self.inputs.iter() {
            acc.entry(input.good)
                .and_modify(|e| *e += input.amount)
                .or_insert(input.amount);
        }
        acc.into_iter().collect()
    }

    /// # Has Requirements
    /// 
    /// Returns the required goods and the amount needed per iteration.
    /// 
    /// Repeats will be consolidated.
    pub fn has_requirements(&self) -> Vec<(usize, f64)> {
        let mut acc = HashMap::new();
        for input in self.inputs.iter().filter(|input| !input.optional) {
            acc.entry(input.good)
                .and_modify(|e| *e += input.amount)
                .or_insert(input.amount);
        }
        acc.into_iter().collect()
    }

    /// # Has Optionals
    /// 
    /// Returns the optional goods and the amount needed per iteration.
    pub fn has_optionals(&self) -> Vec<(usize, f64)> {
        let mut acc = HashMap::new();
        for input in self.inputs.iter().filter(|input| input.optional) {
            acc.entry(input.good)
                .and_modify(|e| *e += input.amount)
                .or_insert(input.amount);
        }
        acc.into_iter().collect()
    }

    /// # Has Factors
    /// 
    /// Returns the factors needed for the process, which are inputs that ignore 
    /// quantity and simply need to be present to get their bonuses, as well as 
    /// whether they are optional or not.
    pub fn has_factors(&self) -> Vec<(usize, bool)> {
        self.inputs.iter()
            .filter(|input| matches!(input.tag, InputType::Factor))
            .map(|input| (input.good, input.optional))
            .collect()
    }

    /// # Requirements
    /// 
    /// Returns the required goods from our inputs.
    pub fn requirements(&self) -> Vec<&ProcessInput> {
        self.inputs.iter()
            .filter(|input| !input.optional)
            .collect()
    }

    /// # Optionals
    /// 
    /// Returns the optional goods from our inputs.
    pub fn optionals(&self) -> Vec<&ProcessInput> {
        self.inputs.iter()
            .filter(|input| input.optional)
            .collect()
    }

    /// # Factors
    /// 
    /// Returns the factors needed for the process, which are inputs that ignore quantity and
    /// simply need to be present to get their bonuses.
    pub fn factors(&self) -> Vec<&ProcessInput> {
        self.inputs.iter()
            .filter(|input| matches!(input.tag, InputType::Factor))
            .collect()
    }

    /// # Do Process
    /// 
    /// Does the process, taking in the input goods and the target number of iterations 
    /// to do, as well as goodData
    /// 
    /// If no target given it will do as many iterations as possible.
    /// 
    /// It returns the change in goods from the process, with positive being output 
    /// and negative being input, as well as any additional effects from the process.
    pub fn do_process(&self, input_goods: &HashMap<usize, f64>, target: Option<f64>, 
    data: &Factuals) -> ProcessResult {
        // New up our process data to eventually consolidate and return
        // Goods destroyed by the process.
        let mut goods_consumed = HashMap::new();
        // Goods used, but not destroyed by the process, such as capital.
        // Goods can't be used twice..
        let mut good_used = HashMap::new();
        // Goods produced by the process.
        let mut goods_produced = HashMap::new();
        // effects added and the number of  iterations the effect covers.
        let mut effects_produced = vec![]; 

        // get factor input and bonuses as they're quick, easy, and modify regardless of 
        // iterations.
        for factor in self.factors() {
            // if factor is required, check that it's present, if not return empty data.
            if !factor.optional && !input_goods.contains_key(&factor.good) {
                return ProcessResult::empty();
            }
            // if factor is optional and present, include it's effects.
            if factor.optional && input_goods.contains_key(&factor.good) {
                for effect in &factor.effects {
                    effects_produced.push((*effect, f64::INFINITY));
                }
            }
        }
        
        // The reserve goods assuming infinite target and no efficiency gains.
        let mut reserve_and_iter = HashMap::new();
        for (good, amt) in self.has_inputs() {
            let input = input_goods.get(&good).unwrap_or(&0.0);
            reserve_and_iter.insert(good, (input, input / amt));
        }

        // Go through optional inputs to calculate efficiency and input bonuses.
        for optional in self.optionals() {
            let input = input_goods.get(&optional.good).unwrap_or(&0.0);
            if *input > 0.0 {
                let ratio = input / reserve_and_iter.get(&optional.good).unwrap();
                // Efficiency bonuses are calculated by the ratio of input to amount, multiplied by the efficiency bonus value.
                for effect in &optional.effects {
                    if let InputEffect::Throughput(value) = effect {
                        effects_produced.push((InputEffect::Throughput(ratio * value), f64::INFINITY));
                    }
                    // Input bonuses are calculated by the ratio of input to amount, multiplied by the input bonus value.
                    if let InputEffect::InputBonus(value) = effect {
                        effects_produced.push((InputEffect::InputBonus(ratio * value), f64::INFINITY));
                    }
                    // Output bonuses are calculated by the ratio of input to amount, multiplied by the output bonus value.
                    if let InputEffect::OutputBonus(value) = effect {
                        effects_produced.push((InputEffect::OutputBonus(ratio * value), f64::INFINITY));
                    }
                }
            }
        }

        // With efficiency and input bonuses calculated, see how many interations 
        // we can now do with the input goods given the efficiency and input bonuses, 
        // as well as the target.

        todo!()
    }
}

/// # Process Result
/// 
/// The result of doing a process, including the change in goods and any additional
/// effects from the process. Helpful for both testing, and job reactivity.
#[derive(Debug, Clone)]
pub struct ProcessResult {
    /// How many iterations we did successfully.
    pub iterations: f64,
    /// The change in goods from the process, with positive being output and negative 
    /// being input.
    pub goods_change: HashMap<usize, f64>,
    /// Goods used, but not destroyed by the process, such as capital. Goods can't be 
    /// used twice.
    pub goods_used: HashMap<usize, f64>,
    /// Any additional effects from the process.
    pub effects: Vec<ProcessEffect>,
}

impl ProcessResult {
    /// # Empty
    /// 
    /// An empty process result with no change in goods and no effects.
    pub fn empty() -> Self {
        ProcessResult {
            iterations: 0.0,
            goods_change: HashMap::new(),
            goods_used: HashMap::new(),
            effects: vec![],
        }
    }

    /// # New
    /// 
    /// News up a process result with the given data.
    pub fn new(iterations: f64, goods_change: HashMap<usize, f64>, goods_used: HashMap<usize, f64>, effects: Vec<ProcessEffect>) -> Self {
        ProcessResult {
            iterations,
            goods_change,
            goods_used,
            effects,
        }
    }
}

/// # Process Effect
/// 
/// Effects that the process has which are not related to good production or 
/// consumption.
#[derive(Debug, Clone, Copy)]
pub enum ProcessEffect {
    /// Process alters the growth rate of pops, positive gets added to birth rate, 
    /// negative to mortality.
    Growth(f64),
    /// Adds authority to the player.
    Authority(f64),
    /// Adds Legitimacy to the player.
    Legitimacy(f64),
    /// Produces Culture
    Culture(f64),
    /// Produces Research (contained to the Firm, a portion given to player).
    Research(f64),
}

/// # Process Input
/// 
/// An input part to the process. Includes the good it needs, how many units of the good
/// are needed, the tag of the input, whether it's optional, and any additional effects 
/// from satisfying the input.
#[derive(Debug, Clone)]
pub struct ProcessInput {
    /// The good the input needs.
    /// TODO: May expand this to accept Buckets, or Classes as well as goods later.
    pub good: usize,
    /// The number of units needed per iteration of the process.
    pub amount: f64,
    /// The Input type tag whih applies additional rules and requirements.
    pub tag: InputType,
    /// Whether the input is optional or not.
    pub optional: bool,
    /// Any additional effects from this input.
    pub effects: Vec<InputEffect>,
}

impl ProcessInput {
    /// # New
    /// 
    /// News up a process input with the given data.
    pub fn new(good: usize, amount: f64, tag: InputType, optional: bool) -> Self {
        ProcessInput {
            good,
            amount,
            tag,
            optional,
            effects: vec![],
        }
    }

    /// # Add Effect
    /// 
    /// Fluently add an effect to the input, checking that the current state of the 
    /// input is valid for the effect being added.
    /// 
    /// All Input Effects require the input to be optional, as they are all bonuses 
    /// for satisfying optional inputs, so this checks for that.
    /// 
    /// Any inputs that create bonuses for required inputs should be ProcessEffects,
    /// not InputEffects, as they would apply regardless of whether the input is 
    /// satisfied or not.
    /// 
    /// Factors cannot have extra output goods, as they don't scale, and aren't consumed.
    /// 
    /// Reduceables can't have efficiency bonuses as that leads to wierd stacking issues,
    /// and they can't have extra output goods as they scale funny.
    pub fn add_effect(mut self, effect: InputEffect) -> Self {
        assert!(!self.optional, "Input effects can only be added to optional inputs.");
        assert!(!(matches!(effect, InputEffect::ExtraOutputGood(_, _)) && matches!(self.tag, InputType::Factor)), 
            "Factors cannot have extra output goods.");
        assert!(!(matches!(effect, InputEffect::Throughput(_)) && matches!(self.tag, InputType::Fixed)), 
            "Reduceables cannot have efficiency bonuses.");
        assert!(!(matches!(effect, InputEffect::ExtraOutputGood(_, _)) && matches!(self.tag, InputType::Fixed)), 
            "Reduceables cannot have extra output good.");
        self.effects.push(effect);
        self
    }
}

/// # Input Effect
/// 
/// Effects for satisfying an input.
#[derive(Debug, Clone)]
pub enum InputEffect {
    /// Increases both the number of input goods needed and output goods, keeping fixed 
    /// inputs the same.
    /// 
    /// This is calculated by reducing the amount of fixed goods needed for 
    /// the process, by the inverse of total throughput bonus.
    /// 
    /// IE. a 100% throughput bonus would reduce fixed inputs by 50%.
    Throughput(f64),
    /// The input, when satisfied produces this additional good as output per iteration
    /// completed. This should only be used for optional goods.
    ExtraOutputGood(usize, f64),
    /// Input reduction bonus. Should be between (1.0, 0.0), default cap at 90% 
    /// reduction for sanity reasons.
    InputBonus(f64),
    /// Output Bonus, should be greater than 0.0. No cap on result.
    OutputBonus(f64),
    /// Alters the growth rate of pops working in the job.
    /// Should be small value.
    GrowthEffect(f64),
}

impl InputEffect {
    /// Modify efficiency by value given. Value should be between (-1.0, 0.0).
    /// Efficiency is innately capped at -1.0 (cumulative) but may be capped further
    /// by the process.
    pub fn efficiency(value: f64) -> Self {
        assert!(value < 0.0 && value > -1.0, "Efficiency bonus must be between -1.0 and 0.0");
        InputEffect::Throughput(value)
    }

    /// The input, when satisfied produces this additional good as output per iteration
    /// completed. This should only be used for optional goods.
    pub fn extra_output_good(good: usize, amount: f64) -> Self {
        InputEffect::ExtraOutputGood(good, amount)
    }

    /// Input reduction bonus. Should be between (1.0, 0.0), default cap at 90% 
    /// reduction for sanity reasons.
    pub fn input_bonus(value: f64) -> Self {
        assert!(value < 1.0 && value > 0.0, "Input bonus must be between 0.0 and 1.0");
        InputEffect::InputBonus(value)
    }

    /// Output Bonus, should be greater than 0.0. No cap on result.
    pub fn output_bonus(value: f64) -> Self {
        assert!(value > 0.0, "Output bonus must be greater than 0.0");
        InputEffect::OutputBonus(value)
    }

    /// Alters the growth rate of pops working in the job.
    /// Should be small value.
    pub fn growth_effect(value: f64) -> Self {
        assert!(value < 1.0 && value > -1.0, "Growth effect must be between -1.0 and 1.0");
        InputEffect::GrowthEffect(value)
    }
}

/// # Input Type
/// 
/// Flags for inputs, which modify how processes are treated and any additional
/// effects that occur.
#[derive(Debug, Clone)]
pub enum InputType {
    /// Input, Standard Input, is destroyed without producing it's consumed output.
    /// 
    /// Reduced by Input bonuses, and increased by Throughput bonuses.
    Input,
    /// An Fixed input, mostly meant for time/Labor/skill. Never altered by bonuses, 
    /// and produces it's consumed output instead of being destroyed.
    Fixed,
    /// An input that is consumed and produces the output of the good instead of being
    /// destroyed. A useful shorthand for processes so we don't need to include every 
    /// consumed output.
    /// 
    /// Reduced by Input bonuses, and increased by Throughput bonuses.
    Consumed,
    /// A good that is used, but not consumed or destroyed by the process. It is returned
    /// by the process.
    /// 
    /// Reduced by Input bonuses, and increased by Throughput bonuses.
    Capital,
    /// A factor is an environmental input which ignores quantity, simply needing any 
    /// amount of the good to be present for it's effects.
    Factor
}

/// # Process Output
/// 
/// What is being output from a process.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    /// The good being output.
    pub good: usize,
    /// How much of that good per iteration.
    pub amount : f64,
    /// Any output tags attached which modify this output.
    pub tag: OutputType
}

/// # Output Type
/// 
/// Flags for outputs with additional rules and effects on them.
#[derive(Debug, Clone)]
pub enum OutputType {
    /// Standard output, benefits from output bonuses.
    Standard,
    /// Static, a non-standard output which does not gain output or efficiency
    /// boni. Used for stuff like Research, Culture, or skills.
    Static
}