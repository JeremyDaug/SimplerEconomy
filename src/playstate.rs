use std::collections::{HashMap, HashSet};

use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::game::actors::Actors;
use crate::game::factuals::Factuals;
use crate::game::market::Market;

/// # Play State
///
/// This is the highest level of our game, overseeing a the state of the current game.
///
/// It stores all active data for one game. V1 has exactly one market.
#[derive(Debug)]
pub struct PlayState {
    pub factuals: Factuals, // Highly static, Arc or & for sharing/multithreading
    pub market: Market,     // The one market every pop and firm trades in
    pub actors: Actors,     // Active pops and firms

    // Optional: turn metadata, game clock, etc.
    pub turn: u64,
    pub is_paused: bool,
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
    /// 2. Player action phase.
    /// 3. Apply player demographic changes and bonuses.
    /// 4. Market day.
    /// 5. Release holds.
    /// 6. Production.
    /// 7. Consumption.
    /// 8. Sentiments.
    /// 9. Growth.
    /// 10. Decay.
    /// 11. Rescale.
    /// 12. Record keeping.
    /// 13. Planning.
    pub fn advance_turn(&mut self) {
        self.turn += 1;
        self.start_of_day();
        self.player_action_phase();
        self.apply_player_demographic_changes();
        self.market_day();
        self.release_holds();
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
    /// is added or its bucket edited.
    ///
    /// Runs [`Actors::start_day`] so each pop receives its morning time.
    /// Desire updates are not written yet.
    fn start_of_day(&mut self) {
        self.actors.start_day(&mut self.market, &self.factuals);
    }

    /// # Player Action Phase
    ///
    /// Runs the player's queued orders.
    fn player_action_phase(&mut self) {
        // skipped as player actions are a ways away.
        let _ = self;
    }

    /// # Apply Player Demographic Changes And Bonuses
    ///
    /// Updates each pop's desires from its demographic desires.
    ///
    /// The update is [`crate::game::pop::Pop::apply_demographic_changes`]. Bonuses are
    /// dispensed later: pop change bonuses in growth, satisfaction and
    /// sentiments in [`Self::sentiments`]. Pop change bonuses are not
    /// written yet.
    fn apply_player_demographic_changes(&mut self) {
        let factuals = &self.factuals;
        for pop in self.actors.pops.values_mut() {
            pop.apply_demographic_changes(factuals);
        }
    }

    /// # Market Day
    ///
    /// Runs the market's day.
    ///
    /// Pops and firms listed on the market are taken out of [`Actors`] for
    /// the day, in ascending id order, and returned after. The rng is
    /// seeded from `turn`. The day is [`Market::market_day`].
    fn market_day(&mut self) {
        let mut pops = take_listed(&mut self.actors.pops, &self.market.pops);
        let mut firms = take_listed(&mut self.actors.firms, &self.market.firms);
        let mut rng = StdRng::seed_from_u64(self.turn ^ (self.market.id as u64));
        self.market
            .market_day(&self.factuals, &mut pops, &mut firms, &mut rng);
        for pop in pops {
            self.actors.pops.insert(pop.id, pop);
        }
        for firm in firms {
            self.actors.firms.insert(firm.id, firm);
        }
    }

    /// # Release Holds
    ///
    /// Releases each pop's savings and reserves before production, so the
    /// coins are in property for decay.
    fn release_holds(&mut self) {
        for pop in self.actors.pops.values_mut() {
            pop.release_holds();
        }
    }

    /// # Production
    ///
    /// Runs production for every pop and firm.
    ///
    /// Each actor runs on its own thread. They share `factuals` and do not
    /// share their own goods. A pop runs its job. A firm runs its lines,
    /// and that run is priced from the market.
    fn production(&mut self) {
        let factuals = &self.factuals;
        let market = &self.market;
        let pops = &mut self.actors.pops;
        let firms = &mut self.actors.firms;
        std::thread::scope(|scope| {
            for pop in pops.values_mut() {
                scope.spawn(|| pop.produce(factuals));
            }
            for firm in firms.values_mut() {
                scope.spawn(|| firm.produce(factuals, Some(market)));
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
    /// Hands the walk to [`Actors::sentiments`], with the market a pop
    /// reads for wealth.
    fn sentiments(&mut self) {
        self.actors.sentiments(&self.market);
    }

    /// # Growth
    ///
    /// Births and deaths, rebalance if capped, and pruning.
    ///
    /// Each pop runs births and deaths on its own thread. Cap rebalance
    /// then uses the whole market. Prune removes pops too small to survive.
    fn growth(&mut self) {
        self.births_and_deaths();
        self.market.rebalance_capped(&mut self.actors.pops);
        self.prune_pops();
    }

    /// # Births And Deaths
    ///
    /// Runs [`crate::game::pop::Pop::growth_phase`] for every pop.
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

    /// # Prune Pops
    ///
    /// Removes pops whose household count is under 1.
    ///
    /// The market records [`Market::prune_under_one`] for its members, then
    /// the actor is dropped.
    fn prune_pops(&mut self) {
        let pops = &mut self.actors.pops;
        self.market.prune_under_one(pops);
        pops.retain(|_, pop| pop.demographics.household.count >= 1.0);
    }

    /// # Decay
    ///
    /// Decays goods and desires.
    ///
    /// Hands the pops and firms to [`Actors::decay`].
    fn decay(&mut self) {
        self.actors.decay(&self.factuals);
    }

    /// # Rescale
    ///
    /// Brings each pop's plans and desire amounts to its population after growth.
    ///
    /// Each pop runs [`crate::game::pop::Pop::rescale`] on its own thread. Firms are left as
    /// they are.
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
    /// Rolls the day's goods into the market, then closes each actor's day.
    ///
    /// The market runs first, so it still sees the flow counters, and then
    /// moves its prices. [`Actors::record_keeping`] closes the pops and
    /// firms after that.
    fn record_keeping(&mut self) {
        self.market
            .record_keeping(&self.factuals, &self.actors.pops, &self.actors.firms);
        self.actors.record_keeping(&self.factuals);
    }

    /// # Planning
    ///
    /// Runs each actor's plan for the next day.
    ///
    /// Hands the walk to [`Actors::planning`], with the market a pop or firm
    /// reads for prices.
    fn planning(&mut self) {
        self.actors.planning(&self.factuals, &self.market);
    }
}

/// # Take Listed
///
/// Removes each id in `listed` from `all` and returns those values.
///
/// Ids are taken in ascending order. An id already absent is skipped.
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
