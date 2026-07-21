use std::collections::HashMap;

use crate::game::{factuals::Factuals, firm::Firm, institution::Institution, pop::Pop};

/// # Actors
/// 
/// Common storage for active game actors (pops, firms, institutions, …).
/// Markets and other systems should hold membership ids / indexes, not duplicate
/// ownership of these entities.
#[derive(Debug)]
pub struct Actors {
    pub pops: HashMap<usize, Pop>,
    pub firms: HashMap<usize, Firm>,
    pub institutions: HashMap<usize, Institution>,
    // Could hold spatial indices or tile->agent mappings for quick lookup
}

impl Actors {
    pub fn new() -> Self {
        Self {
            pops: HashMap::new(),
            firms: HashMap::new(),
            institutions: HashMap::new(),
        }
    }
    
    pub(crate) fn decay_goods(&self, factuals: &Factuals) {
        todo!("Go through all actor property and decay their goods.")
    }
}
