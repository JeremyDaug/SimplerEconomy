use std::{collections::HashMap};

use bevy::platform::collections::HashSet;

use crate::game::{
    actor::Actor, config::pop_constants, desire::{Desire, DesireEffect, DesireSource, DesireTarget, DesireTargetType}, factuals::Factuals, good::GoodTag, household::DemographicRates, market::{Market, MarketHistory}, marketorder::MarketOrder, scalingfactor::ScalingFactor, sentiment::{Sentiment, SentimentKind, SentimentMod}, util::lerp,
};

pub use crate::game::effects::PopEffect;
pub use crate::game::pop_property::{
    DemoRow, PopPRow, PopRecords,
};

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
    /// 
    /// Desires should never change tier. If they do, it's a new desire.
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
    /// 
    /// As this is one row, Demographic groups should never change after creation.
    /// Assimilation/migration handles changing between groups.
    pub demographics: DemoRow,

    /// The amount of growth (or decline if negative) in the population since yesterday.
    /// Used for various success tracking and scaling of things between days.
    /// 
    /// Should never be larger than or equal to `self.demographics.count()`.
    /// (Negative pops aren't real, they can't hurt you.)
    pub previous_growth: f64,

    /// Same-day deferred effects (environment, events, process spillover, …).
    /// Growth arms → [`Self::growth_phase`]; 
    /// mood/sentiment/satisfaction → [`Self::update_sentiments`]; 
    /// [`PopEffect::BonusGood`] → [`Self::decay_goods`].
    pub stored_effects: Vec<PopEffect>,

    /// Political / social feeling of this pop (shares sum to 1).
    /// Updated in [`Self::update_sentiments`]; blendable into firms, markets, etc.
    pub sentiment: Sentiment,

    /// End-of-pass snapshot from [`Self::update_sentiments`] (tier sat, wealth, …).
    pub records: PopRecords,

}

impl Pop {
    /// Emigration / mobility pressure for this pop (mood × size × mobility, …).
    pub fn calculate_migratory_pressure(&mut self, factuals: &Factuals, _region: &Market) {
        let _ = (self, factuals);
        // Get cultural and environmental effects that modify migratory pressure.
        // get mood effects on migratory pressure.
        // Modify by the current demographics of the pop.
        // return result
        todo!("Pop calculate migratory pressure")
    }

    /// Job-to-job moves inside the same market (internal migration).
    pub fn process_internal_migration(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Pop process internal migration")
    }

    /// # Apply Scaling Factor
    /// 
    /// Resolves a `ScalingFactor` against this pop's demographics, returning the
    /// effective multiplier (scalar weight times households, adults, labor, etc.).
    pub fn get_scaling_factor(&self, scaling: ScalingFactor) -> f64 {
        match scaling {
            ScalingFactor::Fixed(f) => f,
            ScalingFactor::All(f) => f * self.demographics.total_population(),
            ScalingFactor::Household(f) => f * self.demographics.household.count,
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
                    debug_assert!(desire.amount >= 1.0, "Desire Amount should always be >= 1.0.");
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
        let growth_f = self.demographics.count() / (self.demographics.count() - self.previous_growth);
        debug_assert!(growth_f.is_finite(), "population count - previous growth reached 0. Something has gone wrong.");
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
            for demo in species.desires.values() {
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
                for demo in culture.desires.values() {
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
                for demo in religion.desires.values() {
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

    /// # Initial Reservations and Update Satisfaction
    ///
    /// Called early in the day (Pop Day §3.4–3.6), after demographics / desire resize.
    /// Prepares the pop for market day and later consumption:
    ///
    /// 1. Clear `reserved` on all property rows (fresh day earmarks).
    /// 2. Decay each desire's `satisfaction` by multiplying by `desire.decay`
    ///    (`0.0` wipes carryover; never `1.0` per Desire contract).
    /// 3. Reserve on-hand goods for **one full desire level** (`amount` units of
    ///    satisfaction), matching what `satisfy_one_desire` will try to apply later.
    ///    Order: basic → common → luxury, then within-tier list order (priority).
    ///    Within a desire: high-priority targets first, then higher efficiency
    ///    (`Desire::ordered_targets`).
    ///
    /// Reservation only increases `reserved`; it does **not** reduce `quantity` or
    /// grant satisfaction (that is `consume`). Savings is secondary to consumption:
    /// reservable stock is `available()` (`quantity - reserved`), so earmarks may
    /// claim stock that was counted toward `saved`. Missing goods stay unreserved —
    /// market day buys the gap.
    ///
    /// Does not call `update_desires`.
    pub fn initial_reservations_and_update_satisfaction(&mut self) {
        // 1. Fresh reservation slate for the day.
        for row in self.property.values_mut() {
            row.reserved = 0.0;
        }

        // 2. Carry satisfaction forward after overnight decay.
        for tier in self.desires.iter_mut() {
            for desire in tier.iter_mut() {
                desire.satisfaction *= desire.decay;
            }
        }

        // 3. Reserve goods for one full level per desire, in priority order.
        for tier_idx in 0..self.desires.len() {
            let desire_count = self.desires[tier_idx].len();
            for desire_idx in 0..desire_count {
                self.reserve_one_desire_level(tier_idx, desire_idx);
            }
        }
    }

    /// Reserves goods for one full satisfaction level of a desire (`amount` sat).
    /// Uses target caps / efficiency like `satisfy_one_desire`, but only earmarks stock.
    /// 
    /// This is part of `initial_reservations_and_update_satisfaction`
    fn reserve_one_desire_level(&mut self, tier_idx: usize, desire_idx: usize) {
        let amount = self.desires[tier_idx][desire_idx].amount;
        if amount <= 0.0 {
            return;
        }
        // Owned copies so we can mutate property without fighting the desire borrow.
        let targets: Vec<DesireTarget> = self.desires[tier_idx][desire_idx]
            .ordered_targets()
            .into_iter()
            .cloned()
            .collect();

        let mut remaining = amount;
        for target in targets {
            if remaining <= 0.0 {
                break;
            }
            let Some(row) = self.property.get_mut(&target.good) else {
                continue;
            };
            // Unreserved stock (savings is secondary; may claim into `saved`).
            let reservable = row.available().max(0.0);
            if reservable <= 0.0 {
                continue;
            }
            // Cap this good's contribution this level (same shape as satisfy_one_desire).
            let want_sat = remaining.min(amount * target.cap);
            if want_sat <= 0.0 || target.efficiency <= 0.0 {
                continue;
            }
            let needed_qty = want_sat / target.efficiency;
            let take = needed_qty.min(reservable);
            row.reserved += take;
            remaining -= take * target.efficiency;
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

    /// # Next Shopping Trip
    /// 
    /// Used during the day and called when a pop has run out of existing buy orders.
    /// 
    /// This solitifies purchases for the day, reserving new stuff, then creates a 
    /// new set of orders by `create_orders`.
    pub fn next_shopping_trip(&self) {
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
        // Restoring original tier order: priority is index for the pop and is set in update_desires.
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
    /// 
    /// This is part of consumption, and so will reduce quantity of goods.
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
    ///
    /// Target order matches reservation: [`Desire::ordered_targets`] (high-priority
    /// first, then higher efficiency).
    ///
    /// Draw from full on-hand [`PopPRow::quantity`]; `saved` is a wish target only and
    /// does **not** cap consumption (same priority as reservation).
    ///
    /// This is part of Consumption, and so will reduce quantity of goods.
    pub(crate) fn satisfy_one_desire(&mut self, desire: &mut Desire) -> f64 {
        // Owned copies so we can mutate property / desire without fighting borrows.
        let targets: Vec<DesireTarget> = desire
            .ordered_targets()
            .into_iter()
            .cloned()
            .collect();

        let mut remaining = desire.amount;

        for target in targets {
            if remaining <= 0.0 {
                break;
            }
            // get the target good, or continue on to the next target.
            if let Some(row) = self.property.get_mut(&target.good)
                && row.quantity > 0.0
            {
                // remaining (capped at this target's cap) / efficiency = qty needed.
                // Full quantity is fair game; savings does not fence stock.
                let needed = remaining.min(desire.amount * target.cap) / target.efficiency;
                let take = needed.min(row.quantity);

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
                    }
                    DesireTargetType::Use => {
                        row.used += take;
                        let sat_gained = take * target.efficiency;
                        desire.satisfaction += sat_gained;
                        remaining -= sat_gained;
                    }
                }
            }
        }
        // The current satisfaction rate.
        desire.satisfaction / desire.amount
    }

    /// # Growth Phase
    /// 
    /// Sum this pop's growth factors, multiply current households by that factor,
    /// record the delta in `previous_growth`, and apply it to household count.
    /// 
    /// Sum this pop's demographic rates then calls the household's update, changing
    /// the cohort sizes to match.
    /// 
    /// Sources of growth/decline are:
    /// 1. Base growth is 2.0% (via household birth − mortality demographics).
    /// 2. Basic Needs (species) reduces up to -30.0% from lack of satisfaction, plus
    ///    Birthrate/Mortality desire effects on basic desires.
    /// 3. Common Needs: `-0.0002 * total tiers_satisfied` in the tier, plus desire effects.
    /// 4. Luxury Needs: `-0.005 * total tiers_satisfied` in the tier, plus desire effects.
    /// 5. Institutional and Demographic effects can also be added in. Demographic
    ///    effects are brought in here, while Institutional effects are noted during
    ///    Demographic Update or Day Start.
    /// 6. Same-day [`PopEffect::Birthrate`] / [`PopEffect::Mortality`] from
    ///    [`Self::stored_effects`] (applied then removed; other stored arms kept).
    ///
    /// Advances composition via [`Household::update`]. Same-day desire and stored
    /// growth mods are stacked onto structural rates from
    /// [`Factuals::get_demographic_rates`] (recomputed per call; not stored on the pop).
    ///
    /// `previous_growth` is the change in household `count`. Dead pops
    /// (`count < 1`) skip update for cleanup.
    pub fn growth_phase(&mut self, factuals: &Factuals) {
        let old_count = self.demographics.household.count;
        if old_count < 1.0 {
            return;
        }

        // Structural rates: recompute each pop/day via factuals (no shared rate cache).
        // If this shows up in profiles at very large pop counts, prefer day-fill of
        // unique demographic keys on factuals rather than per-pop storage; see
        // Factuals::get_demographic_rates docs.
        let mut rates = factuals.get_demographic_rates(self.demographics);
        // Same-day sat / stored effects stay per-pop and are never centrally cached.
        rates = rates.add(&self.same_day_growth_rate_mods());

        self.demographics.household.update(&rates);
        self.previous_growth = self.demographics.household.count - old_count;
    }

    /// Same-day rate deltas from desire satisfaction and stored growth effects.
    /// Drains birth/mortality arms from `stored_effects`.
    fn same_day_growth_rate_mods(&mut self) -> DemographicRates {
        let mut mods = DemographicRates::zero();

        // get and apply effects from low basic satisfaction
        let basic_sat = self.tier_avg_satisfaction(0);
        let basic_penalty = 0.30 * (1.0 - basic_sat);
        mods.child_mortality.0 += basic_penalty;
        mods.adult_mortality.0 += basic_penalty;
        mods.elder_mortality.0 += basic_penalty;

        // get and apply effects from high satisfaction of common and luxury.
        // TODO: Double check these values here. These may be too strong now.
        mods.birth_per_woman -= 0.0002 * self.tier_total_satisfaction(1);
        mods.birth_per_woman -= 0.0005 * self.tier_total_satisfaction(2);

        // apply satisfaction based effects
        for tier in 0..3 {
            self.apply_tier_desire_growth_to_rates(tier, &mut mods);
        }

        // Lastly, apply any additional stored effects as needed.
        // TODO: update these to take into account the much greater variety of possible PopEffects.
        let mut kept = Vec::with_capacity(self.stored_effects.len());
        for effect in self.stored_effects.drain(..) {
            match effect {
                PopEffect::Birthrate(v) => {
                    debug_assert!(v.is_finite(), "Stored birthrate must be finite.");
                    mods.birth_per_woman += v;
                }
                PopEffect::Mortality(v) => {
                    debug_assert!(v.is_finite(), "Stored mortality must be finite.");
                    mods.adult_mortality.0 += v;
                }
                other => kept.push(other),
            }
        }
        self.stored_effects = kept;

        mods
    }

    /// Fold desire Birthrate/Mortality effects from desires for one tier into `mods`.
    fn apply_tier_desire_growth_to_rates(&self, tier: usize, mods: &mut DemographicRates) {
        let Some(desires) = self.desires.get(tier) else {
            return;
        };
        for desire in desires {
            let sat = desire.tiers_satisfied().clamp(0.0, 1.0);
            let lack = 1.0 - sat;
            for effect in &desire.effect {
                match effect {
                    DesireEffect::Birthrate(v, true) => mods.birth_per_woman += v * sat,
                    DesireEffect::Birthrate(v, false) => mods.birth_per_woman -= v * lack,
                    DesireEffect::Mortality(v, true) => mods.adult_mortality.0 += v * sat,
                    DesireEffect::Mortality(v, false) => mods.adult_mortality.0 += v * lack,
                    DesireEffect::BonusGood(_, _, _)
                    | DesireEffect::Satisfaction(_, _)
                    | DesireEffect::SentimentFlat(_, _, _)
                    | DesireEffect::SentimentRelative(_, _, _) => {}
                }
            }
        }
    }

    /// # Tier Average Satisfaction
    ///
    /// Average desire success rate in a tier (`sum / count`). Used where a 0–1-ish
    /// completeness is needed (e.g. growth penalties). Not what [`PopRecords::tier_sat`] stores.
    fn tier_avg_satisfaction(&self, tier: usize) -> f64 {
        let Some(desires) = self.desires.get(tier) else {
            return 1.0;
        };
        if desires.is_empty() {
            return 1.0;
        }
        let sum: f64 = desires
            .iter()
            .map(|d| d.tiers_satisfied())
            .sum();
        sum / desires.len() as f64
    }

    /// # Tier Satisfaction
    ///
    /// Sum of desire success rates in a tier: `Sum(satisfaction / amount)`.
    /// Empty tier counts as `1.0` (no unmet needs). This is the unboosted form of
    /// what is written into [`PopRecords::tier_sat`].
    fn tier_satisfaction(&self, tier: usize) -> f64 {
        let Some(desires) = self.desires.get(tier) else {
            return 1.0;
        };
        if desires.is_empty() {
            return 1.0;
        }
        desires.iter().map(|d| d.tiers_satisfied()).sum()
    }

    /// Sum of `tiers_satisfied` across all desires in a tier (uncapped; luxury oversat counts).
    /// Empty tier returns `0.0` (unlike [`Self::tier_satisfaction`]).
    fn tier_total_satisfaction(&self, tier: usize) -> f64 {
        let Some(desires) = self.desires.get(tier) else {
            return 0.0;
        };
        desires.iter().map(|d| d.tiers_satisfied()).sum()
    }

    /// # Update Sentiments
    /// 
    /// Run after Growth_phase. This processes the Satisfaction of desires, applying some
    ///  modifications, and recording the results to alter the pop's Sentiments.
    ///
    /// Late-day satisfaction-boost + mood pass (after consume and growth).
    /// Does **not** apply growth arms ([`Self::growth_phase`]) or bonus goods
    /// ([`Self::decay_goods`]).
    ///
    /// 1. Collect satisfaction and apply any boosts.
    ///    - Desire [`DesireEffect::Satisfaction`] -> boost for **that desire's tier**.
    ///      Desire boosts are sat-scaled via [`DesireEffect::signed_strength`].
    ///    - Stored [`PopEffect::Satisfaction`] -> boost for the **named tier**;
    ///      amount is already scaled (e.g. process output / pop).
    ///    - **Tier sat** result (sum of success rates, not an average):
    ///      `Sum(satisfaction / amount) + boost`
    ///      (no common hard cap; surplus gives reduced sentiment weight when normalized).
    /// 2. Write [`PopRecords`]: tier sat, property wealth (AMV), satisfaction units.
    /// 3. Baseline sentiment shifts from tier sat (mood path normalizes by desire count).
    /// 4. Desire + stored sentiment effects.
    /// 5. Leave bonus-good stored arms for later phases (growth should already be consumed).
    ///
    /// `market_history` supplies prices for wealth AMV (missing good prices default to 1.0).
    pub fn update_sentiments(&mut self, market_history: &MarketHistory) {
        // 1. Collect boosts per tier (desire effects + stored Satisfaction).
        let mut tier_boosts = [0.0_f64; 3];
        // 1a. Desire Bonuses first.
        for (tier_idx, tier) in self.desires.iter().enumerate() {
            for desire in tier {
                let sat01 = desire.tiers_satisfied().clamp(0.0, 1.0);
                for effect in &desire.effect {
                    if let DesireEffect::Satisfaction(_, _) = *effect {
                        debug_assert!(
                            tier_idx != 0,
                            "DesireEffect::Satisfaction is not allowed on basic desires"
                        );
                        if tier_idx == 0 {
                            continue;
                        }
                        tier_boosts[tier_idx] += effect.signed_strength(sat01);
                    }
                }
            }
        }
        // 1b. Gather satisfaction bonuses from stored_effects.
        // While we're at it, drain setiment effects into a separate list for later processing.
        let pending_stored: Vec<PopEffect> = self.stored_effects.drain(..).collect();
        let mut sentiment_effects = Vec::with_capacity(pending_stored.len());
        let mut kept = Vec::with_capacity(pending_stored.len());
        for effect in pending_stored {
            match effect {
                PopEffect::Satisfaction { tier, amount } => {
                    debug_assert!(
                        tier == 1 || tier == 2,
                        "PopEffect::Satisfaction tier must be common (1) or luxury (2)"
                    );
                    if tier == 1 || tier == 2 {
                        debug_assert!(amount.is_finite(), "Satisfaction boost must be finite.");
                        tier_boosts[tier] += amount;
                    }
                },
                PopEffect::SentimentFlat { .. } | PopEffect::SentimentRelative { .. } => {
                    sentiment_effects.push(effect);
                },
                PopEffect::Birthrate(..) | PopEffect::Mortality(..) => {
                    debug_assert!(false, "PopEffect::Birthrate and PopEffect::Mortality should have been applied in growth_phase, not update_sentiments.");
                },
                other => kept.push(other),
            }
        }
        // wrap up by putting remainder back in stored_effects (bonus goods, etc).
        self.stored_effects = kept;

        // 2. Day records: tier sat (sums of success rates), wealth, satisfaction units.
        let tier_sat_boosted = [
            self.tier_satisfaction(0),
            self.tier_sat_with_boost(1, tier_boosts[1]),
            self.tier_sat_with_boost(2, tier_boosts[2]),
        ];
        self.records.tier_sat = tier_sat_boosted;
        self.records.wealth_amv = self.property_wealth_amv(market_history);
        self.records.satisfaction_units_total = self
            .desires
            .iter()
            .flatten()
            .map(|d| d.satisfaction)
            .sum();
        self.records.update_living_standard();
        self.records.update_trend();

        // 3. From our updated records, shift sentiments.
        let mut mods = self.sentiment_mods_from_satisfaction();

        // 4. Desire sentiment effects (skip growth, bonus goods, satisfaction — done).
        for tier in &self.desires {
            for desire in tier {
                let sat01 = desire.tiers_satisfied().clamp(0.0, 1.0);
                for effect in &desire.effect {
                    match *effect {
                        DesireEffect::Birthrate(_, _)
                        | DesireEffect::Mortality(_, _)
                        | DesireEffect::BonusGood(_, _, _)
                        | DesireEffect::Satisfaction(_, _) => {}
                        DesireEffect::SentimentFlat(kind, _, _) => {
                            let delta = effect.signed_strength(sat01);
                            if delta != 0.0 {
                                mods.push(SentimentMod::Flat { kind, delta });
                            }
                        }
                        DesireEffect::SentimentRelative(kind, _, _) => {
                            let relative = effect.signed_strength(sat01);
                            if relative != 0.0 {
                                mods.push(SentimentMod::Relative { kind, relative });
                            }
                        }
                    }
                }
            }
        }

        // 5. Remaining stored: sentiment in; bonus goods kept.
        // Growth arms must already have been applied in `growth_phase`.
        for effect in sentiment_effects {
            match effect {
                PopEffect::SentimentFlat { kind, delta } => {
                    if delta != 0.0 {
                        mods.push(SentimentMod::Flat { kind, delta });
                    }
                },
                PopEffect::SentimentRelative { kind, relative } => {
                    if relative != 0.0 {
                        mods.push(SentimentMod::Relative { kind, relative });
                    }
                },
                other => {
                    unreachable!("sentiment_effects should only contain sentiment effects, got {other:?}");
                },
            }
        }

        self.sentiment.apply_mods(mods);
        debug_assert!(self.sentiment.is_valid());
    }

    /// # Sentiment Mods from Satisfaction
    /// 
    /// Helper that takes the current state of the pop's records, current standard of 
    /// living, SOL Trend, Specific Satisfaction rates, etc, and calculates shifts in
    /// sentiment. 
    /// 
    /// It returns it from this function for testing purposes, does not apply it.
    /// 
    /// The way things are 'expected' to work is that each tier is satisfied fully 
    /// before moving onto the next. As such, logic assumes little mixing of
    /// satisfaction.
    fn sentiment_mods_from_satisfaction(&self) -> Vec<SentimentMod> {
        // `records.tier_sat` stores sums of desire success rates; normalize by count for mood.
        // Empty tiers are recorded as 1.0 (treated as fully satisfied).
        let basic = if self.desires[0].is_empty() {
            1.0
        } else {
            self.records.tier_sat[0] / self.desires[0].len() as f64
        };
        let common = if self.desires[1].is_empty() {
            1.0
        } else {
            self.records.tier_sat[1] / self.desires[1].len() as f64
        };
        let luxury = if self.desires[2].is_empty() {
            1.0
        } else {
            self.records.tier_sat[2] / self.desires[2].len() as f64
        };
        // Common mood alteration, half rate above 1.0
        let common_mood = Self::common_sat_mood_weight(common);
        // Use our satisfactions to cerate modifiers for our Sentiment.
        // TODO: Modify the values below to meet the game's needs later.
        let mut mods: Vec<SentimentMod> = vec![
            SentimentMod::Flat {
                kind: SentimentKind::Anger, // 100% at 
                delta: lerp(pop_constants::ANGER_SENTIMENT_RATE, 0.0, basic),
            },
            SentimentMod::Flat {
                kind: SentimentKind::Fear,
                delta: lerp(pop_constants::FEAR_SENTIMENT_RATE, 0.0, basic),
            },
            SentimentMod::Flat {
                kind: SentimentKind::Contentment,
                delta: lerp(0.0, pop_constants::CONTENTMENT_SENTIMENT_RATE, basic * common_mood),
            },
            SentimentMod::Flat {
                kind: SentimentKind::Happiness,
                delta: lerp(0.0, pop_constants::HAPPINESS_SENTIMENT_RATE, common_mood),
            },
            SentimentMod::Flat {
                kind: SentimentKind::Hope,
                delta: lerp(0.0, pop_constants::HOPE_SENTIMENT_RATE, luxury),
            },
        ];

        // Create Modifications based on the trend of SOL.
        let trend = self.records.trend;
        if trend.abs() >= pop_constants::SENTIMENT_TREND_DEADBAND {
            let sol = self.records.living_standard.max(0.5);
            let relative = trend / sol;

            if relative > 0.0 {
                let strength = relative * pop_constants::SENTIMENT_RISE_GAIN;
                // rising: Extra Contentment, Happiness, and Hope.
                mods.push(SentimentMod::Relative {
                    kind: SentimentKind::Contentment,
                    relative: relative * pop_constants::TREND_CONTENTMENT_SENTIMENT_RATE,
                });
                mods.push(SentimentMod::Relative {
                    kind: SentimentKind::Happiness,
                    relative: relative * pop_constants::TREND_HAPPINESS_SENTIMENT_RATE,
                });
                mods.push(SentimentMod::Relative {
                    kind: SentimentKind::Hope,
                    relative: relative * pop_constants::TREND_HOPE_SENTIMENT_RATE,
                });
            } else {
                let strength = relative * pop_constants::SENTIMENT_FALL_GAIN;
                // falling: Extra Anger and Fear.
                mods.push(SentimentMod::Relative {
                    kind: SentimentKind::Anger,
                    relative: relative * pop_constants::TREND_ANGER_SENTIMENT_RATE,
                });
                mods.push(SentimentMod::Relative {
                    kind: SentimentKind::Fear,
                    relative: relative * pop_constants::TREND_FEAR_SENTIMENT_RATE,
                });
            }
        }

        mods
    }

    /// # Tier Satisfaction with Boost
    ///
    /// Sum of desire success rates in a non-basic tier, plus a satisfaction boost,
    /// without mutating individual desires.
    ///
    /// ```text
    /// Sum(satisfaction / amount) + boost
    /// ```
    ///
    /// `boost` is satisfaction-boost mass (same units as desire success rates):
    /// desire effects contribute via sat-scaled rates; stored effects contribute an
    /// already-scaled amount (e.g. process output / pop).
    ///
    /// No upper clamp. Floor at 0. Empty tier: `1.0` (no unmet needs), boost ignored.
    ///
    /// TODO: Might allow negative values eventually, but not just yet.
    fn tier_sat_with_boost(&self, tier: usize, boost: f64) -> f64 {
        // TODO, consider removing this. A pop with an 'ascetic' religous/cultural trait
        // might be a worthwhile thing to consider. Back burner for now.
        debug_assert!(
            tier == 1 || tier == 2,
            "Satisfaction boosts only apply to common (1) or luxury (2), got {tier}"
        );
        debug_assert!(boost.is_finite(), "Satisfaction boost must be finite.");
        let desires = self.desires.get(tier)
            .unwrap();
        if desires.is_empty() {
            return 1.0;
        }
        let sum_desire_sat: f64 = desires
            .iter()
            .map(|d| {
                debug_assert!(d.amount > 0.0, "Desire amount must be positive.");
                d.satisfaction / d.amount
            })
            .sum();
        (sum_desire_sat + boost).max(0.0)
    }

    /// Map common tier sat to sentiment weight: full effect on `[0, 1]`, half effect
    /// on any overflow above 1.0 (common sat surplus).
    fn common_sat_mood_weight(common_sat: f64) -> f64 {
        let c = common_sat.max(0.0);
        if c <= 1.0 {
            c
        } else {
            1.0 + 0.5 * (c - 1.0)
        }
    }

    /// AMV of on-hand property: `Sum(quantity * price)`.
    /// Missing prices default to `1.0` (same convention as order costing).
    pub fn property_wealth_amv(&self, market_history: &MarketHistory) -> f64 {
        let mut total = 0.0;
        for (good_id, row) in &self.property {
            debug_assert!(row.quantity.is_finite(), "Property quantity must be finite.");
            let price = market_history.prices.get(good_id).copied().unwrap_or(1.0);
            debug_assert!(price.is_finite(), "Market price must be finite.");
            total += row.quantity * price;
        }
        total
    }

    /// End-of-day bookkeeping for this pop (satisfaction stats, property notes, …).
    /// Only external input is factuals.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Pop record keeping")
    }

    /// # Decay Goods
    ///
    /// Called at the very end of the day (Pop Day §10). Only external input is
    /// factuals (good definitions: decay rate, byproducts, tags).
    ///
    /// 1. Move `used` stock back into `quantity`.
    /// 2. Decay remaining `quantity` by each good's `decay_rate` (skipped for
    ///    [`GoodTag::Exposure`] — those only decay when unowned).
    /// 3. Fully destroy `consumed` (100% decay) and clear the bucket.
    /// 4. Credit decay byproducts from steps 2–3 into property (same-day
    ///    byproducts do not decay again; goods only decay one level).
    /// 5. Grant desire [`DesireEffect::BonusGood`] bonuses scaled by
    ///    `tiers_satisfied` (malus path is ignored here).
    /// 6. Pay out [`PopEffect::BonusGood`] from [`Self::stored_effects`] and
    ///    remove those entries (other stored effects should have been removed in other
    ///    phases).
    ///
    /// Does **not** rewrite `saved` / `reserved` after stock falls: `saved` is a
    /// wish target (shortfall should remain visible for mood), and `reserved` is
    /// cleared at next day-start.
    pub fn decay_goods(&mut self, factuals: &Factuals) {
        // Byproducts and bonus goods applied after the main pass so they do not
        // decay again the same day, and so we can insert missing property rows.
        let mut gains: HashMap<usize, f64> = HashMap::new();

        for (&good_id, row) in self.property.iter_mut() {
            // 1. Return used goods to quantity (consume had moved them out).
            if row.used != 0.0 {
                row.quantity += row.used;
                row.used = 0.0;
            }

            // 2. Decay on-hand goods by the good's rate, excluding Exposure while owned.
            let good = factuals.find_good(good_id);
            let exposure = good.tags.contains(&GoodTag::Exposure);
            if !exposure && good.decay_rate > 0.0 && row.quantity > 0.0 {
                let lost = row.quantity * good.decay_rate;
                row.quantity -= lost;
                for (&byproduct, &ratio) in &good.decay_result {
                    if ratio != 0.0 && lost != 0.0 {
                        *gains.entry(byproduct).or_insert(0.0) += lost * ratio;
                    }
                }
            }

            // 3. Consumed goods: full destruction + byproducts.
            if row.consumed > 0.0 {
                let lost = row.consumed;
                row.consumed = 0.0;
                for (&byproduct, &ratio) in &good.decay_result {
                    if ratio != 0.0 && lost != 0.0 {
                        *gains.entry(byproduct).or_insert(0.0) += lost * ratio;
                    }
                }
            }
        }

        // 4. Apply decay byproducts into property.
        for (good_id, amount) in gains {
            if amount == 0.0 {
                continue;
            }
            self.property
                .entry(good_id)
                .or_insert_with(|| PopPRow::new(0.0))
                .quantity += amount;
        }

        // 5. Bonus goods from satisfied desires.
        let mut bonus_gains: HashMap<usize, f64> = HashMap::new();
        for tier in &self.desires {
            for desire in tier {
                let sat = desire.tiers_satisfied().max(0.0);
                if sat <= 0.0 {
                    continue;
                }
                for effect in &desire.effect {
                    if let DesireEffect::BonusGood(good_id, amount, true) = *effect {
                        let qty = amount * sat;
                        if qty != 0.0 {
                            *bonus_gains.entry(good_id).or_insert(0.0) += qty;
                        }
                    }
                }
            }
        }
        for (good_id, amount) in bonus_gains {
            self.property
                .entry(good_id)
                .or_insert_with(|| PopPRow::new(0.0))
                .quantity += amount;
        }

        // 6. Pay out BonusGood.
        // Mood/sentiment should already be gone after update_sentiments.
        let mut kept_effects = Vec::with_capacity(self.stored_effects.len());
        for effect in self.stored_effects.drain(..) {
            match effect {
                PopEffect::BonusGood { good, amount } => {
                    if amount != 0.0 {
                        self.property
                            .entry(good)
                            .or_insert_with(|| PopPRow::new(0.0))
                            .quantity += amount;
                    }
                }
                other => {
                    debug_assert!(
                        false,
                        "Unexpected stored effect at decay (should be applied earlier): {other:?}"
                    );
                    kept_effects.push(other);
                }
            }
        }
        // This is last action of the day, so stored effects should be empty
        debug_assert!(
            kept_effects.is_empty(),
            "No ether effects should exist at this point."
        );
        debug_assert!(
            self.stored_effects.is_empty(),
            "No ether stored effects should exist at this point."
        );
    }
}

#[cfg(test)]
mod pop {
    use std::collections::{HashMap, HashSet};

    use crate::game::{
        desire::{Desire, DesireEffect, DesireSource, DesireTarget, DesireTargetType},
        factuals::Factuals,
        good::Good,
        household::{DemographicRates, Household},
        market::MarketHistory,
        pop::{DemoRow, Pop, PopEffect, PopPRow, PopRecords},
        scalingfactor::ScalingFactor,
        sentiment::Sentiment,
    };

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
                household: Household::with_count(10.0),
                species: 0,
                culture: 0,
                class: 0,
                religion: 0,
            },
            current_orders: vec![],
            previous_growth: 0.0,
            stored_effects: vec![],
            sentiment: Sentiment::new(),
            records: PopRecords::default(),
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

            pop.demographics.household.count += 10.0; // double households
            pop.previous_growth += 10.0; // include growth change.
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
            let species_demo = household_demo(10, 1.0, 0, 0);
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
            assert_eq!(*pop.desires[0][0].source.demo_desire_id(), 10);
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

        #[test]
        fn shrinks_population_preserves_satisfaction_ratio_and_luxury_oversat() {
            // Basic: half-satisfied. Luxury: oversatisfied (3x amount).
            let basic = household_demo(1, 1.0, 0, 0);
            let luxury = household_demo(2, 1.0, 0, 2);
            let culture = Culture::new(1, "Test")
                .with_desire(basic.clone())
                .with_desire(luxury.clone());
            let factuals = Factuals::new().with_culture(culture);

            let mut pop = make_pop(); // 10 households → amount 10 each
            pop.demographics.culture = 1;
            let mut d_basic = basic.create_desire(&pop, DesireSource::Culture(1, 0));
            let mut d_lux = luxury.create_desire(&pop, DesireSource::Culture(1, 0));
            d_basic.satisfaction = 5.0;   // 0.5 tiers
            d_lux.satisfaction = 30.0;    // 3.0 tiers (oversat)
            pop.desires[0].push(d_basic);
            pop.desires[2].push(d_lux);

            pop.demographics.household.count = 5.0; // shrink to half
            pop.update_desires(&factuals);

            // amount 1.0 * 5 = 5; satisfaction scales by 5/10
            assert_eq!(pop.desires[0][0].amount, 5.0);
            assert_eq!(pop.desires[0][0].satisfaction, 2.5);
            assert!((pop.desires[0][0].tiers_satisfied() - 0.5).abs() < 1e-9);

            assert_eq!(pop.desires[2][0].amount, 5.0);
            assert_eq!(pop.desires[2][0].satisfaction, 15.0);
            assert!((pop.desires[2][0].tiers_satisfied() - 3.0).abs() < 1e-9);
        }

        #[test]
        fn removes_desires_no_longer_on_demographic() {
            let keep = household_demo(1, 1.0, 0, 0);
            let drop = household_demo(2, 1.0, 1, 0);
            let mut pop = make_pop();
            pop.demographics.culture = 1;
            // Pop still holds both; culture only keeps `keep` (player removed `drop`).
            let d_keep = keep.create_desire(&pop, DesireSource::Culture(1, 0));
            let d_drop = drop.create_desire(&pop, DesireSource::Culture(1, 0));
            pop.desires[0].push(d_keep);
            pop.desires[0].push(d_drop);

            let culture_after = Culture::new(1, "Test").with_desire(keep.clone());
            let factuals = Factuals::new().with_culture(culture_after);

            pop.update_desires(&factuals);

            assert_eq!(pop.desires[0].len(), 1);
            assert_eq!(*pop.desires[0][0].source.demo_desire_id(), 1);
            assert_eq!(pop.desires[0][0].priority, 0);
        }

        #[test]
        fn scales_property_targets_with_previous_growth_positive_zero_and_negative() {
            // growth_f = count / (count - previous_growth)
            // count fixed at 10 for all three cases.
            fn run(previous_growth: f64) -> (f64, f64) {
                let demo = household_demo(1, 1.0, 0, 0);
                let factuals = Factuals::new()
                    .with_culture(Culture::new(1, "Test").with_desire(demo.clone()));
                let mut pop = make_pop();
                pop.demographics.culture = 1;
                pop.previous_growth = previous_growth;
                let desire = demo.create_desire(&pop, DesireSource::Culture(1, 0));
                pop.desires[0].push(desire);
                pop.property.insert(100, PopPRow::new(0.0).with_target(20.0).with_desire_need(10.0));
                // Zero targets should stay zero (not scaled).
                pop.property.insert(101, PopPRow::new(0.0).with_target(0.0).with_desire_need(0.0));

                pop.update_desires(&factuals);
                (
                    pop.property[&100].shop_target,
                    pop.property[&100].desire_needs,
                )
            }

            // positive: 10 / (10 - 5) = 2.0 → 20*2=40, 10*2=20
            let (shop_pos, need_pos) = run(5.0);
            assert!((shop_pos - 40.0).abs() < 1e-9);
            assert!((need_pos - 20.0).abs() < 1e-9);

            // zero: factor 1.0
            let (shop_zero, need_zero) = run(0.0);
            assert!((shop_zero - 20.0).abs() < 1e-9);
            assert!((need_zero - 10.0).abs() < 1e-9);

            // negative: 10 / (10 + 10) = 0.5 → 10 and 5
            let (shop_neg, need_neg) = run(-10.0);
            assert!((shop_neg - 10.0).abs() < 1e-9);
            assert!((need_neg - 5.0).abs() < 1e-9);
        }

        #[test]
        fn multi_tier_adds_and_removes_in_one_update() {
            // Start: species demos 1 (basic), 2 (common); culture demos 10 (basic), 11 (luxury).
            // After: remove species 2 & culture 10; add species 3 (luxury) & culture 12 (common).
            let s1 = household_demo(1, 1.0, 0, 0);
            let s2 = household_demo(2, 2.0, 0, 1);
            let s3 = household_demo(3, 1.5, 0, 2);
            let c10 = household_demo(10, 1.0, 1, 0);
            let c11 = household_demo(11, 1.0, 0, 2);
            let c12 = household_demo(12, 4.0, 0, 1);

            let species_after = Species::new(0, "Human")
                .with_desire(s1.clone())
                .with_desire(s3.clone());
            let culture_after = Culture::new(1, "Test")
                .with_desire(c11.clone())
                .with_desire(c12.clone());
            let factuals = Factuals::new()
                .with_species(species_after)
                .with_culture(culture_after);

            let mut pop = make_pop(); // 10 households
            pop.demographics.culture = 1;
            let d_s1 = s1.create_desire(&pop, DesireSource::Species(0, 0));
            let d_s2 = s2.create_desire(&pop, DesireSource::Species(0, 0));
            let d_c10 = c10.create_desire(&pop, DesireSource::Culture(1, 0));
            let d_c11 = c11.create_desire(&pop, DesireSource::Culture(1, 0));
            pop.desires[0].push(d_s1);
            pop.desires[1].push(d_s2);
            pop.desires[0].push(d_c10);
            pop.desires[2].push(d_c11);

            pop.update_desires(&factuals);

            // Basic: kept species 1, removed culture 10 → only species 1
            assert_eq!(pop.desires[0].len(), 1);
            assert!(matches!(pop.desires[0][0].source, DesireSource::Species(0, 1)));
            assert_eq!(pop.desires[0][0].amount, 10.0);

            // Common: removed species 2, added culture 12 → only culture 12 at 4.0*10
            assert_eq!(pop.desires[1].len(), 1);
            assert!(matches!(pop.desires[1][0].source, DesireSource::Culture(1, 12)));
            assert_eq!(pop.desires[1][0].amount, 40.0);
            assert_eq!(pop.desires[1][0].satisfaction, 0.0);

            // Luxury: kept culture 11, added species 3 — sorted Species before Culture
            assert_eq!(pop.desires[2].len(), 2);
            assert!(matches!(pop.desires[2][0].source, DesireSource::Species(0, 3)));
            assert!(matches!(pop.desires[2][1].source, DesireSource::Culture(1, 11)));
            assert_eq!(pop.desires[2][0].amount, 15.0); // 1.5 * 10
            assert_eq!(pop.desires[2][1].amount, 10.0);
            assert_eq!(pop.desires[2][0].priority, 0);
            assert_eq!(pop.desires[2][1].priority, 1);
        }
    }

    mod growth_phase_should {
        use super::*;

        #[test]
        fn updates_household_and_records_previous_growth() {
            let mut pop = make_pop();
            let factuals = make_default_factuals();
            let old_count = pop.demographics.household.count;
            pop.growth_phase(&factuals);
            assert!(pop.demographics.household.count.is_finite());
            assert!(pop.demographics.household.count > 0.0);
            assert!((pop.previous_growth - (pop.demographics.household.count - old_count)).abs() < 1e-12);
        }

        #[test]
        fn skips_dead_pop() {
            let mut pop = make_pop();
            let factuals = make_default_factuals();
            pop.demographics.household.count = 0.0;
            pop.demographics.household.adult = 0.0;
            pop.demographics.household.elder = 0.0;
            pop.demographics.household.child = 0.0;
            pop.previous_growth = 1.0;
            pop.growth_phase(&factuals);
            assert_eq!(pop.demographics.household.count, 0.0);
            assert_eq!(pop.previous_growth, 1.0); // unchanged
        }

        #[test]
        fn drains_stored_birthrate_and_mortality() {
            let mut pop = make_pop();
            let factuals = make_default_factuals();
            pop.stored_effects.push(PopEffect::Birthrate(0.01));
            pop.stored_effects.push(PopEffect::Mortality(0.005));
            pop.stored_effects.push(PopEffect::BonusGood {
                good: 100,
                amount: 1.0,
            });
            pop.growth_phase(&factuals);
            assert_eq!(pop.stored_effects.len(), 1);
            assert!(matches!(
                pop.stored_effects[0],
                PopEffect::BonusGood {
                    good: 100,
                    amount: 1.0
                }
            ));
        }
    }

    mod demographic_rates_and_update_desires_should {
        use super::*;
        use crate::game::{
            culture::Culture, desire::DemoDesire, household::DemographicRates, religion::Religion,
            species::Species,
        };

        fn household_demo(id: usize, amount: f64, priority: isize, tier: usize) -> DemoDesire {
            DemoDesire::new(id)
                .with_amount(amount)
                .with_priority(priority)
                .with_tier(tier)
                .with_scalar(ScalingFactor::Household(1.0))
        }

        fn birth_mod(birth_per_woman: f64) -> DemographicRates {
            let mut m = DemographicRates::zero();
            m.birth_per_woman = birth_per_woman;
            m
        }

        #[test]
        fn get_demographic_rates_stacks_species_culture_religion() {
            let mut species = Species::new(0, "Human");
            species.species_demo_eff = birth_mod(0.01);

            let mut culture = Culture::new(1, "Test");
            let mut cmod = DemographicRates::zero();
            cmod.infant_mortality = 0.05;
            culture.culture_demo_eff = cmod;

            let mut religion = Religion::new(2, "Faith");
            let mut rmod = DemographicRates::zero();
            rmod.adult_mortality.0 = -0.001;
            religion.religion_demo_eff = rmod;

            let factuals = Factuals::new()
                .with_species(species)
                .with_culture(culture)
                .with_religion(religion);

            let mut pop = make_pop();
            pop.demographics.species = 0;
            pop.demographics.culture = 1;
            pop.demographics.religion = 2;
            let adult_before = pop.demographics.household.adult;

            let rates = factuals.get_demographic_rates(pop.demographics);

            let expected = DemographicRates::baseline()
                .add(&birth_mod(0.01))
                .add(&{
                    let mut m = DemographicRates::zero();
                    m.infant_mortality = 0.05;
                    m
                })
                .add(&{
                    let mut m = DemographicRates::zero();
                    m.adult_mortality.0 = -0.001;
                    m
                });
            assert_eq!(rates, expected);
            assert!(
                (rates.birth_per_woman - DemographicRates::baseline().birth_per_woman - 0.01)
                    .abs()
                    < 1e-9
            );
            assert_eq!(pop.demographics.household.adult, adult_before);
        }

        #[test]
        fn get_demographic_rates_does_not_mutate_household() {
            let mut species = Species::new(0, "Human");
            species.species_demo_eff = birth_mod(0.5);

            let factuals = Factuals::new().with_species(species);
            let mut pop = make_pop();
            pop.demographics.household.adult = 99.0;

            let rates = factuals.get_demographic_rates(pop.demographics);

            assert!((pop.demographics.household.adult - 99.0).abs() < 1e-9);
            assert!(
                (rates.birth_per_woman - DemographicRates::baseline().birth_per_woman - 0.5).abs()
                    < 1e-9
            );
        }

        #[test]
        fn update_desires_adds_missing_culture_desires() {
            let demo = household_demo(7, 2.0, 0, 1); // common tier, base 2.0
            let culture = Culture::new(1, "Test").with_desire(demo);

            let factuals = Factuals::new().with_culture(culture);
            let mut pop = make_pop(); // count 10
            pop.demographics.culture = 1;
            assert!(pop.desires[1].is_empty());

            pop.update_desires(&factuals);

            assert_eq!(pop.desires[1].len(), 1);
            assert_eq!(*pop.desires[1][0].source.demo_desire_id(), 7);
            assert_eq!(pop.desires[1][0].amount, 20.0); // 2.0 * 10 households
            assert_eq!(pop.desires[1][0].satisfaction, 0.0);
        }

        #[test]
        fn update_desires_rescales_existing_desires_for_previous_growth() {
            let demo = household_demo(3, 1.0, 0, 0);
            let culture = Culture::new(1, "Test").with_desire(demo.clone());
            let factuals = Factuals::new().with_culture(culture);

            let mut pop = make_pop(); // count 10
            pop.demographics.culture = 1;
            let mut desire = demo.create_desire(&pop, DesireSource::Culture(1, 0));
            desire.satisfaction = 5.0; // half of amount 10
            pop.desires[0].push(desire);

            pop.demographics.household.count = 20.0;
            pop.previous_growth = 10.0;

            pop.update_desires(&factuals);

            // amount 1.0 * 20 = 20; satisfaction 5 * (20/10) = 10
            assert_eq!(pop.desires[0].len(), 1);
            assert_eq!(pop.desires[0][0].amount, 20.0);
            assert_eq!(pop.desires[0][0].satisfaction, 10.0);
        }

        #[test]
        fn get_demographic_rates_skips_culture_and_religion_id_zero() {
            let mut culture = Culture::new(1, "Unused");
            culture.culture_demo_eff = birth_mod(100.0);

            let mut species = Species::new(0, "Human");
            species.species_demo_eff = DemographicRates::zero();

            let factuals = Factuals::new()
                .with_species(species)
                .with_culture(culture);

            let mut pop = make_pop();
            pop.demographics.culture = 0;
            pop.demographics.religion = 0;

            let rates = factuals.get_demographic_rates(pop.demographics);

            // Only baseline + zero species (culture id 0 skipped).
            assert_eq!(rates, DemographicRates::baseline());
        }
    }

    mod initial_reservations_and_update_satisfaction_should {
        use super::*;

        #[test]
        fn clears_reserved_decays_satisfaction_and_reserves_one_level() {
            let mut pop = make_pop();
            let mut desire = make_desire(
                0,
                DesireTarget::new(100, DesireTargetType::Consume, 1.0),
                10.0,
            );
            desire.satisfaction = 10.0;
            desire.decay = 0.5; // overnight → 5.0 sat remaining
            pop.desires[0].push(desire);

            // Stale reserved from yesterday; free stock of 20.
            pop.property.insert(100, PopPRow::new(20.0).with_reserve(7.0));

            pop.initial_reservations_and_update_satisfaction();

            assert_eq!(pop.desires[0][0].satisfaction, 5.0);
            // One full level: amount 10 @ eff 1.0 → reserve 10; quantity untouched.
            assert_eq!(pop.property[&100].reserved, 10.0);
            assert_eq!(pop.property[&100].quantity, 20.0);
        }

        #[test]
        fn prefers_high_priority_then_efficiency_and_respects_cap() {
            let mut pop = make_pop();
            let mut desire = make_desire(
                0,
                DesireTarget::new(100, DesireTargetType::Consume, 2.0), // higher eff, not high prio
                10.0,
            );
            desire.target.push(
                DesireTarget::new(101, DesireTargetType::Consume, 1.0)
                    .with_high_priority(true)
                    .with_cap(0.5), // at most 5 sat from good 101
            );
            pop.desires[0].push(desire);
            pop.property.insert(100, PopPRow::new(100.0));
            pop.property.insert(101, PopPRow::new(100.0));

            pop.initial_reservations_and_update_satisfaction();

            // High-priority 101 first: 5 sat / 1.0 eff = 5 qty.
            assert_eq!(pop.property[&101].reserved, 5.0);
            // Remainder 5 sat / 2.0 eff = 2.5 qty on 100.
            assert_eq!(pop.property[&100].reserved, 2.5);
        }

        #[test]
        fn earlier_desires_claim_shared_goods_first() {
            let mut pop = make_pop();
            // Two basic desires both want good 100; only 12 on hand.
            pop.desires[0].push(make_desire(
                0,
                DesireTarget::new(100, DesireTargetType::Consume, 1.0),
                10.0,
            ));
            pop.desires[0].push(make_desire(
                1,
                DesireTarget::new(100, DesireTargetType::Consume, 1.0),
                10.0,
            ));
            pop.property.insert(100, PopPRow::new(12.0));

            pop.initial_reservations_and_update_satisfaction();

            // First desire takes 10; second only gets the leftover 2.
            assert_eq!(pop.property[&100].reserved, 12.0);
        }

        #[test]
        fn may_reserve_stock_counted_toward_saved() {
            let mut pop = make_pop();
            pop.desires[0].push(make_desire(
                0,
                DesireTarget::new(100, DesireTargetType::Consume, 1.0),
                10.0,
            ));
            // All 10 units are also marked saved; consumption reserve still claims them.
            pop.property.insert(100, PopPRow::new(10.0).with_saved(10.0));

            pop.initial_reservations_and_update_satisfaction();

            assert_eq!(pop.property[&100].reserved, 10.0);
            assert_eq!(pop.property[&100].saved, 10.0);
            assert_eq!(pop.property[&100].quantity, 10.0);
        }
    }

    mod update_sentiments_should {
        use super::*;
        use crate::game::sentiment::SentimentKind;

        #[test]
        fn records_tier_sat_and_nudges_sentiment_from_unmet_basic() {
            let mut pop = make_pop();
            // Unsatisfied basic desire of amount 10.
            pop.desires[0].push(make_desire(
                0,
                DesireTarget::new(100, DesireTargetType::Consume, 1.0),
                10.0,
            ));
            // satisfaction stays 0 → basic sum of success rates = 0.

            let history = make_default_market_history();
            pop.update_sentiments(&history);

            assert_eq!(pop.records.tier_sat[0], 0.0);
            // Empty common/luxury count as fully satisfied (recorded as 1.0).
            assert_eq!(pop.records.tier_sat[1], 1.0);
            assert!(pop.sentiment.anger() > 0.0);
            assert!(pop.sentiment.fear() > 0.0);
            assert!(pop.sentiment.is_valid());
        }

        #[test]
        fn applies_desire_sentiment_flat_and_skips_growth_and_bonus() {
            let mut pop = make_pop();
            let mut desire = make_desire(
                0,
                DesireTarget::new(100, DesireTargetType::Consume, 1.0),
                10.0,
            );
            desire.satisfaction = 10.0; // fully sat once
            desire.effect.push(DesireEffect::SentimentFlat(
                SentimentKind::Hope,
                0.20,
                true,
            ));
            desire.effect.push(DesireEffect::Birthrate(0.5, true));
            desire.effect.push(DesireEffect::BonusGood(300, 99.0, true));
            pop.desires[0].push(desire);

            let history = make_default_market_history();
            pop.update_sentiments(&history);

            // Bonus +0.20 hope * sat 1.0, plus small baseline hope from luxury empty=1.
            assert!(pop.sentiment.hope() > 0.15);
            assert!(pop.property.get(&300).is_none()); // bonus good not granted here
            assert!(pop.sentiment.is_valid());
        }

        #[test]
        fn drains_mood_stored_effects_keeps_bonus_goods() {
            let mut pop = make_pop();
            pop.stored_effects.push(PopEffect::Satisfaction {
                tier: 1,
                amount: 2.0,
            });
            pop.stored_effects.push(PopEffect::SentimentFlat {
                kind: SentimentKind::Anger,
                delta: 0.05,
            });
            pop.stored_effects.push(PopEffect::BonusGood {
                good: 300,
                amount: 2.0,
            });

            let history = make_default_market_history();
            pop.update_sentiments(&history);

            assert!(pop.sentiment.anger() > 0.0);
            assert_eq!(pop.stored_effects.len(), 1);
            assert!(matches!(
                pop.stored_effects[0],
                PopEffect::BonusGood {
                    good: 300,
                    amount: 2.0
                }
            ));
            assert!(pop.sentiment.is_valid());
        }

        #[test]
        fn desire_satisfaction_boosts_common_ratio_may_exceed_one() {
            let mut pop = make_pop();
            let mut d1 = make_desire(
                0,
                DesireTarget::new(200, DesireTargetType::Consume, 1.0),
                10.0,
            );
            d1.satisfaction = 8.0; // ratio 0.8
            let mut donor = make_desire(
                1,
                DesireTarget::new(201, DesireTargetType::Consume, 1.0),
                10.0,
            );
            donor.satisfaction = 10.0; // ratio 1.0
            // +0.5 ratio-mass when fully satisfied.
            donor.effect.push(DesireEffect::Satisfaction(0.5, true));
            pop.desires[1].push(d1);
            pop.desires[1].push(donor);

            let history = make_default_market_history();
            pop.update_sentiments(&history);

            // Per-desire values unchanged.
            assert_eq!(pop.desires[1][0].satisfaction, 8.0);
            assert_eq!(pop.desires[1][1].satisfaction, 10.0);
            // Sum of success rates + boost: 0.8 + 1.0 + 0.5 = 2.3 (not averaged).
            assert!((pop.records.tier_sat[1] - 2.3).abs() < 1e-9);
            assert!((pop.records.satisfaction_units_total - 18.0).abs() < 1e-9);
        }

        #[test]
        fn desire_satisfaction_boosts_luxury_allows_oversat() {
            let mut pop = make_pop();
            let mut lux = make_desire(
                0,
                DesireTarget::new(300, DesireTargetType::Consume, 1.0),
                10.0,
            );
            lux.satisfaction = 10.0; // ratio 1.0
            lux.effect.push(DesireEffect::Satisfaction(0.5, true));
            pop.desires[2].push(lux);

            let history = make_default_market_history();
            pop.update_sentiments(&history);

            // Per-desire unchanged; recorded sum = 1.0 + 0.5 = 1.5.
            assert_eq!(pop.desires[2][0].satisfaction, 10.0);
            assert!((pop.records.tier_sat[2] - 1.5).abs() < 1e-9);
        }

        #[test]
        fn stored_satisfaction_boosts_named_tier() {
            let mut pop = make_pop();
            let mut common = make_desire(
                0,
                DesireTarget::new(200, DesireTargetType::Consume, 1.0),
                10.0,
            );
            common.satisfaction = 5.0; // ratio 0.5
            pop.desires[1].push(common);
            pop.stored_effects.push(PopEffect::Satisfaction {
                tier: 1,
                amount: 0.3, // success-rate mass
            });

            let history = make_default_market_history();
            pop.update_sentiments(&history);

            // Desire unchanged; recorded sum = 0.5 + 0.3 = 0.8.
            assert_eq!(pop.desires[1][0].satisfaction, 5.0);
            assert!((pop.records.tier_sat[1] - 0.8).abs() < 1e-9);
            assert!(pop
                .stored_effects
                .iter()
                .all(|e| !matches!(e, PopEffect::Satisfaction { .. })));
        }

        #[test]
        fn records_property_wealth_amv() {
            let mut pop = make_pop();
            pop.property.insert(100, PopPRow::new(10.0));
            pop.property.insert(101, PopPRow::new(5.0));
            let mut history = make_default_market_history();
            history.prices.insert(100, 2.0);
            history.prices.insert(101, 4.0);
            // 10*2 + 5*4 = 40; per household count 10 → 4.0
            pop.update_sentiments(&history);
            assert!((pop.records.wealth_amv - 40.0).abs() < 1e-9);
        }

        #[test]
        fn records_satisfaction_units_total() {
            let mut pop = make_pop();
            let mut d = make_desire(
                0,
                DesireTarget::new(100, DesireTargetType::Consume, 1.0),
                10.0,
            );
            d.satisfaction = 7.0;
            pop.desires[0].push(d);
            let history = make_default_market_history();
            pop.update_sentiments(&history);
            assert!((pop.records.satisfaction_units_total - 7.0).abs() < 1e-9);
        }

        #[test]
        fn common_oversat_uses_half_weight_above_one() {
            // common_sat_mood_weight(1.2) = 1.0 + 0.5*0.2 = 1.1
            assert!((Pop::common_sat_mood_weight(0.5) - 0.5).abs() < 1e-9);
            assert!((Pop::common_sat_mood_weight(1.0) - 1.0).abs() < 1e-9);
            assert!((Pop::common_sat_mood_weight(1.2) - 1.1).abs() < 1e-9);
        }
    }

    mod decay_goods_should {
        use super::*;
        use crate::game::good::{Good, GoodTag};

        fn good_with_decay(
            id: usize,
            decay_rate: f64,
            decay_result: HashMap<usize, f64>,
        ) -> Good {
            Good {
                id,
                name: format!("good_{id}"),
                class: None,
                decay_rate,
                decay_result,
                tags: HashSet::new(),
                categories: vec![],
            }
        }

        #[test]
        fn returns_used_then_decays_quantity_with_byproducts() {
            let mut pop = make_pop();
            // 10 on hand + 5 used → 15 before rate decay at 0.2 → lose 3, keep 12.
            // Byproduct 200 at 0.5 of lost → 1.5.
            pop.property.insert(
                100,
                PopPRow::new(10.0).with_used(5.0),
            );

            let mut factuals = Factuals::new();
            factuals.goods.insert(
                100,
                good_with_decay(100, 0.2, HashMap::from([(200, 0.5)])),
            );
            factuals.goods.insert(200, good_with_decay(200, 0.0, HashMap::new()));

            pop.decay_goods(&factuals);

            assert_eq!(pop.property[&100].used, 0.0);
            assert!((pop.property[&100].quantity - 12.0).abs() < 1e-9);
            assert!((pop.property[&200].quantity - 1.5).abs() < 1e-9);
        }

        #[test]
        fn consumed_decays_fully_with_byproducts() {
            let mut pop = make_pop();
            // Quantity already reduced at consume time; consumed holds 8 for 100% decay.
            pop.property.insert(
                100,
                PopPRow::new(2.0).with_consumed(8.0),
            );

            let mut factuals = Factuals::new();
            factuals.goods.insert(
                100,
                good_with_decay(100, 0.0, HashMap::from([(200, 1.0)])),
            );
            factuals.goods.insert(200, good_with_decay(200, 0.0, HashMap::new()));

            pop.decay_goods(&factuals);

            assert_eq!(pop.property[&100].consumed, 0.0);
            assert_eq!(pop.property[&100].quantity, 2.0); // rate 0, stock untouched
            assert_eq!(pop.property[&200].quantity, 8.0);
        }

        #[test]
        fn exposure_goods_skip_quantity_decay_while_owned() {
            let mut pop = make_pop();
            pop.property.insert(100, PopPRow::new(10.0));

            let mut factuals = Factuals::new();
            let mut g = good_with_decay(100, 1.0, HashMap::from([(200, 1.0)]));
            g.tags.insert(GoodTag::Exposure);
            factuals.goods.insert(100, g);

            pop.decay_goods(&factuals);

            assert_eq!(pop.property[&100].quantity, 10.0);
            assert!(!pop.property.contains_key(&200));
        }

        #[test]
        fn grants_bonus_goods_scaled_by_tiers_satisfied() {
            let mut pop = make_pop();
            let mut desire = make_desire(
                0,
                DesireTarget::new(100, DesireTargetType::Consume, 1.0),
                10.0,
            );
            desire.satisfaction = 20.0; // 2.0 tiers
            desire.effect.push(DesireEffect::BonusGood(300, 4.0, true));
            pop.desires[0].push(desire);

            let mut factuals = Factuals::new();
            factuals.goods.insert(100, good_with_decay(100, 0.0, HashMap::new()));

            pop.decay_goods(&factuals);

            // 4.0 * 2.0 tiers = 8.0
            assert_eq!(pop.property[&300].quantity, 8.0);
        }

        #[test]
        fn leaves_saved_target_unchanged_when_stock_decays() {
            let mut pop = make_pop();
            pop.property.insert(
                100,
                PopPRow::new(10.0).with_saved(10.0),
            );

            let mut factuals = Factuals::new();
            factuals.goods.insert(
                100,
                good_with_decay(100, 0.5, HashMap::new()),
            );

            pop.decay_goods(&factuals);

            assert_eq!(pop.property[&100].quantity, 5.0);
            // saved is a wish target; shortfall remains for mood / planning.
            assert_eq!(pop.property[&100].saved, 10.0);
        }

        #[test]
        fn applies_bonus_goods_from_stored_effects() {
            let mut pop = make_pop();
            pop.stored_effects.push(PopEffect::BonusGood {
                good: 300,
                amount: 7.5,
            });
            pop.stored_effects.push(PopEffect::BonusGood {
                good: 301,
                amount: 2.0,
            });

            let factuals = Factuals::new();
            pop.decay_goods(&factuals);

            assert_eq!(pop.property[&300].quantity, 7.5);
            assert_eq!(pop.property[&301].quantity, 2.0);
            assert_eq!(pop.stored_effects.len(), 0);
            
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
