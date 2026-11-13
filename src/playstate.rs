use std::collections::{HashMap, HashSet};

use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::game::actors::Actors;
use crate::game::factuals::Factuals;
use crate::game::firm::Firm;
use crate::game::mapdata::MapData;
use crate::game::market::{Market, MarketHistory, MarketLookups};
use crate::game::players::Players;
use crate::game::pop::Pop;

/// # Play State
///
/// This is the highest level of our game, overseeing a the state of the current game.
///
/// It stores all active data for one game.
#[derive(Debug)]
pub struct PlayState {
    pub factuals: Factuals, // Highly static, Arc or & for sharing/multithreading
    pub map_data: MapData,  // Environmental + spatial
    pub actors: Actors,     // Active pops, firms, institutions, …
    pub players: Players,   // Human/AI players, resources, etc.

    // Optional: turn metadata, game clock, etc.
    pub turn: u64,
    pub is_paused: bool,
    /// Per-market AMV snapshots and pop-to-market ids for this turn.
    /// Rebuild after prices change or pops move; borrow during pop phases.
    pub market_lookups: MarketLookups,
    // ... any other global flags
}

impl PlayState {
    /// # Advance Turn
    ///
    /// Advances this game by one turn.
    ///
    /// Increments `turn` by 1, then runs these steps in order. Each step's
    /// method holds that step's effects.
    ///
    /// 1. Start of day.
    /// 2. Environmental phase.
    /// 3. Player action phase.
    /// 4. Apply player demographic changes and bonuses.
    /// 5. Actor reservations.
    /// 6. Intra-market day.
    /// 7. Inter-market day. Savings and reserves are released before production.
    /// 8. Production.
    /// 9. Consumption.
    /// 10. Sentiments.
    /// 11. Growth.
    /// 12. Decay.
    /// 13. Rescale.
    /// 14. Record keeping.
    /// 15. Planning.
    pub fn advance_turn(&mut self) {
        self.turn += 1;
        self.start_of_day();
        self.environmental_phase();
        self.player_action_phase();
        self.apply_player_demographic_changes();
        self.intra_market_day();
        self.inter_market_day();
        self.production();
        self.consumption();
        self.sentiments();
        self.growth();
        self.decay();
        self.rescale();
        self.record_keeping();
        self.planning();
    }

    /// # Start Of Day
    ///
    /// Generates the day's time. Desires can also update here, if a new one
    /// is added or its bucket edited. Special resource extraction can be
    /// here, depending on what kind of special.
    ///
    /// Runs [`Actors::start_day`] so each pop receives its morning time.
    /// Desire updates and special extraction are not written yet.
    fn start_of_day(&mut self) {
        self.actors
            .start_day(self.map_data.markets_mut(), &self.factuals);
        // At here would go the market's returning plots to their rightful share owners.
    }

    /// # Environmental Phase
    ///
    /// Disasters or climactic shifts. Currently not much.
    fn environmental_phase(&mut self) {
        // Currently skipped as simple market testing comes first.
        let _ = self;
    }

    /// # Player Action Phase
    ///
    /// Runs map actions, unit movements, and the other player actions.
    fn player_action_phase(&mut self) {
        // skipped as player actions are a ways away.
        let _ = self;
    }

    /// # Apply Player Demographic Changes And Bonuses
    ///
    /// Updates each pop's desires from its demographic desires.
    ///
    /// The update is [`Pop::apply_demographic_changes`]. Bonuses are
    /// dispensed later: pop change bonuses in growth, satisfaction and
    /// sentiments in [`Self::sentiments`]. Pop change bonuses are not
    /// written yet.
    fn apply_player_demographic_changes(&mut self) {
        let factuals = &self.factuals;
        for pop in self.actors.pops.values_mut() {
            pop.apply_demographic_changes(factuals);
        }
    }

    /// # Intra Market Day
    ///
    /// Runs each market's day on its own thread.
    ///
    /// Pops and firms listed on a market are moved to that market for the
    /// day and returned after. A member listed on several markets goes to
    /// the lowest market id. Institutions and players stay put and are
    /// borrowed shared, because one of them can sit in more than one market.
    /// Each market receives its own rng, seeded from `turn` and the market
    /// id. The buys themselves are not written yet.
    fn intra_market_day(&mut self) {
        let mut ids: Vec<usize> = self.map_data.markets_mut().keys().copied().collect();
        ids.sort_unstable();
        let mut batches = Vec::with_capacity(ids.len());
        for id in ids {
            let market = self
                .map_data
                .markets_mut()
                .remove(&id)
                .unwrap_or_else(|| panic!("market {id} is not in MapData"));
            let pops = take_listed(&mut self.actors.pops, &market.pops);
            let firms = take_listed(&mut self.actors.firms, &market.firms);
            // Passed through for the market day. The offer split does not read it.
            let history = self
                .market_lookups
                .histories
                .get(&id)
                .cloned()
                .unwrap_or_else(MarketHistory::new);
            batches.push(MarketDayBatch {
                market,
                history,
                pops,
                firms,
            });
        }
        let turn = self.turn;
        let factuals = &self.factuals;
        let institutions = &self.actors.institutions;
        let players = &self.players;
        std::thread::scope(|scope| {
            for batch in &mut batches {
                scope.spawn(|| {
                    let mut rng = StdRng::seed_from_u64(turn ^ (batch.market.id as u64));
                    batch.market.market_day(
                        factuals,
                        &batch.history,
                        &mut batch.pops,
                        &mut batch.firms,
                        institutions,
                        players,
                        &mut rng,
                    );
                });
            }
        });
        for batch in batches {
            for pop in batch.pops {
                self.actors.pops.insert(pop.id, pop);
            }
            for firm in batch.firms {
                self.actors.firms.insert(firm.id, firm);
            }
            self.map_data.markets.insert(batch.market.id, batch.market);
        }
    }

    /// # Inter Market Day
    ///
    /// Trade between markets. Sets up trade routes, creates shipping plans,
    /// and moves them between markets to buy and sell.
    ///
    /// Trade routes are not written. Savings and reserves are released
    /// here, before production, so the coins are in property for decay.
    fn inter_market_day(&mut self) {
        for pop in self.actors.pops.values_mut() {
            pop.release_holds();
        }
    }

    /// # Production
    ///
    /// Runs production for every pop, firm, and institution.
    ///
    /// Each actor runs on its own thread. They share `factuals` and do not
    /// share their own goods. Special resource extraction can be here,
    /// depending on what kind of special. A pop runs its job. Firm and
    /// institution production are not written yet.
    fn production(&mut self) {
        let factuals = &self.factuals;
        let pops = &mut self.actors.pops;
        let firms = &mut self.actors.firms;
        let institutions = &mut self.actors.institutions;
        std::thread::scope(|scope| {
            for pop in pops.values_mut() {
                scope.spawn(|| pop.produce(factuals));
            }
            for firm in firms.values_mut() {
                scope.spawn(|| firm.produce(factuals));
            }
            for institution in institutions.values_mut() {
                scope.spawn(|| institution.produce(factuals));
            }
        });
    }

    /// # Consumption
    ///
    /// Pops consume for their desires.
    ///
    /// Each pop runs on its own thread. They share `factuals` and do not
    /// share their own goods.
    fn consumption(&mut self) {
        let factuals = &self.factuals;
        let pops = &mut self.actors.pops;
        std::thread::scope(|scope| {
            for pop in pops.values_mut() {
                scope.spawn(|| pop.consume(factuals));
            }
        });
    }

    /// # Pop Sentiments
    ///
    /// Runs pop sentiments. Satisfaction and sentiment bonuses are
    /// dispensed here.
    ///
    /// Hands the walk to [`Actors::sentiments`], with the markets a pop
    /// reads for wealth.
    fn sentiments(&mut self) {
        self.actors.sentiments(&self.map_data.markets);
    }

    /// # Growth
    ///
    /// Births and deaths, rebalance if capped, pruning, and migration.
    ///
    /// Each pop runs births and deaths on its own thread. Cap rebalance
    /// then uses the whole market. Prune removes pops too small to survive.
    /// Migration inside a market is local, then regional. Interregional
    /// people follow. Organized movers follow the people. Regional,
    /// interregional, and organized movement are not written yet.
    fn growth(&mut self) {
        self.births_and_deaths();
        self.rebalance_capped();
        self.prune_pops();
        // Local, then regional inside each market. Regional is not written yet.
        self.migrate_inside_markets();
        // People move between markets one market after another. Not written yet.
        self.map_data
            .process_inter_market_migration(&mut self.actors, &self.factuals);
        // Firms, institutions, and player efforts follow. Not written yet.
        self.map_data.process_organized_migration(
            &mut self.actors,
            &self.players,
            &self.factuals,
        );
    }

    /// # Births And Deaths
    ///
    /// Runs [`Pop::growth_phase`] for every pop.
    ///
    /// Each pop runs on its own thread. They share `factuals` and do not
    /// share a household.
    fn births_and_deaths(&mut self) {
        let factuals = &self.factuals;
        let pops = &mut self.actors.pops;
        std::thread::scope(|scope| {
            for pop in pops.values_mut() {
                scope.spawn(|| pop.growth_phase(factuals));
            }
        });
    }

    /// # Rebalance Capped
    ///
    /// Brings each market's capped pops back to the market ceiling.
    ///
    /// The scale is [`Market::rebalance_capped`].
    fn rebalance_capped(&mut self) {
        let pops = &mut self.actors.pops;
        for market in self.map_data.markets.values() {
            market.rebalance_capped(pops);
        }
    }

    /// # Prune Pops
    ///
    /// Removes pops whose household count is under 1.
    ///
    /// Each market records [`Market::prune_under_one`] for its members.
    /// The actor is dropped after every market has recorded.
    fn prune_pops(&mut self) {
        let pops = &mut self.actors.pops;
        for market in self.map_data.markets.values_mut() {
            market.prune_under_one(pops);
        }
        pops.retain(|_, pop| pop.demographics.household.count >= 1.0);
    }

    /// # Migrate Inside Markets
    ///
    /// Runs local and regional migration in each market.
    ///
    /// `map` is the hex grid. The two passes are
    /// [`Market::internal_migration`].
    fn migrate_inside_markets(&mut self) {
        let factuals = &self.factuals;
        let map = &self.map_data.map;
        let pops = &mut self.actors.pops;
        for market in self.map_data.markets.values_mut() {
            market.internal_migration(pops, map, factuals);
        }
    }

    /// # Decay
    ///
    /// Decays goods and desires.
    ///
    /// Hands the pops and firms to [`Actors::decay`]. Goods held in a market
    /// or on a tile are [`crate::game::mapdata::MapData::decay_goods`]. That
    /// pass is not written yet.
    fn decay(&mut self) {
        self.actors.decay(&self.factuals);
    }

    /// # Rescale
    ///
    /// Brings each pop's plans and desire amounts to its population after growth.
    ///
    /// Each pop runs [`Pop::rescale`] on its own thread. Firms and institutions
    /// are left as they are.
    fn rescale(&mut self) {
        let pops = &mut self.actors.pops;
        std::thread::scope(|scope| {
            for pop in pops.values_mut() {
                scope.spawn(|| pop.rescale());
            }
        });
    }

    /// # Record Keeping
    ///
    /// Rolls the day's goods into each market, then closes each actor's day.
    ///
    /// Markets run first, on their own threads, so they still see the flow
    /// counters. Each then moves its prices. [`Actors::record_keeping`]
    /// closes the pops and firms after that.
    fn record_keeping(&mut self) {
        let pop_home = member_homes(&self.map_data.markets, |market| &market.pops);
        let firm_home = member_homes(&self.map_data.markets, |market| &market.firms);
        let factuals = &self.factuals;
        let pops = &self.actors.pops;
        let firms = &self.actors.firms;
        let markets = self.map_data.markets_mut();
        std::thread::scope(|scope| {
            for market in markets.values_mut() {
                scope.spawn(|| {
                    market.record_keeping(factuals, pops, firms, &pop_home, &firm_home);
                });
            }
        });
        self.actors.record_keeping(&self.factuals);
    }

    /// # Planning
    ///
    /// Runs each actor's plan for the next day.
    ///
    /// Hands the walk to [`Actors::planning`], with the markets a pop reads
    /// for prices. Firm and institution planning are not written yet.
    fn planning(&mut self) {
        self.actors
            .planning(&self.factuals, &self.map_data.markets);
    }
}

/// One market and the pops and firms that trade in it today.
struct MarketDayBatch {
    market: Market,
    history: MarketHistory,
    pops: Vec<Pop>,
    firms: Vec<Firm>,
}

/// # Member Homes
///
/// Maps each member id to the lowest market id that lists it.
///
/// `members` reads one market's member set. A member listed on several
/// markets maps to the lowest id. A member listed on none is absent.
fn member_homes(
    markets: &HashMap<usize, Market>,
    members: impl Fn(&Market) -> &HashSet<usize>,
) -> HashMap<usize, usize> {
    let mut homes = HashMap::new();
    let mut ids: Vec<usize> = markets.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        for &member in members(&markets[&id]) {
            homes.entry(member).or_insert(id);
        }
    }
    homes
}

/// # Take Listed
///
/// Removes each id in `listed` from `all` and returns those values.
///
/// Ids are taken in ascending order. An id already absent is skipped, so a
/// member claimed by an earlier market stays there.
fn take_listed<T>(all: &mut HashMap<usize, T>, listed: &HashSet<usize>) -> Vec<T> {
    let mut ids: Vec<usize> = listed.iter().copied().collect();
    ids.sort_unstable();
    let mut taken = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(value) = all.remove(&id) {
            taken.push(value);
        }
    }
    taken
}