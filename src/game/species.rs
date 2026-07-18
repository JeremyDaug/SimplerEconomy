use std::collections::HashMap;

use crate::game::desire::DemoDesire;

/// # Species
/// 
/// The species of a pop. Defines the basic needs required for life, and may later
/// include environment-dependent needs and household baseline modifiers.
/// 
/// By default this is human; additional species types may be added later.
#[derive(Debug, Clone)]
pub struct Species {
    /// The unique ID of the species.
    pub id: usize,
    /// The name of the species.
    pub name: String,
    /// The ID of the state this is connected to. If a species is not connected to any
    /// state, it is set to 0.
    pub state: usize,
    /// Demographic desires keyed by `DemoDesire.id` for O(1) lookup.
    /// 
    /// Tier lives on each `DemoDesire`; amounts are scaled for 1 household.
    pub desires: HashMap<usize, DemoDesire>,
}

impl Species {
    /// # New
    /// 
    /// Creates a species with the given id and name.
    /// State defaults to 0 (no state). Desires start empty.
    pub fn new(id: usize, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            state: 0,
            desires: HashMap::new(),
        }
    }

    /// Sets the species' unique ID.
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    /// Sets the species' display name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the connected state ID (0 means none).
    pub fn with_state(mut self, state: usize) -> Self {
        self.state = state;
        self
    }

    /// Adds a demographic desire keyed by its id.
    /// Debug-asserts that the desire's tier is 0, 1, or 2.
    /// Panics if a desire with the same id already exists.
    pub fn with_desire(mut self, desire: DemoDesire) -> Self {
        debug_assert!(desire.tier <= 2, "Desire tier must be 0, 1, or 2.");
        let id = desire.id;
        if self.desires.insert(id, desire).is_some() {
            panic!("DemoDesire {id} already exists on species {}.", self.id);
        }
        self
    }

    /// Finds a demo desire by id.
    pub fn find_desire(&self, desire_id: usize) -> Option<&DemoDesire> {
        self.desires.get(&desire_id)
    }
}
