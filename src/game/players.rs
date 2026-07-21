
use std::collections::HashMap;

use crate::game::{factuals::Factuals, state::State};

/// # Players
/// 
/// The players of the game. Players are represented by States.
#[derive(Debug)]
pub struct Players {
    pub players: HashMap<usize, State>
}
impl Players {
    pub(crate) fn decay_goods(&self, factuals: &Factuals) {
        todo!()
    }
}