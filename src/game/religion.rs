use std::collections::HashMap;

use crate::game::desire::DemoDesire;

/// # Religion
/// 
/// The religion of a pop. Defines additional common and luxury needs as well as
/// secondary benefits and influence the player can develop.
/// 
/// Religion is maleable to the player it's attached to, similar to Culture.
#[derive(Debug, Clone)]
pub struct Religion {
    /// The unique ID of the religion.
    pub id: usize,
    /// The name of the religion.
    pub name: String,
    /// The ID of the state this is connected to. If a religion is not connected to any
    /// state, it is set to 0.
    pub state: usize,
    /// The desires of the religion, organized by tier (Basic, Common, Luxury).
    /// 
    /// Each tier is a map from `DemoDesire.id` → desire for O(1) lookup. Amounts are
    /// scaled to the needs of 1 household.
    pub desires: Vec<HashMap<usize, DemoDesire>>,
}

impl Religion {
    /// # New
    /// 
    /// Creates a religion with the given id and name.
    /// State defaults to 0 (no state). Desires start as three empty tier maps.
    pub fn new(id: usize, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            state: 0,
            desires: vec![HashMap::new(), HashMap::new(), HashMap::new()],
        }
    }

    /// Sets the religion's unique ID.
    pub fn with_id(mut self, id: usize) -> Self {
        self.id = id;
        self
    }

    /// Sets the religion's display name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Sets the connected state ID (0 means none).
    pub fn with_state(mut self, state: usize) -> Self {
        self.state = state;
        self
    }

    /// Adds a demographic desire into the tier matching `desire.tier`.
    /// Debug-asserts that the desire's tier is 0, 1, or 2.
    /// Panics if a desire with the same id already exists in that tier.
    pub fn with_desire(mut self, desire: DemoDesire) -> Self {
        debug_assert!(desire.tier <= 2, "Desire tier must be 0, 1, or 2.");
        let id = desire.id;
        if self.desires[desire.tier].insert(id, desire).is_some() {
            panic!("DemoDesire {id} already exists on religion {}.", self.id);
        }
        self
    }

    /// Finds a demo desire by id across all tiers (O(1) per tier).
    pub fn find_desire(&self, desire_id: usize) -> Option<&DemoDesire> {
        self.desires.iter().find_map(|tier| tier.get(&desire_id))
    }
}
