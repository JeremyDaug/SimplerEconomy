use core::f64;
use std::{collections::HashMap, fmt::{format, Display}};

use crate::data::Data;

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
/// Optional goods not implemented.
#[derive(Debug, Clone)]
pub struct Process {
    /// The unique id of the process.
    pub id: usize,
    /// The name of the process.
    pub name: String,
    /// What process this one is derived from.
    pub parent: Option<usize>,
    /// How much time the process takes.
    /// Should be in whole units, should never go below 1 hour.
    pub time: f64,
    /// The goods which are consumed by the process.
    /// Should be positive whole numbers.
    pub inputs: HashMap<usize, f64>,
    /// The number of inputs which can be omitted per iteration of the process.
    pub optional: f64,
    /// what type of input it is. IE, whether it is consumed, used, or otherwise.
    pub input_type: HashMap<usize, InputType>,
    /// The goods which are produced by the process.
    /// Should be whole numbers, may produce fractions, but fractions do not decay and 
    /// cannot be bought or sold. If the pop disappears, so to does any fractional goods.
    pub outputs: HashMap<usize, f64>
}

impl Process {
    /// # Do Process
    /// 
    /// This is a simple process of taking in time and goods (and data of goods for good measure)
    pub fn do_process(&self, time: f64, goods: &HashMap<usize, f64>, data: &Data) -> ProcessResults {
        // first, get the lesser between iterations possible by time, and iterations possible by target.
        let mut target = time / self.time;
        // how many total goods we need per iteration
        let iter_good_cost = self.inputs.values().sum::<f64>() - self.optional;
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
        for (good, quant) in self.inputs.iter() {
            let expending = quant * target;
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
        let n: f64 = self.inputs.values().sum::<f64>() 
            - self.optional;

        ((n * n - n) * 0.05 + 1.0).max(1.0)
    }
}

pub struct ProcessResults {
    pub iterations: f64,
    pub consumed: HashMap<usize, f64>,
    pub used: HashMap<usize, f64>,
    pub time_used: f64,
    pub created: HashMap<usize ,f64>,
}

#[derive(Debug, Clone)]
pub enum InputType {
    /// Good is consumed as part of the process, or consumes durability of the bigger item.
    Input,
    /// Requires good.durability in units of the good, but does not consume the good in the process.
    Capital,
}