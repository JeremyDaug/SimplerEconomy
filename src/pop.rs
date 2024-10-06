use std::collections::HashMap;

use crate::{data::Data, desire::Desire, market::Market};

/// # Pop
/// 
/// A number of households grouped together into one unit.
#[derive(Debug, Clone)]
pub struct Pop {
    /// Unique Id of the pop.
    pub id: usize,
    /// how many households are in it. Should be whole numbers.
    pub size: f64,
    /// Desires are what the pop wants. The first is always item 0 Food,
    /// the last is always item 1 Leisure. The last item is infinitely desireable.
    /// TODO replace with culture.
    pub culture: usize,
    /// How many days worth of work a single household in the group does.
    pub efficiency: f64,
    /// What property the pop owns today.
    pub property: HashMap<usize, f64>,
    /// How much time is currently stored up in the pop.
    pub unused_time: f64,
}

impl Pop {
    /// # Get Desires
    /// 
    /// A helper which gets the desire data for a pop.
    /// 
    /// Needs a reference to shared data to get it.
    pub fn get_desires(&self, data: &Data) -> Vec<Desire> {
        data.cultures.get(&self.culture)
            .expect("Culture Not Found!")
                .desires.clone()
    }

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
    /// When dealing with 2 offers directly, we call thes to check if a barter 
    /// trade is valid or invalid.
    /// 
    /// Whether it is accepted follows the following rules.
    /// 
    /// - Items which add or remove from satisfaction are have a value based on 
    ///   the level it satisfies. Each level past the first is worth 0.9 of the
    ///   level below it.
    ///   - Items which are above what is needed counts for 0.33 of the original 
    ///     satisfaction. Every additional amount beyond that first reduces it an 
    ///     additional 0.33 multiplicatively. It should take about 10 levels 
    ///     before these lower ranking extra desires start being felt.
    /// - Any items which don't apply to desires directly are counted by AMV.
    ///   This AMV gained or lost, is then given an estimate of the satisfaction
    ///   it could satisfy if traded. This allows us to compare losses between 
    ///   goods which do satisfy with goods that don't, or some mixture of the two.
    /// - Lastly, as a shortcut, if no goods being offered or lost satisfy desires, 
    ///   then we simply compare the relative AMVs of the goods.
    pub fn check_barter(&self, offer: HashMap<usize, f64>, 
    request: HashMap<usize, f64>, market: &Market, data: &Data) -> bool {
        // clone our data
        let mut after_trade = self.clone();
        // do the trade and go onwards.
        for (&good, &amt) in offer.iter() {
            after_trade.property.entry(good)
                .and_modify(|x| *x += amt)
                .or_insert(amt);
        }
        for (&good, &amt) in request.iter() {
            after_trade.property.entry(good)
                .and_modify(|x| *x -= amt);
        }
        // with both gotten, try to buy goods with our current excess AMV for each.
        let mut current_total = self.possible_satisfaciton_gain(None, market, data);
        let mut resulting_total = after_trade.possible_satisfaciton_gain(None, market, data);
        // subtract the current total from the resulting total to see the difference of the two.
        debug_assert_eq!(current_total.len(), resulting_total.len(), "Current and resulting length mismatch.");
        // with diff created, consolidate into simple value.
        // we can also liquidate the data in our diff satisfaction while we're at it.
        let mut balance = 0.0;
        // iterate while current total and resulting total have any values in them.
        let mut step_mult = 1.0;
        while current_total.iter().any(|x| *x > 0.0) || resulting_total.iter().any(|x| *x > 0.0) {
            for idx in 0..resulting_total.len() {
                let idx_mult = 0.9_f64.powf(idx as f64);
                // subtract current from resulting, capping both at pop size.
                let res_sat =  resulting_total.get_mut(idx).unwrap();
                let cur_sat =  current_total.get_mut(idx).unwrap();
                let diff = res_sat.min(self.size) - cur_sat.min(self.size);
                // subtract from both.
                *res_sat -= res_sat.min(self.size);
                *cur_sat -= cur_sat.min(self.size);
                // add diff, scaled to our current position add the difference to our balance.
                if diff != 0.0 {
                    balance += diff * idx_mult * step_mult;
                }
            }
            step_mult *= 0.33;
        }

        // with our balance calculated, just check if it's positive or negative
        balance > 0.0
    }

    /// # Possible Satisfaction Gain
    /// 
    /// Returns the satisfaction we could get assuming all purchases succeed.
    pub fn possible_satisfaciton_gain(&self, satisfaction: Option<(Vec<f64>, f64, f64)>, 
    market: &Market, data: &Data) -> Vec<f64> {
        let (sat, _, mut excess_amv) = if let Some(sat) = satisfaction {
            sat
        } else {
            self.current_overall_satisfaction(market, data)
        };
        assert!(sat.len() > 0, "Satisfacitons must have some length.");
        // get our edsires
        let desires = self.get_desires(data);
        // copy our satisfaction, then add to this which we'll return.
        let mut result = sat.clone();
        // The current desire index we're looking at.
        let mut idx = 0;
        // how many steps back we've done so far.
        let mut step = 1.0;
        while excess_amv > 0.0 { // while any are not done, continue.
            // Do the current idx level.
            let sat_at_idx = *result.get(idx).unwrap();
            let stepped_desire_target = self.size * step;
            if sat_at_idx <= stepped_desire_target {
                // if current satisfaction is below our current target, try to buy up to that.
                let diff = stepped_desire_target - sat_at_idx; // difference in units
                let good_id = desires.get(idx).unwrap().unwrap(); // our desired good
                let good_amv = market.goods_info.get(&good_id).unwrap().amv; // the good's amv
                let can_buy = excess_amv / good_amv; // how many we can by at price
                let buy = can_buy.min(diff); // how many we will buy.
                *result.get_mut(idx).unwrap() += buy; // add to our results.
                excess_amv -= buy * good_amv;
            }

            // do lower idx levels
            let mut stepped_idx = idx;
            while stepped_idx >= 10 {
                let sat_at_idx = *result.get(stepped_idx).unwrap();
                let stepped_desire_target = self.size * step;
                stepped_idx -= 10;
                if sat_at_idx <= stepped_desire_target {
                    // if current satisfaction is below our current target, try to buy up to that.
                    let diff = stepped_desire_target - sat_at_idx; // difference in units
                    let good_id = desires.get(stepped_idx).unwrap().unwrap(); // our desired good
                    let good_amv = market.goods_info.get(&good_id).unwrap().amv; // the good's amv
                    let can_buy = excess_amv / good_amv; // how many we can by at price
                    let buy = can_buy.min(diff); // how many we will buy.
                    *result.get_mut(stepped_idx).unwrap() += buy; // add to our results.
                    excess_amv -= buy * good_amv;
                }
            }

            idx += 1;
            if idx == sat.len() {
                step += 1.0; // increment step by 1.
                idx = idx.saturating_sub(10);
            }
        }

        result
    }

    /// # Current  Overall Satisfaction
    /// 
    /// Calculates how satisfied the pop is given their current property.
    /// 
    /// Returns a list of how satisfied each desire measured in units applied to it
    /// and the "excess" Leisure time the pop has on top of that
    /// and the excess AMV available to us.
    pub fn current_overall_satisfaction(&self, market: &Market, data: &Data) -> (Vec<f64>, f64, f64) {
        // results we return
        let desires = self.get_desires(data);
        let mut results = vec![0.0; desires.len()];
        // Checker to ensure we've done everything we can. Set idx to true when
        // we fail to add more to that section.
        let mut done = vec![false; desires.len()];
        // property that has not yet been applied to our desires.
        let mut available = self.property.clone();
        let end = desires.len();
        let mut idx = 0;
        while done.iter().any(|x| !*x) { // while any are not done, continue.
            let completed = done.iter().filter(|&&x| x).count();
            // println!("Start of While: Completed parts = {}", completed);
            // println!("Current Index: {}", idx);
            // println!("Is Done at idx? {}", done.get(idx).unwrap());
            // Do the current desire level.
            if !done.get(idx).unwrap() { // skip if this desire is done.
                let desire = desires.get(idx).unwrap();
                match desire {
                    Desire::Consume(id) |
                    Desire::Own(id) => {
                        // get how much we can shift, and shift
                        // TODO when counts_as has been added, this will need to be expanded to cover those other goods.
                        let has = available.get(id).unwrap_or(&0.0).min(self.size);
                        // println!("Available: {}", has);
                        *results.get_mut(idx).unwrap() += has;
                        available.entry(*id).and_modify(|x| *x -= has);
                        if has < self.size { 
                            // println!("Finished idx: {}", idx);
                            // if not able to fully satisfy this level, mark as done.
                            *done.get_mut(idx).unwrap() = true;
                        }
                    },
                }
            }
            // then downgrade by 10 levels and try to fill them, if possible.
            let mut stepped_idx = idx;
            // println!("Before Stepdown");
            while let  Some(step_down) = stepped_idx.checked_sub(10) {
                // step down fully
                stepped_idx = step_down;
                // println!("stepped_idx: {}", stepped_idx);
                // println!("Is Done at idx? {}", done.get(stepped_idx).unwrap());
                if !done.get(stepped_idx).unwrap() { // if curr_idx desire is not done
                    let desire = desires.get(stepped_idx).unwrap();
                    match desire {
                        Desire::Consume(id) |
                        Desire::Own(id) => {
                            // get how much we can shift, and shift
                            // TODO when counts_as has been added, this will need to be expanded to cover those other goods.
                            let has = available.get(id).unwrap_or(&0.0).min(self.size);
                            //println!("Available: {}", has);
                            *results.get_mut(stepped_idx).unwrap() += has;
                            available.entry(*id).and_modify(|x| *x -= has);
                            if has < self.size { 
                                //println!("Finished idx: {}", stepped_idx);
                                // if not able to fully satisfy this level, mark as done.
                                *done.get_mut(stepped_idx).unwrap() = true;
                            }
                        },
                    }
                }
            }

            // current index and downsteps have been done
            idx += 1;
            if idx == end {
                // if we'd walk off the end, downstep by up to 10.
                // println!("Reduce idx by 10.");
                idx = idx.saturating_sub(10);
            }
            // go back to top.
        }
        // take all remaining goods and liquidated them for AMV, include salability.
        let mut excess_amv = 0.0;
        for (good, amt) in available.into_iter() {
            let good_data = market.goods_info.get(&good).unwrap();
            let amv = good_data.amv;
            let sal = good_data.salability;
            excess_amv += Market::salibility_amv_mod(sal) * amv * amt;
        }

        (results, self.unused_time / self.size, excess_amv)
    }

    /// # Excess Goods
    /// 
    /// Get those goods which are totally undesired by the pop.
    pub fn excess_goods(&self, data: &Data) -> HashMap<usize, f64> {
        let desires = self.get_desires(data);
        let mut excess = self.property.clone();
        for desire in desires.iter() {
            match desire {
                Desire::Consume(id) |
                Desire::Own(id) => {
                    // if we have a desire for it, remove it from our excess
                    let has = excess.get(id).unwrap_or(&0.0).min(self.size);
                    //println!("Current Excess {}: {}", id, has);
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
    /// And the average wealth of the pop (excess AMV / size)
    /// 
    /// Do not use to get starving pops, it will not work.
    pub fn satisfaction_spread(&self, market: &Market, data: &Data, 
    satisfaction: Option<(Vec<f64>, f64, f64)>) -> (f64, f64, f64) {
        let (sat_levels, ex_time, ex_amv) = if let Some(sat) = satisfaction {
            sat
        } else {
            self.current_overall_satisfaction(market, data)
        };
        let mut min = ex_time.floor();
        let mut max = ex_time.ceil();

        for &level in sat_levels.iter() {
            if level >= self.size {
                min += 1.0;
                max += 1.0;
            } else if level > 0.0 {
                max += 1.0;
            }
        }

        (min, max, ex_amv / self.size)
    }

    /// # Starving Pops
    /// 
    /// Gets how many pops in this pop is starving.
    pub fn starving_pops(&self,  market: &Market, data: &Data, 
    satisfaction: Option<(Vec<f64>, f64, f64)>) -> f64 {
        let (sat_levels, _, _) = if let Some(sat) = satisfaction {
            sat
        } else {
            self.current_overall_satisfaction(market, data)
        };

        // only need the first desire in the list to be checked.
        let fed = *sat_levels.first().unwrap_or(&0.0);

        self.size - fed
    }

    /// # Consume Goods
    /// 
    /// Consume (or use) any goods needed to satisfy desires. Does not return 
    /// success or satisfaction, just consooms.
    pub fn consume_goods(&mut self, market: &Market, data: &Data, 
    satisfaction: Option<(Vec<f64>, f64, f64)>) {
        // Set extra time to 0.
        self.unused_time = 0.0;
        // start consuming/using goods
        let (sat_levels, _, _) = if let Some(sat) = satisfaction {
            sat
        } else {
            self.current_overall_satisfaction(market, data)
        };
        let desires = self.get_desires(data);

        for (idx, satisfaction) in sat_levels.iter().enumerate() {
            let desire = desires.get(idx).expect("Walked off end of desire array!");
            let satisfaction = satisfaction.min(self.size); // cap what is consumed to the size of the pop.
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
    
    /// # Reset Time
    /// 
    /// Resets the pop's time for today.
    pub fn reset_time(&mut self) {
        self.unused_time = self.available_time();
    }
}