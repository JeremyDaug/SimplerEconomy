use std::collections::HashMap;

use crate::{data::Data, desire::Desire, market::Market};

/// # Pop
/// 
/// A number of households grouped together into one unit.
#[derive(Debug, Clone)]
pub struct Pop {
    /// Unique Id of the pop.
    pub id: usize,
    /// how many households are in it. Should be whole numbers.
    pub size: f64,
    /// Desires are what the pop wants. The first is always item 0 Food,
    /// the last is always item 1 Leisure. The last item is infinitely desireable.
    /// TODO replace with culture.
    pub culture: usize,
    /// How many days worth of work a single household in the group does.
    pub efficiency: f64,
    /// What property the pop owns today.
    pub property: HashMap<usize, f64>,
    /// How much time is currently stored up in the pop.
    pub unused_time: f64,
}

impl Pop {
}