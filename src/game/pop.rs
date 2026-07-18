use std::collections::HashMap;

use bevy::{platform::collections::HashSet, reflect::DynamicArray, utils::default};

use crate::game::{actor::Actor, desire::{Desire, DesireSource, DesireTargetType}, factuals::Factuals, household::HouseholdDef, market::{Market, MarketHistory}, marketorder::MarketOrder, scalingfactor::ScalingFactor};

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

    /// The amount of growth (or decline if negative) in the population since yesterday.
    /// Used for various success tracking and scaling of things between days.
    pub previous_growth: f64,
}

impl Pop {
    /// # Apply Scaling Factor
    /// 
    /// Resolves a `ScalingFactor` against this pop's demographics, returning the
    /// effective multiplier (scalar weight times households, adults, labor, etc.).
    pub fn get_scaling_factor(&self, scaling: ScalingFactor) -> f64 {
        match scaling {
            ScalingFactor::Fixed(f) => f,
            ScalingFactor::All(f) => f * self.demographics.total_population(),
            ScalingFactor::Household(f) => f * self.demographics.count,
            ScalingFactor::Adults(f) => f * self.demographics.adult_pop(),
            ScalingFactor::Children(f) => f * self.demographics.children_pop(),
            ScalingFactor::Elders(f) => f * self.demographics.elder_pop(),
            ScalingFactor::Labor(f) => f * self.demographics.labor(),
        }
    }

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
            let amount = self.get_scaling_factor(*scaling);
            self.property.entry(*good_id)
                .and_modify(|x| x.quantity += amount)
                .or_insert(PopPRow::new(amount));
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
    /// 
    /// When creating oredrs for Luxury needs, it will only do one pass, even if the 
    /// budget has excess at the end.
    pub fn create_orders(&self, market_history: &MarketHistory, factuals: &Factuals) -> Vec<MarketOrder> {
        let mut orders: Vec<MarketOrder> = Vec::new();

        let mut remaining_budget = self.current_excess_value(market_history);
        let mut seen = HashSet::new();

        // go through desires, creating orders only for those goods which have targets
        for tier in self.desires.iter() {
            for desire in tier.iter() {
                for &target in desire.ordered_targets().iter() {
                    if remaining_budget <= 0.0 {
                        // if we've overdrawn by this point, break out early.
                        break;
                    }
                    if !self.property.contains_key(&target.good) ||
                    self.property.get(&target.good).unwrap().shop_target == 0.0 {
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
                    let purchase_target = self.property.get(&target.good).unwrap().shop_target 
                        - self.property.get(&target.good).unwrap().quantity;
                    debug_assert!(purchase_target >= 0.0, "Purchase target should not be negative.");
                    let cost = purchase_target * good_price;

                    // create order for full amount
                    orders.push(MarketOrder::request_order(
                        Actor::Pop(self.id), target.good, purchase_target));
                    remaining_budget -= cost;
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
                        if remaining_budget <= 0.0 {
                            // if we've overdrawn by this point, break out early.
                            break;
                        }
                        if self.property.contains_key(&target.good) &&
                        self.property.get(&target.good).unwrap().shop_target > 0.0 {
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
            let surplus = (prop.quantity - prop.shop_target).max(0.0);
            if surplus > 0.001 {
                excess += surplus * market_history.prices.get(good).unwrap_or(&0.0);
            }
        }
        excess
    }

    /// # Demographic Update
    /// 
    /// Demographic Update is called very early on in the day, after 
    pub fn demographic_update(&mut self, factuals: &Factuals) {
        todo!()
    }

    /// # Update Desires
    /// 
    /// Called near the start of each day, after yesterday's growth/decline, and this
    /// morning's demographic changes created by players. This updates the desire's 
    /// `amount`, `targets`, as well as add/remove desires from the pop, and updates
    /// the PopPRow's `shopping_target` and `desire_needs`, scaling with the pop's current size,
    /// changes in demographic effects, and so on.
    /// 
    /// No prior population snapshot is required: the source `DemoDesire` provides the
    /// base amount, which is multiplied by the current pop scaling factor.
    /// 
    /// ## Note
    /// 
    /// Currently assumes a single demographic row. Source demo desires are resolved
    /// via `Factuals::source_demo_desire`.
    /// 
    /// Flow:
    /// 1. Update existing desires (amount, satisfaction, targets, demo priority) or drop
    ///    ones whose demo no longer exists.
    /// 2. Add any new demo desires from the pop's species/culture/religion that are not
    ///    already present (scaled via `DemoDesire::create_desire`).
    /// 3. Scale property `shop_target` / `desire_needs` for population growth.
    /// 4. Per tier: sort with `Desire::cmp_order`, then bake `priority` to index.
    pub fn update_desires(&mut self, factuals: &Factuals) {
        // Sources already on the pop (after removals), used to skip re-adding.
        let mut existing_desires = HashSet::new();

        // --- 1. Update / remove existing pop desires ---
        for tier_idx in 0..self.desires.len() {
            let mut desire_idx = 0;
            while desire_idx < self.desires[tier_idx].len() {
                if let Some(demo) = factuals.source_demo_desire(&self.desires[tier_idx][desire_idx]) {
                    let new_amount = demo.amount * self.get_scaling_factor(demo.scalar);
                    let priority = demo.priority;
                    let targets = demo.bucket.clone();

                    let desire = &mut self.desires[tier_idx][desire_idx];
                    // Place using the parent demo's priority for this update's sort.
                    desire.priority = priority;
                    // Scale satisfaction with the amount change.
                    desire.satisfaction *= new_amount / desire.amount;
                    // update amount.
                    desire.amount = new_amount;
                    // Override targets from the demo definition.
                    desire.target = targets; // TODO: cheaper sync if needed later.
                    existing_desires.insert(desire.source);
                    desire_idx += 1;
                } else {
                    // Demo desire removed from its demographic — drop from the pop.
                    self.desires[tier_idx].remove(desire_idx);
                }
            }
        }

        // --- 2. Add new desires present on demographics but not yet on the pop ---
        self.add_missing_demographic_desires(factuals, &existing_desires);

        // --- 3. Scale shopping / need targets with population growth ---
        let growth_f = self.demographics.count + self.previous_growth / self.demographics.count;
        for (_, prop) in self.property.iter_mut() {
            if prop.shop_target > 0.0 {
                prop.shop_target *= growth_f;
            }
            if prop.desire_needs > 0.0 {
                prop.desire_needs *= growth_f;
            }
        }

        // --- 4. Sort each tier and bake priority to index ---
        for tier in self.desires.iter_mut() {
            tier.sort_by(Desire::cmp_order);
            for (i, desire) in tier.iter_mut().enumerate() {
                desire.priority = i as isize;
            }
        }
    }

    /// Creates scaled pop desires for any species/culture/religion demo desires not
    /// already present in `existing` (keyed by full `DesireSource`).
    /// 
    /// Culture / religion id `0` means none and is skipped. Class is not supported yet.
    fn add_missing_demographic_desires(
        &mut self,
        factuals: &Factuals,
        existing: &HashSet<DesireSource>,
    ) {
        // Species (0 is the default human id — still valid).
        if let Some(species) = factuals.species.get(&self.demographics.species) {
            for demo in species.desires.iter().flat_map(|tier| tier.values()) {
                let source = DesireSource::Species(species.id, demo.id);
                if !existing.contains(&source) {
                    let tier = demo.tier;
                    let desire = demo.create_desire(self, source);
                    debug_assert!(tier < self.desires.len(), "Desire tier out of range.");
                    self.desires[tier].push(desire);
                }
            }
        }

        // Culture (0 = none).
        if self.demographics.culture != 0 {
            if let Some(culture) = factuals.cultures.get(&self.demographics.culture) {
                for demo in culture.desires.iter().flat_map(|tier| tier.values()) {
                    let source = DesireSource::Culture(culture.id, demo.id);
                    if !existing.contains(&source) {
                        let tier = demo.tier;
                        let desire = demo.create_desire(self, source);
                        debug_assert!(tier < self.desires.len(), "Desire tier out of range.");
                        self.desires[tier].push(desire);
                    }
                }
            }
        }

        // Religion (0 = none).
        if self.demographics.religion != 0 {
            if let Some(religion) = factuals.religion.get(&self.demographics.religion) {
                for demo in religion.desires.iter().flat_map(|tier| tier.values()) {
                    let source = DesireSource::Religion(religion.id, demo.id);
                    if !existing.contains(&source) {
                        let tier = demo.tier;
                        let desire = demo.create_desire(self, source);
                        debug_assert!(tier < self.desires.len(), "Desire tier out of range.");
                        self.desires[tier].push(desire);
                    }
                }
            }
        }
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
        // first do basic desires, only one pass needed.
        let mut working_desires = self.desires.remove(0); // pop off front
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
            if working_desires.is_empty() {
                break;
            } else { iter_target += 1.0; } // otherwise increment target and go again.
        } 
        // Restoring original tier order: priority is the effective index from update_desires.
        ordered_desires.sort_by_key(|d| d.priority);
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
    /// The total amount owned at the moment. Not necessarily how much is available.
    pub quantity: f64,

    /// The Desire Needs is a helper which helps define how much of the good a pop
    /// needs or is targeting to satisfy it's needs. This should be equal to the amount
    /// of goods needed to 
    /// 
    /// This should be equivalent to Reserved after shopping, and used + consumed after
    /// consumption.
    /// 
    /// This get's updated after population changes to match growth/shrink, in 
    /// population, and is used as a touchstone for other targets and values.
    /// 
    /// This does not alter mood based on differences.
    /// 
    /// Note: This may be removed at a later date if a better method to track this value
    /// is found.
    pub desire_needs: f64,
    
    /// The Shopping Target amount the pop desires to have after all shopping is 
    /// complete. Simplifies purchases into bulk purchases.
    /// 
    /// Should target roughly 1.0-1.2x of desire needs at minimum. When a pop has elevated
    /// savings, fear, or similar 'hoarding' moods activated, this target is pushed up.
    /// 
    /// Before Decay, Target should be roughly equal to reserved + saved.
    /// 
    /// Goods which cannot be bought or sold should always have a target of 0.0.
    pub shop_target: f64,
    
    /// The amount that we wish to preserve between days. This is where 'hoarding' is 
    /// recorded and tracked over time for specific goods.
    /// 
    /// This is not reset each day, instead just updating to match population changes.
    /// Savings always comes after consumption.
    /// 
    /// Missing a savings target reduces a pop's mood.
    pub saved: f64,

    /// The amount that has already been earmarked for the pop's use today. Used 
    /// to 'prepare' for consumption. Does not remove from quantity.
    /// 
    /// This is reset at the start of the day.
    pub reserved: f64,

    /// How many of this good was consumed for today's desires.
    /// All goods here are decayed at the end of the day at full percent.
    pub consumed: f64,

    /// Goods that were used for use desires, but not consumed, are stored here
    /// until the end of the day. At day's end, goods here decay normally, and are
    /// returned to total quantity.
    pub used: f64,

    // TODO: consider adding history recording of consumed and used goods, as well as
    // success records of reaching the shop_target and saved target (0.0-1.0).
    // These would help with budgeting and forward looking planning for a pop, but may be too heavy.
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
        self.shop_target = target;
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

    /// Fluent Desire Needs Setter
    pub fn with_desire_need(mut self, desire_needs: f64) -> Self {
        self.desire_needs = desire_needs;
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
        self.quantity - self.shop_target
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
            previous_growth: 0.0,
        }
    }

    fn make_desire(demo_desire_id: usize, desire_target: DesireTarget, amount: f64) -> Desire {
        // Source doesn't matter for most uses, it's just for tracking purpopses.
        // Priority mirrors demo_desire_id so within-tier order matches insertion when
        // consume re-sorts luxury desires by priority (as update_desires would bake).
        Desire {
            source: DesireSource::Religion(0, demo_desire_id),
            priority: demo_desire_id as isize,
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
        factuals.goods.insert(500, make_good(500, "Test Good 6".to_string()));
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
        use crate::game::{factuals::Factuals, good::GoodTag, market::MarketHistory};

        use super::*;

        #[test]
        fn respect_one_time_overdraw_and_stop() {
            let pop = make_pop();
            let pop = add_pop_desires(pop);
            let mut pop = add_pop_targets(pop);

            // 5 AM of extra goods, should stop after first good.
            pop.property.insert(500, PopPRow::new(5.0)); 

            let factuals = make_default_factuals();
            let market_history = make_default_market_history();

            let orders = pop.create_orders(&market_history, &factuals);
            assert_eq!(orders.len(), 1); 
            assert_eq!(orders[0].target, 100); // should be the first good in the list
            assert_eq!(orders[0].target_amount, 10.0); // should be the first good in the list
        }

        #[test]
        fn respect_property_filter_on_first_desires_pass() {
            let pop = make_pop();
            let pop = add_pop_desires(pop);
            let mut pop = add_pop_targets(pop);
            // Remove target from first desire 100, our first desire.
            pop.property.get_mut(&100).unwrap().shop_target = 0.0;

            // 5 AM of extra goods, should stop after first good.
            pop.property.insert(500, PopPRow::new(5.0)); 

            let factuals = make_default_factuals();
            let market_history = make_default_market_history();

            let orders = pop.create_orders(&market_history, &factuals);
            assert_eq!(orders.len(), 1); 
            assert_eq!(orders[0].target, 101); // should be the first good in the list
            assert_eq!(orders[0].target_amount, 10.0); // should be the first good in the list
        }

        #[test]
        fn respect_seen_in_property_first_pass() {
            let pop = make_pop();
            let pop = add_pop_desires(pop);
            let mut pop = add_pop_targets(pop);
            // Add additional target for 100 that double dips.
            pop.desires[0].insert(1, make_desire(1, 
                DesireTarget { good: 100, desire_type: DesireTargetType::Consume, 
                    efficiency: 1.0, cap: 15.0, high_priority: false }, 
                10.0));
            // Keep source id + priority unique after insert shifted this desire.
            pop.desires[0][2].source = pop.desires[0][2].source.with_demo_desire_id(2);
            pop.desires[0][2].priority = 2;

            // 15 AM of extra goods, should stop after first good.
            pop.property.insert(500, PopPRow::new(15.0)); 

            let factuals = make_default_factuals();
            let market_history = make_default_market_history();

            let orders = pop.create_orders(&market_history, &factuals);
            assert_eq!(orders.len(), 2); 
            assert_eq!(orders[0].target, 100); // should be the first good in the list
            assert_eq!(orders[0].target_amount, 10.0); // should be the first good in the list
            assert_eq!(orders[1].target, 101); // should be the first good in the list
            assert_eq!(orders[1].target_amount, 10.0); // should be the first good in the list
        }

        #[test]
        fn add_during_second_pass() {
            let pop = make_pop();
            let pop = add_pop_desires(pop);
            let mut pop = add_pop_targets(pop);
            // Remove targets from all desires
            pop.property.get_mut(&100).unwrap().shop_target = 0.0;
            pop.property.get_mut(&101).unwrap().shop_target = 0.0;
            pop.property.get_mut(&200).unwrap().shop_target = 0.0;
            pop.property.get_mut(&201).unwrap().shop_target = 0.0;
            pop.property.get_mut(&300).unwrap().shop_target = 0.0;

            // 15 AM of extra goods, should stop after first good.
            pop.property.insert(500, PopPRow::new(45.0)); 

            let factuals = make_default_factuals();
            let market_history = make_default_market_history();

            let orders = pop.create_orders(&market_history, &factuals);
            assert_eq!(orders.len(), 5);
            assert_eq!(orders[0].target, 100); // should be the first good in the list
            assert_eq!(orders[0].target_amount, 10.0); // should be the first good in the list
            assert_eq!(orders[1].target, 101); // should be the first good in the list
            assert_eq!(orders[1].target_amount, 10.0); // should be the first good in the list
            assert_eq!(orders[2].target, 200); // should be the first good in the list
            assert_eq!(orders[2].target_amount, 10.0); // should be the first good in the list
            assert_eq!(orders[3].target, 201); // should be the first good in the list
            assert_eq!(orders[3].target_amount, 10.0); // should be the first good in the list
            assert_eq!(orders[4].target, 300); // should be the first good in the list
            assert_eq!(orders[4].target_amount, 10.0); // should be the first good in the list
        }

        #[test]
        fn add_during_second_pass_with_budget() {
            let pop = make_pop();
            let pop = add_pop_desires(pop);
            let mut pop = add_pop_targets(pop);
            // Remove targets from all desires
            pop.property.get_mut(&100).unwrap().shop_target = 0.0;
            pop.property.get_mut(&101).unwrap().shop_target = 0.0;
            pop.property.get_mut(&200).unwrap().shop_target = 0.0;
            pop.property.get_mut(&201).unwrap().shop_target = 0.0;
            pop.property.get_mut(&300).unwrap().shop_target = 0.0;

            // 15 AM of extra goods, should stop after first good.
            pop.property.insert(500, PopPRow::new(15.0)); 

            let factuals = make_default_factuals();
            let market_history = make_default_market_history();

            let orders = pop.create_orders(&market_history, &factuals);
            assert_eq!(orders.len(), 2);
            assert_eq!(orders[0].target, 100); // should be the first good in the list
            assert_eq!(orders[0].target_amount, 10.0); // should be the first good in the list
            assert_eq!(orders[1].target, 101); // should be the first good in the list
            assert_eq!(orders[1].target_amount, 10.0); // should be the first good in the list
        }

        #[test]
        fn skip_untradeable_goods_on_second_pass() {
            let pop = make_pop();
            let pop = add_pop_desires(pop);
            let mut pop = add_pop_targets(pop);
            // Remove targets from all desires
            pop.property.get_mut(&100).unwrap().shop_target = 0.0;
            pop.property.get_mut(&101).unwrap().shop_target = 0.0;
            pop.property.get_mut(&200).unwrap().shop_target = 0.0;
            pop.property.get_mut(&201).unwrap().shop_target = 0.0;
            pop.property.get_mut(&300).unwrap().shop_target = 0.0;

            // 15 AM of extra goods, should stop after first good.
            pop.property.insert(500, PopPRow::new(15.0)); 

            let mut factuals = make_default_factuals();
            factuals.goods.get_mut(&100).unwrap().tags.insert(GoodTag::Untradeable);
            let market_history = make_default_market_history();

            let orders = pop.create_orders(&market_history, &factuals);
            assert_eq!(orders.len(), 2);
            assert_eq!(orders[0].target, 101); // should be the first good in the list
            assert_eq!(orders[0].target_amount, 10.0); // should be the first good in the list
            assert_eq!(orders[1].target, 200); // should be the first good in the list
            assert_eq!(orders[1].target_amount, 10.0); // should be the first good in the list
        }

        #[test]
        fn deal_with_leftover_budget_mixed_passes() {
            let pop = make_pop();
            let pop = add_pop_desires(pop);
            let mut pop = add_pop_targets(pop);
            // Remove targets from all desires
            pop.property.get_mut(&100).unwrap().shop_target = 10.0;
            pop.property.get_mut(&101).unwrap().shop_target = 0.0;
            pop.property.get_mut(&200).unwrap().shop_target = 0.0;
            pop.property.get_mut(&201).unwrap().shop_target = 0.0;
            pop.property.get_mut(&300).unwrap().shop_target = 0.0;

            // 15 AM of extra goods, should stop after first good.
            pop.property.insert(500, PopPRow::new(25.0)); 

            let mut factuals = make_default_factuals();
            factuals.goods.get_mut(&100).unwrap().tags.insert(GoodTag::Untradeable);
            let market_history = make_default_market_history();

            let orders = pop.create_orders(&market_history, &factuals);
            assert_eq!(orders.len(), 3);
            assert_eq!(orders[0].target, 100); // should be the first good in the list
            assert_eq!(orders[0].target_amount, 10.0); // should be the first good in the list
            assert_eq!(orders[1].target, 101); // should be the first good in the list
            assert_eq!(orders[1].target_amount, 10.0); // should be the first good in the list
            assert_eq!(orders[2].target, 200); // should be the first good in the list
            assert_eq!(orders[2].target_amount, 10.0); // should be the first good in the list
        }
    }

    mod update_desires_should {
        use super::*;
        use crate::game::{
            culture::Culture,
            desire::DemoDesire,
            species::Species,
        };

        fn household_demo(id: usize, amount: f64, priority: isize, tier: usize) -> DemoDesire {
            DemoDesire::new(id)
                .with_amount(amount)
                .with_priority(priority)
                .with_tier(tier)
                .with_scalar(ScalingFactor::Household(1.0))
        }

        #[test]
        fn rescales_amount_and_satisfaction_when_households_grow() {
            // Demo base 2.0 per household; pop starts at 10 households.
            let demo = household_demo(10, 2.0, 0, 0);
            let culture = Culture::new(1, "Test").with_desire(demo.clone());
            let factuals = Factuals::new().with_culture(culture);

            let mut pop = make_pop(); // count = 10
            let mut desire = demo.create_desire(&pop, DesireSource::Culture(1, 0));
            // create_desire: amount = 2.0 * 10 = 20; half satisfied.
            desire.satisfaction = 10.0;
            pop.desires[0].push(desire);

            pop.demographics.count = 20.0; // double households
            pop.update_desires(&factuals);

            // new amount = 2.0 * 20 = 40; satisfaction scales 10 * (40/20) = 20
            assert_eq!(pop.desires[0].len(), 1);
            assert_eq!(pop.desires[0][0].amount, 40.0);
            assert_eq!(pop.desires[0][0].satisfaction, 20.0);
            // sole desire; baked priority is its tier index
            assert_eq!(pop.desires[0][0].priority, 0);
            assert_eq!(*pop.desires[0][0].source.demo_desire_id(), 10);
        }

        #[test]
        fn sorts_by_demo_priority_then_bakes_tier_index() {
            // Insert high-priority-value demo after low; sort should put low first.
            let high = household_demo(1, 1.0, 50, 0);
            let low = household_demo(2, 1.0, 1, 0);
            let culture = Culture::new(1, "Test")
                .with_desire(high.clone())
                .with_desire(low.clone());
            let factuals = Factuals::new().with_culture(culture);

            let mut pop = make_pop();
            // Push in reverse of expected final order, and scramble baked priorities.
            let mut d_high = high.create_desire(&pop, DesireSource::Culture(1, 0));
            let mut d_low = low.create_desire(&pop, DesireSource::Culture(1, 0));
            d_high.priority = 99;
            d_low.priority = 99;
            pop.desires[0].push(d_high);
            pop.desires[0].push(d_low);

            pop.update_desires(&factuals);

            assert_eq!(pop.desires[0].len(), 2);
            // low demo priority (1) before high (50)
            assert_eq!(*pop.desires[0][0].source.demo_desire_id(), 2);
            assert_eq!(*pop.desires[0][1].source.demo_desire_id(), 1);
            // priorities rewritten to indices
            assert_eq!(pop.desires[0][0].priority, 0);
            assert_eq!(pop.desires[0][1].priority, 1);
            // amounts still scaled to pop (1.0 * 10 households)
            assert_eq!(pop.desires[0][0].amount, 10.0);
            assert_eq!(pop.desires[0][1].amount, 10.0);
        }

        #[test]
        fn ties_on_demo_priority_break_by_source_kind() {
            // Same demo priority; Species should sort before Culture.
            let species_demo = household_demo(5, 1.0, 0, 0);
            let culture_demo = household_demo(7, 1.0, 0, 0);
            let factuals = Factuals::new()
                .with_species(Species::new(0, "Human").with_desire(species_demo.clone()))
                .with_culture(Culture::new(1, "Test").with_desire(culture_demo.clone()));

            let mut pop = make_pop();
            pop.demographics.culture = 1;
            // Insert culture first so only sort/tie-break can put species ahead.
            let culture_desire = culture_demo.create_desire(&pop, DesireSource::Culture(1, 0));
            let species_desire = species_demo.create_desire(&pop, DesireSource::Species(0, 0));
            pop.desires[0].push(culture_desire);
            pop.desires[0].push(species_desire);

            pop.update_desires(&factuals);

            assert_eq!(pop.desires[0].len(), 2);
            assert!(matches!(pop.desires[0][0].source, DesireSource::Species(_, _)));
            assert!(matches!(pop.desires[0][1].source, DesireSource::Culture(_, _)));
            assert_eq!(*pop.desires[0][0].source.demo_desire_id(), 5);
            assert_eq!(*pop.desires[0][1].source.demo_desire_id(), 7);
            assert_eq!(pop.desires[0][0].priority, 0);
            assert_eq!(pop.desires[0][1].priority, 1);
        }

        #[test]
        fn adds_new_demographic_desires_without_duplicating_existing() {
            let existing_demo = household_demo(1, 1.0, 0, 0);
            let new_demo = household_demo(2, 3.0, 5, 1); // common tier, base 3.0
            let culture = Culture::new(1, "Test")
                .with_desire(existing_demo.clone())
                .with_desire(new_demo.clone());
            let factuals = Factuals::new().with_culture(culture);

            let mut pop = make_pop(); // 10 households
            pop.demographics.culture = 1;
            // Only the first culture desire is on the pop already.
            let existing = existing_demo.create_desire(&pop, DesireSource::Culture(1, 0));
            pop.desires[0].push(existing);

            pop.update_desires(&factuals);

            // Existing basic desire still present; new common desire added once.
            assert_eq!(pop.desires[0].len(), 1);
            assert_eq!(*pop.desires[0][0].source.demo_desire_id(), 1);
            assert_eq!(pop.desires[0][0].amount, 10.0); // 1.0 * 10 households

            assert_eq!(pop.desires[1].len(), 1);
            assert_eq!(*pop.desires[1][0].source.demo_desire_id(), 2);
            assert_eq!(pop.desires[1][0].amount, 30.0); // 3.0 * 10 households
            assert_eq!(pop.desires[1][0].satisfaction, 0.0);
            assert_eq!(pop.desires[1][0].priority, 0); // baked sole index in tier

            // Second pass must not duplicate.
            pop.update_desires(&factuals);
            assert_eq!(pop.desires[0].len(), 1);
            assert_eq!(pop.desires[1].len(), 1);
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
