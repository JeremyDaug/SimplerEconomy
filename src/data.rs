use std::collections::HashMap;

use crate::{good::Good, process::Process};

/// # Data
/// 
/// The main database of object information.
/// 
/// Currently only includes Goods and Processes.
pub struct Data {
    /// # Goods
    /// 
    /// All goods currently in existance.
    pub goods: HashMap<usize, Good>,
    /// # Processes
    /// 
    /// All currently available processes.
    pub processes: HashMap<usize, Process>,
    // TODO, nothing els should be needed here yet.
}