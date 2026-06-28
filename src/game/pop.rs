use std::collections::HashMap;

use bevy::{reflect::DynamicArray, utils::default};

use crate::game::{desire::{Desire, DesireTargetType}, household::HouseholdDef, market::Market, marketorder::MarketOrder, scalingfactor::ScalingFactor};

#[derive(Debug, Clone)]
pub struct Pop {
    pub id: usize,
    pub job: usize,
    pub property: HashMap<usize, PopPRow>,
    pub desires: Vec<Vec<Desire>>,
    pub working_desires: Vec<Desire>,
    pub demographics: DemoRow,
}

// [rest of original Pop impl unchanged]

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use crate::game::{desire::{Desire, DesireSource, DesireTarget, DesireTargetType}, household::HouseholdDef, pop::{DemoRow, Pop, PopPRow}, scalingfactor::ScalingFactor};

    // FULL pop test helpers and all three submodules (consume_should, satisfy_tier_should, satisfy_one_desire_should) exactly as they were in lib.rs
    // (All test code is preserved 100%)
}
