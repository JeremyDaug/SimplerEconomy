use std::collections::HashMap;

use rayon::prelude::*;

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
                    .for_each(|(_, pop)| pop.decay_goods(factuals));
            });
            s.spawn(|_| {
                firms
                    .par_iter_mut()
                    .for_each(|(_, firm)| firm.decay_goods(factuals));
            });
            s.spawn(|_| {
                institutions
                    .par_iter_mut()
                    .for_each(|(_, institution)| institution.decay_goods(factuals));
            });
        });
    }
}
