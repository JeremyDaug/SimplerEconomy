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

impl PlayState {
    pub fn advance_turn(&mut self) {
        self.turn += 1;

        /*
        1. Add turn start resources. Like time and environmental resources.
        2. Environment random effects and results which can interfere with plans
        3. Player actions, all run and applied simultaneously and before anything 
           else could interfere with player actions. This does not include player
           market purchasing, wich is saved for the market proper. This includes
           movement of units, but not map alterations.
        4. Apply new player bonuses, including creating new actors, or applying new
           bonuses.
        5. Intra-Market trading day. This is broken up and organized by group turns.
           The default order is Player(state), Institutions, Firms, and Pops.
           Firms and pops are always in this order, giving firms advantage in gathering
           and consolidating resources for merchants. Institutions may put themselves
           before, between, or after Pops, defined by the institution. States may break
           their market actions and put theme anywhere in this order as well. For example
           a player may put construction and military good purchasing in the front of
           the order, while putting welfare purchasing in the rear.
        6. Inter-market trade. Trade between markets will get it's turn and be 
           analyzed. Any in-progress trades take their turn/movement, the 
           results/effects are analyzed for possible new trades, and new trades are 
           kicked off.
        7. Production and Non-Player Planning. After all trade is done for the day,
           firms, institutions, and the like, look at their prior success, compare
           try to predict what will occur tomorrow, create/modify their production 
           plans, then do their production plans.
        8. Pop Consumption.
        9. Pop Growth/Decline.
        10.Pop migration.
        11.Record Keeping.
        12.Good Decay.
        */
    }
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