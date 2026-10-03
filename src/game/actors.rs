use std::collections::HashMap;

use rayon::prelude::*;

use crate::game::actor::Actor;
use crate::game::deal::DealMaker;
use crate::game::market::Market;
use crate::game::scalingfactor::ScalingFactor;
use crate::game::{factuals::Factuals, firm::Firm, institution::Institution, pop::Pop};

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
    pub fn new() -> Self {
        Self {
            pops: HashMap::new(),
            firms: HashMap::new(),
            institutions: HashMap::new(),
        }
    }

    /// The actor stored for this id. Panics if the id is not in the set.
    pub fn get(&self, actor: Actor) -> &dyn DealMaker {
        match actor {
            Actor::Pop(id) => self
                .pops
                .get(&id)
                .unwrap_or_else(|| panic!("pop {id} is not in Actors")),
            Actor::Firm(id) => self
                .firms
                .get(&id)
                .unwrap_or_else(|| panic!("firm {id} is not in Actors")),
            Actor::Institution(id) => self
                .institutions
                .get(&id)
                .unwrap_or_else(|| panic!("institution {id} is not in Actors")),
            Actor::State(id) => panic!("state {id} is not stored in Actors"),
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

    /// # Pops In Craft
    ///
    /// Pop ids whose job has this craft, lowest id first.
    ///
    /// Craft `0` is no job and returns an empty list. A pop whose craft
    /// differs is left out. This is a lookup: each pop keeps its own job.
    pub fn pops_in_craft(&self, craft: usize) -> Vec<usize> {
        if craft == 0 {
            return Vec::new();
        }
        let mut ids: Vec<usize> = self
            .pops
            .iter()
            .filter(|(_, pop)| pop.job.has_craft(craft))
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Mutable access to the actor stored for this id. Panics if it is missing.
    pub fn get_mut(&mut self, actor: Actor) -> &mut dyn DealMaker {
        match actor {
            Actor::Pop(id) => self
                .pops
                .get_mut(&id)
                .unwrap_or_else(|| panic!("pop {id} is not in Actors")),
            Actor::Firm(id) => self
                .firms
                .get_mut(&id)
                .unwrap_or_else(|| panic!("firm {id} is not in Actors")),
            Actor::Institution(id) => self
                .institutions
                .get_mut(&id)
                .unwrap_or_else(|| panic!("institution {id} is not in Actors")),
            Actor::State(id) => panic!("state {id} is not stored in Actors"),
        }
    }

    /// # Decay Goods
    ///
    /// Runs end-of-day good decay on every actor store. Pops, firms, and
    /// institutions are disjoint and do not need each other — each map is
    /// processed in parallel, and entries within a map use `par_iter_mut`.
    ///
    /// Per-actor logic lives on [`Pop::decay_goods`], [`Firm::decay_goods`],
    /// and [`Institution::decay_goods`].
    pub(crate) fn decay_goods(&mut self, factuals: &Factuals) {
        let pops = &mut self.pops;
        let firms = &mut self.firms;
        let institutions = &mut self.institutions;

        rayon::scope(|s| {
            s.spawn(|_| {
                pops.par_iter_mut()
                    .for_each(|(_, pop)| {
                        let _ = pop.decay_goods(factuals);
                    });
            });
            s.spawn(|_| {
                firms
                    .par_iter_mut()
                    .for_each(|(_, firm)| {
                        let _ = firm.decay_goods(factuals);
                    });
            });
            s.spawn(|_| {
                institutions
                    .par_iter_mut()
                    .for_each(|(_, institution)| institution.decay_goods(factuals));
            });
        });
    }
    
    /// # Day Start
    ///
    /// Gives each pop its morning goods and records them on that pop's market.
    ///
    /// `markets` is every market, keyed by id. Each pop gains good 0 equal to
    /// its labor. A positive amount is recorded with [`Market::note_supply`].
    /// A pop listed on no market still receives the goods.
    pub fn start_day(&mut self, markets: &mut HashMap<usize, Market>) {
        let homes = pop_markets(markets);
        let time_gen = [(0, ScalingFactor::Labor(1.0))];
        for (pop_id, pop) in self.pops.iter_mut() {
            let added = pop.start_day(&time_gen);
            let Some(&market_id) = homes.get(pop_id) else {
                continue;
            };
            let market = markets.get_mut(&market_id).unwrap_or_else(|| {
                panic!("market {market_id} is not in the day-start set")
            });
            for (good_id, amount) in added {
                if amount > 0.0 {
                    market.note_supply(good_id, amount);
                }
            }
        }
    }
}

/// # Pop Markets
///
/// Maps each pop id to the market that lists it.
///
/// `markets` is the set passed to [`Actors::start_day`]. A pop listed in
/// several markets maps to the lowest market id. A pop listed in none is
/// absent.
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::Actors;
    use crate::game::market::{Market, MarketGood};
    use crate::game::pop::Pop;

    fn pop_with_adults(id: usize, adults: f64) -> Pop {
        let mut pop = Pop::new(id);
        pop.demographics.household.adult = adults;
        pop.demographics.household.elder = 0.0;
        pop.demographics.household.child = 0.0;
        pop
    }

    #[test]
    fn start_day_records_time_on_the_pops_market() {
        let mut actors = Actors::new();
        actors.pops.insert(1, pop_with_adults(1, 4.0));
        actors.pops.insert(2, pop_with_adults(2, 2.0));
        actors.pops.insert(3, pop_with_adults(3, 1.0));
        actors.pops.insert(4, pop_with_adults(4, 3.0));

        let mut west = Market::new(7);
        west.pops.insert(1);
        west.pops.insert(2);
        let mut time = MarketGood::new();
        time.stock = 3.0;
        west.goods.insert(0, time);
        let mut east = Market::new(8);
        east.pops.insert(3);
        let mut markets = HashMap::new();
        markets.insert(west.id, west);
        markets.insert(east.id, east);

        actors.start_day(&mut markets);

        let west_time = &markets[&7].goods[&0];
        assert!((west_time.production - 6.0).abs() < 1e-12);
        assert!((west_time.stock - 3.0).abs() < 1e-12);
        let east_time = &markets[&8].goods[&0];
        assert!((east_time.production - 1.0).abs() < 1e-12);
        assert_eq!(east_time.stock, 0.0);
        assert!((actors.pop(1).property[&0].quantity - 4.0).abs() < 1e-12);
        assert!((actors.pop(4).property[&0].quantity - 3.0).abs() < 1e-12);
        assert!(!markets[&7].goods.contains_key(&1));
        assert!(!markets[&8].pops.contains(&4));
    }
}
