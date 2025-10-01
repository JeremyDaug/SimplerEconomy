use std::collections::HashMap;

use crate::{market::Market, pop::Pop};



/// # World
/// 
/// World is the top level manager of everything.
pub struct World {
    /// All Markets in the world.
    pub markets: HashMap<usize, Market>,
    /// All pops in the world currently.
    pub pops: HashMap<usize, Pop>,
}