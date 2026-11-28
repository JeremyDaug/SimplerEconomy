use std::collections::{BTreeMap, HashMap, HashSet};

use rand::rngs::StdRng;
use rand::SeedableRng;

use crate::game::actors::Actors;
use crate::game::factuals::Factuals;
use crate::game::ledger::{CoinFlows, CoinLedger, LedgerDay};
use crate::game::market::Market;
use crate::game::pop::Pop;

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
    /// [`crate::game::firm::Firm::holding`]. Pops are summed lowest id
    /// first, then firms, so the same state always gives the same sum. A
    /// listed id with no actor is skipped. Returns the sums, keyed by
    /// currency good.
    fn coin_holdings(&self) -> BTreeMap<usize, f64> {
        let mut pop_ids: Vec<usize> = self.market.pops.iter().copied().collect();
        pop_ids.sort_unstable();
        let mut firm_ids: Vec<usize> = self.market.firms.iter().copied().collect();
        firm_ids.sort_unstable();
        let mut held = BTreeMap::new();
        for &coin in &self.market.currencies {
            let pops = pop_ids
                .iter()
                .filter_map(|id| self.actors.pops.get(id))
                .map(|pop| pop.holding(coin));
            let firms = firm_ids
                .iter()
                .filter_map(|id| self.actors.firms.get(id))
                .map(|firm| firm.holding(coin));
            held.insert(coin, pops.chain(firms).sum());
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
    /// then uses the whole market. Prune merges pops too small to survive
    /// into another pop, and writes onto `day` any coin that has nowhere
    /// to go.
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
    /// Merges each pop whose household count is under 1 into another pop,
    /// then removes it.
    ///
    /// A dying pop's target is [`merge_target`]. The target takes all of
    /// its goods, coin included, through [`Pop::absorb_property`], and all
    /// of its firm shares through [`Market::pass_ownership`]. Today's
    /// records move with the goods, so the market still sees them and the
    /// ledger books no sink. A dying pop with no target keeps its goods and
    /// is dropped with them. For each currency, such a pop listed on the
    /// market adds its holding to `day`'s pruned. Its coin made today adds
    /// to minted, and its coin spent or consumed today adds to destroyed,
    /// since the market's records will not see it. The market then records
    /// [`Market::prune_under_one`] for its members, and the actors are
    /// dropped.
    fn prune_pops(&mut self, day: &mut LedgerDay) {
        let pops = &mut self.actors.pops;
        let mut dying: Vec<usize> = pops
            .values()
            .filter(|pop| pop.demographics.household.count < 1.0)
            .map(|pop| pop.id)
            .collect();
        dying.sort_unstable();
        for id in dying {
            match merge_target(pops, &self.market, id) {
                Some(target) => {
                    let mut gone = pops.remove(&id).expect("dying pop is present");
                    pops.get_mut(&target)
                        .expect("merge target is present")
                        .absorb_property(&mut gone);
                    pops.insert(id, gone);
                    self.market.pass_ownership(id, target);
                }
                None => {
                    if !self.market.pops.contains(&id) {
                        continue;
                    }
                    let pop = &pops[&id];
                    for &coin in &self.market.currencies {
                        let flows = day.coins.entry(coin).or_default();
                        flows.pruned += pop.holding(coin);
                        if let Some(row) = pop.property.get(&coin) {
                            flows.minted += row.produced;
                            flows.destroyed += row.consumed + row.spent;
                        }
                    }
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

/// # Merge Target
///
/// The pop that takes in dying pop `id`.
///
/// Candidates are the other pops in `pops` with at least one household,
/// listed on `market` exactly when `id` is. The most similar wins: same
/// species and culture, then same species, then same culture, then any.
/// A lower id breaks a tie. Returns that pop's id, or `None` when there is
/// no candidate.
fn merge_target(pops: &HashMap<usize, Pop>, market: &Market, id: usize) -> Option<usize> {
    let dying = &pops[&id];
    let listed = market.pops.contains(&id);
    let species = dying.demographics.species;
    let culture = dying.demographics.culture;
    pops.values()
        .filter(|pop| {
            pop.id != id
                && pop.demographics.household.count >= 1.0
                && market.pops.contains(&pop.id) == listed
        })
        .map(|pop| {
            let same_species = pop.demographics.species == species;
            let same_culture = pop.demographics.culture == culture;
            let rank = match (same_species, same_culture) {
                (true, true) => 3,
                (true, false) => 2,
                (false, true) => 1,
                (false, false) => 0,
            };
            (rank, std::cmp::Reverse(pop.id))
        })
        .max()
        .map(|(_, std::cmp::Reverse(target))| target)
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
    use crate::game::actor::Actor;
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

    pub(super) const DUCKATS: usize = 1;
    pub(super) const BREAD: usize = 2;
    const MINT: usize = 1;
    pub(super) const BAKE: usize = 2;

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
    pub(super) fn closed_market() -> PlayState {
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
    fn merge_a_dying_pop_into_the_most_similar_pop() {
        // Pops 2 and 3 share species 1. Pop 1 is species 0, so pop 2 is the
        // target even though pop 1 has the lower id.
        let mut state = closed_market();
        state.actors.pop_mut(2).demographics.species = 1;
        let dying = state.actors.pop_mut(3);
        dying.demographics.species = 1;
        dying.demographics.household = Household::with_count(0.5);
        dying.property.insert(BREAD, PopPRow::new(5.0));
        state.market.transfer_ownership(OwnershipSubject::Firm(1), 1, 3, 0.5);
        let before = state.coin_holdings()[&DUCKATS];
        let mut day = state.open_ledger_day();
        state.prune_pops(&mut day);
        assert!(!state.actors.pops.contains_key(&3));
        assert!(!state.market.pops.contains(&3));
        let target = state.actors.pop(2);
        assert!((target.holding(DUCKATS) - 40.0).abs() < 1e-9);
        assert!((target.holding(BREAD) - 5.0).abs() < 1e-9);
        assert!((state.actors.pop(1).holding(DUCKATS) - 50.0).abs() < 1e-9);
        let share = state.market.ownership_of(OwnershipSubject::Firm(1), Actor::Pop(2));
        assert!((share - 0.5).abs() < 1e-9);
        assert!((state.coin_holdings()[&DUCKATS] - before).abs() < 1e-9);
        assert_eq!(day.coins[&DUCKATS].pruned, 0.0);
    }

    #[test]
    fn balance_a_day_with_a_merged_pop_and_book_no_sink() {
        let mut state = closed_market();
        state.actors.pop_mut(3).demographics.household = Household::with_count(0.5);
        state.advance_turn();
        let flows = state.ledger.days[0].coins[&DUCKATS];
        assert!(!state.actors.pops.contains_key(&3));
        assert_eq!(flows.pruned, 0.0);
        assert!(flows.balances(), "off by {}: {flows:?}", flows.imbalance());
    }

    #[test]
    fn book_the_last_pops_coin_as_a_sink() {
        // With no other pop to take it, the last pop leaves with its coin.
        let mut state = closed_market();
        for id in [2, 3] {
            state.actors.pops.remove(&id);
            state.market.pops.remove(&id);
        }
        state.actors.pop_mut(1).demographics.household = Household::with_count(0.5);
        let held = state.actors.pop(1).holding(DUCKATS);
        let mut day = state.open_ledger_day();
        state.prune_pops(&mut day);
        assert!(state.actors.pops.is_empty());
        assert!((day.coins[&DUCKATS].pruned - held).abs() < 1e-9);
    }
}

#[cfg(test)]
mod closed_market_should {
    use super::coin_ledger_should::{closed_market, BAKE, BREAD, DUCKATS};
    use super::PlayState;
    use crate::game::job::{Job, JobLine};

    /// [`closed_market`] with no minting and no coin decay: pop 1 bakes
    /// instead of minting, and Duckats keep. Pop 1 owns the firm.
    fn no_new_coin() -> PlayState {
        let mut state = closed_market();
        state.factuals.goods.get_mut(&DUCKATS).expect("duckats").decay_rate = 0.0;
        state.actors.pop_mut(1).job = Job::new([
            JobLine::new(BAKE, None, Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        state
    }

    #[test]
    fn keep_total_coin_fixed_for_a_thousand_days() {
        let mut state = no_new_coin();
        let start = state.coin_holdings()[&DUCKATS];
        for _ in 0..1000 {
            state.advance_turn();
            let flows = state.ledger.days.last().expect("a day").coins[&DUCKATS];
            assert_eq!(flows.sources(), 0.0);
            assert_eq!(flows.sinks(), 0.0);
            assert!((flows.closing - start).abs() <= 1e-9 * start, "{flows:?}");
        }
        assert!(state.ledger.days.iter().any(|day| day.coins[&DUCKATS].transfers > 0.0));
    }

    #[test]
    fn pay_the_owner_pop_what_the_firm_pays_out() {
        // Only pop 1 holds the firm, so only pop 1 is paid, and it is paid
        // the firm's whole wage and dividend.
        let mut state = no_new_coin();
        let mut paid = 0.0;
        for _ in 0..1000 {
            state.advance_turn();
            let firm = state.actors.firm(1);
            let out = firm.budget.wage + firm.budget.dividend;
            assert!((state.actors.pop(1).records.owner_pay - out).abs() < 1e-9);
            for id in [2, 3] {
                assert_eq!(state.actors.pop(id).records.owner_pay, 0.0);
            }
            paid += out;
        }
        assert!(paid > 0.0);
    }

    #[test]
    fn repeat_the_same_run_from_the_same_start() {
        // The market day's rng is seeded from the turn, so two runs match.
        let mut first = no_new_coin();
        let mut second = no_new_coin();
        for _ in 0..200 {
            first.advance_turn();
            second.advance_turn();
        }
        assert_eq!(first.ledger, second.ledger);
        for id in [1, 2, 3] {
            assert_eq!(first.actors.pop(id).holding(DUCKATS), second.actors.pop(id).holding(DUCKATS));
            assert_eq!(first.actors.pop(id).holding(BREAD), second.actors.pop(id).holding(BREAD));
        }
        assert_eq!(first.market.price(BREAD), second.market.price(BREAD));
        assert_eq!(first.market.price(DUCKATS), second.market.price(DUCKATS));
    }
}
