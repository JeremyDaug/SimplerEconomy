use std::collections::HashMap;

use crate::{data::Data, desire::Desire, market::Market, drow::DRow};

/// # Pop
/// 
/// A number of households grouped together into one unit.
#[derive(Debug, Clone)]
pub struct Pop {
    /// Unique Id of the pop.
    pub id: usize,
    /// The Id of the market our Pop is in
    pub market: usize,
    /// The ID of the firm the pop is working at.
    pub firm: usize,

    /// how many households are in it. Fractional values are used to stor population
    /// growth.
    pub size: f64,
    /// Demographic Breakdown of the pop.
    /// This allows us to consolidate multiple categories of pop into a singular
    /// pop group. Pops make have these enforced down into particular limitations,
    /// though this comes at the cost of increased difficulty.
    /// If different groups are in the same pop, then we assume they are being paid the same.
    pub demo_breakdown: DRow,
    /// How many days worth of work a single household in the group does.
    pub efficiency: f64,
    /// What property the pop owns today.
    pub property: HashMap<usize, f64>,
    /// How much time is currently stored up in the pop.
    pub unused_time: f64,
}

impl Pop {
}