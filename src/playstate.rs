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
use crate::game::actors::Actors;
use crate::game::pop::Pop;
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
    pub actors: Actors,               // Active pops, firms, institutions, …
    pub players: Players,             // Human/AI players, resources, etc.

    // Optional: turn metadata, game clock, etc.
    pub turn: u64,
    pub is_paused: bool,
    // ... any other global flags
}

impl PlayState {
    pub fn advance_turn(&mut self) {
        self.turn += 1;

        // 1. Add turn start resources. Like time and environmental resources.
        self.phase_start_of_day();
        // 2. Environment random effects and results which can interfere with plans
        self.phase_environment_events();
        // 3. Player actions, all run and applied simultaneously and before anything 
        //    else could interfere with player actions. This does not include player
        //    market purchasing, wich is saved for the market proper. This includes
        //    movement of units, but not map alterations.
        self.phase_player_actions();
        // 4. Apply new player bonuses, including creating new actors, applying new
        //    bonuses, updating pop demographic desires and priorities, and other similar
        //    changes induced by the player.
        self.phase_player_bonuses_and_demographic_updates();
        // 5. Intra-Market trading day. This is broken up and organized by group turns.
        //    The default order is Player(state), Institutions, Firms, and Pops.
        //    Firms and pops are always in this order, giving firms advantage in gathering
        //    and consolidating resources for merchants. Institutions may put themselves
        //    before, between, or after Pops, defined by the institution. States may break
        //    their market actions and put theme anywhere in this order as well. For example
        //    a player may put construction and military good purchasing in the front of
        //    the order, while putting welfare purchasing in the rear.
        self.phase_intra_market_day();
        // 6. Inter-market trade. Trade between markets will get it's turn and be 
        //    analyzed. Any in-progress trades take their turn/movement, the 
        //    results/effects are analyzed for possible new trades, and new trades are 
        //    kicked off.
        self.phase_inter_market_trade();
        // 7. Production and Non-Player Planning. After all trade is done for the day,
        //    firms, institutions, and the like, look at their prior success, compare
        //    try to predict what will occur tomorrow, create/modify their production 
        //    plans, then do their production plans.
        self.phase_production_and_planning();
        // 8. Pop Consumption.
        self.phase_pop_consumption();
        // 9. Pop Growth/Decline.
        self.phase_pop_growth();
        // 10.Pop migration.
        self.phase_pop_migration();
        // 11.Record Keeping.
        self.phase_record_keeping();
        // 12.Map Changes, like player claims, market consolidation/integration, etc.
        self.phase_map_changes();
        // 13.Good Decay to wrap up the day.
        self.phase_good_decay();
    }

    // --- Turn phase stubs (fill in from advance_turn) ---------------------------

    /// # Phase Start of Day
    /// 
    /// Add turn start resources. Like time and environmental resources.
    /// Goes through 
    fn phase_start_of_day(&mut self) {
        todo!("1. Start-of-day resources (time, environment regen, market day resets, …)")
    }

    fn phase_environment_events(&mut self) {
        todo!("2. Environment random effects")
    }

    fn phase_player_actions(&mut self) {
        todo!("3. Player unit/map actions (not market purchasing)")
    }

    fn phase_player_bonuses_and_demographic_updates(&mut self) {
        todo!("4. Player bonuses, new actors, pop desire / demographic updates")
    }

    fn phase_intra_market_day(&mut self) {
        todo!("5. Intra-market trading day (partition actors, per-market day, merge)")
    }

    fn phase_inter_market_trade(&mut self) {
        todo!("6. Inter-market trade (orchestrated on markets, not by them)")
    }

    fn phase_production_and_planning(&mut self) {
        todo!("7. Firm/institution production and non-player planning")
    }

    fn phase_pop_consumption(&mut self) {
        todo!("8. Pop consumption")
    }

    fn phase_pop_growth(&mut self) {
        todo!("9. Pop growth / decline")
    }

    fn phase_pop_migration(&mut self) {
        todo!("10. Pop migration")
    }

    fn phase_record_keeping(&mut self) {
        todo!("11. Record keeping")
    }

    fn phase_map_changes(&mut self) {
        todo!("12. Map changes (claims, market merge/split, …)")
    }

    /// # Phase Good Decay
    /// 
    /// Goes through Markets and Actors, decaying goods in their storage as is
    /// appropriate.
    /// 
    /// All goods that are stored or have been used (captial goods) are decayed at their
    /// default rate.
    /// 
    /// Goods that were consumed should decay entirely.
    fn phase_good_decay(&mut self) {
        self.map_data.decay_goods(&self.factuals);
        self.actors.decay_goods(&self.factuals);
        self.players.decay_goods(&self.factuals);
    }
}

// In main Bevy app setup:
fn setup_play_state(mut commands: Commands) {
    let mut play_state = PlayState {
        factuals: Factuals::new(),
        map_data: todo!(),
        actors: todo!(),
        players: todo!(),
        turn: todo!(),
        is_paused: todo!(),
    };
    // Load factuals, generate map, init population/players...
    commands.insert_resource(play_state);
}