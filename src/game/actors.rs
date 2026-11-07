use std::collections::HashMap;

use crate::game::actor::Actor;
use crate::game::{firm::Firm, institution::Institution, pop::Pop};

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
    /// # New
    ///
    /// Empty pop, firm, and institution maps.
    pub fn new() -> Self {
        Self {
            pops: HashMap::new(),
            firms: HashMap::new(),
            institutions: HashMap::new(),
        }
    }

    /// # Pop
    ///
    /// The pop stored under `id`.
    ///
    /// Panics if that pop is missing.
    pub fn pop(&self, id: usize) -> &Pop {
        self.pops
            .get(&id)
            .unwrap_or_else(|| panic!("pop {id} is not in Actors"))
    }

    /// # Pop Mut
    ///
    /// Mutable access to the pop stored under `id`.
    ///
    /// Panics if that pop is missing.
    pub fn pop_mut(&mut self, id: usize) -> &mut Pop {
        self.pops
            .get_mut(&id)
            .unwrap_or_else(|| panic!("pop {id} is not in Actors"))
    }

    /// # Firm
    ///
    /// The firm stored under `id`.
    ///
    /// Panics if that firm is missing.
    pub fn firm(&self, id: usize) -> &Firm {
        self.firms
            .get(&id)
            .unwrap_or_else(|| panic!("firm {id} is not in Actors"))
    }

    /// # Firm Mut
    ///
    /// Mutable access to the firm stored under `id`.
    ///
    /// Panics if that firm is missing.
    pub fn firm_mut(&mut self, id: usize) -> &mut Firm {
        self.firms
            .get_mut(&id)
            .unwrap_or_else(|| panic!("firm {id} is not in Actors"))
    }

    /// # Institution
    ///
    /// The institution stored under `id`.
    ///
    /// Panics if that institution is missing.
    pub fn institution(&self, id: usize) -> &Institution {
        self.institutions
            .get(&id)
            .unwrap_or_else(|| panic!("institution {id} is not in Actors"))
    }

    /// # Institution Mut
    ///
    /// Mutable access to the institution stored under `id`.
    ///
    /// Panics if that institution is missing.
    pub fn institution_mut(&mut self, id: usize) -> &mut Institution {
        self.institutions
            .get_mut(&id)
            .unwrap_or_else(|| panic!("institution {id} is not in Actors"))
    }
}
