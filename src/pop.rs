use std::collections::HashMap;

use crate::desire::Desire;

/// # Pop
/// 
/// A number of households grouped together into one unit.
pub struct Pop {
    /// Unique Id of the pop.
    pub id: usize,
    /// how many households are in it. Should be whole numbers.
    pub size: f64,
    /// Desires are what the pop wants. The first is always item 0 Food,
    /// the last is always item 1 Leisure. The last item is infinitely desireable.
    pub desires: Vec<Desire>,
    /// How many days worth of work a single household in the group does.
    pub efficiency: f64,
    /// What property the pop owns today.
    pub property: HashMap<usize, f64>,
    /// How much time is currently stored up in the pop.
    pub unused_time: f64,
}

impl Pop {
    /// # Available Time
    /// 
    /// How much time the pop has each day.
    /// 
    /// Based on the size and current efficiency of the pop (IE how many working adult equivalents is included per 'pop')
    pub fn available_time(&self) -> f64 {
        self.size * self.efficiency
    }

    /// # Check barter
    /// 
    /// When dealing with 2 goods directly, we call thes to check if a barter 
    /// trade is valid or invalid.
    /// 
    /// It's assumed that all goods of higher priority are worth twice any 
    /// good of a lower priority by default. May expand this based on priority 
    /// difference.
    pub fn check_barter(&self, offer_good: usize, offer_amt: f64, request_good: usize, request_amt: f64) -> bool {
        // check desires, skipping those not included here.
        let mut offer_max = 0.0; // the total sum amount that we want for the offered good
        //let mut offer_idcs = vec![]; // the idcs of the offered item.
        let mut request_max = 0.0; // The total sum amount of the requested good we desire.
        //let mut request_idcs = vec![]; // the idcs of the requested item.

        // todo come back here later when market info is available.

        false
    }

    /// # Current  Overall Satisfaction
    /// 
    /// Calculates how satisfied the pop is given their current property.
    /// 
    /// Returns a list of how satisfied each desire is
    /// and the "excess" Leisure time the pop has on top of that.
    pub fn current_overall_satisfaction(&self) -> (Vec<f64>, f64) {
        let mut results = vec![];

        let mut available = self.property.clone();
        for desire in self.desires.iter() {
            match desire {
                Desire::Consume(id) |
                Desire::Own(id) => {
                    let has = *available.get(id).unwrap_or(&0.0);
                    results.push(has);
                    available.entry(*id).and_modify(|x| *x -= has);
                },
            }
        }

        (results, self.unused_time / self.size)
    }

    /// # Excess Goods
    /// 
    /// Get those goods which are totally undesired by the pop.
    pub fn excess_goods(&self) -> HashMap<usize, f64> {
        let mut excess = self.property.clone();
        for desire in self.desires.iter() {
            match desire {
                Desire::Consume(id) |
                Desire::Own(id) => {
                    // if we have a desire for it, remove it from our excess
                    let has = *excess.get(id).unwrap_or(&0.0);
                    excess.entry(*id).and_modify(|x| *x -= has);
                    if *excess.get(id).unwrap() == 0.0 {
                        excess.remove(id);
                    }
                },
            }
        }

        excess
    }

    /// # Satisfaction Spread
    /// 
    /// Calculates the lowest and highest level of satisfaction in the pop (min, max)
    /// 
    /// Lowest is calculated by the number of desires which are fully satisfied
    /// plus excess tiers of liesure time.
    /// 
    /// Highest is calculated by the number of desires with any satisfaction 
    /// plus excess leisure time rounded up.
    /// 
    /// Do not use to get starving pops, it will not work.
    pub fn satisfaction_spread(&self, satisfaction: Option<(Vec<f64>, f64)>) -> (f64, f64) {
        let (sat_levels, ex_time) = if let Some(sat) = satisfaction {
            sat
        } else {
            self.current_overall_satisfaction()
        };
        let mut min = ex_time.floor();
        let mut max = ex_time.ceil();

        for &level in sat_levels.iter() {
            if level == self.size {
                min += 1.0;
                max += 1.0;
            } else if level > 0.0 {
                max += 1.0;
            }
        }

        (min, max)
    }

    /// # Starving Pops
    /// 
    /// Gets how many pops in this pop is starving.
    pub fn starving_pops(&self, satisfaction: Option<(Vec<f64>, f64)>) -> f64 {
        let (sat_levels, _) = if let Some(sat) = satisfaction {
            sat
        } else {
            self.current_overall_satisfaction()
        };

        // only need the first desire in the list to be checked.
        let fed = *sat_levels.first().unwrap_or(&0.0);

        self.size - fed
    }

    /// # Consume Goods
    /// 
    /// Consume (or use) any goods needed to satisfy desires. Does not return 
    /// success or satisfaction, just consooms.
    pub fn consume_goods(&mut self, satisfaction: Option<(Vec<f64>, f64)>) {
        // Set extra time to 0.
        self.unused_time = 0.0;
        // start consuming/using goods
        let (sat_levels, _) = if let Some(sat) = satisfaction {
            sat
        } else {
            self.current_overall_satisfaction()
        };

        for (idx, satisfaction) in sat_levels.iter().enumerate() {
            let desire = self.desires.get(idx).expect("Walked off end of desire array!");
            match desire {
                Desire::Consume(id) => {
                    self.property.entry(*id).and_modify(|x| *x -= satisfaction);
                    if *self.property.get(id).unwrap() == 0.0 {
                        self.property.remove(id);
                    }
                },
                Desire::Own(_) => {}, // own desires don't get consumed, so we don't need to do anything.
            }
        }
    }
}