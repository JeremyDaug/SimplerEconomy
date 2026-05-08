use std::collections::HashMap;

use crate::game::{good::Good, process::Process};

/// # Factuals
/// 
/// This is where all the 'facts' of the world are stored, such as what goods and 
/// processes exist, these rarely, if ever change, and should even be mostly the same
/// between games.
/// 
/// This should include Goods, Processes, Game Rules, etc.
/// 
/// This is as compared to 'game state' which is the current state fo the world in a 
/// given game, such as the map, players, goods in the market, prices, etc.
#[derive(Debug, Clone)]
pub struct Factuals {
    goods: HashMap<usize, Good>,
    processes: HashMap<usize, Process>,
}

impl Factuals {
    /// # New
    /// 
    /// News up empty data.
    pub fn new() -> Self {
        Factuals {
            goods: HashMap::new(),
            processes: HashMap::new(),
        }
    }
}