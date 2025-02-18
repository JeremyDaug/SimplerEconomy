use core::f64;
use std::{collections::HashMap, fmt::Display};

use itertools::Itertools;

use crate::{data::Data, item::Item, markethistory::MarketHistory};

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
    pub outputs: Vec<ProcessOutput>,
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
    ///
    /// # Panics
    ///
    /// Currently the system cannot output a Item::Class(), so if it recieves it it panics.
    pub fn has_output(mut self, output: ProcessOutput) -> Self {
        assert!(!output.item.is_class(), "Output Item cannot be a class.");
        self.outputs.push(output);
        self
    }

    /// # Outputs
    ///
    /// Fluent Output(s) adder, can add multiple outputs.
    ///
    /// # Panics
    ///
    /// Currently the system cannot output a Item::Class(), so if it recieves it it panics.
    pub fn has_outputs(mut self, outputs: Vec<ProcessOutput>) -> Self {
        for output in outputs {
            self = self.has_output(output);
        }
        self
    }

    /// # With Optional
    ///
    /// Fluent Optional Setter.
    pub fn with_optionals(mut self, optional: f64) -> Self {
        assert!(optional >= 0.0, "Optional cannot be negative.");
        //assert!((optional - optional.floor()) > 0.0, "Optional must be a whole value!");
        self.optional = optional;
        self
    }

    /// # Uses Inputs
    ///
    /// Fluent Input adder.
    ///
    /// Inserts such that the input is properly sorted.
    pub fn uses_input(mut self, input: ProcessInput) -> Self {
        self.inputs.push(input);
        let mut current = self.inputs.len()-1;
        loop { // sort downwards until it is both placed proprely tag wise and good id wise.
            if current == 0 {
                // if we got down to 0, then leave as we're already sorted.
                break;
            }
            let curr_tag = self.inputs.get(current).unwrap().tag;
            let next_tag = self.inputs.get(current-1).unwrap().tag;
            // if both have a tag, swap when one should be before the other.
            if next_tag > curr_tag {
                self.inputs.swap(current, current-1);
                current -= 1;
                continue;
            }
            if next_tag < curr_tag {
                // If the next down is the next tag down, then it's in it's place.
                break;
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
    ///
    /// TODO: Add in code to allow for normal inputs to be excluded (Unless they are massless) to help enforce conservation of mass.
    /// TODO: Add in the ability to accept class or want inputs.
    pub fn do_process(&self, goods: &HashMap<usize, f64>, data: &Data, target: f64, market_history: &MarketHistory) -> ProcessResults {
        let mut target = target;
        // get base iteration goods and base iteration cost.
        let mut iter_good_cost = 0.0;
        let mut base_goods = HashMap::new();
        for input in self.inputs.iter() {
            iter_good_cost += input.amount;
            base_goods.entry(input.good)
                .and_modify(|x| *x += input.amount)
                .or_insert(input.amount);
        }
        // finalize base good cost.
        let iter_good_cost = iter_good_cost - self.optional;

        // get our real target to the best of our ability.
        let mut input_goods = HashMap::new();
        let mut total_available = 0.0;
        let mut unused_free_slots = 0.0;
        // get our currently available inputs
        for (good, amt) in base_goods.iter() {
            let available = (amt * target).min(*goods.get(good).unwrap_or(&0.0));
            input_goods.insert(good, available);
            total_available += available;
        }
        // get how many free slots we have available.
        unused_free_slots = total_available - iter_good_cost * target;
        if unused_free_slots < 0.0 {
            // if negative, try a 'near zero' value, and see if we can do any at all.
            // Get the lowest possible non-zero step we can.
            let mut iters = vec![];
            for (good, amt) in base_goods.iter() {
                iters.push(goods.get(good).unwrap_or(&0.0) / *amt);
            }
            let lower_bound = iters.iter().filter(|&&x| x > 0.0)
                .fold(f64::INFINITY, |a, &b| a.min(b));
            // Calculate the value of our current lower bound.
            let mut lower_bound_inputs = HashMap::new();
            let mut lower_bound_available = 0.0;
            for (good, amt) in base_goods.iter() {
                let cur = amt * lower_bound;
                lower_bound_inputs.insert(good, cur);
                lower_bound_available += cur;
            }
            // Get how many optionals we have available at our lower bound.
            let lower_bound_frees = lower_bound_available 
                - iter_good_cost * lower_bound;
            if lower_bound_frees < 0.0 { 
                // if lower bound is still negative, return empty, 
                // we *Cannot* find an intersection.
                return  ProcessResults::new();
            } else if lower_bound_frees == 0.0 { 
                // if lower bound is equal to zero, then we have already hit 
                // the highest possible value that can be done.
                // set our target to the lower bound and update our data
                target = lower_bound;
                input_goods.clear();
                unused_free_slots = -self.optional * target;
                for (good, amt) in base_goods.iter() {
                    let curr = goods.get(good).unwrap_or(&0.0).min(amt * target);
                    input_goods.insert(good, curr);
                    unused_free_slots += curr;
                } // then leave to finish the process.
            } else {
                // if there is a solution between our lower bound and current target, find it.
                loop {
                    // Estimate the new target by finding the intersection between 
                    // the current estimates and the iter_cost line.
                    break;
                }
            }
        }

        // subtract any extra free slots, starting from the most expensive good and going down.
        let mut remaining_frees = unused_free_slots.max(0.0).min(target * self.optional);
        println!("Available: {}", total_available);
        println!("target_compare: {}", unused_free_slots);
        println!("Remaining frees: {}", remaining_frees);
        while remaining_frees > 0.0 {
            // Get the most expensive good and remove as many units as possible.
            let costliest = input_goods.iter()
                .sorted_by(|(&a, _), (&b, _)| {
                    {
                        let a_price = market_history.get_record(*a).price;
                        let b_price = market_history.get_record(*b).price;
                        a_price.partial_cmp(&b_price).expect("Prices not set right.")
                    }
                }).last();
            if let Some((&good, &amt)) = costliest {
                println!("Costliest Good: {}", good);
                // How many we can remove.
                let remove = amt.min(remaining_frees);
                remaining_frees -= remove; // remove from free.
                // remove from expending goods.
                let update = input_goods.remove(&good).unwrap() - remove;
                if update > 0.0 { // if not reduced to zero, add back in.
                    input_goods.insert(good, update);
                }
            } else { // if no costliest, then we probably have a problem.
                unreachable!("Somehow no goods in Expending goods.");
            }
        }

        // with actual expenses gotten, begin adding to the results
        let mut consumed = HashMap::new();
        let mut used = HashMap::new();
        let mut created = HashMap::new();
        // All inputs
        for input in self.inputs.iter() {
            // get how much we're removing, capped at what's actually in expending goods.
            let remove = (input.amount * target).min(*input_goods.get(&input.good).unwrap_or(&0.0));
            // always remove from expending
            input_goods.entry(&input.good)
                .and_modify(|x| *x -= remove);
            // Any shortfall is ignored.
            match input.tag {
                InputTag::None => { // Normal Consumption
                    consumed.entry(input.good)
                        .and_modify(|x| *x += remove)
                        .or_insert(remove);
                },
                InputTag::Used => { // Used
                    used.entry(input.good)
                        .and_modify(|x| *x += remove)
                        .or_insert(remove);
                },
                InputTag::Consumed => { // consumed, put decay into output.
                    consumed.entry(input.good)
                        .and_modify(|x| *x += remove)
                        .or_insert(remove);
                    let good_info = data.get_good(input.good);
                    if let Some((decay_good, rate)) = good_info.decays_to {
                        created.entry(Item::Good(decay_good))
                            .and_modify(|x| *x += rate * remove)
                            .or_insert(rate * remove);
                    }
                },
            }
        }

        // Then outputs.
        for output in self.outputs.iter() {
            let add = output.amount * target;
            created.entry(output.item)
                .and_modify(|x| *x += add)
                .or_insert(add);
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
    pub tag: InputTag,
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
        //assert!((amount - amount.floor()) > 0.0, "Amount must be an integer value.");
        Self {
            good,
            amount,
            tag: InputTag::None,
        }
    }

    /// # With Tag
    ///
    /// Consuming setter for Tag.
    pub fn with_tag(mut self, tag: InputTag) -> Self {
        self.tag = tag;
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
    /// No tag, a place holder so we don't need to use Optional<InputTag>.
    None,
    /// Used marks the input as being 'used' not consumed or destroyed.
    Used,
    /// Consumed marks the input as resulting in the decay good being produced
    /// rather than destroyed.
    Consumed,
}

impl Display for InputTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputTag::None => write!(f, "None"),
            InputTag::Used => write!(f, "Used"),
            InputTag::Consumed => write!(f, "Consumed"),
        }
    }
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