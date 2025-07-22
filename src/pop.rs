use core::f64;
use std::collections::{HashMap, VecDeque};

use itertools::Itertools;

use crate::{data::Data, desire::{Desire, DesireTag}, drow::DRow, household::Household, item::Item, markethistory::MarketHistory, offerresult::OfferResult};


use crate::constants::TIME_ID;

/// # Pop
/// 
/// A number of households grouped together into one unit.
/// 
/// ## Satisfaction and Desires
/// 
/// Currently, each desire 
#[derive(Debug, Clone)]
pub struct Pop {
    /// Unique Id of the pop.
    pub id: usize,
    /// The Id of the market our Pop is in
    pub market: usize,
    /// The ID of the firm the pop is working at.
    pub firm: usize,

    /// This is the collated households of the pop group, a the results of adding all
    /// demograpchic data together.
    pub households: Household,
    /// The actual population of the pop. This defines how many actual people are in this
    /// pop. How many mouths there are to feed and satisfy.
    /// 
    /// Eventually, this will likely be broken up into various sub-components, 
    /// Children, Adults, and Elders being the baseline. Species may alter the structure.
    /// 
    /// This uses fractional units to track population growth between turns.
    pub population: f64,
    /// Demographic Breakdown of the pop.
    /// This allows us to consolidate multiple categories of pop into a singular
    /// pop group. Pops have these enforced down into particular limitations,
    /// though this comes at the cost of increased processing and complexity.
    /// If different groups are in the same pop, then we assume they are being paid the same.
    /// If you want a pop to be paid differently, keep them separate.
    /// 
    /// The sum of each DRow should be equal to the size of the pop.
    /// 
    /// Fractional values in the breakdown represent stored up population growth.
    /// Fractions are dropped 
    /// 
    /// This is sorted by household count, largest to smallest.
    pub demo_breakdown: Vec<DRow>,
    /// How many days worth of work a single household in the group does.
    pub efficiency: f64,
    /// The consolidated desires of the pop, formed out of the consolidated desires of the pop.
    pub desires: VecDeque<Desire>,
    /// What property the pop owns and how they are using it.
    pub property: HashMap<usize, PropertyRecord>,
    /// What wants the pop currently has in their 
    pub wants: HashMap<usize, WantRecord>,
    /// Storage for satisfying desires between functions. This should be empty by the 
    /// end of the day.
    pub working_desires: VecDeque<Desire>,
    /// The current satisfaction of the pop. Should be updated periodically.
    pub satisfaction: SatisfactionValues,
}

impl Pop {
    pub fn new(id: usize, market: usize, firm: usize) -> Self {
        Pop {
            id,
            market,
            firm,
            households: Household::zeroed_household(),
            population: 0.0,
            demo_breakdown: vec![],
            efficiency: 1.0,
            desires: VecDeque::new(),
            property: HashMap::new(),
            wants: HashMap::new(),
            working_desires: VecDeque::new(),
            satisfaction: SatisfactionValues::zero()
        }
    }

    /// # Add Demo
    /// 
    /// Adds a demographic row to the pop. Does not combine households.
    /// 
    /// This does not update desires. Do that separately.
    pub fn add_demo(mut self, demo: DRow) -> Self {
        match self.demo_breakdown.binary_search_by(|x| x.household.count.total_cmp(&demo.household.count)) {
            Ok(pos) | 
            Err(pos) => self.demo_breakdown.insert(pos, demo),
        }
        self
    }

    /// # Include Demo
    /// 
    /// Adds a demographic row to the demographic breakdown in proper ordering.
    pub fn include_demo(&mut self, demo: DRow) {
        match self.demo_breakdown.binary_search_by(|x| x.household.count.total_cmp(&demo.household.count)) {
            Ok(pos) | 
            Err(pos) => self.demo_breakdown.insert(pos, demo),
        }
    }

    /// # Combine Households
    /// 
    /// Combines the households of the pop and combines them into one in the pop.
    /// 
    /// Does not handle Demographic Desires.
    pub fn combine_households(&mut self, _data: &Data) {
        self.households = Household::zeroed_household();
        for row in self.demo_breakdown.iter() {
            self.households.combine(&row.household);
        }
    }

    /// # Reset
    /// 
    /// Resets property and want's to just owned, zeroing out remainder.
    /// 
    /// Resets desire satisaction as well.
    pub fn reset(&mut self) {
        for (_, prop) in self.property.iter_mut() {
            prop.expended = 0.0;
            prop.offered = 0.0;
            prop.reserved = 0.0;
            prop.traded = 0.0;
            prop.used = 0.0;
        }
        for (_, want) in self.wants.iter_mut() {
            want.expected = 0.0;
            want.expended = 0.0;
            want.reserved = 0.0;
        }
        for desire in self.desires.iter_mut() {
            desire.satisfaction = 0.0;
        }

        self.satisfaction = SatisfactionValues::zero();
    }

    /// # Make Offer
    /// 
    /// Takes in the good(s) which is attempting to be purchased, market and good data
    /// and returns the goods being offered in return for those goods.
    /// 
    /// The goods being requested should satisfy more than is being sacrifised and
    /// the AMV cost gained should be higher than what is lost.
    /// 
    /// It also has an optional input for a pre-defined AMV value, typically given by.
    /// 
    /// ## Notes
    /// 
    /// price_hint should generally have a lower AMV value than the request itself. 
    /// This makes satisfying via the price a better AMV deal for the buyer.
    /// 
    /// This is not a hard rule, just a suggestion for later coding.
    pub fn make_offer(&self, request: &HashMap<usize, f64>, data: &Data, 
    market: &MarketHistory, price_hint: &HashMap<usize, f64>) -> HashMap<usize, f64>{
        // get the AMV of the request
        let mut req_amv = 0.0;
        // check if we have been given a price hint.
        if price_hint.len() > 0 {
            for (&good, &amt) in price_hint.iter() {
                req_amv += market.get_record(good).price * amt;
            }
        } else { // if no price hint, get market AMV for hint value.
            for (good, amt) in request.iter() {
                req_amv += market.get_record(*good).price * amt;
            }
        }
        println!("Requested AMV: {}", req_amv);
        // get the satisfaction gain from the request.
        let sat_gain = self.satisfaction_gain(request, data, market);
        println!("Sat Gained: {}", sat_gain.steps);

        // with our amv, and satisfaction gains, try to find things to give up that are worth more than
        // the AMV but less then our sat and levels gained.
        // Effectively, make change with our money, then do so with the rest of our goods.
        // start by trying to use just the price hint
        let mut offer_goods = HashMap::new();
        let mut offer_amv = 0.0;
        println!("Hint Section ---");
        for (good, prop_info) in self.property.iter()
        .filter(|x| price_hint.contains_key(x.0))
        .sorted_by(|a, b| {
            // iterate over our goods, sorting by current AMV value.
            let val_a = market.get_record(*a.0).price;
            let val_b = market.get_record(*b.0).price;
            val_b.total_cmp(&val_a)
        }) {
            println!("Good: {}", good);
            // start with most valuable and either get just enough, or all available for it.
            let unit_amv = market.get_record(*good).price;
            println!("Good AMV: {}", unit_amv);
             // get target, capped at available, and rounded down.
            let mut shift = price_hint.get(good).unwrap().min(prop_info.available()).floor();
            let mut debug_counter = 0;
            if shift > 0.0 { // check we can shift anything, if so, shift.
                loop { // find if we can add without hurting satisfaction too much.
                    println!("Shifting: {}", shift);
                    offer_goods.insert(*good, shift);
                    // check that the sacrifice is worth it
                    let sat_lost = self.satisfaction_lost(&offer_goods, data, market);
                    // TODO: Update this to target properly instead of estimating half reductions.
                    if sat_lost.steps > sat_gain.steps { // if too much, reduce by half (round down) and go back
                        shift = (shift / 2.0).floor();
                        offer_goods.remove(good);
                    } else { // if not overdrawing, break out and stay there.
                        // This should NEVER get us stuck as we never want to lose more satisfaction than we gain.
                        println!("Shifted: {}", shift);
                        break; 
                    }
                    if debug_counter > 9 {
                        assert!(false);
                    }
                    debug_counter += 1;
                }
            }
            offer_amv += shift * unit_amv;
            // if we get enough AMV, break out here
            if offer_amv >= req_amv {
                break;
            }
        }
        // check we're done or not.
        if offer_amv >= req_amv { // if so, return our offer.
            return offer_goods;
        }
        // Start by using any currencies of the market.
        println!("Money Section ---");
        for (good, prop_info) in self.property.iter()
        .filter(|x| market.currencies.contains(x.0) && // In currencies
            !price_hint.contains_key(x.0)) // and not the hint.
        .sorted_by(|a, b| {
            // iterate over our goods, sorting by current AMV value.
            let val_a = market.get_record(*a.0).price;
            let val_b = market.get_record(*b.0).price;
            val_b.total_cmp(&val_a)
        }) {
            println!("Good: {}", good);
            // start with most valuable and either get just enough, or all available for it.
            let unit_amv = market.get_record(*good).price;
            println!("Good AMV: {}", unit_amv);
            let target_amt = ((req_amv - offer_amv) / unit_amv).ceil();
             // get target, capped at available, and rounded down.
            let mut shift = target_amt.min(prop_info.available()).floor();
            let mut debug_counter = 0;
            if shift > 0.0 { // check we can shift anything, if so, shift.
                loop { // find if we can add without hurting satisfaction too much.
                    println!("Shifting: {}", shift);
                    offer_goods.insert(*good, shift);
                    // check that the sacrifice is worth it
                    let sat_lost = self.satisfaction_lost(&offer_goods, data, market);
                    // TODO: Update this to target properly instead of estimating half reductions.
                    println!("Satisfaciton Lost: {}", sat_lost.steps);
                    if sat_lost.steps > sat_gain.steps { // if too much, reduce by half (round down) and go back
                        shift = (shift / 2.0).floor();
                        offer_goods.remove(good);
                    } else { // if not overdrawing, break out and stay there.
                        // This should NEVER get us stuck as we never want to lose more satisfaction than we gain.
                        println!("Updated shift to: {}", shift);
                        break; 
                    }
                    if debug_counter > 9 {
                        assert!(false);
                    }
                    debug_counter += 1;
                }
            }
            offer_amv += shift * unit_amv;
            // if we get enough AMV, break out here
            if offer_amv >= req_amv {
                break;
            }
        }
        // check we're done or not.
        if offer_amv >= req_amv { // if so, return our offer.
            return offer_goods;
        }

        for (good, prop_info) in self.property.iter()
        .filter(|x| !market.currencies.contains(x.0) || // not in currencies
        !price_hint.contains_key(x.0)) // or not the hint.
        .sorted_by(|a, b| {
            // iterate over our goods, sorting by current AMV value.
            let val_a = market.get_record(*a.0).price;
            let val_b = market.get_record(*b.0).price;
            val_b.total_cmp(&val_a)
        }) {
            // start with most valuable and either get just enough, or all available for it.
            let unit_amv = market.get_record(*good).price;
            println!("Good AMV: {}", unit_amv);
            let target_amt = ((req_amv - offer_amv) / unit_amv).ceil();
             // get target, capped at available, and rounded down.
            let mut shift = target_amt.min(prop_info.available()).floor();
            if shift > 0.0 { // check we can shift anything, if so, shift.
                loop { // find if we can add without hurting satisfaction too much.
                    offer_goods.insert(*good, shift);
                    // check that the sacrifice is worth it
                    let sat_lost = self.satisfaction_lost(&offer_goods, data, market);
                    // TODO: Update this to target properly instead of estimating half reductions.
                    if sat_lost.steps > sat_gain.steps { // if too much, reduce by half (round down) and go back
                        shift = (shift / 2.0).floor();
                        offer_goods.remove(good);
                    } else { // if not overdrawing, break out and stay there.
                        // This should NEVER get us stuck as we never want to lose more satisfaction than we gain.
                        break; 
                    }
                }
            }
            offer_amv += shift * unit_amv;
            // if we get enough AMV, break out here
            if offer_amv >= req_amv {
                break;
            }
        }
        // if we get here, regardless of whether we actually have a 'good' offer or not
        // return it and see what happens.
        offer_goods
    }

    /// # Check Offer
    /// 
    /// Takes in the results of an offer made by someone else and checks that the offer is
    /// worth it to the pop.
    /// 
    /// Includes the requested goods, the offer made for it, and the price hint it originally given.
    /// 
    /// It's a simple check of satisfaction gained vs lost.
    /// 
    /// Pops currently do not return change, that's for businesses as pops have no reputation to
    /// protect.
    pub fn check_offer(&self, request: &HashMap<usize, f64>, offer: &HashMap<usize, f64>,
    price_hint: &HashMap<usize, f64>, data: &Data, market: &MarketHistory) -> OfferResult {
        // start by checking against the price hint, 
        // if it's valid (greater than or equal to on all parts) accept
        // we assume that the price hint was correctly calculated in the first place.
        let mut acceptable = true;
        for (good, amt) in offer.iter() {
            if let Some(req_amt) = price_hint.get(good) {
                if req_amt > amt {
                    acceptable = false;
                }
            }
            if !acceptable {
                break;
            }
        }
        // if beat price hint, and we have a price hit, return success.
        if acceptable && price_hint.len() > 0 {
            println!("Price Hint exists and offer meets it.");
            return OfferResult::Accept;
        }

        // if it doesn't meet the price hint, check sat change and include possible
        // gain from AMV.
        // Combine request and offer for satisfaction change checking.
        let mut change = offer.clone();
        for (&good, &amt) in request.iter() {
            change.entry(good)
                .and_modify(|x| *x -= amt)
                .or_insert(-amt);
        }
        let dup =  self.clone();
        let change = dup.satisfaction_change(&change, data, market);
        println!("Changed Steps: {}", change.steps);
        println!("Changed Range: {}", change.range);
        println!("Changed AMV: {}", change.amv);
        if change.steps > 0.0 {
            // if steps increase, don't care about range and accept.
            return OfferResult::Accept;
        } 
        let self_density = self.satisfaction.density();
        let change_density = change.density();
        if self_density < change_density {
            // If steps haven't changed, but we increase in density 
            // (reducing range with same steps), accept.
            return OfferResult::Accept;
        }
        // lastly, if no change in range or density, check for AMV gain.
        if change.amv > 0.0 {
            return OfferResult::Accept;
        }
        OfferResult::Reject
    }

    // purchase logic, figure out what to buy and if we have anything to offer for it.
    // selling logic, create a list of things the pop is willing to offer in exchange for other things.

    // day startup, does the initial work needed for the pop before the day begins, for pops, this typically includes exchanging their time and skills for payment in work.
    // standard day action, the work done by the pop during the day. This is primarily the buying of goods from the market.
    // day end, the final action of the day, covers wrapping up, consumpution, and some additional work, possibly including taxes and the like.

    /// # Excess AMV
    /// 
    /// Get's the pop's unused goods and calculate it's current running AMV of these
    /// goods.
    pub fn excess_amv(&self, market: &MarketHistory) -> f64 {
        let mut amv = 0.0;
        for (&good, &data) in self.property.iter() {
            amv += market.get_record(good).price * data.available();
        }
        amv
    }

    /// # Get Shopping Target
    /// 
    /// When called, this looks at the current desires of the pop which have not been satisfied
    /// and selects the most highly desired one (first in working desires).
    /// 
    /// It returns how much it needs to satisfy the current tier as well.
    /// 
    /// If a specific good, it returns a planned target which should be gotten immediately.
    /// 
    /// All others return just what is needed. The Market decides which to get based on availablity.
    pub fn get_shopping_target(&self) -> Option<(Item, f64)> {
         if let Some(desire) = self.working_desires.front()
         {
            match desire.item {
                Item::Want(_) | Item::Class(_) => {
                    return Some((desire.item, desire.amount));
                },
                Item::Good(id) => {
                    // if we have this in our property, and it has a target, try to get that target.
                    if let Some(property) = self.property.get(&id) {
                        let target = property.current_target();
                        return Some((Item::Good(id), target));
                    } else {
                        return Some((Item::Good(id), desire.amount));
                    }
                },
            }
         }
         return None
    }

    /// # Satisfaction from Multiple AMVs
    /// 
    /// Given an list of AMVs, how much Satisfaction could we (hypothetically) gain 
    /// from applying each, and all total.
    /// 
    /// Assumes the market price is accurate and all it needs can be gained. Ignores 
    /// shop time cost in the calculation.
    /// 
    /// Returns the number of levels satisfied and the value of those levels, plus
    /// the summ of all gain.
    pub fn satisfaction_from_multiple_amvs(&self, amv_gains: Vec<f64>, 
    market: &MarketHistory) -> Vec<SatisfactionValues> {
        // preemptively get the satisfaction we currently have.
        let self_sat = self.get_satisfaction(market);
        // create Duplicate for working on.
        let mut dup = self.clone();
        dup.recalculate_working_desires(); // recalculate the working desires
        let mut results = vec![];
        let mut range_acc = 0.0;
        let mut step_acc = 0.0;

        for amv in amv_gains.iter() {
            let mut amv_remaining = *amv;
            // iterate over desires
            loop {
                // if nothing left to desire, break
                if dup.working_desires.len() == 0 {
                    break;
                }
                if amv_remaining <= 0.0 { // if nothing else to purchase, break.
                    break;
                }
                // get the price of the item we want to purchase.
                let mut desire = dup.working_desires.pop_front().unwrap();
                let unit_price = match desire.item {
                    Item::Want(id) => *market.want_prices.get(&id).unwrap_or(&0.0),
                    Item::Class(id) => *market.class_prices.get(&id).unwrap_or(&0.0),
                    Item::Good(id) => {
                        if let Some(good) = market.good_records.get(&id) {
                            good.price
                        } else {
                            0.0
                        }
                    },
                };
                // get how many we want to purchase, capping at amount
                let target = desire.amount - (desire.satisfaction % desire.amount);
                // how many we can purchase.
                let can_purchase = amv_remaining / unit_price;
                // how many we will acutally purchase.
                let purchase_amt = can_purchase.min(target);
                // update satisfaction
                desire.satisfaction += purchase_amt;
                // reduce our amount.
                amv_remaining -= purchase_amt * unit_price;
                // if desire is fully satisfied, add to finished.
                if desire.is_fully_satisfied() { // if fully satisfied, push to desires.
                    dup.desires.push_back(desire);
                } else { // 
                    Pop::ordered_desire_insert(&mut dup.working_desires, desire);
                }
            }
            // once you purchase and fill up the desires, get satisfcation and calculate how much was gained.
            let dup_sat = dup.get_satisfaction(market);
            let curr_range = dup_sat.range - self_sat.range - range_acc;
            let curr_steps = dup_sat.steps - self_sat.steps - step_acc;
            println!("Current Satisfaction Level: {}", curr_range);
            println!("Current Satisfaction Value: {}", curr_steps);
            results.push(SatisfactionValues::new(curr_range, curr_steps, 0.0));
            // add to the accumulators
            range_acc += curr_range;
            step_acc += curr_steps;
        }
        // append the total sum of the end.
        results.push(SatisfactionValues::new(range_acc, step_acc, 0.0));

        results
    }

    /// # Satisfaction from AMV
    /// 
    /// Given an amount of AMV, how much Satisfaction could we (hypothetically) gain.
    /// 
    /// Assumes the market price is accurate and all it needs can be gained. Ignores 
    /// shop time cost in the calculation.
    /// 
    /// Returns the number of levels satisfied and the value of those levels.
    pub fn satisfaction_from_amv(&self, amv_gain: f64, market: &MarketHistory) -> SatisfactionValues {
        // create Duplicate for working on.
        let mut dup = self.clone();
        dup.recalculate_working_desires(); // recalculate the working desires
        let mut amv_remaining = amv_gain;

        // iterate over desires
        loop {
            // if nothing left to desire, break
            if dup.working_desires.len() == 0 {
                break;
            }
            if amv_remaining <= 0.0 { // if nothing else to purchase, break.
                break;
            }
            // get the price of the item we want to purchase.
            let mut desire = dup.working_desires.pop_front().unwrap();
            let unit_price = match desire.item {
                Item::Want(id) => *market.want_prices.get(&id).unwrap_or(&0.0),
                Item::Class(id) => *market.class_prices.get(&id).unwrap_or(&0.0),
                Item::Good(id) => {
                    if let Some(good) = market.good_records.get(&id) {
                        good.price
                    } else {
                        0.0
                    }
                },
            };
            // get how many we want to purchase, capping at amount
            let target = desire.amount - (desire.satisfaction % desire.amount);
            // how many we can purchase.
            let can_purchase = amv_remaining / unit_price;
            // how many we will acutally purchase.
            let purchase_amt = can_purchase.min(target);
            // update satisfaction
            desire.satisfaction += purchase_amt;
            // reduce our amount.
            amv_remaining -= purchase_amt * unit_price;
            // if desire is fully satisfied,
            if desire.is_fully_satisfied() { 
                dup.desires.push_back(desire);
            } else {
                Pop::ordered_desire_insert(&mut dup.working_desires, desire);
            }
        }
        // once you purchase and fill up the desires, get satisfcation and calculate how much was gained.
        let dup_sat = dup.get_satisfaction(market);
        let self_sat = self.get_satisfaction(market);
        SatisfactionValues::new(dup_sat.range - self_sat.range, dup_sat.steps - self_sat.steps, dup_sat.amv - self_sat.amv)
    }

    /// # Add Back to Working Desires
    /// 
    /// Resets our desires to be working desires unless they are 
    /// totally satisfied.
    /// 
    /// Useful for satisfaction from AMV as well as possible recalculations
    /// when we have resources to expend and run out of working desires. 
    /// ~~(Maybe IDK, probably not that smart to do.)~~
    fn recalculate_working_desires(&mut self) {
        let mut idx = 0;
        loop {
            if self.desires.len() <= idx {
                break;
            }
            let desire = self.desires.remove(idx).expect("Walked Off desires somehow.");
            if desire.is_fully_satisfied() { 
                // if it's fully satisfied, just add back
                self.desires.insert(idx, desire);
                idx += 1;
            } else { // not satisfied, add back to working
                Pop::ordered_desire_insert(&mut self.working_desires, desire);
            }
        }
    }

    // TODO: Satisfaction Lost and Gained, should be smarter. Improvement. Using partial satisfaction, 
    // they only add or remove once from there, rather than doing it all again. This builds on the simplification
    // that if previously solved, then adding guarantees it's use immediately.
    // For lost, this assumption makes it so that it can strip from as yet un-reserved goods, meaning that it will
    // always result in a satisfaction loss (per unit) less than the current level being looked at, and 
    // could be used 

    /// # Satisfaction Change
    /// 
    /// Given a number of goods added/removed returns the result of that change in goods.
    /// 
    /// Returns levels satisfied and levels
    /// 
    /// # Note Not tested
    pub fn satisfaction_change(&self, change: &HashMap<usize, f64>, data: &Data, market: &MarketHistory) -> SatisfactionValues {
        let mut temp_pop = self.clone();
        temp_pop.reset();
        for (good, val) in change.iter() {
            temp_pop.property.entry(*good)
                .and_modify(|x| x.owned += val)
                .or_insert(PropertyRecord::new(*val));
        }
        temp_pop.try_satisfy_all_desires(data, market);
        let range = self.satisfaction.range - temp_pop.satisfaction.range;
        let steps = self.satisfaction.steps - temp_pop.satisfaction.steps;
        let amv = self.satisfaction.amv - temp_pop.satisfaction.amv;
        SatisfactionValues::new(range, steps, amv)
    }

    /// # Satisfaction Lost
    /// 
    /// Calculates the satisfaction lost by removing goods and wants from the 
    /// 
    /// Calculates it by cloning the pop, removing goods, then satisfying desires.
    /// 
    /// Returns levels satisfied and levels
    /// 
    /// # Note Not tested
    pub fn satisfaction_lost(&self, removing: &HashMap<usize, f64>, data: &Data, market: &MarketHistory) -> SatisfactionValues {
        // Clone existing pop.
        let mut temp_pop = self.clone();
        // Reset the 
        temp_pop.reset(); 
        for (good, val) in removing.iter() {
            temp_pop.property.get_mut(good).unwrap().owned -= *val;
        }
        // satisfy the desires of the temporary pop.
        temp_pop.try_satisfy_all_desires(data, market);
        // with satisfaciton done, return the difference between the current and possible new
        let range = self.satisfaction.range - temp_pop.satisfaction.range;
        let steps = self.satisfaction.steps - temp_pop.satisfaction.steps;
        let amv = self.satisfaction.amv - temp_pop.satisfaction.amv;
        SatisfactionValues::new(range, steps, amv)
    }

    /// # Satisfaction Gain
    /// 
    /// Calculates the satisfaction gained by adding these goods.
    /// 
    /// Calculates it by cloning the pop, adding the desire, then satisfying desires.
    /// 
    /// Returns levels satisfied and levels
    /// 
    /// # Note Not tested
    pub fn satisfaction_gain(&self, new_goods: &HashMap<usize, f64>, 
    data: &Data, market: &MarketHistory) -> SatisfactionValues {
        // Clone existing pop.
        let mut temp_pop = self.clone();
        // Reset the 
        temp_pop.reset(); 
        for (good, val) in new_goods.iter() {
            temp_pop.property.entry(*good)
                .and_modify(|x| x.owned += val)
                .or_insert(PropertyRecord::new(*val));
        }
        // satisfy the desires of the temporary pop.
        temp_pop.try_satisfy_all_desires(data, market);
        // with satisfaciton done, return the difference between the current and possible new
        let range = temp_pop.satisfaction.range - self.satisfaction.range;
        let steps = temp_pop.satisfaction.steps - self.satisfaction.steps;
        let amv = temp_pop.satisfaction.amv - self.satisfaction.amv;
        debug_assert!(steps >= 0.0, "Satisfaction Gained must be non-negative.");
        SatisfactionValues::new(range, steps, amv)
    }

    /// # Get Satisfaction
    /// 
    /// Calculates the current satisfaction of the pop, returning the range of 
    /// satisfied priorities, and the satisfied steps desires summed.
    /// 
    /// This takes into account both the 'completed' desires and working desires.
    /// 
    /// We should always target more steps if possible, and a smaller range second.
    /// 
    /// NOTE: Does not save to the pop.
    /// NOTE: This has not been tested. It is assumed to be correct.
    pub fn get_satisfaction(&self, market: &MarketHistory) -> SatisfactionValues {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        let mut steps = 0.0;
        for desire in self.desires.iter() {
            if desire.satisfaction == 0.0 {
                // If no satisfaciton, skip it.
                continue;
            }
            low = low.min(desire.start_priority);
            high = high.max(desire.satisfied_to_priority());
            // println!("Current Low: {}", low);
            // println!("Current High: {}", high);
            steps += desire.satisfied_steps();
        }
        for desire in self.working_desires.iter() {
            if desire.satisfaction == 0.0 {
                // If no satisfaciton, skip it.
                continue;
            }
            low = low.min(desire.start_priority);
            high = high.max(desire.satisfied_to_priority());
            // println!("Current Low: {}", low);
            // println!("Current High: {}", high);
            steps += desire.satisfied_steps();
        }
        // sanity check that he reached something.
        if high == f64::NEG_INFINITY || low == f64::INFINITY {
            // if nothing reached (both will ilkely be infinity)
            // set high and low to zero.
            high = 0.0;
            low = 0.0;
        }

        SatisfactionValues::new(high - low, steps, self.excess_amv(market))
    }

    /// # Consume Desires
    /// 
    /// Consumes all goods to satisfy desires.
    /// 
    /// This will destroy wants and goods.
    /// 
    /// It returns the range of desires satisfied and the total valuation of all
    /// desires summed together.
    /// 
    /// Higher valuation is always preferred, regardless of change in range.
    /// An increase in range is only acceptable
    pub fn consume_desires(&mut self, data: &Data, market: &MarketHistory) -> SatisfactionValues {
        let mut working_desires = VecDeque::new();
        // get desires and reset satisfaction while we're at it.
        for desire in self.desires.iter() {
            let mut d = desire.clone();
            d.satisfaction = 0.0;
            working_desires.push_back(d);
        }
        let mut finished = vec![];
        loop {
            let mut current_desire = working_desires.pop_front().unwrap();

            if self.consume_desire(&mut current_desire, data) { // if successful at satisfying
                let next_step = current_desire.satisfied_to_priority();
                println!("Next Step: {}", next_step);
                if let Some(end) = current_desire.end() { 
                    if next_step < end { // if not past the end
                        // put back
                        Pop::ordered_desire_insert(&mut working_desires, 
                            current_desire);
                    } else { // if at or after the end, finish.
                        finished.push(current_desire);
                    }
                } else { // if no end to walk off, just put back.
                    Pop::ordered_desire_insert(&mut working_desires, 
                        current_desire);
                }
            } else {
                // if did not satisfy the desire level completely, add to finished.
                finished.push(current_desire);
            }

            // if no working desires left. GTFO.
            if working_desires.len() == 0 {
                break;
            }
        }
        // with all finished, push back into our desires
        // clear out old desires
        self.desires.clear();
        for desire in finished {
            Pop::ordered_desire_insert(&mut self.desires, desire);
        }

        // push satisfaction back into original desires.
        self.get_satisfaction(market)
    }

    /// # Consume Desire
    /// 
    /// Given a desire, it satisfies one level of it.
    /// 
    /// Returns true if succeeded at fully satisfying the desire, false otherwise.
    /// 
    /// This does not alter reservation amounts. Instead, it adds to expended
    /// and we can sanity check that reservations matched our expended values.
    pub(crate) fn consume_desire(&mut self, current_desire: &mut Desire, data: &Data) -> bool {
        let mut shifted = 0.0;
        match current_desire.item {
            crate::item::Item::Want(id) => {
                println!("Getting Wants");
                // want is the most complicated, but follows a standard priority method.
                // First, try to get wants from storage.
                if let Some(want_rec) = self.wants.get_mut(&id) {
                    // get available want
                    let shift = want_rec.owned.min(current_desire.amount - shifted);
                    println!("Shifting: {}", shift);
                    if shift > 0.0 {
                        println!("Have want already, reserving.");
                        want_rec.owned -= shift; // remove from owned
                        want_rec.expended += shift; // add to expended.
                        current_desire.satisfaction += shift;
                        shifted += shift;
                    }
                }
                // First try to get via ownership
                if shifted < current_desire.amount { // check if we need more.
                    let want = data.get_want(id);
                    // get the goods we can use for this.
                    for good in want.ownership_sources.iter() {
                        // with a good gotten, reserve as much as necessary to satisfy it.
                        if let Some(good_rec) = self.property.get_mut(good) {
                            // Get how many of the good we need to reserve for it.
                            let good_data = data.get_good(*good);
                            let eff = *good_data.own_wants.get(&id)
                                .expect("Want not found in good ownership effects.");
                            let target = (current_desire.amount - shifted) / eff;
                            let shift = target.min(good_rec.owned);
                            if shift > 0.0 {
                                println!("Getting Ownership Source.");
                                // shift and reserve
                                shifted += shift * eff;
                                good_rec.owned -= shift; // remove from owned
                                good_rec.used += shift; // add to expended.
                                current_desire.satisfaction += shift * eff;
                                // add the extra wants to expected for later uses.
                                for (&want, &eff) in good_data.own_wants.iter() { 
                                    if let Some(rec) = self.wants.get_mut(&want) {
                                        if want == id {
                                            // add to expended
                                            rec.expended += eff * shift;
                                        } else {
                                            // if not what we're consuming, add to owned.
                                            rec.owned += eff * shift;
                                        }
                                    } else {
                                        // new want entirely, just make new.
                                        let mut rec = WantRecord {
                                            owned: 0.0,
                                            reserved: 0.0,
                                            expected: 0.0,
                                            expended: 0.0,
                                        };
                                        if want == id {
                                            // If what we're consuming, add to expended.
                                            rec.expended += eff * shift;
                                        } else {
                                            // if not, add to owned.
                                            rec.owned += eff * shift;
                                        }
                                        self.wants.insert(want, rec);
                                    }
                                }
                            }
                        }
                        if shifted > current_desire.amount {
                            break;
                        }
                    }
                }
                // Then try for use if we still need more.
                if shifted < current_desire.amount { // then try for use
                    let want = data.get_want(id);
                    // get the goods we can use for this.
                    for good in want.use_sources.iter() {
                        // with a good gotten, reserve as much as necessary to satisfy it.
                        if self.property.contains_key(good) {
                            // get time and the good
                            let mut good_rec = self.property.remove(good).unwrap();
                            // Get how many of the good we need to reserve for it.
                            let good_data = data.get_good(*good);
                            // get efficiency of producing that want.
                            let eff = *good_data.use_wants.get(&id)
                                .expect("Want not found in good ownership effects.");
                            let mut target = (current_desire.amount - shifted) / eff;
                            // get time target
                            let time_target = good_data.use_time * target;
                            // get our available time, capped at our target.
                            let available_time = time_target
                                .min(self.property.get(&TIME_ID)
                                    .unwrap_or(&PropertyRecord::new(0.0)).owned
                                );
                            if available_time != time_target { // if available time is not enough
                                // reduce target by available time.
                                target = available_time / time_target * target;
                            }
                            // with target gotten and possibly corrected, do the shift
                            let shift = target.min(good_rec.owned);
                            if shift > 0.0 {
                                // shift and reserve good and the want
                                shifted += shift * eff;
                                good_rec.owned -= shift; // remove from owned.
                                good_rec.used += shift; // add to expended.
                                current_desire.satisfaction += shift * eff;
                                // shift time as well
                                self.property.get_mut(&TIME_ID).unwrap()
                                    .owned -= shift * good_data.consumption_time;
                                self.property.get_mut(&TIME_ID).unwrap()
                                    .expended += shift * good_data.consumption_time;
                                // add the extra wants to expected for later uses.
                                for (&want, &eff) in good_data.use_wants.iter() { 
                                    // add the wants to expected.
                                    if let Some(rec) = self.wants.get_mut(&want) {
                                        if want == id {
                                            // if the want we're consuming, remove from owned
                                            // and add to expended
                                            rec.expended += eff * shift;
                                        } else {
                                            // if not what we're consuming, just add to owned.
                                            rec.owned += eff * shift;
                                        }
                                    } else {
                                        let mut rec = WantRecord {
                                            owned: 0.0,
                                            reserved: 0.0,
                                            expected: 0.0,
                                            expended: 0.0,
                                        };
                                        if want == id {
                                            // If what we're consuming, add to expended.
                                            rec.expended += eff * shift;
                                        } else {
                                            // if not, add to owned.
                                            rec.owned += eff * shift;
                                        }
                                        self.wants.insert(want, rec);
                                    }
                                }
                            }
                            // put good_rec back in regardless of result
                            self.property.insert(*good, good_rec);
                        }
                        if shifted > current_desire.amount {
                            break;
                        }
                    }
                }
                if shifted < current_desire.amount { // lastly consumption
                    let want = data.get_want(id);
                    // get the goods we can consume for this.
                    for good in want.consumption_sources.iter() {
                        // with a good gotten, reserve as much as necessary to satisfy it.
                        if self.property.contains_key(good) {
                            // get time and the good
                            let mut good_rec = self.property.remove(good).unwrap();
                            // Get how many of the good we need to reserve for it.
                            let good_data = data.get_good(*good);
                            // get efficiency of producing that want.
                            let eff = *good_data.consumption_wants.get(&id)
                                .expect("Want not found in good ownership effects.");
                            let mut target = (current_desire.amount - shifted) / eff;
                            // get time target
                            let time_target = good_data.consumption_time * target;
                            // get our available time, capped at our target.
                            let available_time = time_target
                                .min(self.property.get(&TIME_ID)
                                    .unwrap_or(&PropertyRecord::new(0.0)).owned
                                );
                            if available_time != time_target { // if available time is not enough
                                // reduce target by available time.
                                target = available_time / time_target * target;
                            }
                            // with target gotten and possibly corrected, do the shift
                            let shift = target.min(good_rec.owned);
                            if shift > 0.0 {
                                // shift and reserve good and the want
                                shifted += shift * eff;
                                good_rec.owned -= shift;
                                good_rec.expended += shift;
                                current_desire.satisfaction += shift * eff;
                                // shift time as well
                                self.property.get_mut(&TIME_ID).unwrap()
                                    .owned -= shift * good_data.consumption_time;
                                self.property.get_mut(&TIME_ID).unwrap()
                                    .expended += shift * good_data.consumption_time;
                                // add the extra wants to expected for later uses.
                                for (&want, &eff) in good_data.consumption_wants.iter() {
                                    // add the wants to expected.
                                    if let Some(rec) = self.wants.get_mut(&want) {
                                        if want == id {
                                            // Add to expended
                                            rec.expended += eff * shift;
                                        } else {
                                            // if not what we're consuming, just add to owned.
                                            rec.owned += eff * shift;
                                        }
                                    } else {
                                        let mut rec = WantRecord {
                                            owned: 0.0,
                                            reserved: 0.0,
                                            expected: 0.0,
                                            expended: 0.0,
                                        };
                                        if want == id {
                                            // If what we're consuming, add to expended.
                                            rec.expended += eff * shift;
                                        } else {
                                            // if not, add to owned.
                                            rec.owned += eff * shift;
                                        }
                                        self.wants.insert(want, rec);
                                    }
                                }
                            }
                            // put good_rec back in regardless of result
                            self.property.insert(*good, good_rec);
                        }
                        if shifted > current_desire.amount {
                            break;
                        }
                    }
                }
            },
            crate::item::Item::Class(id) => {
                // get members of the class
                let members = data.get_class(id);
                for member in members.iter().sorted() {
                    // if we have the member, use it.
                    if let Some(rec) = self.property.get_mut(member) {
                        // get how much we can shift over, capping at the target sans already moved goods.
                        let shift = if rec.owned == 0.0 {
                            continue;
                        } else {
                            rec.owned.min(current_desire.amount - shifted)
                        };
                        rec.owned -= shift;
                        rec.expended += shift; // and add to expended for checking.
                        current_desire.satisfaction += shift;
                        shifted += shift;
                    }
                    if shifted == current_desire.amount {
                        // if shifted enough to cover desire, stop trying.
                        break;
                    }
                }
            },
            crate::item::Item::Good(id) => {
                println!("Satisfying Good: {}.", id);
                // Good, so just find and insert
                if let Some(rec) = self.property.get_mut(&id) {
                    println!("Has in property.");
                    // How much we can shift over.
                    let shift = rec.owned.min(current_desire.amount);
                    println!("Shifting: {}", shift);
                    shifted += shift; // add to shifted for later checking
                    rec.owned -= shift;
                    rec.expended += shift; // and add to expended for checking.
                    current_desire.satisfaction += shift; // and to satisfaction.
                    println!("Current Satisfaction: {}", current_desire.satisfaction);
                }
            },
        }
        shifted == current_desire.amount
    }

    /// # Satisfy Desire
    /// 
    /// Takes a given desire and tries to satisfy it alone, adding 1 step worth of
    /// satisfaction if possible.
    /// 
    /// Returns the desire, regardless of the total success. Also returns the amount of satisfaction
    /// added to the desire.
    /// 
    /// TODO: Expand to include a step/satisfaction target parameter so it can do more than 1 level at a time.
    pub fn satisfy_desire(&mut self, mut current_desire: Desire, data: &Data) -> (Desire, f64) {
        // prep our shifted record for checking if we succeeded at satisfying the desire.
        let mut shifted = 0.0;
        match current_desire.item {
            Item::Want(id) => {
                //println!("Getting Wants");
                // want is the most complicated, but follows a standard priority method.
                // First, try to get wants from storage.
                if let Some(want_rec) = self.wants.get_mut(&id) {
                    // get available want
                    let shift = want_rec.available().min(current_desire.amount - shifted);
                    if shift > 0.0 {
                        //println!("Have want already, reserving.");
                        want_rec.reserved += shift; // shift
                        current_desire.satisfaction += shift;
                        shifted += shift;
                    }
                }
                // First try to get via ownership
                if shifted < current_desire.amount { // check if we need more.
                    let want = data.get_want(id);
                    // get the goods we can use for this.
                    for good in want.ownership_sources.iter() {
                        // with a good gotten, reserve as much as necessary to satisfy it.
                        if let Some(good_rec) = self.property.get_mut(good) {
                            // Get how many of the good we need to reserve for it.
                            let good_data = data.get_good(*good);
                            let eff = *good_data.own_wants.get(&id)
                                .expect("Want not found in good ownership effects.");
                            let target = (current_desire.amount - shifted) / eff;
                            let shift = target.min(good_rec.available());
                            if shift > 0.0 {
                                //println!("Getting Ownership Source.");
                                // shift and reserve
                                shifted += shift * eff;
                                good_rec.reserved += shift;
                                current_desire.satisfaction += shift * eff;
                                // add the extra wants to expected for later uses.
                                for (&want, &eff) in good_data.own_wants.iter() { 
                                    // add the wants to expected.
                                    if let Some(rec) = self.wants.get_mut(&want) {
                                        rec.expected += eff * shift;
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                    } else {
                                        let mut rec = WantRecord {
                                            owned: 0.0,
                                            reserved: 0.0,
                                            expected: eff * shift,
                                            expended: 0.0,
                                        };
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                        self.wants.insert(want, rec);
                                    }
                                }
                            }
                        }
                        if shifted > current_desire.amount {
                            break;
                        }
                    }
                }
                // Then try for use if we still need more.
                if shifted < current_desire.amount { // then try for use
                    let want = data.get_want(id);
                    // get the goods we can use for this.
                    for good in want.use_sources.iter() {
                        // with a good gotten, reserve as much as necessary to satisfy it.
                        if self.property.contains_key(good) {
                            // get time and the good
                            let mut good_rec = self.property.remove(good).unwrap();
                            // Get how many of the good we need to reserve for it.
                            let good_data = data.get_good(*good);
                            // get efficiency of producing that want.
                            let eff = *good_data.use_wants.get(&id)
                                .expect("Want not found in good ownership effects.");
                            let mut target = (current_desire.amount - shifted) / eff;
                            // get time target
                            let time_target = good_data.use_time * target;
                            // get our available time, capped at our target.
                            let available_time = time_target
                                .min(self.property.get(&TIME_ID)
                                    .unwrap_or(&PropertyRecord::new(0.0)).available()
                                );
                            if available_time != time_target { // if available time is not enough
                                // reduce target by available time.
                                target = available_time / time_target * target;
                            }
                            // with target gotten and possibly corrected, do the shift
                            let shift = target.min(good_rec.available());
                            if shift > 0.0 {
                                // shift and reserve good and the want
                                shifted += shift * eff;
                                good_rec.reserved += shift;
                                current_desire.satisfaction += shift * eff;
                                // shift time as well
                                self.property.get_mut(&TIME_ID).unwrap()
                                    .reserved += shift * good_data.use_time;
                                // add the extra wants to expected for later uses.
                                for (&want, &eff) in good_data.use_wants.iter() { 
                                    // add the wants to expected.
                                    if let Some(rec) = self.wants.get_mut(&want) {
                                        rec.expected += eff * shift;
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                    } else {
                                        let mut rec = WantRecord {
                                            owned: 0.0,
                                            reserved: 0.0,
                                            expected: eff * shift,
                                            expended: 0.0,
                                        };
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                        self.wants.insert(want, rec);
                                    }
                                }
                            }
                            // put good_rec back in regardless of result
                            self.property.insert(*good, good_rec);
                        }
                        if shifted > current_desire.amount {
                            break;
                        }
                    }
                }
                if shifted < current_desire.amount { // lastly consumption
                    let want = data.get_want(id);
                    // get the goods we can consume for this.
                    for good in want.consumption_sources.iter() {
                        // with a good gotten, reserve as much as necessary to satisfy it.
                        if self.property.contains_key(good) {
                            // get time and the good
                            let mut good_rec = self.property.remove(good).unwrap();
                            // Get how many of the good we need to reserve for it.
                            let good_data = data.get_good(*good);
                            // get efficiency of producing that want.
                            let eff = *good_data.consumption_wants.get(&id)
                                .expect("Want not found in good ownership effects.");
                            let mut target = (current_desire.amount - shifted) / eff;
                            // get time target
                            let time_target = good_data.consumption_time * target;
                            // get our available time, capped at our target.
                            let available_time = time_target
                                .min(self.property.get(&TIME_ID)
                                    .unwrap_or(&PropertyRecord::new(0.0)).available()
                                );
                            if available_time != time_target { // if available time is not enough
                                // reduce target by available time.
                                target = available_time / time_target * target;
                            }
                            // with target gotten and possibly corrected, do the shift
                            let shift = target.min(good_rec.available());
                            if shift > 0.0 {
                                // shift and reserve good and the want
                                shifted += shift * eff;
                                good_rec.reserved += shift;
                                current_desire.satisfaction += shift * eff;
                                // shift time as well
                                self.property.get_mut(&TIME_ID).unwrap()
                                    .reserved += shift * good_data.consumption_time;
                                // add the extra wants to expected for later uses.
                                for (&want, &eff) in good_data.consumption_wants.iter() {
                                    // add the wants to expected.
                                    if let Some(rec) = self.wants.get_mut(&want) {
                                        rec.expected += eff * shift;
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                    } else {
                                        let mut rec = WantRecord {
                                            owned: 0.0,
                                            reserved: 0.0,
                                            expected: eff * shift,
                                            expended: 0.0,
                                        };
                                        if want == id {
                                            rec.reserved += eff * shift;
                                        }
                                        self.wants.insert(want, rec);
                                    }
                                }
                            }
                            // put good_rec back in regardless of result
                            self.property.insert(*good, good_rec);
                        }
                        if shifted > current_desire.amount {
                            break;
                        }
                    }
                }
            },
            Item::Class(id) => {
                // get members of the class
                let members = data.get_class(id);
                for member in members.iter().sorted() {
                    // if we have the member, use it.
                    if let Some(rec) = self.property.get_mut(member) {
                        // get how much we can shift over, capping at the target sans already moved goods.
                        let shift = if rec.available() == 0.0 {
                            continue;
                        } else {
                            rec.available().min(current_desire.amount - shifted)
                        };
                        rec.reserved += shift;
                        current_desire.satisfaction += shift;
                        shifted += shift;
                    }
                    if shifted == current_desire.amount {
                        // if shifted enough to cover desire, stop trying.
                        break;
                    }
                }
            },
            Item::Good(id) => {
                //println!("Satisfying Good: {}.", id);
                // Good, so just find and insert
                if let Some(rec) = self.property.get_mut(&id) {
                    //println!("Has in property.");
                    // How much we can shift over.
                    let shift = rec.available().min(current_desire.amount);
                    //println!("Shifting: {}", shift);
                    shifted += shift; // add to shifted for later checking
                    rec.reserved += shift; // add to reserved.
                    current_desire.satisfaction += shift; // and to satisfaction.
                    //println!("Current Satisfaction: {}", current_desire.satisfaction);
                }
            },
        }
        (current_desire, shifted)
    }

    /// # Satisfy Next desire
    /// 
    /// Satisfies the next desire in working_desires.
    /// 
    /// This will reserve wants and goods for the desires.
    /// 
    /// If a desire is not satisfied, it returns that desire and the step 
    /// at which it failed to satisfy.
    pub(crate) fn satisfy_next_desire(&mut self, working_desires: &mut VecDeque<Desire>, 
    data: &Data) -> Option<Desire> {
        assert!(working_desires.len() > 0, "Working Desires cannot be empty.");
        // Get current step and desire from the front of the working desires. If no next one, leave loop.
        let current_desire = 
        if let Some(current_desire) = working_desires.pop_front() {
            //println!("Current Step: {}", current_step);
            current_desire
        } else {
            return None;
        };// If did not succeed at satisfying this time, or desire is fully satisfied, add to finished.
        let (current_desire, shifted) = self.satisfy_desire(current_desire, data);
        if shifted < current_desire.amount || current_desire.is_fully_satisfied() {
            //println!("Finished with desire. SHifted: {}, desire_target: {}", shifted, current_desire.amount);
            //println!("Fully Satisfied: {}", current_desire.is_fully_satisfied());
            return Some(current_desire);
        } else { // otherwise, put back into working desires to try and satisfy again. Putting to the next spot it woud do
            //println!("Repeat Desire.");
            Self::ordered_desire_insert(working_desires, current_desire);
            None
        }
    }

    /// # Satisfy until Incomplete
    /// 
    /// Satisfies desires until an desire is unable to satisfy itself.
    /// 
    /// The working desires starts with the next desire this will start with. So no need
    /// to give a starting vaule or desire.
    /// 
    /// Returns the desire that was incomplete and the tier at which it was incomplete.
    pub fn satisfy_until_incomplete(&mut self, working_desires: &mut VecDeque<Desire>, 
    data: &Data) -> Option<Desire> {
        loop {
            // satisfy the next desire
            if let Some(result) = self.satisfy_next_desire(working_desires, data) {
                // if we get a desire here, escape out. We're done.
                return Some(result);
            }
            // if didn't find anything to stop us, go to the next.
        }
    }

    /// # Try Satisfy All Desires
    /// 
    /// This takes the pop as it is and tries to satisfy it's desires to the best 
    /// of it's abilities, not stopping at any desires it cannot satisfy.
    /// 
    /// Once it does everything it can, it sets our satisfaction.
    /// 
    /// ## Notes
    /// 
    /// This tries to satisfy everything it can. This means that it will end with no 
    /// desires left in self.working_desires.
    /// 
    /// This will also re-open any previously 'finished' desires to try and fill them 
    /// out also.
    /// 
    /// This does not reset current satisfaction or pre-existing reservations, it just 
    /// adds all desires back to work, and goes from there.
    /// 
    /// This should be called near day start.
    /// 
    /// There's no special prioritization, start at the bottom of desires, add to
    /// the first, and go from there. 
    /// 
    /// After all is done, it saves the work, and records the satisfaction achieved.
    pub fn try_satisfy_all_desires(&mut self, data: &Data, market: &MarketHistory) {
        // Move current desires into a working btreemap for easier organization and management.
        //println!("Satisfying Desires.");
        // create a working desires to pass around our 
        let mut working_desires = VecDeque::new();
        // Working desires, includes the current tier it's on, and the desire.
        while let Some(desire) = self.desires.pop_front() { // Initial list is always sorted, so just move over.
            Pop::ordered_desire_insert(&mut working_desires, desire);
        }
        // also move over from self.working_desires
        while let Some(desire) = self.working_desires.pop_front() {
            Pop::ordered_desire_insert(&mut working_desires, desire);
        }
        // A holding space for desires that have been totally satisfied to simplify
        let mut finished: Vec<Desire> = vec![];
        while working_desires.len() > 0 {
            // satisfy until something can't be satisfied
            if let Some(incomplete_desire) = self.satisfy_until_incomplete(&mut working_desires, data) {
                finished.push(incomplete_desire);
            }
        }
        // after doing all satisfactions, put them back in.
        while let Some(desire) = finished.pop() {
            Pop::ordered_desire_insert(&mut self.desires, desire);
        }
        // wrap up by getting satisfaciton and saving it.
        self.satisfaction = self.get_satisfaction(market);
    }

    /// # Update Desires Full
    /// 
    /// Call this on a pop that has it's demographic rows updated and needs
    /// it's desires updated to match. 
    /// 
    /// This will totally recalculate the desires of a pop from scratch, so only use 
    /// if the pop is new or there was a major change that a simple update would not cover.
    /// 
    /// NOTE: Didn't bother to test as it's most complex parts have been broken out and tested separately.
    pub fn update_desires_full(&mut self, data: &Data) {
        // Insert all desires into our vector, scaling to the appropriate tags of the 
        // desire. If they are the same desire (with different desire values) combine them.
        let mut desires: Vec<Desire> = vec![];
        for row in self.demo_breakdown.iter() {
            // species
            let species = data.get_species(row.species);
            Self::integrate_desires(&species.desires, row, &mut desires);
            // culture
            if let Some(culture_id) = row.culture {
                let culture = data.get_culture(culture_id);
                Self::integrate_desires(&culture.desires, row, &mut desires);
            }
            // Remaining sections go here.
        }
    }

    /// Helper for getting desires from a part of demographics into our total desires.
    /// 
    /// Takes in the desires of the source demographic part (Species.desires, culture.desires)
    /// Takes in the row for household information.
    /// 
    /// And it takes in, and modifies, the desires we are adding to and will eventually set
    /// self.desires to.
    pub(crate) fn integrate_desires(source_desires: &Vec<Desire>, row: &DRow, desires: &mut Vec<Desire>) {
        for desire in source_desires.iter() {
            //println!("---");
            //println!("Start: {}", desire.start);
            //println!("Good: {}", desire.item.unwrap());
            //println!("Amount: {}", desire.amount);
            // copy base over
            let mut new_des = desire.clone();
            // get multiplier
            Self::get_desire_multiplier(desire, row, &mut new_des);
            // with desire scaled properly, find if it already exists in our desires
            // desires are always sorted.
            let mut current = if let Some((est, _)) = desires.iter()
            .find_position(|x| x.start_priority <= new_des.start_priority) {
                // find the first one which is equal to or greater than our new destination.
                est
            } else { desires.len() }; // if none was found then it is either the last or only one.
            //println!("First Pos: {}", current);
            // with first match found, try to find duplicates while walking up. 
            loop {
                if current >= desires.len() {
                    // if at or past the end, insert at the end and continue.
                    //println!("Insert Position: {}", current);
                    desires.push(new_des);
                    break;
                } else if desires.get(current).unwrap().equals(&new_des) {
                    // if new_desire matches existing desire, add to it.
                    //println!("Insert Position: {}", current);
                    desires.get_mut(current).unwrap().amount += new_des.amount;
                    break;
                } else if desires.get(current).unwrap().start_priority < new_des.start_priority {
                    // If the desire we're looking at is greater than our current, insert
                    //println!("Insert Position: {}", current);
                    desires.insert(current, new_des);
                    break;
                }
                // If we haven't walked off the end just yet,
                // and we haven't found a match
                // AND we the current is still less than or equal to our new desires start
                // step up 1 and try again.
                //println!("Current Start: {}", desires.get(current).unwrap().start);
                current += 1;
            }
        }
    }

    /// Helper for getting multiplier on desires based on tags. This is is used in
    /// multiple places and is likely to change in the future.
    pub(crate) fn get_desire_multiplier(desire: &Desire, row: &DRow, new_des: &mut Desire) {
        // get multiplier
        let mut multiplier = 0.0;
        for tag in desire.tags.iter() {
            if let DesireTag::HouseholdNeed = tag {
                debug_assert!(multiplier == 0.0, 
                    "Mulitpliper already set here, either duplicate tag found or another tag is HouseMemberNeed, which shouldn't be next to HouseholdNeed.");
                multiplier = row.household.count;
            } else if let DesireTag::HouseMemberNeed(member) = tag {
                debug_assert!(multiplier == 0.0, 
                    "Mulitpliper already set here, either duplicate tag found or another tag is HouseMemberNeed, which shouldn't be next to HouseholdNeed.");
                match member {
                    crate::household::HouseholdMember::Adult => multiplier = row.household.total_adults(),
                    crate::household::HouseholdMember::Child => multiplier = row.household.total_children(),
                    crate::household::HouseholdMember::Elder => multiplier = row.household.total_elders(),
                }
            }
        }
        // If no other tag sets Multiplier, then set to total population.
        if multiplier == 0.0 {
            multiplier = row.household.population();
        }
        // multiply the desrie amount by the multiplier.
        new_des.amount = new_des.amount * multiplier;
    }
    
    /// # Ordered Desire Insert
    /// 
    /// Helper function, inserts a desire into the working desires list.
    /// 
    /// Highest value to lowest order. Any duplicates values are added at the end 
    /// of the duplicates.
    /// 
    /// ## NOTE: This may need to be reworked to store the current priority also to reduce compulational load.
    pub(crate) fn ordered_desire_insert(working_desires: &mut VecDeque<Desire>, desire: Desire) {
        let value = desire.current_priority();
        for idx in 0..working_desires.len() {
            if value < working_desires.get(idx).unwrap().current_priority() {
                working_desires.insert(idx, desire);
                return;
            }
        }
        working_desires.push_back(desire);
    }
}

/// Helper for pop property.
#[derive(Debug, Copy, Clone)]
pub struct PropertyRecord {
    /// How many units are owned by the pop right now.
    pub owned: f64,
    /// How many they want to keep at all times. This also covers
    /// reservations to satisfy desires.
    pub reserved: f64,
    /// How many they have used today to satisfy desires.
    /// This records how many were consumed today.
    /// 
    /// At the end of the day, this should be equivalent to reserved goods.
    pub expended: f64,
    /// How many has been 'used' and cannot be used or expended again.
    /// 
    /// This covers storage for both ownership and used products.
    pub used: f64,
    /// How many were given up in trade.
    pub traded: f64,
    /// How many were offered, but not accepted.
    pub offered: f64,
    /// A Target number of the good we want to have by day end.
    pub target: f64,
}

impl PropertyRecord {
    pub fn new(owned: f64) -> Self {
        Self {
            owned,
            reserved: 0.0,
            expended: 0.0,
            used: 0.0,
            traded: 0.0,
            offered: 0.0,
            target: 0.0,
        }
    }

    /// Available
    /// 
    /// How many goods are available to be used/expended.
    /// This is effectively the difference between owned and reserved.
    pub fn available(&self) -> f64 {
        self.owned - self.reserved
    }

    /// # Current Target
    /// 
    /// How many more goods we need to reach our target.
    /// 
    /// Equal to target - owned
    pub fn current_target(&self) -> f64 {
        self.target - self.owned
    }
}

/// # Want Record
/// 
/// Records want data for the pop, including how much is available today,
/// reserved wants,
/// 
/// At the end of the day, reserved and expended should be equvialent.
#[derive(Debug, Clone)]
pub struct WantRecord {
    /// How much is currnetly owned.
    pub owned: f64,
    /// How much has been reserved for desires
    pub reserved: f64,
    /// How many we are expecting to get during consumption.
    pub expected: f64,
    /// How many we have expended. Used do record wants that have been
    /// expended in consuming desires.
    /// 
    /// this should be equal to reserved at the end of the day.
    pub expended: f64,
}

impl WantRecord {
    pub fn new() -> Self {
        Self {
            owned: 0.0,
            reserved: 0.0,
            expected: 0.0,
            expended: 0.0
        }
    }
    
    /// # Available 
    /// 
    /// How many wants are available for planning purposes.
    /// 
    /// Includes currently owned and expected and removes wants that are 
    /// already reserved.
    /// 
    /// As wants cannot be traded, this should be safe in all cases.
    fn available(&self) -> f64 {
        self.owned - self.reserved + self.expected
    }
}

/// # Satsifaction Values
/// 
/// A helper storage unit which contains both the range of satisfaction and the 
/// number of steps satisfied.
#[derive(Debug, Copy, Clone)]
pub struct SatisfactionValues {
    pub range: f64,
    pub steps: f64,
    pub amv: f64,
}

impl SatisfactionValues {
    pub fn new(range: f64, steps: f64, amv: f64) -> Self {
        Self {
            range,
            steps,
            amv
        }
    }

    /// Helper that gets the density of the current satisfaction.
    pub fn density(&self) -> f64 {
        self.steps / self.range
    }
    
    fn zero() -> SatisfactionValues {
        Self::new(0.0, 0.0, 0.0)
    }
}