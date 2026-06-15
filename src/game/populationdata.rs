use std::collections::HashMap;

use crate::game::{firm::Firm, institution::Institution, pop::Pop};

/// # Population Data
/// 
/// The Common storage for actors in the game.
#[derive(Debug)]
pub struct PopulationData {
    pub pops: HashMap<usize, Pop>,                   // Or Vec<Pop> / storage optimized
    pub firms: HashMap<usize, Firm>,
    pub institutions: HashMap<usize, Institution>,   // States, governments, etc.
    // Could hold spatial indices or tile->agent mappings for quick lookup
}

impl PopulationData {
    pub fn new() -> Self {
        Self {
            pops: HashMap::new(),
            firms: HashMap::new(),
            institutions: HashMap::new(),
        }
    }
}