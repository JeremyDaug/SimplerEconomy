use std::collections::HashMap;

use crate::game::nation::Nation;

/// # Players
///
/// The players of the game. Players are represented by nations.
#[derive(Debug)]
pub struct Players {
    pub players: HashMap<usize, Nation>,
}

impl Players {
    /// # Nation
    ///
    /// The nation stored under `id`.
    ///
    /// Panics if that nation is missing.
    pub fn nation(&self, id: usize) -> &Nation {
        self.players
            .get(&id)
            .unwrap_or_else(|| panic!("nation {id} is not in Players"))
    }

    /// # Nation Mut
    ///
    /// Mutable access to the nation stored under `id`.
    ///
    /// Panics if that nation is missing.
    pub fn nation_mut(&mut self, id: usize) -> &mut Nation {
        self.players
            .get_mut(&id)
            .unwrap_or_else(|| panic!("nation {id} is not in Players"))
    }
}
