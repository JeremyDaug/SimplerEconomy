use std::collections::HashMap;

use bevy::prelude::*;
use crate::game::factuals::Factuals; use crate::game::firm::Firm;
use crate::game::institution::Institution;
// adjust paths as needed
use crate::game::map::Map; 
use crate::game::mapdata::MapData;
use crate::game::market::Market;
use crate::game::players::Players;
// etc.
use crate::game::pop::Pop;
use crate::game::populationdata::PopulationData;
use crate::game::state::State;

/// # Play State
/// 
/// This is the highest level of our game, overseeing a the state of the current game.
/// 
/// It stores all active data, and acts as the fulcrum by which Bevy interacts with the
/// more complex state of the game. 
#[derive(Resource, Debug)]
pub struct PlayState {
    pub factuals: Factuals,           // Highly static, Arc or & for sharing/multithreading
    pub map_data: MapData,            // Environmental + spatial
    pub population: PopulationData,   // Active actors (Pops, Firms, Institutions)
    pub players: Players,             // Human/AI players, resources, etc.

    // Optional: turn metadata, game clock, etc.
    pub turn: u64,
    pub is_paused: bool,
    // ... any other global flags
}

// In main Bevy app setup:
fn setup_play_state(mut commands: Commands) {
    let mut play_state = PlayState {
        factuals: Factuals::new(),
        map_data: todo!(),
        population: todo!(),
        players: todo!(),
        turn: todo!(),
        is_paused: todo!(),
    };
    // Load factuals, generate map, init population/players...
    commands.insert_resource(play_state);
}