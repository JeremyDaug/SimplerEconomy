use std::collections::HashMap;

use crate::game::{factuals::Factuals, firm::Firm, market::Market, pop::Pop, scalingfactor::ScalingFactor};

/// # Actors
///
/// Common storage for active game actors (pops and firms).
/// The market and other systems should hold membership ids / indexes, not
/// duplicate ownership of these entities.
#[derive(Debug)]
pub struct Actors {
    pub pops: HashMap<usize, Pop>,
    pub firms: HashMap<usize, Firm>,
}

impl Actors {
    /// # New
    ///
    /// Empty pop and firm maps.
    pub fn new() -> Self {
        Self {
            pops: HashMap::new(),
            firms: HashMap::new(),
        }
    }

    /// # Pop
    ///
    /// The pop stored under `id`.
    ///
    /// Panics if that pop is missing.
    pub fn pop(&self, id: usize) -> &Pop {
        self.pops
            .get(&id)
            .unwrap_or_else(|| panic!("pop {id} is not in Actors"))
    }

    /// # Pop Mut
    ///
    /// Mutable access to the pop stored under `id`.
    ///
    /// Panics if that pop is missing.
    pub fn pop_mut(&mut self, id: usize) -> &mut Pop {
        self.pops
            .get_mut(&id)
            .unwrap_or_else(|| panic!("pop {id} is not in Actors"))
    }

    /// # Firm
    ///
    /// The firm stored under `id`.
    ///
    /// Panics if that firm is missing.
    pub fn firm(&self, id: usize) -> &Firm {
        self.firms
            .get(&id)
            .unwrap_or_else(|| panic!("firm {id} is not in Actors"))
    }

    /// # Firm Mut
    ///
    /// Mutable access to the firm stored under `id`.
    ///
    /// Panics if that firm is missing.
    pub fn firm_mut(&mut self, id: usize) -> &mut Firm {
        self.firms
            .get_mut(&id)
            .unwrap_or_else(|| panic!("firm {id} is not in Actors"))
    }

    /// # Day Start
    ///
    /// Gives each pop its morning goods and records them on the market.
    ///
    /// `market` is the market. `factuals` supplies `labor.time_per_labor`.
    /// Each pop gains good 0 at that rate per unit of household labor. The
    /// default is [`crate::game::config::TIME_PER_LABOR`]. A positive amount
    /// is recorded with [`Market::record_supply`]. A pop not listed on the
    /// market still receives the goods, and they are not recorded.
    pub fn start_day(&mut self, market: &mut Market, factuals: &Factuals) {
        let time_per_labor = factuals.config.labor.time_per_labor;
        let time_gen = [(0, ScalingFactor::Labor(time_per_labor))];
        for (pop_id, pop) in self.pops.iter_mut() {
            let added = pop.start_day(&time_gen);
            if !market.pops.contains(pop_id) {
                continue;
            }
            for (good_id, amount) in added {
                if amount > 0.0 {
                    market.record_supply(good_id, amount);
                }
            }
        }
    }

    /// # Record Keeping
    ///
    /// Closes the day for each pop and each firm.
    ///
    /// `factuals` supplies the world. Each pop runs [`Pop::record_keeping`]
    /// on its own thread. Each firm then runs [`Firm::record_keeping`] on
    /// its own thread.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let pops = &mut self.pops;
        std::thread::scope(|scope| {
            for pop in pops.values_mut() {
                scope.spawn(|| pop.record_keeping());
            }
        });
        let firms = &mut self.firms;
        std::thread::scope(|scope| {
            for firm in firms.values_mut() {
                scope.spawn(|| firm.record_keeping(factuals));
            }
        });
    }

    /// # Decay
    ///
    /// Decays each pop's goods and desires, and each firm's goods.
    ///
    /// `factuals` supplies the goods. Each pop runs [`Pop::decay_goods`] and
    /// [`Pop::decay_desires`] on its own thread. Each firm runs
    /// [`Firm::decay_goods`] on its own thread.
    pub fn decay(&mut self, factuals: &Factuals) {
        let pops = &mut self.pops;
        let firms = &mut self.firms;
        std::thread::scope(|scope| {
            for pop in pops.values_mut() {
                scope.spawn(|| {
                    pop.decay_goods(factuals);
                    pop.decay_desires();
                });
            }
            for firm in firms.values_mut() {
                scope.spawn(|| {
                    firm.decay_goods(factuals);
                });
            }
        });
    }

    /// # Planning
    ///
    /// Runs planning for each pop and firm.
    ///
    /// `factuals` supplies the world. `market` is the market. Each actor
    /// runs on its own thread. A pop or firm listed on the market plans
    /// against it. One that is not listed plans with no market.
    pub fn planning(&mut self, factuals: &Factuals, market: &Market) {
        let pops = &mut self.pops;
        let firms = &mut self.firms;
        std::thread::scope(|scope| {
            for pop in pops.values_mut() {
                let read = market.pops.contains(&pop.id).then_some(market);
                scope.spawn(move || pop.planning(factuals, read));
            }
            for firm in firms.values_mut() {
                let read = market.firms.contains(&firm.id).then_some(market);
                scope.spawn(move || firm.planning(factuals, read));
            }
        });
    }

    /// # Sentiments
    ///
    /// Runs sentiments for each pop.
    ///
    /// `market` is the market. Each pop runs on its own thread. A pop
    /// listed on the market prices its wealth against it. The mood is
    /// updated first, then stored satisfaction is dropped.
    pub fn sentiments(&mut self, market: &Market) {
        let pops = &mut self.pops;
        std::thread::scope(|scope| {
            for pop in pops.values_mut() {
                let read = market.pops.contains(&pop.id).then_some(market);
                scope.spawn(move || {
                    pop.update_sentiments(read);
                    pop.calculate_sol();
                });
            }
        });
    }
}
