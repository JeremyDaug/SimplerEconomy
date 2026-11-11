use std::collections::HashMap;

use crate::game::{factuals::Factuals, firm::Firm, institution::Institution, market::Market, pop::Pop, scalingfactor::ScalingFactor};

/// # Actors
///
/// Common storage for active game actors (pops, firms, institutions, …).
/// Markets and other systems should hold membership ids / indexes, not duplicate
/// ownership of these entities.
#[derive(Debug)]
pub struct Actors {
    pub pops: HashMap<usize, Pop>,
    pub firms: HashMap<usize, Firm>,
    pub institutions: HashMap<usize, Institution>,
    // Could hold spatial indices or tile->agent mappings for quick lookup
}

impl Actors {
    /// # New
    ///
    /// Empty pop, firm, and institution maps.
    pub fn new() -> Self {
        Self {
            pops: HashMap::new(),
            firms: HashMap::new(),
            institutions: HashMap::new(),
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

    /// # Institution
    ///
    /// The institution stored under `id`.
    ///
    /// Panics if that institution is missing.
    pub fn institution(&self, id: usize) -> &Institution {
        self.institutions
            .get(&id)
            .unwrap_or_else(|| panic!("institution {id} is not in Actors"))
    }

    /// # Institution Mut
    ///
    /// Mutable access to the institution stored under `id`.
    ///
    /// Panics if that institution is missing.
    pub fn institution_mut(&mut self, id: usize) -> &mut Institution {
        self.institutions
            .get_mut(&id)
            .unwrap_or_else(|| panic!("institution {id} is not in Actors"))
    }
    
    /// # Day Start
    ///
    /// Gives each pop its morning goods and records them on that pop's market.
    ///
    /// `markets` is every market, keyed by id. `factuals` supplies
    /// `labor.time_per_labor`. Each pop gains good 0 at that rate per unit of
    /// household labor. The default is [`crate::game::config::TIME_PER_LABOR`].
    /// A positive amount is recorded with [`Market::record_supply`]. A pop
    /// listed on no market still receives the goods.
    pub fn start_day(&mut self, markets: &mut HashMap<usize, Market>, factuals: &Factuals) {
        let homes = pop_markets(markets);
        let time_per_labor = factuals.config.labor.time_per_labor;
        let time_gen = [(0, ScalingFactor::Labor(time_per_labor))];
        for (pop_id, pop) in self.pops.iter_mut() {
            let added = pop.start_day(&time_gen);
            let Some(&market_id) = homes.get(pop_id) else {
                continue;
            };
            let market = markets
                .get_mut(&market_id)
                .unwrap_or_else(|| panic!("market {market_id} is not in the day-start set"));
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
    /// Runs planning for each pop, firm, and institution.
    ///
    /// `factuals` supplies the world. `markets` are the markets a pop may
    /// read. Each actor runs on its own thread. A pop listed on a market
    /// plans against that market. Firm and institution planning are not
    /// written yet.
    pub fn planning(&mut self, factuals: &Factuals, markets: &HashMap<usize, Market>) {
        let homes = pop_markets(markets);
        let pops = &mut self.pops;
        let firms = &mut self.firms;
        let institutions = &mut self.institutions;
        std::thread::scope(|scope| {
            for pop in pops.values_mut() {
                let market = homes.get(&pop.id).and_then(|id| markets.get(id));
                scope.spawn(move || pop.planning(factuals, market));
            }
            for firm in firms.values_mut() {
                scope.spawn(|| firm.planning(factuals));
            }
            for institution in institutions.values_mut() {
                scope.spawn(|| institution.planning(factuals));
            }
        });
    }
}

/// # Pop Markets
///
/// Maps each pop id to the market that lists it.
///
/// `markets` is the set passed to [`Actors::start_day`] and
/// [`Actors::planning`]. A pop listed in several markets maps to the lowest
/// market id. A pop listed in none is absent.
fn pop_markets(markets: &HashMap<usize, Market>) -> HashMap<usize, usize> {
    let mut homes = HashMap::new();
    let mut ids: Vec<usize> = markets.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        for &pop_id in &markets[&id].pops {
            homes.entry(pop_id).or_insert(id);
        }
    }
    homes
}