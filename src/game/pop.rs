use std::collections::HashMap;

use bevy::{platform::collections::HashSet, reflect::DynamicArray, utils::default};

use crate::game::{actor::Actor, desire::{Desire, DesireTargetType}, factuals::Factuals, household::HouseholdDef, market::{Market, MarketHistory}, marketorder::MarketOrder, scalingfactor::ScalingFactor};

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

    /// The current orders of the pop, should be empty between turns.
    /// 
    /// Used for keeping track of what we want to buy/sell, and adjusting the buudget
    /// as it continues on.
    pub current_orders: Vec<MarketOrder>,

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

    /// # Initial Satisfaction
    /// 
    /// Called early in the day, it sets aside goods for the pops desires pre-emptively, 
    /// reserving and ensuring desires can be readily satisfied.
    /// 
    /// Used to prepare for the day and pre-empt
    pub fn initial_satisfaction(&mut self) {

    }

    /// # Create Orders
    /// 
    /// Creates request and offer orders for a pop. 
    /// 
    /// For now, this only puts out it's desires, not its, possible offers.
    /// 
    /// When Offer Orders can be put out, it should only be when the pop is 
    /// despirate enough to offer goods in exchange for other things.
    pub fn create_orders(&self, market_history: &MarketHistory, factuals: &Factuals) -> Vec<MarketOrder> {
        let mut orders: Vec<MarketOrder> = Vec::new();

        let mut remaining_budget = self.current_excess_value(market_history);
        let mut seen = HashSet::new();

        // go through desires, creating orders only for those goods which have targets
        for tier in self.desires.iter() {
            for desire in tier.iter() {
                for &target in desire.ordered_targets().iter() {
                    if !self.property.contains_key(&target.good) ||
                    self.property.get(&target.good).unwrap().target == 0.0 {
                        // skip if we don't have a record of the good
                        // or if the good has no target.
                        continue;
                    }
                    if  seen.contains(&target.good) {
                        continue;
                    }
                    seen.insert(target.good);

                    let good_price = market_history.prices.get(&target.good)
                        .unwrap_or(&1.0);
                    let purchase_target = self.property.get(&target.good).unwrap().target 
                        - self.property.get(&target.good).unwrap().quantity;
                    let cost = purchase_target * good_price;

                    // create order for full amount
                    orders.push(MarketOrder::request_order(
                        Actor::Pop(self.id), target.good, purchase_target));
                    remaining_budget -= cost;
                    if remaining_budget <= 0.0 {
                        // if we've overdrawn by this point, break out early.
                        break;
                    }
                }
            }
        }

        // if we have no budget left, return our current orders.
        if remaining_budget <= 0.0 {
            return orders;
        }

        // if we still have budget, repeat, adding all desires to possibly reach our goals until we do
        if remaining_budget > 0.0 {
            for tier in self.desires.iter() {
                for desire in tier.iter() {
                    for &target in desire.ordered_targets().iter() {
                        if self.property.contains_key(&target.good) &&
                        self.property.get(&target.good).unwrap().target > 0.0 {
                            // If we have a record of it, nad that record has a target, 
                            // we've already added it, so skip.
                            continue;
                        }
                        if !factuals.goods[&target.good].is_buyable() {
                            // if the good is not buyable, skip it.
                            continue;
                        }
                        if  seen.contains(&target.good) {
                            continue;
                        }
                        seen.insert(target.good);

                        let good_price = market_history.prices.get(&target.good).unwrap_or(&0.0);
                        let purchase_target = desire.amount * target.cap / target.efficiency;
                        let cost = purchase_target * good_price;

                        // create order for full amount
                        orders.push(MarketOrder::request_order(
                            Actor::Pop(self.id), target.good, purchase_target));
                        remaining_budget -= cost;

                        if remaining_budget <= 0.0 {
                            // if we've overdrawn by this point, break out early.
                            break;
                        }
                    }
                }
            }
        }

        // There is no third, but if there was, we'd just loop the last tier until we did run out of budget.

        orders
    }

    /// # Current Excess AMV
    /// 
    /// Returns the total AMV value of goods this pop holds above their individual targets.
    /// This is the "excess" they can offer in trade to fund purchases.
    pub fn current_excess_value(&self, market_history: &MarketHistory) -> f64 {
        let mut excess: f64 = 0.0;
        for (good, prop) in &self.property {
            let surplus = (prop.quantity - prop.target).max(0.0);
            if surplus > 0.001 {
                excess += surplus * market_history.prices.get(good).unwrap_or(&0.0);
            }
        }
        excess
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
    /// savings, fear, or similar 'hoarding' moods activated, this target is pushed up.
    /// 
    /// Before Decay, Target should be roughly equal to reserved + saved.
    /// 
    /// Goods which cannot be bought or sold should always have a target of 0.0.
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
    use std::collections::{HashMap, HashSet};

    use bevy::ecs::name;

use crate::game::{desire::{
        Desire, DesireSource, DesireTarget, DesireTargetType
    }, factuals::Factuals, good::Good, household::HouseholdDef, market::MarketHistory, pop::{DemoRow, Pop, PopPRow}, scalingfactor::ScalingFactor};

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
            current_orders: vec![],
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
            decay: 0.0,
        }
    }

    fn add_desire(mut pop: Pop, desire: Desire, tier: usize) -> Pop {
        pop.desires[tier].push(desire);
        pop
    }

    fn add_pop_desires(mut pop: Pop) -> Pop {
        // Add a desire for a good with no property entry
        let desire0 = make_desire(0, DesireTarget::new(100, DesireTargetType::Consume, 1.0), 10.0);
        let desire1 = make_desire(1, DesireTarget::new(101, DesireTargetType::Consume, 1.0), 10.0);
        let desire2 = make_desire(0, DesireTarget::new(200, DesireTargetType::Consume, 1.0), 10.0);
        let desire3 = make_desire(1, DesireTarget::new(201, DesireTargetType::Consume, 1.0), 10.0);
        let desire4 = make_desire(0, DesireTarget::new(300, DesireTargetType::Consume, 1.0), 10.0);
        pop.desires[0].push(desire0); // Basic tier
        pop.desires[0].push(desire1); // Basic tier
        pop.desires[1].push(desire2); // Common tier
        pop.desires[1].push(desire3); // Common tier
        pop.desires[2].push(desire4); // Luxury tier
        pop
    }

    fn add_pop_targets(mut pop: Pop) -> Pop {
        // Add a desire for a good with no property entry
        pop.property.insert(100, PopPRow::new(0.0).with_target(10.0));
        pop.property.insert(101, PopPRow::new(0.0).with_target(10.0));
        pop.property.insert(200, PopPRow::new(0.0).with_target(10.0));
        pop.property.insert(201, PopPRow::new(0.0).with_target(10.0));
        pop.property.insert(300, PopPRow::new(0.0).with_target(40.0));
        pop
    }

    fn make_good(id: usize, name: String) -> Good {
        Good {
            id,
            name,
            class: None,
            decay_rate: 1.0,
            decay_result: HashMap::new(),
            tags: HashSet::new(),
            categories: vec![],
        }
    }

    fn make_default_factuals() -> Factuals {
        let mut factuals = Factuals::new();
        factuals.goods.insert(100, make_good(100, "Test Good".to_string()));
        factuals.goods.insert(101, make_good(101, "Test Good 2".to_string()));
        factuals.goods.insert(200, make_good(200, "Test Good 3".to_string()));
        factuals.goods.insert(201, make_good(201, "Test Good 4".to_string()));
        factuals.goods.insert(300, make_good(300, "Test Good 5".to_string()));
        factuals
    }

    fn make_default_market_history() -> MarketHistory {
        let mut market_history = MarketHistory::new();
        market_history.prices.insert(100, 1.0);
        market_history.prices.insert(101, 1.0);
        market_history.prices.insert(200, 1.0);
        market_history.prices.insert(201, 1.0);
        market_history.prices.insert(300, 1.0);
        market_history.prices.insert(500, 1.0);
        market_history
    }

    mod create_orders_should {
        use crate::game::{factuals::Factuals, market::MarketHistory};

        use super::*;

        #[test]
        fn respect_one_time_overdraw_and_stop() {
            let pop = make_pop();
            let pop = add_pop_desires(pop);
            let mut pop = add_pop_targets(pop);

            pop.property.insert(500, PopPRow::new(5.0)); // 5 AMV, should stop after first good.

            let factuals = make_default_factuals();
            let market_history = make_default_market_history();

            let orders = pop.create_orders(&market_history, &factuals);
            assert_eq!(orders.len(), 1); 
            assert_eq!(orders[0].target, 100); // should be the first good in the list
            assert_eq!(orders[0].target_amount, 10.0); // should be the first good in the list
        }

        #[test]
        fn respect_property_filter_on_first_desires_pass() {
            let mut pop = make_pop();
            // Add a desire for a good with no property entry
            let desire = make_desire(0, DesireTarget::new(999, DesireTargetType::Consume, 1.0), 10.0);
            pop.desires[1].push(desire); // Common tier

            let market_history = MarketHistory::new();
            let factuals = Factuals::new(); // assume goods registered

            let orders = pop.create_orders(&market_history, &factuals);
            assert!(orders.is_empty()); // should skip because no property target
        }

        #[test]
        fn create_order_for_property_targeted_good() {
            let mut pop = make_pop();
            pop.property.insert(100, PopPRow::new(5.0).with_target(15.0));

            let desire = make_desire(0, DesireTarget::new(100, DesireTargetType::Consume, 1.0), 10.0);
            pop.desires[0].push(desire);

            // Mock market with price
            let mut market_history = MarketHistory::new();
            market_history.prices.insert(100, 2.0);

            let factuals = Factuals::new();

            let orders = pop.create_orders(&market_history, &factuals);
            assert_eq!(orders.len(), 1);
            assert_eq!(orders[0].target, 100);
            assert_eq!(orders[0].target_amount, 10.0);
            // amount should be ~10 (shortfall)
        }
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
