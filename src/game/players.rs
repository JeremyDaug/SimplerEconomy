
use std::collections::HashMap;

use crate::game::state::State;

/// # Players
/// 
/// The players of the game. Players are represented by States.
#[derive(Debug)]
pub struct Players {
    pub players: HashMap<usize, State>
}