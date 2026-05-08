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

/// # Input Effect
/// 
/// Effects for satisfying an input.
#[derive(Debug, Clone)]
pub enum InputEffect {
    /// Modify efficiency by value given. Value should be between (-1.0, 0.0).
    /// Efficiency is innately capped at -1.0 (cumulative) but may be capped further
    /// by the process.
    Efficiency(f64),
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

/// # Input Type
/// 
/// Flags for inputs, which modify how processes are treated and any additional
/// effects that occur.
#[derive(Debug, Clone)]
pub enum InputType {
    /// Input, Standard Input, is destroyed without producing it's consumed output.
    Input,
    /// An input processes, mostly meant for time/Labor. Efficiency reduces these
    /// factors.
    Reduceables,
    /// An input that is consumed and produces the output of the good instead of being
    /// destroyed. A useful shorthand for processes so we don't need to include every 
    /// consumed output.
    Consumed,
    /// A good that is used, but not consumed or destroyed by the process. It is returned
    /// by the process.
    Capital,
    /// A factor is an environmental input which ignores quantity, simply needing any 
    /// amount to be input to get the bonuses. 
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