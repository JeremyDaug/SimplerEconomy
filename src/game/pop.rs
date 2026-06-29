use std::collections::HashMap;

use bevy::{reflect::DynamicArray, utils::default};

use crate::game::{desire::{Desire, DesireTargetType}, household::HouseholdDef, market::{Market, MarketHistory}, marketorder::MarketOrder, scalingfactor::ScalingFactor};

#[derive(Debug, Clone)]
pub struct Pop {
    /// The unique ID of the pop. May drop this or replace it to simplify.
    /// 
    /// Should be stored in the market alone. ID may be allowed to be non-unique
    /// between markets, but unique within a market.
    pub id: usize,

    /// The ID of the pop's job. Each pop only has 1 right now.
    pub job: usize,

    ///  The property and details of the property
    pub property: HashMap<usize, PopPRow>,
    
    /// Desires of a pop, a consolidated and organized for satisfaction calculations.
    /// 
    /// Nested Vec of Vecs.
    /// 0: Basic Needs
    /// 1: Common Needs
    /// 2: Luxury Needs
    /// 
    /// When trying to satisfy desires, they will always try to fill all of basic needs 
    /// first, then common needs, then Luxury needs. Once it has filled up Luxury needs
    /// it will repeatedly fill up Luxury needs indefinitely, stoping only when it runs
    /// out of goods to satisfy the desires with.
    pub desires: Vec<Vec<Desire>>,

    /// The working desires of the pop, a flat structure that goes :
    /// Basic Needs -> Common Needs -> Luxury Needs. 
    /// 
    /// If a pop satisfies all of these, Luxury needs will be duplicated and added
    /// to the end. Repeat until they are unable to satisfy any more, or run out of
    /// useable trade goods.
    /// 
    /// When a working desire is done (either full or unable to be satisfied) it is
    /// returned to desires proper.
    pub working_desires: Vec<Desire>,

    /// The demographic breakdown of this pop.
    /// 
    /// This may be expanded to be a vector of Demographic Rows, to consolidate
    /// multiple pops of different cultures into one.
    pub demographics: DemoRow,
}

impl Pop {
    /// # Start Day
    /// 
    /// Function called at the start of the day to give a pop it's daily generating
    /// goods.
    /// 
    /// `new_goods` are the goods the pop is gaining at the start of a day.
    /// 
    /// This includes both the good in question and the factor by which it is scaled,
    /// if any. Some factors cannot be handled here, and must be replaced at higher 
    /// levels.
    /// 
    /// The choice of Scaling factor ensures it can scale here, rather than above.
    pub fn start_day(&mut self, new_goods: &Vec<(usize, ScalingFactor)>) {
        for (good_id, scaling) in new_goods.iter() {
            match scaling {
                ScalingFactor::Fixed(f) => {
                    self.property.entry(*good_id)
                        .and_modify(|x| x.quantity += f)
                        .or_insert(PopPRow::new(*f));
                },
                ScalingFactor::All(f) => {
                    self.property.entry(*good_id)
                        .and_modify(|x| x.quantity += f * self.demographics.total_population())
                        .or_insert(PopPRow::new(f * self.demographics.total_population()));
                },
                ScalingFactor::Household(f) => {
                    self.property.entry(*good_id)
                        .and_modify(|x| x.quantity += f * self.demographics.count)
                        .or_insert(PopPRow::new(f * self.demographics.count));
                },
                ScalingFactor::Adults(f) => {
                    self.property.entry(*good_id)
                        .and_modify(|x| x.quantity += f * self.demographics.adult_pop())
                        .or_insert(PopPRow::new(f * self.demographics.adult_pop()));
                },
                ScalingFactor::Children(f) => {
                    self.property.entry(*good_id)
                        .and_modify(|x| x.quantity += f * self.demographics.children_pop())
                        .or_insert(PopPRow::new(f * self.demographics.children_pop()));
                },
                ScalingFactor::Elders(f) => {
                    self.property.entry(*good_id)
                        .and_modify(|x| x.quantity += f * self.demographics.elder_pop())
                        .or_insert(PopPRow::new(f * self.demographics.elder_pop()));
                },
                ScalingFactor::Labor(f) => {
                    self.property.entry(*good_id)
                        .and_modify(|x| x.quantity += f * self.demographics.labor())
                        .or_insert(PopPRow::new(f * self.demographics.labor()));
                },
            }
        }
    }

    /// # Create Orders
    /// 
    /// Creates request and offer orders for a pop. 
    /// 
    /// For now, this only puts out it's desires, not its, possible offers.
    /// 
    /// When Offer Orders can be put out, it should only be when the pop is 
    /// despirate enough to offer goods in exchange for other things.
    pub fn create_orders(&self, market: &MarketHistory) -> Vec<MarketOrder> {
        
    }

    /// # Update Desires
    /// 
    /// Called at the end of each day, after the population has changed in size due to 
    /// growth and migration, this updates the amount requested by desires to correctly 
    /// scale with the new population.
    pub fn update_desires(&mut self) {
        todo!()
    }

    /// # Next Shopping Trip
    /// 
    /// Used during the day, this decides what a pop will want to buy next. It does this
    /// 
    pub fn next_shopping_trip(&self) {
        todo!()
    }

    /// # Process Satisfaction
    /// 
    /// This extracts satisfaction data, applies effects from desires, and recalculates
    /// the mood of a pop for political and future needs.
    pub fn process_satisfaction(&mut self) -> () {
        todo!()
    }

    /// # Consume
    /// 
    /// Consumes goods from `property` to satisfy a pop's desires.
    /// 
    /// Goods should already be reserved and ready to be consumed, so do so.
    /// 
    /// - Basic (0) and Common (1) tiers are processed **once each**, in list order,
    ///   filling to the best of the pop's ability before moving to the next tier.
    /// - Luxury (2) desires are **repeatedly cycled** and overfilled as much as possible
    ///   until no further progress can be made with remaining goods.
    ///
    /// For desires with a bucket of goods, higher-efficiency goods are preferred.
    ///
    /// The results of the consumption is stored in the desires as satisfaction.
    ///
    /// Also mutates `self.property` (reduces `quantity`, increases `consumed`).
    /// 
    /// The function assumes that all desires are currently in `self.desires` and
    /// none are in `self.working_desires`.
    pub fn consume(&mut self) {
        let mut curr_tier = 0;
        let mut working_desires = vec![];
        
        // first do basic desires, only one pass needed.
        working_desires = self.desires.remove(0); // pop off front
        self.satisfy_tier(&mut working_desires); // satisfy them
        self.desires.insert(0, working_desires); // put back

        // second do common needs, only one pass needed.
        working_desires = self.desires.remove(1); // pop off
        self.satisfy_tier(&mut working_desires); // satisfy
        self.desires.insert(1, working_desires); // put back

        // Last is Luxury Needs, do until we produce no more satisfaction.
        let mut iter_target = 1.0;
        working_desires = self.desires.remove(2); // pop off
        let mut ordered_desires = vec![];
        loop {// loop over desires
            // satisfy the current working desires
            self.satisfy_tier(&mut working_desires);
            // remove any desires not fully satisfied.
            let mut idx = 0;
            loop {
                if idx >= working_desires.len() { break; } // break out if we walk off the end.
                if working_desires[idx].tiers_satisfied() < iter_target {
                    // if not satisfied to our target, move to ordered_desires
                    ordered_desires.push(working_desires.remove(idx));
                } else {
                    // otherwise, increment idx by one and go on
                    idx += 1;
                }
            }
            // if nothing to go onto next time, break out.
            if working_desires.len() == 0 {
                break;
            } else { iter_target += 1.0; } // otherwise increment target and go again.
        } 
        // sasitsfacions done, reorganize
        ordered_desires.sort_by(|a, b| a.idx.cmp(&b.idx));
        self.desires.insert(2, ordered_desires); // put back
    }

    /// # Satisfy Tier
    /// 
    /// Takes a list of desires (presumably a tier) and tries to satisfy each desire
    /// in order.
    /// 
    /// Will consume desires for satisfaction.
    /// 
    /// Returns the highest success rate, useful for checking if any desire reached the 
    /// next full level.
    pub fn satisfy_tier(&mut self, desires: &mut Vec<Desire>) -> f64 {
        let mut success: f64 = 0.0;
        for desire in desires.iter_mut() {
            let result = self.satisfy_one_desire(desire);
            success = success.max(result);
        }
        success
    }

    /// # Satisfy One Desire
    /// 
    /// A helper which takes a single desire and tries to satisfy it to one level. It 
    /// returns final satisfaction level.
    pub(crate) fn satisfy_one_desire(&mut self, desire: &mut Desire) -> f64 {
        // Clone + sort by efficiency descending (best substitutes first)
        let mut targets = desire.target.clone();
        targets.sort_by(|a, b| b.efficiency.partial_cmp(&a.efficiency)
            .unwrap_or(std::cmp::Ordering::Equal));

        let mut remaining = desire.amount;

        for target in targets.iter() {
            if remaining <=  0.0 {
                break;
            }
            // get the target good, or continue on to the next target.
            if let Some(row) = self.property.get_mut(&target.good) && row.consumeable() > 0.0 {
                // remaining (Capped at the cap amount of the desire) divided by 
                // efficiency is how much is needed.
                let needed = remaining.min(desire.amount * target.cap) / target.efficiency;
                let take = needed.min(row.consumeable());

                // remove from quantity and reserve.
                row.quantity -= take;
                row.reserved -= take;
                match target.desire_type {
                    DesireTargetType::Consume => {
                        // shift to consumed.
                        row.consumed += take;
                        let sat_gained = take * target.efficiency;
                        desire.satisfaction += sat_gained;
                        remaining -= sat_gained;
                    },
                    DesireTargetType::Use => {
                        row.used += take;
                        let sat_gained = take * target.efficiency;
                        desire.satisfaction += sat_gained;
                        remaining -= sat_gained;
                    },
                }
            }
        }
        // The current satisfaction rate.
        desire.satisfaction / desire.amount
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DemoRow {
    /// The Number of households, floating point as growth storage.
    pub count: f64,
    /// The definition of this row's household. This is the sum of the baseline plus
    /// all other demographic effects.
    pub household: HouseholdDef,
    /// The species ID, currently should always be 0, which is default human.
    pub species: usize,
    /// The culture ID, 0 means none.
    pub culture: usize,
    /// The class ID, 0 means none.
    pub class: usize,
    /// The Religious ID, 0 means none.
    pub religion: usize,
}

impl DemoRow {
    /// # Total Population
    /// 
    /// Gets the total population of a demographic row, equal to the size
    /// of a household times it's count.
    pub fn total_population(&self) -> f64 {
        self.count * self.household.size()
    }

    pub fn adult_pop(&self) -> f64 {
        self.count * self.household.adults
    }

    pub fn elder_pop(&self) -> f64 {
        self.count * self.household.elders
    }

    pub fn children_pop(&self) -> f64 {
        self.count * self.household.children
    }

    pub fn labor(&self) -> f64 {
        self.count * self.household.labor()
    }
}

/// # Pop Property Row
/// 
/// A row of data attached to the property of a pop. This is used for fine tuned
/// property management and consumption details.
/// 
/// This currently only contains the current quantity, the amount reserved for 
/// consumption, and the target they want to reach.
#[derive(Debug, Clone, Copy, Default)]
pub struct PopPRow {
    /// The total amount owned at the moment.
    pub quantity: f64,
    
    /// The Target amount the pop desires to have after all shopping is complete.
    /// Simplifies purchases into bulk purchases.
    /// 
    /// Should target roughly 1.0-1.2x of the desire at minimum. When a pop has elevated
    /// savings, fear, or similar 'hodl' moods activated, this target is pushed up.
    /// Supply volatility can also push it up to ensure 
    pub target: f64,

    /// The amount that has already been earmarked for the pop's use today. Used 
    /// to 'prepare' for consumption. Does not remove from quantity.
    pub reserved: f64,
    /// The amount that we wish to preserve between days.
    pub saved: f64,

    /// How many of this good was consumed for today's desires.
    /// All goods here are decayed at the end of the day.
    pub consumed: f64,
    /// Goods that were used for use desires, but not consumed, are stored here
    /// until the end of the day. At day's end, goods here decay normally, and are
    /// returned to total quantity.
    pub used: f64,
}

impl PopPRow {
    pub fn new(quantity: f64) -> Self {
        Self {
            quantity,
            ..default()
        }
    }

    /// Fluent target setter
    pub fn with_target(mut self, target: f64) -> Self {
        self.target = target;
        self
    }

    /// fruent reserved setter
    pub fn with_reserve(mut self, reserve: f64) -> Self {
        self.reserved = reserve;
        self
    }

    /// Fluent saved setter
    pub fn with_saved(mut self, saved: f64) -> Self {
        self.saved = saved;
        self
    }

    /// Fluent Consumed Setter
    pub fn with_consumed(mut self, consumed: f64) -> Self {
        self.consumed = consumed;
        self
    }

    /// Fluent Used Setter
    pub fn with_used(mut self, used: f64) -> Self {
        self.used = used;
        self
    }

    /// # Exchange
    /// 
    /// `quantity` - `target`.
    /// 
    /// This gives the difference between the target to reach and the amount a pop owns.
    /// 
    /// Negative values are how much they will want to buy. Positive values are how many
    /// they are willing to offer or sell.
    pub fn exchange(&self) -> f64 {
        self.quantity - self.target
    }

    /// # Consumeable
    /// 
    /// `quantity` - `saved`.
    /// 
    /// Gives the difference between Current quantity and the amount a pop wants to
    /// save between days.
    /// 
    /// Should only be negative after decay has occurred.
    /// 
    /// Useful for picking out goods for consumption without overconsuming them.
    /// 
    /// This is typically a fraction of the target, and is modified along with target 
    /// and 
    pub fn consumeable(&self) -> f64 {
        self.quantity - self.saved
    }

    /// # Available
    /// 
    /// `quantity` - `reserved`.
    /// 
    /// This gives the amount of goods a pop has that have yet to be claimed by 
    /// another desire. Should always be non-negative.
    pub fn available(&self) -> f64 {
        self.quantity - self.reserved
    }
}

#[cfg(test)]
mod pop {
    use std::collections::HashMap;

    use crate::game::{desire::{
        Desire, DesireSource, DesireTarget, DesireTargetType
    }, household::HouseholdDef, pop::{DemoRow, Pop, PopPRow}, scalingfactor::ScalingFactor};

    static CONSUMED_GOOD: usize = 100;
    static USED_GOOD: usize = 101;
    static DECAY_GOOD: usize = 200;

    fn make_pop() -> Pop {
        Pop {
            id: 0,
            job: 0,
            property: HashMap::new(),
            desires: vec![vec![]; 3],
            working_desires: vec![],
            demographics: DemoRow {
                count: 10.0,
                household: HouseholdDef::default(),
                species: 0,
                culture: 0,
                class: 0,
                religion: 0,
            },
        }
    }

    fn make_desire(idx: usize, desire_target: DesireTarget, amount: f64) -> Desire {
        // Source doesn't matter for most uses, it's just for tracking purpopses.
        Desire {
            idx,
            source: DesireSource::Religion(0),
            target: vec![desire_target],
            amount,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Household(1.0),
        }
    }

    fn add_desire(mut pop: Pop, desire: Desire, tier: usize) -> Pop {
        pop.desires[tier].push(desire);
        pop
    }

    mod consume_should {
        use super::*;

        #[test]
        fn correctly_satisfy_desires_across_all_tiers() {
            // make pop
            let mut pop = make_pop();

            // make a bunch of desires across it's tiers.
            // ensure shared good between at least 2 tiers.
            // 2 basic
            let basicdesire1 = make_desire(0, 
                DesireTarget::new(100, DesireTargetType::Consume, 1.0), 10.0);
            let basicdesire2 = make_desire(1, 
                DesireTarget::new(101, DesireTargetType::Consume, 1.0), 10.0);
            pop.desires[0].push(basicdesire1);
            pop.desires[0].push(basicdesire2);
            // 2 common
            let commondesire1 = make_desire(0, 
                DesireTarget::new(200, DesireTargetType::Consume, 1.0), 10.0);
            let commondesire2 = make_desire(1, 
                DesireTarget::new(101, DesireTargetType::Consume, 1.0), 10.0);
            pop.desires[1].push(commondesire1);
            pop.desires[1].push(commondesire2);
            // 2 luxuries
            let luxurydesire1 = make_desire(0, 
                DesireTarget::new(300, DesireTargetType::Consume, 1.0), 10.0);
            let luxurydesire2 = make_desire(1, 
                DesireTarget::new(100, DesireTargetType::Consume, 1.0), 10.0);
            pop.desires[2].push(luxurydesire1);
            pop.desires[2].push(luxurydesire2);

            // fill it's property as needed.
            pop.property.insert(100, 
                PopPRow::new(100.0).with_reserve(100.0));
            pop.property.insert(101, 
                PopPRow::new(100.0).with_reserve(20.0));
            pop.property.insert(200, 
                PopPRow::new(100.0).with_reserve(10.0));
            pop.property.insert(300, 
                PopPRow::new(100.0).with_reserve(100.0));

            // run test
            pop.consume();

            // check results
            // check property is correct
            assert_eq!(pop.property[&100].quantity, 0.0);
            assert_eq!(pop.property[&100].reserved, 0.0);
            assert_eq!(pop.property[&100].consumed, 100.0);
            assert_eq!(pop.property[&101].quantity, 80.0);
            assert_eq!(pop.property[&101].reserved, 0.0);
            assert_eq!(pop.property[&101].consumed, 20.0);
            assert_eq!(pop.property[&200].quantity, 90.0);
            assert_eq!(pop.property[&200].reserved, 0.0);
            assert_eq!(pop.property[&200].consumed, 10.0);
            assert_eq!(pop.property[&300].quantity, 0.0);
            assert_eq!(pop.property[&300].reserved, 0.0);
            assert_eq!(pop.property[&300].consumed, 100.0);
            // check 
            assert_eq!(pop.desires[0][0].satisfaction, 10.0);
            assert_eq!(pop.desires[0][1].satisfaction, 10.0);
            assert_eq!(pop.desires[1][0].satisfaction, 10.0);
            assert_eq!(pop.desires[1][1].satisfaction, 10.0);
            assert_eq!(pop.desires[2][0].satisfaction, 100.0);
            assert_eq!(pop.desires[2][1].satisfaction, 90.0);
        }
    }

    mod satisfy_tier_should {
        use super::*;

        #[test]
        fn satisfy_multiple_empty_desires() {
            // create Pop
            let mut test_pop = make_pop();

            // new up some simple desires
            let des1 = make_desire(0, DesireTarget::new(100, 
                DesireTargetType::Consume, 1.0), 10.0);
            let des2 = make_desire(1, DesireTarget::new(101, 
                DesireTargetType::Consume, 1.0), 10.0);
            let des3 = make_desire(2, DesireTarget::new(100, 
                DesireTargetType::Consume, 1.0), 10.0);
            let mut test_desires = vec![des1, des2, des3];

            // insert property to be consumed, don't forget the reservations
            test_pop.property.insert(100, PopPRow::new(100.0).with_reserve(20.0));
            test_pop.property.insert(101, PopPRow::new(100.0).with_reserve(20.0));

            // call function
            let result = test_pop.satisfy_tier(&mut test_desires);

            // check outcomes
            assert_eq!(result, 1.0);
            assert_eq!(test_pop.property[&100].quantity, 80.0);
            assert_eq!(test_pop.property[&100].reserved, 0.0);
            assert_eq!(test_pop.property[&100].consumed, 20.0);
            assert_eq!(test_pop.property[&101].quantity, 90.0);
            assert_eq!(test_pop.property[&101].reserved, 10.0);
            assert_eq!(test_pop.property[&101].consumed, 10.0);
            assert_eq!(test_desires[0].satisfaction, 10.0);
            assert_eq!(test_desires[1].satisfaction, 10.0);
            assert_eq!(test_desires[2].satisfaction, 10.0);
        }

        #[test]
        fn satisfy_multiple_after_first_pass_desires() {
            // create Pop
            let mut test_pop = make_pop();

            // new up some simple desires
            let mut des1 = make_desire(0, DesireTarget::new(100, 
                DesireTargetType::Consume, 1.0), 10.0);
                des1.satisfaction = 10.0;
            let mut des2 = make_desire(1, DesireTarget::new(101, 
                DesireTargetType::Consume, 1.0), 10.0);
                des2.satisfaction = 10.0;
            let mut des3 = make_desire(2, DesireTarget::new(100, 
                DesireTargetType::Consume, 1.0), 10.0);
                des3.satisfaction = 10.0;
            let mut test_desires = vec![des1, des2, des3];

            // insert property to be consumed, don't forget the reservations
            test_pop.property.insert(100, PopPRow::new(100.0).with_reserve(20.0));
            test_pop.property.insert(101, PopPRow::new(100.0).with_reserve(20.0));

            // call function
            let result = test_pop.satisfy_tier(&mut test_desires);

            // check outcomes
            assert_eq!(result, 2.0);
            assert_eq!(test_pop.property[&100].quantity, 80.0);
            assert_eq!(test_pop.property[&100].reserved, 0.0);
            assert_eq!(test_pop.property[&100].consumed, 20.0);
            assert_eq!(test_pop.property[&101].quantity, 90.0);
            assert_eq!(test_pop.property[&101].reserved, 10.0);
            assert_eq!(test_pop.property[&101].consumed, 10.0);
            assert_eq!(test_desires[0].satisfaction, 20.0);
            assert_eq!(test_desires[1].satisfaction, 20.0);
            assert_eq!(test_desires[2].satisfaction, 20.0);
        }

        #[test]
        fn return_largest_when_not_equal_satisfactions() {
            // create Pop
            let mut test_pop = make_pop();

            // new up some simple desires
            let des1 = make_desire(0, DesireTarget::new(100, 
                DesireTargetType::Consume, 1.0), 10.0);
            let des2 = make_desire(1, DesireTarget::new(101, 
                DesireTargetType::Consume, 1.0), 10.0);
            let des3 = make_desire(2, DesireTarget::new(100, 
                DesireTargetType::Consume, 1.0), 10.0);
            let mut test_desires = vec![des1, des2, des3];

            // insert property to be consumed, don't forget the reservations
            test_pop.property.insert(100, PopPRow::new(7.0).with_reserve(7.0));
            test_pop.property.insert(101, PopPRow::new(1.0).with_reserve(1.0));

            // call function
            let result = test_pop.satisfy_tier(&mut test_desires);

            // check outcomes
            assert_eq!(result, 0.7);
            assert_eq!(test_pop.property[&100].quantity, 0.0);
            assert_eq!(test_pop.property[&100].reserved, 0.0);
            assert_eq!(test_pop.property[&100].consumed, 7.0);
            assert_eq!(test_pop.property[&101].quantity, 0.0);
            assert_eq!(test_pop.property[&101].reserved, 0.0);
            assert_eq!(test_pop.property[&101].consumed, 1.0);
            assert_eq!(test_desires[0].satisfaction, 7.0);
            assert_eq!(test_desires[1].satisfaction, 1.0);
            assert_eq!(test_desires[2].satisfaction, 0.0);
        }
    }

    mod satisfy_one_desire_should {
        use super::*;

        #[test]
        fn correctly_satisfy_simple_consume_desire() {
            let mut test_pop = make_pop();

            test_pop.property.insert(CONSUMED_GOOD, 
                PopPRow::new(100.0).with_reserve(10.0));
            
            let mut test_desire = make_desire(0, DesireTarget::new(CONSUMED_GOOD, 
                DesireTargetType::Consume, 1.0), 10.0);

            let result = test_pop.satisfy_one_desire(&mut test_desire);
            assert_eq!(result, 1.0);
            assert_eq!{test_desire.satisfaction, 10.0};
            assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 90.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 10.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
        }

        #[test]
        fn correctly_satisfy_simple_use_desire() {
            let mut test_pop = make_pop();

            test_pop.property.insert(USED_GOOD, 
                PopPRow::new(100.0).with_reserve(10.0));
            
            let mut test_desire = make_desire(0, DesireTarget::new(USED_GOOD, 
                DesireTargetType::Use, 1.0), 10.0);

            let result = test_pop.satisfy_one_desire(&mut test_desire);
            assert_eq!(result, 1.0);
            assert_eq!{test_desire.satisfaction, 10.0};
            assert_eq!(test_pop.property[&USED_GOOD].quantity, 90.0);
            assert_eq!(test_pop.property[&USED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].used, 10.0);
        }

        #[test]
        fn partially_fill_desire() {
            let mut test_pop = make_pop();

            test_pop.property.insert(CONSUMED_GOOD, 
                PopPRow::new(5.0).with_reserve(5.0));
            
            let mut test_desire = make_desire(0, DesireTarget::new(CONSUMED_GOOD, 
                DesireTargetType::Consume, 1.0), 10.0);

            let result = test_pop.satisfy_one_desire(&mut test_desire);
            assert_eq!(result, 0.5);
            assert_eq!{test_desire.satisfaction, 5.0};
            assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 5.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
        }

        #[test]
        fn not_touch_savings() {
            let mut test_pop = make_pop();

            let prop = PopPRow::new(10.0)
                .with_saved(5.0)
                .with_reserve(5.0);
            test_pop.property.insert(CONSUMED_GOOD, prop);
            
            let mut test_desire = make_desire(0, DesireTarget::new(CONSUMED_GOOD, 
                DesireTargetType::Consume, 1.0), 10.0);

            let result = test_pop.satisfy_one_desire(&mut test_desire);
            assert_eq!(result, 0.5);
            assert_eq!{test_desire.satisfaction, 5.0};
            assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 5.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].saved, 5.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 5.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
        }

        #[test]
        fn correctly_satisfy_complex_desire_same_efficiencies() {
            let mut test_pop = make_pop();
            
            let mut test_desire = make_desire(0, DesireTarget::new(USED_GOOD, 
                DesireTargetType::Use, 1.0), 10.0);
            test_desire.target.push(
                DesireTarget::new(CONSUMED_GOOD, DesireTargetType::Consume, 1.0));

            // Split evenly
            test_pop.property.insert(CONSUMED_GOOD, PopPRow::new(5.0)
                .with_reserve(5.0));
            test_pop.property.insert(USED_GOOD, PopPRow::new(5.0)
                .with_reserve(5.0));

            let result = test_pop.satisfy_one_desire(&mut test_desire);
            assert_eq!(result, 1.0);
            assert_eq!{test_desire.satisfaction, 10.0};
            assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 5.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].quantity, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].used, 5.0);
        }

        #[test]
        fn correctly_satisfy_complex_desire_different_efficiencies() {
            let mut test_pop = make_pop();
            
            let mut test_desire = make_desire(0, DesireTarget::new(USED_GOOD, 
                DesireTargetType::Use, 0.5), 10.0);
            test_desire.target.push(
                DesireTarget::new(CONSUMED_GOOD, DesireTargetType::Consume, 1.0));

            // Split evenly
            test_pop.property.insert(CONSUMED_GOOD, PopPRow::new(5.0)
                .with_reserve(5.0));
            test_pop.property.insert(USED_GOOD, PopPRow::new(5.0)
                .with_reserve(5.0));

            let result = test_pop.satisfy_one_desire(&mut test_desire);
            assert_eq!(result, 0.75);
            assert_eq!{test_desire.satisfaction, 7.5};
            assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 5.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].quantity, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].used, 5.0);
        }

        #[test]
        fn correctly_satisfy_complex_desire_capped_inputs() {
            let mut test_pop = make_pop();
            
            let mut test_target = DesireTarget::new(USED_GOOD, DesireTargetType::Use, 1.0);
            test_target.cap = 0.5;
            let mut test_desire = make_desire(0, test_target, 10.0);
            test_desire.target.push(
                DesireTarget::new(CONSUMED_GOOD, DesireTargetType::Consume, 1.0));

            // Split evenly
            test_pop.property.insert(CONSUMED_GOOD, PopPRow::new(10.0)
                .with_reserve(5.0));
            test_pop.property.insert(USED_GOOD, PopPRow::new(10.0)
                .with_reserve(5.0));

            let result = test_pop.satisfy_one_desire(&mut test_desire);
            assert_eq!(result, 1.0);
            assert_eq!{test_desire.satisfaction, 10.0};
            assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 5.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 5.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].quantity, 5.0);
            assert_eq!(test_pop.property[&USED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].used, 5.0);
        }

        #[test]
        fn correctly_satisfy_complex_desire_with_correct_order_priority() {
            let mut test_pop = make_pop();
            
            let used_target = DesireTarget::new(USED_GOOD, DesireTargetType::Use, 1.0);

            let mut test_desire = make_desire(0, used_target, 10.0);
            test_desire.target.push(
                DesireTarget::new(CONSUMED_GOOD, DesireTargetType::Consume, 1.0));

            // used first, consumed second
            test_pop.property.insert(CONSUMED_GOOD, PopPRow::new(10.0));
            test_pop.property.insert(USED_GOOD, PopPRow::new(10.0)
                .with_reserve(10.0));

            let result = test_pop.satisfy_one_desire(&mut test_desire);
            assert_eq!(result, 1.0);
            assert_eq!{test_desire.satisfaction, 10.0};
            assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 10.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].quantity, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].used, 10.0);
        }

        #[test]
        fn correctly_satisfy_complex_desire_with_correct_efficiency_priority() {
            let mut test_pop = make_pop();
            
            let test_target = DesireTarget::new(USED_GOOD, DesireTargetType::Use, 1.0);
            let mut test_desire = make_desire(0, test_target, 10.0);

            test_desire.target.push(
                DesireTarget::new(CONSUMED_GOOD, DesireTargetType::Consume, 1.25));

            // consume first, then used
            test_pop.property.insert(CONSUMED_GOOD, PopPRow::new(10.0)
                .with_reserve(8.0));
            test_pop.property.insert(USED_GOOD, PopPRow::new(10.0)
                .with_reserve(10.0));

            let result = test_pop.satisfy_one_desire(&mut test_desire);
            assert_eq!(result, 1.0);
            assert_eq!{test_desire.satisfaction, 10.0};
            assert_eq!(test_pop.property[&CONSUMED_GOOD].quantity, 2.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].reserved, 0.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].consumed, 8.0);
            assert_eq!(test_pop.property[&CONSUMED_GOOD].used, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].quantity, 10.0);
            assert_eq!(test_pop.property[&USED_GOOD].reserved, 10.0);
            assert_eq!(test_pop.property[&USED_GOOD].consumed, 0.0);
            assert_eq!(test_pop.property[&USED_GOOD].used, 0.0);
        }
    }
}

#[cfg(test)]
mod firm {
    use crate::game::factuals::Factuals;
    use crate::game::good::Good; // if you need Good defs
    use crate::game::market::{Market, MarketGood};
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput, ProcessEffect};
    use std::collections::HashMap;
    use crate::game::firm::{Firm, FirmPRow, ProductionLine, ProductionReport};

    fn make_good(id: usize, name: &str, decay_result: HashMap<usize, f64>) -> Good {
        Good {
            id,
            name: name.to_string(),
            class: None,
            tags: Default::default(),
            decay_rate: 0.0,
            decay_result,
            categories: vec![],
            // add any other fields your Good actually has
        }
    }

    // Helper to build a minimal Factuals with one process
    fn make_factuals_with_process(process: Process) -> Factuals {
        let mut processes = HashMap::new();
        processes.insert(process.id, process);
        let mut goods = HashMap::new();
        Factuals {
            goods, // goods not strictly needed for do_process in these tests
            processes,
        }
    }

    // Helper to build a Market with AMV data for the goods we care about
    fn make_market_with_amvs(amvs: &[(usize, f64)]) -> Market {
        let mut goods = HashMap::new();
        for &(id, amv) in amvs {
            goods.insert(id, MarketGood {
                amv,
                production: 0.0,
                consumption: 0.0,
                imported: 0.0,
                stock: 0.0,
            });
        }
        Market {
            id: 42,
            pops: HashMap::new(),
            goods,
        }
    }

    fn empty_firm_row(quantity: f64) -> FirmPRow {
        FirmPRow {
            quantity,
            rolling_average: 0.0,
            target: 0.0,
            reserve: 0.0,
            average_cost: 0.0,
            used_capital: 0.0,
        }
    }

    fn empty_production_line(process_id: usize) -> ProductionLine {
        ProductionLine {
            process: process_id,
            target: None,
            inputs: vec![],
            historical_productivity: 0.0,
            last_success_rate: 0.0,
            last_iterations: 0.0,
            last_effects: vec![],
            last_missing_goods: vec![],
            last_amv_consumed: 0.0,
            last_amv_produced: 0.0,
        }
    }

    mod run_production_should {
        use crate::game::process::InputEffect;
        use super::*;

        #[test]
        fn test_basic_production_run() {
            // Simple process: 2 wood -> 1 plank (Consumed input, fixed output)
            let process = Process::new(1, "sawmill", 0)
                .with_input(ProcessInput::new(10, 2.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "plank", HashMap::new()));

            let mut firm = Firm::new(1, "Test Sawmill".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow {
                quantity: 10.0,
                rolling_average: 0.0,
                target: 0.0,
                reserve: 0.0,
                average_cost: 0.0,
                used_capital: 0.0,
            });

            // Add a production line
            firm.production_line.push(ProductionLine {
                process: 1,
                target: None,
                inputs: vec![10],
                historical_productivity: 0.0,
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
            });

            let market = make_market_with_amvs(&[(10, 5.0), (20, 12.0)]);

            let report = firm.run_production(&factuals, &market);

            // Property should be updated
            assert_eq!(firm.property[&10].quantity, 0.0);
            assert_eq!(firm.property[&20].quantity, 5.0); // 5 iterations * 1.0

            // Report should show what was produced and consumed
            assert_eq!(report.produced.get(&20), Some(&5.0));
            assert_eq!(report.consumed.get(&10), Some(&10.0));
            assert!(report.effects.is_empty());

            // Line should have recorded success + AMV snapshots
            let line = &firm.production_line[0];
            assert_eq!(line.last_success_rate, 1.0);
            assert_eq!(line.last_iterations, 5.0);
            assert_eq!(line.last_amv_consumed, 50.0);
            assert_eq!(line.last_amv_produced, 60.0);
        }

        #[test]
        fn test_partial_run_with_target_and_missing_goods() {
            let process = Process::new(2, "limited_craft", 0)
                .with_input(ProcessInput::new(30, 3.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(40, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(30, make_good(30, "wood", HashMap::new()));
            factuals.goods.insert(40, make_good(40, "plank", HashMap::new()));

            let mut firm = Firm::new(2, "Limited Workshop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(30, FirmPRow {
                quantity: 6.0, // only enough for 2 iterations (need 3 per iter)
                ..Default::default() // we'll add used_capital etc. via insert if needed
            });

            firm.production_line.push(ProductionLine {
                process: 2,
                target: Some(10.0), // wants 10, will only get ~2
                inputs: vec![30],
                historical_productivity: 0.0,
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
            });

            let market = make_market_with_amvs(&[(30, 2.0), (40, 8.0)]);

            let report = firm.run_production(&factuals, &market);

            // check property changes
            assert_eq!(firm.property[&30].quantity, 0.0);
            assert_eq!(firm.property[&40].quantity, 2.0);

            let line = &firm.production_line[0];
            //assert!((line.last_success_rate - 0.233333).abs() < 0.01);
            assert_eq!(line.last_success_rate, 0.2);
            assert_eq!(line.last_iterations, 2.0);
            assert_eq!(line.last_missing_goods, vec![30]);
            assert_eq!(line.last_amv_consumed, 12.0);
            assert_eq!(line.last_amv_produced, 16.0);

            assert_eq!(report.consumed.get(&30), Some(&6.0));
            assert_eq!(report.produced.get(&40), Some(&2.0));
        }

        #[test]
        fn test_capital_goods_not_counted_as_consumed() {
            // Process that uses a Capital good (e.g. saw blade) + consumes wood
            let process = Process::new(3, "capital_test", 0)
                .with_input(ProcessInput::new(50, 1.0, true, InputType::Capital, false)) // saw
                .with_input(ProcessInput::new(10, 2.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(20, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(20, "wood", HashMap::new()));
            factuals.goods.insert(50, make_good(50, "plank", HashMap::new()));

            let mut firm = Firm::new(3, "Capital Test Firm".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow { quantity: 10.0, ..Default::default() });
            firm.property.insert(50, FirmPRow { quantity: 1.0, ..Default::default() });

            firm.production_line.push(ProductionLine {
                process: 3,
                target: None,
                inputs: vec![50, 10],
                historical_productivity: 0.0,
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
            });

            let market = make_market_with_amvs(&[(10, 5.0), (20, 12.0), (50, 100.0)]);

            let report = firm.run_production(&factuals, &market);

            // Capital good should be recorded in used_capital, not in report.consumed
            assert_eq!(firm.property[&50].used_capital, 1.0);
            assert_eq!(firm.property[&50].quantity, 0.0);
            assert_eq!(firm.property[&10].quantity, 8.0);

            assert!(report.consumed.get(&50).is_none()); // capital should NOT appear in consumed
            assert_eq!(report.consumed.get(&10), Some(&2.0));
            assert_eq!(report.produced.get(&20), Some(&1.0));
        }

        #[test]
        fn test_effects_and_new_output_good() {
            let process = Process::new(4, "researchy", 0)
                .with_input(ProcessInput::new(10, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(99, 2.0, true))
                .with_effect(ProcessEffect::Research(10.0));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(10, make_good(10, "wood", HashMap::new()));
            factuals.goods.insert(20, make_good(99, "plank", HashMap::new()));

            let mut firm = Firm::new(4, "Research Lab".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(10, FirmPRow { quantity: 5.0, ..Default::default() });

            firm.production_line.push(ProductionLine {
                process: 4,
                target: None,
                inputs: vec![10],
                historical_productivity: 0.0,
                last_success_rate: 0.0,
                last_iterations: 0.0,
                last_effects: vec![],
                last_missing_goods: vec![],
                last_amv_consumed: 0.0,
                last_amv_produced: 0.0,
            });

            let market = make_market_with_amvs(&[(10, 3.0), (99, 50.0)]);

            let report = firm.run_production(&factuals, &market);

            assert_eq!(report.effects.len(), 1);
            assert!(matches!(report.effects[0], ProcessEffect::Research(50.0)));

            // New good 99 should have been created in property
            assert!(firm.property.contains_key(&99));
            assert_eq!(firm.property[&99].quantity, 10.0);
        }

        #[test]
        #[should_panic(expected = "Process not found!")]
        fn test_unknown_process_panics() {
            let factuals = Factuals {
                goods: HashMap::new(),
                processes: HashMap::new(),
            };

            let mut firm = Firm::new(5, "Broken Firm".into(), 42, hexx::Hex::new(0, 0));
            firm.production_line.push(ProductionLine {
                process: 999, // does not exist
                target: Some(5.0),
                inputs: vec![],
                historical_productivity: 0.0,
                last_success_rate: 0.42,
                last_iterations: 3.0,
                last_effects: vec![ProcessEffect::Culture(1.0)],
                last_missing_goods: vec![1],
                last_amv_consumed: 10.0,
                last_amv_produced: 0.0,
            });

            let market = make_market_with_amvs(&[]);

            firm.run_production(&factuals, &market);
        }
    
        #[test]
        fn test_multi_line_chain_with_shared_capital() {
            // Line 1: wood (Consumed) + saw (Capital) → planks
            // Line 2: planks (Consumed) + saw (Capital) → furniture
            let sawmill = Process::new(10, "sawmill", 0)
                .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false)) // wood
                .with_input(ProcessInput::new(200, 1.0, true, InputType::Capital, false))  // saw
                .with_output(ProcessOutput::new(110, 1.0, true)); // planks

            let workshop = Process::new(11, "workshop", 0)
                .with_input(ProcessInput::new(110, 1.0, true, InputType::Destroyed, false)) // planks
                .with_input(ProcessInput::new(200, 1.0, true, InputType::Capital, false))  // same saw
                .with_output(ProcessOutput::new(120, 1.0, true)); // furniture

            let mut factuals = make_factuals_with_process(sawmill);
            factuals.processes.insert(11, workshop);
            factuals.goods.insert(100, make_good(100, "wood", HashMap::new()));
            factuals.goods.insert(110, make_good(110, "plank", HashMap::new()));
            factuals.goods.insert(120, make_good(120, "table", HashMap::new()));
            factuals.goods.insert(200, make_good(200, "saw", HashMap::new()));

            let mut firm = Firm::new(1, "Integrated Workshop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(100, empty_firm_row(20.0)); // wood
            firm.property.insert(200, empty_firm_row(20.0));  // saw (shared capital)
            firm.property.insert(110, empty_firm_row(0.0));  // planks (will be produced then consumed)

            // Two lines in priority order
            firm.production_line.push(empty_production_line(10)); // sawmill
            firm.production_line[0].inputs = vec![100, 200];
            firm.production_line[0].target = Some(5.0);

            firm.production_line.push(empty_production_line(11)); // workshop
            firm.production_line[1].inputs = vec![110, 200];
            firm.production_line[1].target = Some(3.0);

            let market = make_market_with_amvs(&[(100, 2.0), (110, 5.0), (120, 15.0), (200, 50.0)]);

            let report = firm.run_production(&factuals, &market);

            // Property assertions
            assert_eq!(firm.property[&100].quantity, 15.0);   // 20 - 5
            assert_eq!(firm.property[&110].quantity, 2.0);    // produced 5, consumed 3, 
            assert_eq!(firm.property[&200].used_capital, 8.0); // 5 + 3
            assert_eq!(firm.property[&200].quantity, 12.0);    // 20- 5 - 3
            // (adjust expected numbers based on exact per-iter costs you want)

            // Report aggregation across both lines
            assert_eq!(report.produced.get(&110), Some(&5.0)); // planks created
            assert_eq!(report.produced.get(&120), Some(&3.0)); // tables created
            assert_eq!(report.consumed.get(&100), Some(&5.0)); // wood
            assert_eq!(report.consumed.get(&110), Some(&3.0));  // planks consumed in line 2
            assert!(report.consumed.get(&200).is_none());       // capital never in consumed

            // Both lines recorded AMV snapshots
            assert_eq!(firm.production_line[0].last_amv_consumed, 10.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 25.0);
            assert_eq!(firm.production_line[1].last_amv_consumed, 15.0);
            assert_eq!(firm.production_line[1].last_amv_produced, 45.0);
        }

        #[test]
        fn test_required_and_optional_factors() {
            // Required factor (water) + optional factor (skilled labor bonus)
            let process = Process::new(20, "factor_test", 0)
                .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false))
                .with_input(ProcessInput::new(110, 1.0, false, InputType::Destroyed, false))
                .with_input(ProcessInput::new(300, 1.0, true, InputType::Factor, false)) // required water
                .with_input(ProcessInput::new(301, 1.0, true, InputType::Factor, true)   // optional skilled
                    .with_optional(InputEffect::Throughput(0.5)))
                .with_output(ProcessOutput::new(120, 1.0, false));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(100, make_good(100, "wood", HashMap::new()));
            factuals.goods.insert(110, make_good(110, "planks", HashMap::new()));
            factuals.goods.insert(120, make_good(120, "ash", HashMap::new()));
            factuals.goods.insert(300, make_good(300, "sunlight", HashMap::new()));
            factuals.goods.insert(301, make_good(301, "clear skys", HashMap::new()));

            let mut firm = Firm::new(2, "Factor Firm".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(100, empty_firm_row(20.0));
            firm.property.insert(110, empty_firm_row(40.0));
            firm.property.insert(300, empty_firm_row(1.0)); // has required factor
            // 301 (skilled) deliberately missing

            firm.production_line.push(empty_production_line(20));
            firm.production_line[0].inputs = vec![100, 110, 300, 301];
            firm.production_line[0].target = None;

            let market = make_market_with_amvs(&[(100, 2.0), (110, 6.0), (120, 20.0)]);

            let report = firm.run_production(&factuals, &market);

            // Should run (required factor present) but without the optional throughput bonus
            assert!(firm.production_line[0].last_success_rate > 0.9);
            assert_eq!(firm.production_line[0].last_iterations, 20.0);
            assert_eq!(firm.production_line[0].last_missing_goods.len(), 1);
            assert!(firm.production_line[0].last_missing_goods.contains(&100));
            assert_eq!(firm.production_line[0].last_amv_consumed, 160.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 400.0);
            assert_eq!(report.consumed.get(&100), Some(&20.0)); // 10 iterations * 2.0
            assert_eq!(report.consumed.get(&110), Some(&20.0)); // 10 iterations * 2.0
            assert_eq!(report.produced.get(&120), Some(&20.0)); // 10 iterations * 2.0

            // test with optional factor included
            firm.property.insert(301, empty_firm_row(1.0));
            firm.property.get_mut(&100).unwrap().quantity += 20.0;
            firm.property.get_mut(&110).unwrap().quantity += 20.0;
            firm.production_line[0].last_amv_consumed = 0.0;
            firm.production_line[0].last_amv_produced = 0.0;
            firm.production_line[0].last_iterations = 0.0;
            firm.production_line[0].last_success_rate = 0.0;

            let report = firm.run_production(&factuals, &market);

            // Should run (required factor present) but without the optional throughput bonus
            assert!(firm.production_line[0].last_success_rate > 0.9);
            assert_eq!(firm.production_line[0].last_iterations, 20.0);
            assert_eq!(firm.production_line[0].last_missing_goods.len(), 1);
            assert!(firm.production_line[0].last_missing_goods.contains(&100));
            assert_eq!(firm.production_line[0].last_amv_consumed, 220.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 600.0);
            assert_eq!(report.consumed.get(&100), Some(&20.0)); // 10 iterations * 2.0
            assert_eq!(report.consumed.get(&110), Some(&30.0)); // 10 iterations * 2.0
            assert_eq!(report.produced.get(&120), Some(&30.0)); // 10 iterations * 2.0
        }

        #[test]
        fn test_optional_inputs_and_bonuses() {
            let process = Process::new(30, "optional_bonus", 0)
                .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false))
                .with_input(ProcessInput::new(400, 1.0, true, InputType::Destroyed, true) // optional catalyst
                    .with_optional(InputEffect::Output(0.25))) // +25% output
                .with_output(ProcessOutput::new(110, 1.0, false));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(100, make_good(100, "wood", HashMap::new()));
            factuals.goods.insert(400, make_good(400, "ash", HashMap::new()));
            factuals.goods.insert(110, make_good(110, "treated wood", HashMap::new()));

            let mut firm = Firm::new(3, "Catalyst Tester".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(100, empty_firm_row(10.0));
            firm.property.insert(400, empty_firm_row(3.0)); // present → bonus applies

            firm.production_line.push(empty_production_line(30));
            firm.production_line[0].inputs = vec![100, 400];
            firm.production_line[0].target = None;

            let market = make_market_with_amvs(&[(100, 2.0), (110, 7.0), (400, 10.0)]);

            let report = firm.run_production(&factuals, &market);

            // With catalyst bonus we should get more than the base 5 iterations worth of output
            assert_eq!(firm.production_line[0].last_iterations, 10.0);
            assert_eq!(firm.production_line[0].last_amv_consumed, 50.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 75.25);
            assert_eq!(report.consumed[&100], 10.0);
            assert_eq!(report.consumed[&400], 3.0);
            assert_eq!(report.produced[&110], 10.75);
        }

        #[test]
        fn test_decay_results_recorded_in_produced() {
            // Wood (Consumed) decays into sawdust
            let process = Process::new(40, "decay_test", 0)
                .with_input(ProcessInput::new(100, 1.0, true, InputType::Consumed, false))
                .with_output(ProcessOutput::new(110, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            // Add decay info to the good definition (even if goods map is mostly empty)
            let wood = Good {
                id: 100,
                name: "Wood".into(),
                class: None,
                decay_rate: 0.25,
                decay_result: HashMap::from([(130, 0.5)]), // 50% becomes sawdust
                tags: Default::default(),
                categories: vec![],
            };
            factuals.goods.insert(100, wood);
            factuals.goods.insert(130, make_good(110, "nice wood", HashMap::new()));
            factuals.goods.insert(130, make_good(130, "ash", HashMap::new()));

            let mut firm = Firm::new(4, "Decay Workshop".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(100, empty_firm_row(8.0));

            firm.production_line.push(empty_production_line(40));
            firm.production_line[0].inputs = vec![100];
            firm.production_line[0].target = None;

            let market = make_market_with_amvs(&[(100, 2.0), (110, 6.0), (130, 0.5)]);

            let report = firm.run_production(&factuals, &market);

            assert_eq!(report.produced.get(&110), Some(&8.0));  // main output
            assert_eq!(report.produced.get(&130), Some(&4.0));  // decay result (8 iters * 0.5)
            assert_eq!(report.consumed.get(&100), Some(&8.0)); 
            assert_eq!(firm.production_line[0].last_amv_consumed, 16.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 50.0);
            assert_eq!(firm.production_line[0].last_iterations, 8.0);
        }

        #[test]
        fn test_target_with_throughput_bonus_overshoot() {
            // Throughput bonus from optional input should allow more iterations than target
            // (per do_process rules: target is scaled on fixed inputs only)
            let process = Process::new(50, "throughput_target", 0)
                .with_input(ProcessInput::new(100, 1.0, true, InputType::Destroyed, false))
                .with_input(ProcessInput::new(110, 1.0, false, InputType::Destroyed, false))
                .with_input(ProcessInput::new(500, 1.0, true, InputType::Destroyed, true)
                    .with_optional(InputEffect::Throughput(1.0))) // doubles throughput
                .with_output(ProcessOutput::new(120, 1.0, true))
                .with_output(ProcessOutput::new(130, 1.0, false));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(100, make_good(100, "fixed good", HashMap::new()));
            factuals.goods.insert(110, make_good(110, "normal good", HashMap::new()));
            factuals.goods.insert(120, make_good(120, "fixed output", HashMap::new()));
            factuals.goods.insert(130, make_good(130, "normal output", HashMap::new()));
            factuals.goods.insert(500, make_good(500, "bonus good", HashMap::new()));

            let mut firm = Firm::new(5, "Throughput Lab".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(100, empty_firm_row(20.0));
            firm.property.insert(110, empty_firm_row(40.0));
            firm.property.insert(500, empty_firm_row(5.0)); // enough for bonus

            firm.production_line.push(empty_production_line(50));
            firm.production_line[0].inputs = vec![100, 110, 500];
            firm.production_line[0].target = Some(8.0); // would be 8 without bonus, more with it

            let market = make_market_with_amvs(&[(100, 2.0), (110, 3.0), (120, 10.0), (130, 5.0), (500, 1.0)]);

            let report = firm.run_production(&factuals, &market);

            assert_eq!(report.produced.len(), 2);
            assert_eq!(report.consumed.len(), 3);
            assert_eq!(report.produced.get(&120), Some(&8.0));  // main output
            assert_eq!(report.produced.get(&130), Some(&13.0));  // decay result (8 iters * 0.5)
            assert_eq!(report.consumed.get(&100), Some(&8.0)); 
            assert_eq!(report.consumed.get(&110), Some(&13.0)); 
            assert_eq!(report.consumed.get(&500), Some(&5.0)); 
            assert_eq!(firm.production_line[0].last_amv_consumed, 2.0*8.0 + 3.0*13.0 + 5.0*1.0);
            assert_eq!(firm.production_line[0].last_amv_produced, 8.0*10.0 + 13.0*5.0);
            assert_eq!(firm.production_line[0].last_iterations, 8.0);
            assert_eq!(firm.property[&100].quantity, 12.0);
            assert_eq!(firm.property[&110].quantity, 27.0);
            assert_eq!(firm.property[&120].quantity, 8.0);
            assert_eq!(firm.property[&130].quantity, 13.0);
            assert_eq!(firm.property[&500].quantity, 0.0);
        }

        #[test]
        fn test_amv_fallback_uses_one_point_zero() {
            // Good 999 is deliberately missing from the Market
            let process = Process::new(60, "missing_good_amv", 0)
                .with_input(ProcessInput::new(999, 1.0, true, InputType::Consumed, false))
                .with_output(ProcessOutput::new(110, 1.0, true));

            let mut factuals = make_factuals_with_process(process);
            factuals.goods.insert(999, make_good(999, "missing market good", HashMap::new()));
            factuals.goods.insert(110, make_good(110, "output good", HashMap::new()));

            let mut firm = Firm::new(6, "Mystery Good Firm".into(), 42, hexx::Hex::new(0, 0));
            firm.property.insert(999, empty_firm_row(5.0));

            firm.production_line.push(empty_production_line(60));
            firm.production_line[0].inputs = vec![999];
            firm.production_line[0].target = None;

            // Market does NOT contain good 999
            let market = make_market_with_amvs(&[(110, 4.0)]);

            let _report = firm.run_production(&factuals, &market);

            // Should fall back to the economic default of 1.0
            assert_eq!(
                firm.production_line[0].last_amv_consumed, 5.0,
                "Missing goods should default to AMV 1.0"
            );
        }
    }
}
