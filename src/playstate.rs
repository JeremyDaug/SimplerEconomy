use bevy::prelude::*;
use rayon::prelude::*;

use crate::game::actors::Actors;
use crate::game::factuals::Factuals;
use crate::game::mapdata::MapData;
use crate::game::market::MarketHistory;
use crate::game::players::Players;

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
        // environmental refresh, create new plants, animals, and regenerating natural features
        // plot updating and and environmental factor disbursment.
        // Pop time Generation
        todo!("1. Start-of-day resources (time, environment regen, market day resets, …)")
    }

    fn phase_environment_events(&mut self) {
        todo!("2. Environment random effects")
    }

    fn phase_player_actions(&mut self) {
        todo!("3. Player unit/map actions (not market purchasing)")
    }

    /// # Phase Player Bonuses and Demographic Updates
    ///
    /// Outside-market effects from institutions, firms, and demographic edits.
    /// **Sequential** (institutions may touch firms and pops; firms touch pops).
    ///
    /// Order:
    /// 1. Institutions push passive effects onto firms / pops.
    /// 2. Firms push passive bonuses onto pops.
    /// 3. Each pop runs [`Pop::update_desires`].
    /// 4. Clear shared demographic `household_changed` flags on factuals.
    ///
    /// Institutions do not rewrite household/desires directly; they push effects
    /// (and later demographic mods). Non-demo household overlays (D1) stay deferred.
    fn phase_player_bonuses_and_demographic_updates(&mut self) {
        let markets = &self.map_data.markets;
        let factuals = &self.factuals;
        let Actors {
            pops,
            firms,
            institutions,
        } = &mut self.actors;

        // 1. Institutions → firms / pops.
        for institution in institutions.values() {
            institution.apply_passive_effects(pops, firms, markets);
        }

        // 2. Firms → pops.
        for firm in firms.values() {
            firm.apply_passive_bonuses(pops);
        }

        // 3. Pops: update desires from demographics if needed, resync desires.
        for pop in pops.values_mut() {
            pop.update_desires(factuals);
        }

        // 4. Orchestrator clears shared flags after all pops have read them.
        self.factuals.clear_household_changed_flags();
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

    /// # Phase Pop Consumption
    /// 
    /// At this point, pops consume all goods they have reserved and planned.
    fn phase_pop_consumption(&mut self) {
        let pops = &mut self.actors.pops;
        pops.par_iter_mut().for_each(|(_, pop)| {
            pop.consume();
        });
    }

    /// # Phase Pop Growth
    ///
    /// Population growth occurs here.
    ///
    /// Each pop resolves structural demographic rates from factuals (recomputed per
    /// pop; not a prebuilt combo table), stacks same-day mods, and runs
    /// `Household::update`. Parallel over pops with shared `&Factuals` is safe
    /// because rate resolution is pure recompute today.
    ///
    /// If rate resolution ever dominates at huge pop counts, day-fill a cache of
    /// unique demographic keys on factuals before this phase (see
    /// `Factuals::get_demographic_rates`); keep growth as a read of that table.
    fn phase_pop_growth(&mut self) {
        let factuals = &self.factuals;
        self.actors.pops.par_iter_mut().for_each(|(_, pop)| {
            pop.growth_phase(factuals);
        });
    }

    /// # Phase Pop Migration
    /// 
    /// This covers and deals with the movement of pops.
    /// 
    /// This happens in a phased stage.
    /// 
    /// 1. Calculate Migratory Pressure on each pop. 
    ///    Unhappiness * Pop Size * Pop Mobility Factor ~= Emmigration Pressure.
    ///    Hiring Pressure also exists, pulling pops into the 
    /// 2. Sum and calculate Migratory pressure on the Market Region. 
    ///    Will need to keep Positive, negative, and sum in mind.
    /// 3. Process Organized Migration Efforts (Migratory Firms, Institutions, Player Driven Efforts)
    /// 4. Process Internal Migration (between jobs in the same market).
    /// 5. Process Inter-Market Migration (Personal, long distance migration).
    /// 
    /// ### High Level Explanation
    /// 
    /// The idea is that all pops have a desire to move based on their mood and ability
    /// to move (which is tied to their mobile wealth and cultural inclinations).
    /// This is counteracted by Hiring/Expansion pressure, other pops who are wealthy(er)
    /// and thus people wish to join, or employers wish to hire more employees.
    /// 
    /// These two combined in a market define the overall migratory pressure. Negative
    /// pressure wants to draw people in while positive wants two push them out. 
    /// It also creates a pool of potential migrants who will be available for moving 
    /// today. This group should be preserved over the days.
    /// 
    /// After this calculation and summation, organized methods of migration occur first.
    /// These are organize, intelligent, and mass migration patters that occur. These can
    /// be created by pops with the right culture or institutions as well as by some 
    /// firms which seek out workers from far and wide. They tend to move pops quickly 
    /// and in large numbers, but only after building up enough internal and resources.
    /// 
    /// Second is internal migration, where pops move between jobs in the same market. 
    /// This is where most pops move about, but should be relatively quick to do.
    ///  
    /// Lastly, Intermarket Migration takes the pool created in the previous steps and
    /// opens up a portion of it to migration, attenuating it further by mobile 
    /// wealth. The Longer the distance and the more expensive it is to move and
    /// the more mobile their wealth needs or the more powerful the desire to leave
    /// needs to be.
    fn phase_pop_migration(&mut self) {
        let factuals = &self.factuals;

        // 1. Per-pop emigration pressure (needs market region) and per-firm hiring pressure.
        // Pop half walks markets so each pop gets its region; firm half stays MT.
        {
            let pops = &mut self.actors.pops;
            let markets = &self.map_data.markets;
            for market in markets.values() {
                for &pop_id in &market.pops {
                    if let Some(pop) = pops.get_mut(&pop_id) {
                        pop.calculate_migratory_pressure(factuals, market);
                    }
                }
            }
        }
        {
            let firms = &mut self.actors.firms;
            firms.par_iter_mut().for_each(|(_, firm)| {
                firm.calculate_hiring_pressure(factuals);
            });
        }

        // 2. Sum pressures onto each market region (markets independent; MT).
        let actors = &self.actors;
        self.map_data.markets.par_iter_mut().for_each(|(_, market)| {
            market.sum_migratory_pressure(actors, factuals);
        });

        // 3. Organized / mass migration (cross-market; sequential).
        self.map_data.process_organized_migration(
            &mut self.actors,
            &self.players,
            factuals,
        );

        // 4. Internal migration within each market (pops/firms independent; MT).
        let pops = &mut self.actors.pops;
        let firms = &mut self.actors.firms;
        rayon::join(
            || {
                pops.par_iter_mut().for_each(|(_, pop)| {
                    pop.process_internal_migration(factuals);
                });
            },
            || {
                firms.par_iter_mut().for_each(|(_, firm)| {
                    firm.process_internal_labor_migration(factuals);
                });
            },
        );

        // 5. Inter-market personal migration (cross-market; sequential).
        self.map_data.process_inter_market_migration(
            &mut self.actors,
            factuals,
        );
    }

    /// # Phase Record Keeping
    /// 
    /// End-of-day bookkeeping for each independent actor/store. Only shared input is
    /// `factuals` (read-only). Markets, pops, firms, institutions, and player states
    /// do not need each other and can run in parallel.
    fn phase_record_keeping(&mut self) {
        // TODO: Consider folding the actors into a singular thread and having the actor struct do the delegation. Shouldn't make a real difference, but just a thought.
        let factuals = &self.factuals;
        let markets = &mut self.map_data.markets;
        let pops = &mut self.actors.pops;
        let firms = &mut self.actors.firms;
        let institutions = &mut self.actors.institutions;
        let states = &mut self.players.players;

        // Top-level stores are disjoint; each spawn only mutates its own map.
        // Within a store, entries are independent and use par_iter_mut.
        rayon::scope(|s| {
            s.spawn(|_| {
                markets
                    .par_iter_mut()
                    .for_each(|(_, market)| market.record_keeping(factuals));
            });
            s.spawn(|_| {
                // Stub: pops have no market_id yet; real wiring walks each market's history.
                let history = MarketHistory::new();
                pops.par_iter_mut()
                    .for_each(|(_, pop)| pop.record_keeping(factuals, &history));
            });
            s.spawn(|_| {
                firms
                    .par_iter_mut()
                    .for_each(|(_, firm)| firm.record_keeping(factuals));
            });
            s.spawn(|_| {
                institutions
                    .par_iter_mut()
                    .for_each(|(_, institution)| institution.record_keeping(factuals));
            });
            s.spawn(|_| {
                states
                    .par_iter_mut()
                    .for_each(|(_, state)| state.record_keeping(factuals));
            });
        });
    }

    /// # Phase Map Changes
    /// 
    /// Alters the map, completing claims, altering market regions, moving units on the
    /// map, and doing any slow terrain alterations that otherwise occur.
    fn phase_map_changes(&mut self) {
        todo!("12. Map Changes. ")
        // 1. First, go through the players and add any new claims they placed this turn,
        //    noting any conflicts that arise from this.
        // 2. Shift Tiles between market regions and consolidate/create. Be sure to 
        //    record what was in the tile before moving and keep track of it for at 
        //    least a little bit.
        // 3. Complete any unit movements (troops, traders, workers, etc)
        // 4. Any procedural (non-event driven) map changes are processed and arise here.
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