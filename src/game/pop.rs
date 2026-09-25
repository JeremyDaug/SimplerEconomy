use std::{collections::HashMap};

use bevy::platform::collections::HashSet;

use crate::game::{
    desire::{Desire, DesireEffect, DesireSource, DesireTarget}, factuals::Factuals, good::GoodTag,
    household::DemographicRates, market::{Market, MarketHistory},
    marketorder::MarketOrder, scalingfactor::ScalingFactor,
    sentiment::Sentiment,
};

pub use crate::game::effects::PopEffect;
pub use crate::game::pop_property::{
    BuyStopReason, DemoRow, PopPRow, PopRecords,
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

    /// Same-day deferred effects (environment, events, process spillover, …).
    /// Growth arms → [`Self::growth_phase`]; 
    /// mood/sentiment/satisfaction → [`Self::update_sentiments`]; 
    /// [`PopEffect::BonusGood`] → [`Self::decay_goods`].
    pub stored_effects: Vec<PopEffect>,

    /// Political / social feeling of this pop (shares sum to 1).
    /// Updated in [`Self::update_sentiments`]; blendable into firms, markets, etc.
    pub sentiment: Sentiment,

    /// End-of-day records including SOL Wealth, as well as setting loose financial 
    /// plans and targets. 
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
    /// 1. Update existing desires (amount, satisfaction, targets, effects, demo
    ///    priority) or drop ones whose demo no longer exists.
    /// 2. Add any new demo desires from the pop's species/culture/religion that are not
    ///    already present (scaled via `DemoDesire::create_desire`).
    /// 3. Scale property `shop_target` / `desire_needs` for population growth.
    /// 4. Per tier: sort with `Desire::cmp_order`, then bake `priority` to index.
    pub fn update_desires(&mut self, factuals: &Factuals) {
        todo!()
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

    /// # Current Excess AMV
    /// 
    /// Returns the total AMV value of goods this pop holds above their individual targets.
    /// This is the "excess" they can offer in trade to fund purchases.
    pub fn current_excess_value(&self, market_history: &MarketHistory) -> f64 {
        let mut excess: f64 = 0.0;
        for (good, prop) in &self.property {
            let surplus = prop.available();
            if surplus > 0.001 {
                excess += surplus * market_history.prices.get(good).unwrap_or(&0.0);
            }
        }
        excess
    }

    /// # Take Good
    ///
    /// Removes this good's property row and returns the on-hand quantity.
    /// Returns 0.0 if the good was not held.
    pub fn take_good(&mut self, good: usize) -> f64 {
        self.property.remove(&good).map(|row| row.quantity).unwrap_or(0.0)
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
        self.records.previous_growth = self.demographics.household.count - old_count;
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
        let mut kept = Vec::with_capacity(self.stored_effects.len());
        for effect in self.stored_effects.drain(..) {
            match effect {
                PopEffect::Birthrate(v) => {
                    debug_assert!(v.is_finite(), "Stored birthrate must be finite.");
                    mods.birth_per_woman += v;
                }
                PopEffect::Mortality(target, v) => {
                    debug_assert!(v.is_finite(), "Stored mortality must be finite.");
                    mods.apply_mortality(target, v);
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
                    DesireEffect::Mortality(target, v, true) => {
                        mods.apply_mortality(*target, v * sat);
                    }
                    DesireEffect::Mortality(target, v, false) => {
                        // Malus: lack of sat raises mortality on the targeted group.
                        mods.apply_mortality(*target, v * lack);
                    }
                    _ => {}
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

    /// # Property Wealth AMV
    /// 
    /// AMV of on-hand property: `Sum(quantity * price)`.
    /// Missing prices default to `1.0` (same convention as order costing).
    pub fn property_wealth_amv(&self, market_history: &MarketHistory) -> f64 {
        let mut total = 0.0;
        for (good_id, row) in &self.property {
            debug_assert!(row.quantity.is_finite(), "Property quantity must be finite.");
            let price = market_history.price(*good_id);
            debug_assert!(price.is_finite(), "Market price must be finite.");
            total += row.quantity * price;
        }
        total
    }

    /// # Property Liquid Wealth
    /// 
    /// Spendable wealth: `Sum(qty * price * salability)` for tradeable goods.
    /// Missing prices default to `1.0`. Missing salability defaults to
    /// [`crate::game::config::market_constants::SALABILITY_DEFAULT`].
    pub fn property_liquid_wealth(&self, market_history: &MarketHistory, factuals: &Factuals) -> f64 {
        let mut total = 0.0;
        for (good_id, row) in &self.property {
            debug_assert!(row.quantity.is_finite(), "Property quantity must be finite.");
            let good = factuals.find_good(*good_id);
            if good.tags.contains(&GoodTag::Untradeable) {
                continue;
            }
            let price = market_history.price(*good_id);
            let salability = market_history.salability(*good_id);
            debug_assert!(price.is_finite(), "Market price must be finite.");
            debug_assert!(salability.is_finite(), "Market salability must be finite.");
            total += row.quantity
                * price
                * market_history.salability(*good_id).min(0.1);
        }
        total
    }

    /// # Property Saved Wealth
    ///
    /// AMV of actual saved units: `Sum(saved() * price)`.
    /// Missing prices default to `1.0`.
    pub fn property_saved_wealth_amv(&self, market_history: &MarketHistory) -> f64 {
        let mut total = 0.0;
        for (good_id, row) in &self.property {
            let qty = row.saved();
            debug_assert!(qty.is_finite(), "Saved units must be finite.");
            if qty == 0.0 {
                continue;
            }
            let price = market_history.price(*good_id);
            debug_assert!(price.is_finite(), "Market price must be finite.");
            total += qty * price;
        }
        total
    }

    /// # Record Keeping
    /// 
    /// Record Keeping goes through the pop's work today and records the details for the
    /// day.
    /// 
    /// This also includes doing some reworking of the pop's financial planning and 
    /// targets.
    /// 
    /// This is not meant to clean up dead pops (Household.size < 1.0). That is for 
    /// the market and firm to ultimately handle.
    pub fn record_keeping(
        &mut self,
        factuals: &Factuals,
        market_history: &MarketHistory,
    ) {
        todo!()
    }

    /// AMV of goods consumed and used today. Missing prices default to `1.0`.
    pub fn property_consumption_amv(&self, market_history: &MarketHistory) -> f64 {
        let mut total = 0.0;
        for (good_id, row) in &self.property {
            let qty = row.consumed + row.used;
            debug_assert!(qty.is_finite(), "Consumed/used units must be finite.");
            if qty == 0.0 {
                continue;
            }
            let price = market_history.price(*good_id);
            debug_assert!(price.is_finite(), "Market price must be finite.");
            total += qty * price;
        }
        total
    }

    /// One full desire level in target units (`amount * cap / efficiency`).
    /// True when the tier is empty or every desire has at least one full level.
    fn tier_is_complete(desires: &[Desire]) -> bool {
        desires.is_empty()
            || desires.iter().all(|desire| desire.tiers_satisfied() >= 1.0)
    }

    /// One consume level: spend `on_hand` along `ordered_targets`, then split
    /// remaining sat equally across remaining buyable substitutes (respecting
    /// each target's cap). Mutates `on_hand`.
    fn consume_needs(
        desires: &[Desire],
        on_hand: &mut HashMap<usize, f64>,
        factuals: &Factuals,
    ) -> HashMap<usize, f64> {
        let mut need = HashMap::new();
        for desire in desires {
            if desire.amount <= 0.0 {
                continue;
            }
            let mut remaining = desire.amount;
            let targets: Vec<DesireTarget> = desire
                .ordered_targets()
                .into_iter()
                .cloned()
                .collect();
            let mut assigned: HashMap<usize, f64> = HashMap::new();
            for target in &targets {
                if remaining <= 0.0 {
                    break;
                }
                debug_assert!(
                    target.efficiency > 0.0,
                    "Desire target efficiency must be positive"
                );
                let have = on_hand.get(&target.good).copied().unwrap_or(0.0);
                if have <= 0.0 {
                    continue;
                }
                let cap_sat = desire.amount * target.cap;
                let used = assigned.get(&target.good).copied().unwrap_or(0.0);
                let sat = remaining
                    .min(cap_sat - used)
                    .min(have * target.efficiency)
                    .max(0.0);
                if sat <= 0.0 {
                    continue;
                }
                let take = sat / target.efficiency;
                *on_hand.entry(target.good).or_insert(0.0) -= take;
                *need.entry(target.good).or_insert(0.0) += take;
                *assigned.entry(target.good).or_insert(0.0) += sat;
                remaining -= sat;
            }
            loop {
                if remaining <= 1e-12 {
                    break;
                }
                let open: Vec<&DesireTarget> = targets
                    .iter()
                    .filter(|target| {
                        debug_assert!(
                            target.efficiency > 0.0,
                            "Desire target efficiency must be positive"
                        );
                        if !factuals.find_good(target.good).is_buyable() {
                            return false;
                        }
                        let cap_sat = desire.amount * target.cap;
                        let used = assigned.get(&target.good).copied().unwrap_or(0.0);
                        used + 1e-12 < cap_sat
                    })
                    .collect();
                if open.is_empty() {
                    break;
                }
                let share = remaining / open.len() as f64;
                for target in open {
                    let cap_sat = desire.amount * target.cap;
                    let used = assigned.get(&target.good).copied().unwrap_or(0.0);
                    let sat = share.min(cap_sat - used).max(0.0);
                    if sat <= 0.0 {
                        continue;
                    }
                    *need.entry(target.good).or_insert(0.0) += sat / target.efficiency;
                    *assigned.entry(target.good).or_insert(0.0) += sat;
                    remaining -= sat;
                }
            }
        }
        need
    }

    fn good_durability(factuals: &Factuals, good_id: usize) -> f64 {
        (1.0 - factuals.find_good(good_id).decay_rate).clamp(0.0, 1.0)
    }

    /// # Decay Goods
    ///
    /// Called at the very end of the day. Only external input is factuals (good 
    /// definitions: decay rate, byproducts, tags).
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
    /// Does not modify saved or reserved stock.
    /// 
    /// Returns `(decayed, volume)` per good. Volume is on-hand after `used`
    /// is returned, plus `consumed` (eaten stock does not count as rot).
    pub fn decay_goods(&mut self, factuals: &Factuals) -> HashMap<usize, (f64, f64)> {
        // Byproducts and bonus goods applied after the main pass so they do not
        // decay again the same day, and so we can insert missing property rows.
        let mut gains: HashMap<usize, f64> = HashMap::new();
        let mut rot: HashMap<usize, (f64, f64)> = HashMap::new();

        for (&good_id, row) in self.property.iter_mut() {
            // 1. Return used goods to quantity (consume had moved them out).
            if row.used != 0.0 {
                row.quantity += row.used;
                row.used = 0.0;
            }

            let volume = (row.quantity.max(0.0) + row.consumed.max(0.0)).max(0.0);

            // 2. Decay on-hand goods by the good's rate, excluding Exposure while owned.
            let good = factuals.find_good(good_id);
            let exposure = good.tags.contains(&GoodTag::Exposure);
            let mut lost = 0.0;
            if !exposure && good.decay_rate > 0.0 && row.quantity > 0.0 {
                lost = row.quantity * good.decay_rate;
                row.quantity -= lost;
                for (&byproduct, &ratio) in &good.decay_result {
                    if ratio != 0.0 && lost != 0.0 {
                        *gains.entry(byproduct).or_insert(0.0) += lost * ratio;
                    }
                }
            }
            if volume > 0.0 || lost > 0.0 {
                let entry = rot.entry(good_id).or_insert((0.0, 0.0));
                entry.0 += lost;
                entry.1 += volume;
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
        rot
    }
}

#[cfg(test)]
mod pop {
    use std::collections::{HashMap, HashSet};

    use crate::game::{
        desire::{Desire, DesireEffect, DesireSource, DesireTarget, DesireTargetType},
        factuals::Factuals,
        good::Good,
        household::{Household, HouseholdTarget},
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

    fn make_good(id: usize, name: String) -> Good {
        Good {
            id,
            name,
            class: None,
            decay_rate: 0.0,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 1.0,
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

    mod take_good_should {
        use super::*;

        #[test]
        fn returns_quantity_and_removes_the_row() {
            let mut pop = make_pop();
            pop.property.insert(100, PopPRow::new(2.3));
            pop.property.insert(101, PopPRow::new(5.0));
            assert!((pop.take_good(100) - 2.3).abs() < 1e-12);
            assert!(!pop.property.contains_key(&100));
            assert_eq!(pop.property[&101].quantity, 5.0);
        }

        #[test]
        fn returns_zero_when_the_good_is_not_held() {
            let mut pop = make_pop();
            assert_eq!(pop.take_good(100), 0.0);
        }
    }

    mod update_desires_should {
        use super::*;
        use crate::game::desire::DemoDesire;

        fn household_demo(id: usize, amount: f64, priority: isize, tier: usize) -> DemoDesire {
            DemoDesire::new(id)
                .with_amount(amount)
                .with_priority(priority)
                .with_tier(tier)
                .with_scalar(ScalingFactor::Household(1.0))
        }


        #[test]
        fn bakes_additive_culture_effect_with_households() {
            let demo = household_demo(1, 1.0, 0, 1)
                .with_effect(DesireEffect::Culture(0.5, true));
            let pop = make_pop();
            let desire = demo.create_desire(&pop, DesireSource::Culture(1, 0));
            assert_eq!(desire.effect, vec![DesireEffect::Culture(5.0, true)]);
        }

        #[test]
        fn leaves_birthrate_effect_unscaled() {
            let demo = household_demo(1, 1.0, 0, 0)
                .with_effect(DesireEffect::Birthrate(0.2, true));
            let pop = make_pop();
            let desire = demo.create_desire(&pop, DesireSource::Culture(1, 0));
            assert_eq!(desire.effect, vec![DesireEffect::Birthrate(0.2, true)]);
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
            assert!((pop.records.previous_growth - (pop.demographics.household.count - old_count)).abs() < 1e-12);
        }

        #[test]
        fn skips_dead_pop() {
            let mut pop = make_pop();
            let factuals = make_default_factuals();
            pop.demographics.household.count = 0.0;
            pop.demographics.household.adult = 0.0;
            pop.demographics.household.elder = 0.0;
            pop.demographics.household.child = 0.0;
            pop.records.previous_growth = 1.0;
            pop.growth_phase(&factuals);
            assert_eq!(pop.demographics.household.count, 0.0);
            assert_eq!(pop.records.previous_growth, 1.0); // unchanged
        }

        #[test]
        fn drains_stored_birthrate_and_mortality() {
            let mut pop = make_pop();
            let factuals = make_default_factuals();
            pop.stored_effects.push(PopEffect::Birthrate(0.01));
            pop.stored_effects
                .push(PopEffect::Mortality(HouseholdTarget::ADULT, 0.005));
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
            culture::Culture, household::DemographicRates, religion::Religion, species::Species,
        };

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


    mod property_saved_wealth_amv_should {
        use super::*;


        #[test]
        fn floors_negative_reserved_so_saved_is_leftover_qty() {
            let mut pop = make_pop();
            pop.property.insert(100, PopPRow::new(0.0).with_reserve(-20.0));
            let mut history = make_default_market_history();
            history.prices.insert(100, 2.0);
            let saved = pop.property_saved_wealth_amv(&history);
            assert!((saved - 0.0).abs() < 1e-9);
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
                mass: 1.0,
                volume: 1.0,
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
                PopPRow::new(10.0).with_save_target(10.0),
            );

            let mut factuals = Factuals::new();
            factuals.goods.insert(
                100,
                good_with_decay(100, 0.5, HashMap::new()),
            );

            pop.decay_goods(&factuals);

            assert_eq!(pop.property[&100].quantity, 5.0);
            // saved is a wish target; shortfall remains for mood / planning.
            assert_eq!(pop.property[&100].save_target, 10.0);
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

        #[test]
        fn reports_leftover_rot_against_leftover_plus_consumed() {
            let mut pop = make_pop();
            pop.property.insert(100, PopPRow::new(10.0).with_consumed(10.0));

            let mut factuals = Factuals::new();
            factuals.goods.insert(100, good_with_decay(100, 1.0, HashMap::new()));

            let rot = pop.decay_goods(&factuals);
            assert_eq!(rot[&100], (10.0, 20.0));
        }

        #[test]
        fn reports_zero_rot_when_only_consumed_remains() {
            let mut pop = make_pop();
            pop.property.insert(100, PopPRow::new(0.0).with_consumed(10.0));

            let mut factuals = Factuals::new();
            factuals.goods.insert(100, good_with_decay(100, 1.0, HashMap::new()));

            let rot = pop.decay_goods(&factuals);
            assert_eq!(rot[&100], (0.0, 10.0));
        }
    }
}
