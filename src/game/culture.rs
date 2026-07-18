use std::collections::HashMap;

use crate::game::desire::DemoDesire;

/// # Culture
/// 
/// The culture of a pop. Defines the common and luxury needs as well as secondary
/// benefits of the culture.
/// 
/// Culture is highly maleable to the player it's attached to.
#[derive(Debug, Clone)]
pub struct Culture {
    /// The unique ID of the culture.
    pub id: usize,
    /// The name of the culture.
    pub name: String,
    /// The ID of the state this is connected to. If a culture is not connected to any
    /// state, it is set to 0.
    pub state: usize,
    /// Demographic desires keyed by `DemoDesire.id` for O(1) lookup.
    /// 
    /// Tier lives on each `DemoDesire`; amounts are scaled for 1 household.
    pub desires: HashMap<usize, DemoDesire>,
}

impl Culture {
    /// # New
    /// 
    /// Creates a culture with the given id and name.
    /// State defaults to 0 (no state). Desires start empty.
    pub fn new(id: usize, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            state: 0,
            desires: HashMap::new(),
        }
    }

    /// Sets the culture's unique ID.
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    /// Sets the culture's display name.
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
            panic!("DemoDesire {id} already exists on culture {}.", self.id);
        }
        self
    }

    /// Finds a demo desire by id.
    pub fn find_desire(&self, desire_id: usize) -> Option<&DemoDesire> {
        self.desires.get(&desire_id)
    }
}
