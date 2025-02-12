use core::f64;
use std::collections::HashMap;

use crate::{data::Data, item::{Item, Product}};

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
    pub time: f64,
    /// The goods which are needed for the process, de facto.
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
    pub fn uses_input(mut self, input: ProcessInput) -> Self {
        self.inputs.push(input);
        self
    }

    /// # Uses Inputs
    /// 
    /// Fluent Input adder. Can add mulitple inputs at once.
    pub fn uses_inputs(mut self, inputs: Vec<ProcessInput>) -> Self {
        self.inputs.extend(inputs);
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
    /// the effective change of the process. May use a target amount to cap everything at.
    pub fn do_process(&self, goods: &HashMap<usize, f64>, data: &Data, target: Option<f64>) -> ProcessResults {
        // get our initial upper bound
        let upper_bound = if let Some(t) = target { t } else { f64::INFINITY };
        // get all the goods we need and have.
        let mut expending_goods = HashMap::new();
        for input in self.inputs.iter() {
            if let Product::Good(id) = input.product {
                expending_goods.insert(id, *goods.get(&id).unwrap_or(&0.0));
            } else if let Product::Class(id) = input.product {
                let members = data.get_class(id);
                // get all goods which are members.
                for (&good, &amt) in goods.iter()
                .filter(|(id, _)| members.contains(&id)) {
                    expending_goods.insert(good, amt);
                }
            }
        }
        todo!()
    }

    /// # Do Process
    /// 
    /// This is a simple process of taking in time and goods (and data of goods for good measure)
    /// 
    /// TODO: This needs testing.
    #[deprecated]
    pub fn do_process_old(&self, time: f64, goods: &HashMap<usize, f64>, data: &Data) -> ProcessResults {
        // first, get the lesser between iterations possible by time, and iterations possible by target.
        let mut target = time / self.time;
        // how many total goods we need per iteration
        let iter_good_cost = self.inputs.iter().map(|x| x.amount).sum::<f64>() - self.optional;
        // get the goods we need, we'll need these details later.
        // all the goods we will expend at the end of it.
        let mut expending_goods = HashMap::new();
        for (good, &quant) in goods.iter() {
            let expending = quant * target;
            expending_goods.insert(*good, expending.min(quant));
        }
        // how many goods we have available to expend
        let available = expending_goods.values().sum::<f64>();
        let mut target_result = available - target * iter_good_cost;
        if target_result < 0.0 {
            // If we don't have enough goods and free slots, reduce the target downwards
            target = target + target_result;
        }
        // with the target set, get expenditures.
        // update what will be expending
        for (input) in self.inputs.iter() {
            let expending = input.amount * target;
            expending_goods.insert(*good, expending.min(*expending_goods.get(good).unwrap_or(&0.0)));
        }
        // recalculate our target result base on the new target
        let available = expending_goods.values().sum::<f64>();
        target_result = available - target * iter_good_cost;

        // subtract any extra free slots, starting from the highest ID to the lowest.
        let mut remaining_frees = target_result.max(0.0).min(target * self.optional);
        while remaining_frees > 0.0 {
            if let Some(&highest) = expending_goods.keys().max() {
                if *expending_goods.get(&highest).unwrap() > 0.0 {
                    expending_goods.entry(highest)
                        .and_modify(|x| *x -= 1.0);
                    remaining_frees -= 1.0;
                } else {
                    expending_goods.remove(&highest);
                }
            }
        }

        // split expending goods into consumed and used.
        let mut used_goods = HashMap::new();
        let mut consumed_goods = HashMap::new();
        for (good, intype) in self.input_type.iter() {
            if !expending_goods.contains_key(good) {
                continue;// skip if we removed from our expenses.
            }
            match intype {
                InputType::Input => consumed_goods.insert(*good, *expending_goods.get(good).expect("Good not found? Make sure all input_types have a good and vice versa.")),
                InputType::Capital => used_goods.insert(*good, *expending_goods.get(good).expect("Good not found? Make sure all input_types have a good and vice versa.")),
            };
        }
        debug_assert_eq!(self.inputs.len(), self.input_type.len(), "Inputs and InputTypes must have the same length so no goods are missing.");

        let mut created = HashMap::new();
        let eff = self.efficiency();
        for (good, quant) in self.outputs.iter() {
            // add outputs, correctly multiplying if durability above 1.0 
            let good_info = data.goods.get(good).expect(format!("Good '{}' not found.", good).as_str());
            if good_info.durability > 1.0 {
                created.insert(*good, (quant * good_info.durability * target * eff).ceil());
            } else {
                created.insert(*good, (quant * target * eff).ceil());
            }
        }

        ProcessResults {
            iterations: target,
            consumed: consumed_goods,
            used: used_goods,
            time_used: self.time * target,
            created
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
#[derive(Debug, Clone)]
pub struct ProcessInput {
    /// The item (Want, Class of Good, or Good) is being used.
    pub product: Product,
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
    pub fn new(product: Product, amount: f64) -> Self {
        assert!(amount > 0.0, "Amount must be a Positive value.");
        Self {
            product,
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

pub struct ProcessResults {
    pub iterations: f64,
    /// Goods Destroyed by the process.
    pub consumed: HashMap<usize, f64>,
    /// Goods used but not destoyed by the process.
    pub used: HashMap<usize, f64>,
    /// The Items created by the process.
    pub created: HashMap<Item, f64>,
}

/// # Input Tag
/// 
/// Input tags are attached to ProcessInputs and define additional features that
/// apply only to that part of the process.
/// 
/// A Process Input with no tags is, de facto, destroyed by the process.
#[derive(Debug, Clone, Copy)]
pub enum InputTag {
    /// Consumed marks the input as resulting in the decay good being produced
    /// rather than destroyed.
    Consumed,
    /// Used marks the input as being 'used' not consumed or destroyed.
    Used,
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