use core::f64;
use std::{cmp, collections::HashMap, hash::Hash};

use itertools::Itertools;

use crate::{data::Data, item::{Item, Product}, markethistory::MarketHistory};

/// # Process
/// 
/// Process transforms time, stock, and capital into outputs.
/// 
/// Processes have a standard form and efficiency gain.
/// 
/// Goods which don't decay cannot be capital. Goods which always decay must be stock.
/// 
/// Processes always take at least 1 hour of time. All items are whole units.
/// 
/// Each stock and capital increases the efficiency of the output by N * 0.1 + (N-1) * 0.05.
/// Where N is the total number of stock and capital - the optional stock and capital.
/// 
/// ## Mass Conservation Rules
/// 
/// A soft rule that will/should be added is that the mass being destroyed by the process should always be
/// equal to the mass coming out of it. 
/// 
/// Ways to enforce this are
/// 
/// 1. Tagless inputs (goods destroyed by the process) cannot be reduced via optional effects.
/// 2. The number of Optional inputs cannot cut into Tagless inputs.
#[derive(Debug, Clone)]
pub struct Process {
    /// The unique id of the process.
    pub id: usize,
    /// The name of the process.
    pub name: String,
    /// The variant name of the process. Should be unique when combined with primary name.
    pub variant_name: String,
    /// The complexity value of the process. This is a calculated value.
    pub complexity: f64,
    /// What process this one is derived from.
    pub parent: Option<usize>,
    /// How much time the process takes if done sequentially.
    /// 
    /// This is not used internally, instead it is used externally with scheduling.
    pub time: f64,
    /// The goods which are needed for the process, de facto.
    /// 
    /// These come with a particular sorted order.
    /// 
    /// TODO: VvvV This VvvV
    /// 
    /// First by tag, None -> Enum Order
    /// 
    /// Then within those tag groups by good ID order.
    /// 
    /// ## Note: 
    /// 
    /// When Classes and Wants added back, they will also need a specific order 
    /// for them (likely Good -> Class -> Want).
    pub inputs: Vec<ProcessInput>,
    /// The number of inputs which can be omitted per iteration of the process.
    pub optional: f64,
    /// The goods which are produced by the process.
    /// Should be whole numbers, may produce fractions, but fractions do not decay and 
    /// cannot be bought or sold. If the pop disappears, so to does any fractional goods.
    /// Goods that are Consumed or Used are not included here.
    pub outputs: Vec<ProcessOutput>
}

impl Process {
    /// # New
    /// 
    /// Creates a new, empty process. 
    /// Does not check for unique id or name.
    pub fn new(id: usize, name: String, variant_name: String) -> Self {
        Process {
            id,
            name,
            variant_name,
            complexity: 0.0,
            parent: None,
            time: 0.0,
            inputs: vec![],
            optional: 0.0,
            outputs: vec![],
        }
    }

    /// # Has Output
    /// 
    /// Fluent Output adder, only single output to add.
    pub fn has_output(mut self, output: ProcessOutput) -> Self {
        self.outputs.push(output);
        self
    }

    /// # Outputs
    /// 
    /// Fluent Output(s) adder, can add multiple outputs.
    pub fn has_outputs(mut self, outputs: Vec<ProcessOutput>) -> Self {
        self.outputs.extend(outputs);
        self
    }

    /// # With Optional
    /// 
    /// Fluent Optional Setter.
    pub fn with_optionals(mut self, optional: f64) -> Self {
        assert!(optional >= 0.0, "Optional cannot be negative.");
        self.optional = optional;
        self
    }

    /// # Uses Inputs
    /// 
    /// Fluent Input adder.
    /// 
    /// Inserts such that the input is properly sorted.
    /// 
    /// TODO: Test this actually works.
    pub fn uses_input(mut self, input: ProcessInput) -> Self {
        self.inputs.push(input);
        let mut current = self.inputs.len();
        loop { // sort downwards until it is both placed proprely tag wise and good id wise.
            if current == 0 {
                // if we got down to 0, then leave as we're already sorted.
                break;
            }
            let cur_tag = self.inputs.get(current).unwrap().tag;
            let next_tag = self.inputs.get(current-1).unwrap().tag;
            if next_tag.is_some() && cur_tag.is_none() { // if next down has a a tag and we don't swap.
                self.inputs.swap(current, current-1);
                current -= 1;
                continue;
            }
            if let (Some(next), Some(curr)) = (next_tag, cur_tag) {
                // if both have a tag, swap when one should be before the other.
                if next > curr {
                    self.inputs.swap(current, current-1);
                    current -= 1;
                    continue;
                }
                if next < curr {
                    // If the next down is the next tag down, then it's in it's place.
                    break;
                }
            }
            // if tags are the same, reorganize by good ID
            let curr_id = self.inputs.get(current).unwrap().good;
            let next_id = self.inputs.get(current-1).unwrap().good;
            if next_id > curr_id {
                self.inputs.swap(current, current-1);
                current -= 1;
                continue;
            }
            // Tag is properly placed, and id is proprely placed, must be the end.
            break;
        }
        self
    }

    /// # Uses Inputs
    /// 
    /// Fluent Input adder. Can add mulitple inputs at once.
    /// 
    /// Inserts them in the order defined above.
    pub fn uses_inputs(mut self, inputs: Vec<ProcessInput>) -> Self {
        for input in inputs {
            self = self.uses_input(input);
        }
        self
    }

    /// # With Time
    /// 
    /// Fluent Time Setter. Consumes Orinigal.
    /// 
    /// # Panics
    /// 
    /// Time must be Non-Negative.
    pub fn with_time(mut self, time: f64) -> Self {
        assert!(time >= 0.0, "Time must be non-negative.");
        self.time = time;
        self
    }

    /// # Has Parent
    /// 
    /// Fluent Parent Setter. Consumes original.
    pub fn has_parent(mut self, parent: usize) -> Self {
        self.parent = Some(parent);
        self
    }

    /// # Do Process
    /// 
    /// Do process takes in the goods available and returns 
    /// the effective change of the process. 
    /// 
    /// A target number of processes is always needed.
    pub fn do_process(&self, goods: &HashMap<usize, f64>, data: &Data, target: f64, market_history: &MarketHistory) -> ProcessResults {
        let mut target = target;
        // Get how many we need in an iteration
        let iter_good_cost = self.inputs.iter().map(|x| x.amount).sum::<f64>() - self.optional;
        // get all the goods we need
        let mut expense_targets = HashMap::new();
        for input in self.inputs.iter() {
            expense_targets
                .entry(input.good)
                .and_modify(|x| *x += target * input.amount)
                .or_insert(target * input.amount);
        }
        
        // next get the goods we can expend (capping at our targets).
        let mut expending_goods = HashMap::new();
        for (good, amt) in goods.iter()
        .filter(|(good, _)| expense_targets.contains_key(good)) {
            expending_goods.insert(good, amt.min(*expense_targets.get(good).unwrap()));
        }
        
        let available = expending_goods.values().sum::<f64>();
        let mut target_result = available - target * iter_good_cost;
        if target_result < 0.0 { // if we don't have enough goods and free slots, reduce the target downward.
            target = target + target_result;
        }
        if target == 0.0 {
            return ProcessResults::new();
        }
        
        // with target set, get actual expenditures.
        for input in self.inputs.iter() {
            let expending = input.amount * target;
            expending_goods.insert(&input.good, 
                expending.min(*expending_goods.get(&input.good).unwrap_or(&0.0))
            );
        }

        // recalculate our target result based on the new target
        let available = expending_goods.values().sum::<f64>();
        target_result = available - target * iter_good_cost;

        // subtract any extra free slots, starting from the most expensive good and going down.
        let mut remaining_frees = target_result.max(0.0).min(target * self.optional);
        while remaining_frees > 0.0 {
            // Get the most expensive good and remove as many units as possible.
            let costliest = expending_goods.iter()
                .sorted_by(|a, b| {
                    a.1.partial_cmp(b.1).unwrap()
                }).last();
            if let Some((&good, &amt)) = costliest {
                // How many we can remove.
                let remove = amt.min(remaining_frees);
                remaining_frees -= remove; // remove from free.
                // remove from expending goods.
                let update = expending_goods.remove(good).unwrap() - remove;
                if update > 0.0 { // if not reduced to zero, add back in.
                    expending_goods.insert(good, update);
                }
            } else { // if no costliest, then we probably have a problem.
                unreachable!("Somehow no goods in Expending goods.");
            }
        }

        // with actual expenses gotten, begin adding to the results
        let mut consumed = HashMap::new();
        let mut used = HashMap::new();
        let mut created = HashMap::new();
        for input in self.inputs.iter() {
            // go through the inputs and deal up to the target amount of that good for our current iterations.
            // Any shortfall is 
            if let Some(tag) = input.tag {
                match tag {
                    InputTag::Consumed => {

                    },
                    InputTag::Used => {

                    },
                }
            } else { // Consumed input.

            }
        }

        ProcessResults {
            iterations: target,
            consumed,
            used,
            created,
        }
    }

    /// How much extra efficiency is gained due to the amount of stock and
    /// capital needed.
    /// 
    /// Where N = the number of goods needed - the optional good value.
    /// 
    /// returns N^2 / 2.0 * 0.05
    pub fn efficiency(&self) -> f64 {
        let n: f64 = self.inputs.iter().map(|x| x.amount).sum::<f64>() 
            - self.optional;

        ((n * n - n) * 0.05 + 1.0).max(1.0)
    }
}

/// The input information for a process.
/// 
/// Currently, only Goods are accepted by processes.
/// 
/// Eventually I would like to expand this to include Classes and Wants.
#[derive(Debug, Clone)]
pub struct ProcessInput {
    /// The Good that is being used.
    pub good: usize,
    /// The number of units needed.
    pub amount: f64,
    /// Additional information which modifies the input.
    pub tag: Option<InputTag>,
}

impl ProcessInput {
    /// # New
    /// 
    /// Creates a new (destroyed) input.
    /// 
    /// # Panics
    /// 
    /// Amount must be Positive.
    pub fn new(good: usize, amount: f64) -> Self {
        assert!(amount > 0.0, "Amount must be a Positive value.");
        Self {
            good,
            amount,
            tag: None,
        }
    }

    /// # With Tag
    /// 
    /// Consuming setter for Tag.
    pub fn with_tag(mut self, tag: InputTag) -> Self {
        self.tag = Some(tag);
        self
    }
}

/// # Process Output
/// 
/// The information about process outputs.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    /// The item being output.
    pub item: Item,
    /// The amount of that output made.
    pub amount: f64,
    /// The additional effects when a hte output is made.
    pub tags: Vec<OutputTag>,
}

impl ProcessOutput {
    pub fn new(item: Item, amount: f64) -> Self {
        assert!(amount > 0.0, "Amount must be positive.");
        Self {
            item,
            amount,
            tags: vec![],
        }
    }

    pub fn with_tag(mut self, tag: OutputTag) -> Self {
        self.tags.push(tag);
        self
    }

    pub fn with_tags(mut self, tags: Vec<OutputTag>) -> Self {
        self.tags.extend(tags);
        self
    }
}

/// # Process Results
/// 
/// The results of completing a process.
pub struct ProcessResults {
    pub iterations: f64,
    /// Goods Destroyed by the process.
    pub consumed: HashMap<usize, f64>,
    /// Goods used but not destoyed by the process.
    pub used: HashMap<usize, f64>,
    /// The Items created by the process.
    pub created: HashMap<Item, f64>,
}

impl ProcessResults {
    fn new() -> Self {
        Self { 
            iterations: 0.0,
            consumed: HashMap::new(),
            used: HashMap::new(),
            created: HashMap::new()
        }
    }
}

/// # Input Tag
/// 
/// Input tags are attached to ProcessInputs and define additional features that
/// apply only to that part of the process.
/// 
/// A Process Input with no tags is, de facto, destroyed by the process.
/// 
/// ## Possible additions
/// 
/// - Specific Optional
///     - Specific Optional would be an input which can be specifically excluded by 
///         the process. This input being included should offer some kind of addiitonal 
///         benefit, possibly a reduction in process time and possibly reduction in
///         'massless' inputs like work time (good 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InputTag {
    /// Used marks the input as being 'used' not consumed or destroyed.
    Used,
    /// Consumed marks the input as resulting in the decay good being produced
    /// rather than destroyed.
    Consumed,
}

/// # Output Tags
/// 
/// Adds special information to the outputs of a process part.
/// 
/// Currently used for sanity checking outputs.
#[derive(Debug, Clone, Copy)]
pub enum OutputTag {
    ConsumedOutput,
    UsedOutput
}