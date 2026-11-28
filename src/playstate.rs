use std::collections::{BTreeMap, HashMap, HashSet};

use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::game::actors::Actors;
use crate::game::factuals::Factuals;
use crate::game::ledger::{CoinFlows, CoinLedger, LedgerDay};
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
    pub ledger: CoinLedger, // Each day's coin sources, sinks, and transfers

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
    ///
    /// The coin ledger opens before step 1 and closes after step 13, and
    /// the day is pushed onto [`Self::ledger`]. Debug builds check that
    /// each currency's day balances.
    pub fn advance_turn(&mut self) {
        self.turn += 1;
        let mut day = self.open_ledger_day();
        self.start_of_day();
        self.player_action_phase();
        self.apply_player_demographic_changes();
        self.market_day();
        self.release_holds();
        self.production();
        self.consumption();
        self.sentiments();
        self.growth(&mut day);
        self.decay();
        self.rescale();
        self.record_keeping();
        self.planning();
        self.close_ledger_day(&mut day);
        for (good, flows) in &day.coins {
            debug_assert!(
                flows.balances(),
                "turn {} coin {good} is off by {}: {flows:?}",
                day.turn,
                flows.imbalance()
            );
        }
        self.ledger.days.push(day);
    }

    /// # Coin Holdings
    ///
    /// Units of each market currency held by the pops and firms listed on
    /// the market.
    ///
    /// Each actor's units are [`crate::game::pop::Pop::holding`] or
    /// [`crate::game::firm::Firm::holding`]. A listed id with no actor is
    /// skipped. Returns the sums, keyed by currency good.
    fn coin_holdings(&self) -> BTreeMap<usize, f64> {
        let mut held = BTreeMap::new();
        for &coin in &self.market.currencies {
            let pops: f64 = self
                .market
                .pops
                .iter()
                .filter_map(|id| self.actors.pops.get(id))
                .map(|pop| pop.holding(coin))
                .sum();
            let firms: f64 = self
                .market
                .firms
                .iter()
                .filter_map(|id| self.actors.firms.get(id))
                .map(|firm| firm.holding(coin))
                .sum();
            held.insert(coin, pops + firms);
        }
        held
    }

    /// # Open Ledger Day
    ///
    /// Starts today's ledger, with each currency's opening holdings from
    /// [`Self::coin_holdings`]. Returns that day.
    fn open_ledger_day(&self) -> LedgerDay {
        let coins = self
            .coin_holdings()
            .into_iter()
            .map(|(coin, opening)| (coin, CoinFlows { opening, ..CoinFlows::default() }))
            .collect();
        LedgerDay { turn: self.turn, coins }
    }

    /// # Close Ledger Day
    ///
    /// Finishes `day` from the market's records and the closing holdings.
    ///
    /// Each currency's production is minted, its consumption is destroyed,
    /// its rot is decayed, and its payments are transfers. These add to
    /// what pruned pops already wrote. Closing is [`Self::coin_holdings`].
    fn close_ledger_day(&self, day: &mut LedgerDay) {
        for (coin, closing) in self.coin_holdings() {
            let flows = day.coins.entry(coin).or_default();
            if let Some(row) = self.market.goods.get(&coin) {
                flows.minted += row.production;
                flows.destroyed += row.consumption;
                flows.decayed += row.decayed;
                flows.transfers += row.paid;
            }
            flows.closing = closing;
        }
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
    /// then uses the whole market. Prune removes pops too small to survive,
    /// and writes their coin onto `day`.
    fn growth(&mut self, day: &mut LedgerDay) {
        self.births_and_deaths();
        self.market.rebalance_capped(&mut self.actors.pops);
        self.prune_pops(day);
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
    /// the actor is dropped. For each currency, a dropped pop listed on the
    /// market adds its holding to `day`'s pruned. Its coin made today adds
    /// to minted, and its coin spent or consumed today adds to destroyed,
    /// since the market's records will not see it.
    fn prune_pops(&mut self, day: &mut LedgerDay) {
        let pops = &mut self.actors.pops;
        for pop in pops.values() {
            if pop.demographics.household.count >= 1.0 || !self.market.pops.contains(&pop.id) {
                continue;
            }
            for &coin in &self.market.currencies {
                let flows = day.coins.entry(coin).or_default();
                flows.pruned += pop.holding(coin);
                if let Some(row) = pop.property.get(&coin) {
                    flows.minted += row.produced;
                    flows.destroyed += row.consumed + row.spent;
                }
            }
        }
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

#[cfg(test)]
mod coin_ledger_should {
    use std::collections::{HashMap, HashSet};

    use super::PlayState;
    use crate::game::actors::Actors;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::firm::{Firm, FirmPRow};
    use crate::game::good::{Good, GoodTag, TIME};
    use crate::game::household::Household;
    use crate::game::job::{Job, JobLine};
    use crate::game::ledger::{CoinFlows, CoinLedger};
    use crate::game::market::{Market, OwnershipSubject};
    use crate::game::pop::Pop;
    use crate::game::pop_property::PopPRow;
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};
    use crate::game::scalingfactor::ScalingFactor;

    const DUCKATS: usize = 1;
    const BREAD: usize = 2;
    const MINT: usize = 1;
    const BAKE: usize = 2;

    fn good(id: usize, decay_rate: f64, tags: HashSet<GoodTag>) -> Good {
        Good {
            id,
            name: format!("g{id}"),
            class: None,
            decay_rate,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 0.0,
            tags,
            categories: vec![],
        }
    }

    fn bread_desire() -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![DesireTarget::new(BREAD, DesireTargetType::Consume, 1.0)],
            amount: 3.0,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Household(1.0),
            decay: 1.0,
        }
    }

    fn pop(id: usize, process: usize, coins: f64) -> Pop {
        let mut pop = Pop::new(id);
        pop.demographics.household = Household::with_count(10.0);
        pop.job = Job::new([
            JobLine::new(process, None, Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        pop.desires[0].push(bread_desire());
        pop.property.insert(DUCKATS, PopPRow::new(coins));
        pop
    }

    /// One pop mints Duckats, two bake bread, and an owned firm sells bread
    /// stock. Duckats decay at 0.001 a day. Nothing crosses the border.
    fn closed_market() -> PlayState {
        let mut factuals = Factuals::new();
        factuals.goods.insert(TIME, good(TIME, 1.0, HashSet::new()));
        factuals.goods.insert(DUCKATS, good(DUCKATS, 0.001, HashSet::from([GoodTag::Money])));
        factuals.goods.insert(BREAD, good(BREAD, 0.0, HashSet::new()));
        factuals.processes.insert(
            MINT,
            Process::new(MINT, "make duckats", 0)
                .with_input(ProcessInput::new(TIME, 8.0, false, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(DUCKATS, 1.0, false)),
        );
        factuals.processes.insert(
            BAKE,
            Process::new(BAKE, "bake", 0)
                .with_input(ProcessInput::new(TIME, 2.0, false, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(BREAD, 1.0, false)),
        );
        let mut market = Market::new(1);
        market.currencies.push(DUCKATS);
        let mut actors = Actors::new();
        for pop in [pop(1, MINT, 50.0), pop(2, BAKE, 20.0), pop(3, BAKE, 20.0)] {
            market.pops.insert(pop.id);
            actors.pops.insert(pop.id, pop);
        }
        let mut firm = Firm::new(1, "bakery".into());
        firm.property.insert(DUCKATS, FirmPRow::new().with_quantity(100.0));
        firm.property.insert(BREAD, FirmPRow::new().with_quantity(200.0));
        market.firms.insert(firm.id);
        market.issue_ownership(OwnershipSubject::Firm(firm.id), 1, 1.0);
        actors.firms.insert(firm.id, firm);
        PlayState {
            factuals,
            market,
            actors,
            ledger: CoinLedger::default(),
            turn: 0,
            is_paused: false,
        }
    }

    #[test]
    fn balance_every_day_for_a_thousand_days() {
        let mut state = closed_market();
        let start: f64 = state.coin_holdings().values().sum();
        for _ in 0..1000 {
            state.advance_turn();
        }
        assert_eq!(state.ledger.days.len(), 1000);
        let mut total = CoinFlows::default();
        for (at, day) in state.ledger.days.iter().enumerate() {
            let flows = day.coins[&DUCKATS];
            assert!(flows.balances(), "day {at} is off by {}: {flows:?}", flows.imbalance());
            if at > 0 {
                let yesterday = state.ledger.days[at - 1].coins[&DUCKATS].closing;
                assert!((flows.opening - yesterday).abs() < 1e-9);
            }
            total.minted += flows.minted;
            total.decayed += flows.decayed;
            total.destroyed += flows.destroyed;
            total.pruned += flows.pruned;
            total.transfers += flows.transfers;
        }
        let end: f64 = state.coin_holdings().values().sum();
        // Every flow this run has was exercised.
        assert!(total.minted > 0.0);
        assert!(total.decayed > 0.0);
        assert!(total.transfers > 0.0);
        let expected = start + total.minted - total.decayed - total.destroyed - total.pruned;
        assert!(
            (end - expected).abs() <= 1e-9 * start.max(end),
            "{end} != {expected}"
        );
    }

    #[test]
    fn book_a_pruned_pops_coin_as_a_sink() {
        // Pop 3 falls under one household, so growth drops it and its coin.
        let mut state = closed_market();
        state.actors.pop_mut(3).demographics.household = Household::with_count(0.5);
        state.advance_turn();
        let flows = state.ledger.days[0].coins[&DUCKATS];
        assert!(!state.actors.pops.contains_key(&3));
        assert!(flows.pruned > 0.0);
        assert!(flows.balances(), "off by {}: {flows:?}", flows.imbalance());
    }
}
