use std::collections::{HashMap, HashSet};

use crate::game::{
    actor::Actor, deal::{DealMaker, DealResponse, ProposedDeal, SellerBook}, demographic_source::DemographicSource, desire::{Desire, DesireTargetType}, effects::ProcessEffect, factuals::Factuals, good::GoodTag, household::{DemographicRates, Household}, job::Job, market::{Market, MarketHistory}, marketorder::MarketOrder, scalingfactor::ScalingFactor, sentiment::Sentiment,
};

pub use crate::game::effects::PopEffect;
pub use crate::game::pop_property::{DemoRow, PopPRow, PopRecords};

/// A population slice: one household block, its goods, and its desires.
#[derive(Debug, Clone)]
pub struct Pop {
    pub id: usize,

    /// This pop's cottage work. The value is this pop's own plan; another pop
    /// with the same craft is a different job.
    pub job: Job,

    /// Goods on hand.
    pub property: HashMap<usize, PopPRow>,

    /// Desires grouped by tier: 0 basic, 1 common, 2 luxury.
    pub desires: Vec<Vec<Desire>>,

    /// Species, culture, class id, and religion for this slice.
    pub demographics: DemoRow,

    /// Same-day effects waiting for growth or decay.
    pub stored_effects: Vec<PopEffect>,

    /// Mood shares. How they change other behavior is not wired.
    pub sentiment: Sentiment,

    /// Day-end records. The struct is a placeholder.
    pub records: PopRecords,

    /// Where [`Pop::satisfy`] stopped. [`Pop::satisfy_continue`] walks from here.
    satisfy_cursor: Option<SatisfyCursor>,
}

/// How far holding-value cost may run past face credit when satisfaction rose.
const LOSS_LIMIT: f64 = 4.0;

/// Satisfaction one basket side places on a single desire.
struct EndUse {
    tier: usize,
    order: usize,
    satisfaction: f64,
}

/// Bookmark for one satisfaction walk.
///
/// `target_index` is into `desire.target` as stored. The walk picks that
/// target at random. `target_start_satisfaction` is the desire's satisfaction
/// when that target began receiving goods, so a resume does not spend its cap twice.
#[derive(Debug, Clone, Copy)]
struct SatisfyCursor {
    tier: usize,
    desire_index: usize,
    target_index: usize,
    target_start_satisfaction: Option<f64>,
    iter_target: f64,
}

impl SatisfyCursor {
    fn start() -> Self {
        Self {
            tier: 0,
            desire_index: 0,
            target_index: 0,
            target_start_satisfaction: None,
            iter_target: 1.0,
        }
    }
}

/// Result of reserving goods for one desire.
enum SatisfyProgress {
    Done,
    Blocked {
        target_index: usize,
        target_start_satisfaction: f64,
    },
}

impl Pop {
    /// Empty pop. Three desire tiers, no stock, no satisfaction bookmark.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            job: Job::none(),
            property: HashMap::new(),
            desires: vec![vec![]; 3],
            demographics: DemoRow {
                household: Household::new(),
                species: 0,
                culture: 0,
                class: 0,
                religion: 0,
            },
            stored_effects: vec![],
            sentiment: Sentiment::new(),
            records: PopRecords::default(),
            satisfy_cursor: None,
        }
    }

    /// Emigration pressure. Not written yet.
    pub fn calculate_migratory_pressure(&mut self, factuals: &Factuals, _region: &Market) {
        let _ = (self, factuals);
        todo!("Pop calculate migratory pressure")
    }

    /// Job-to-job moves inside the same market. Not written yet.
    pub fn process_internal_migration(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Pop process internal migration")
    }

    /// Resolves a [`ScalingFactor`] against this pop's household.
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
    /// Adds the day's generated goods, each scaled by the scaling factor given.
    ///
    /// `new_goods` is pairs of good id and scaling factor. Each amount is this
    /// pop's factor for that scaling, added to `quantity`. A missing row is
    /// created.
    ///
    /// Returns each good id and the amount added, in the same order.
    pub fn start_day(&mut self, new_goods: &[(usize, ScalingFactor)]) -> Vec<(usize, f64)> {
        let mut added = Vec::with_capacity(new_goods.len());
        for (good_id, scaling) in new_goods {
            let amount = self.get_scaling_factor(*scaling);
            self.property
                .entry(*good_id)
                .and_modify(|row| row.quantity += amount)
                .or_insert(PopPRow::new(amount));
            added.push((*good_id, amount));
        }
        added
    }

    /// Adds species, culture, and religion desires that this pop does not already have.
    ///
    /// Does not shop, and does not rewrite satisfaction. Culture and religion id
    /// `0` are skipped. Class desires are not supported yet.
    /// Day-end size matching is [`Self::rescale_desires`].
    pub fn update_desires(&mut self, factuals: &Factuals) {
        let mut existing = HashSet::new();
        for tier in &self.desires {
            for desire in tier {
                existing.insert((desire.source, desire.demo_desire_id));
            }
        }
        self.add_missing_demographic_desires(factuals, &existing);
    }

    fn add_missing_demographic_desires(
        &mut self,
        factuals: &Factuals,
        existing: &HashSet<(DemographicSource, usize)>,
    ) {
        if let Some(species) = factuals.species.get(&self.demographics.species) {
            for demo in species.desires.values() {
                let source = DemographicSource::Species(species.id);
                if !existing.contains(&(source, demo.id)) {
                    let tier = demo.tier;
                    let desire = demo.create_desire(self, source);
                    debug_assert!(tier < self.desires.len(), "Desire tier out of range.");
                    self.desires[tier].push(desire);
                }
            }
        }

        if self.demographics.culture != 0 {
            if let Some(culture) = factuals.cultures.get(&self.demographics.culture) {
                for demo in culture.desires.values() {
                    let source = DemographicSource::Culture(culture.id);
                    if !existing.contains(&(source, demo.id)) {
                        let tier = demo.tier;
                        let desire = demo.create_desire(self, source);
                        debug_assert!(tier < self.desires.len(), "Desire tier out of range.");
                        self.desires[tier].push(desire);
                    }
                }
            }
        }

        if self.demographics.religion != 0 {
            if let Some(religion) = factuals.religion.get(&self.demographics.religion) {
                for demo in religion.desires.values() {
                    let source = DemographicSource::Religion(religion.id);
                    if !existing.contains(&(source, demo.id)) {
                        let tier = demo.tier;
                        let desire = demo.create_desire(self, source);
                        debug_assert!(tier < self.desires.len(), "Desire tier out of range.");
                        self.desires[tier].push(desire);
                    }
                }
            }
        }
    }

    /// # Apply Craft
    ///
    /// Gives this pop's job the processes of its craft that it does not already run.
    ///
    /// `factuals` supplies this job's effective craft. A line already on the
    /// job is left alone. Craft `0`, or a craft the world does not have, adds
    /// nothing.
    pub fn apply_craft(&mut self, factuals: &Factuals) {
        let processes = factuals.craft_processes(
            self.job.craft,
            self.demographics.culture,
            self.demographics.religion,
        );
        self.job.ensure_lines(&processes);
    }

    /// # Complexity Cost
    ///
    /// This pop's complexity cost for its job against the effective craft.
    ///
    /// `factuals` resolves the craft from the job's craft id and this pop's
    /// culture and religion, and supplies the craft-distance weight. The
    /// job's lines are the processes measured. No base craft returns `1.0`.
    pub fn complexity_cost(&self, factuals: &Factuals) -> f64 {
        let Some(craft) = factuals.effective_craft(
            self.job.craft,
            self.demographics.culture,
            self.demographics.religion,
        ) else {
            return 1.0;
        };
        self.job
            .complexity_cost(&craft, factuals.config.pop.craft_distance)
    }

    /// Removes this good's row and returns the on-hand quantity.
    pub fn take_good(&mut self, good: usize) -> f64 {
        self.property
            .remove(&good)
            .map(|row| row.quantity)
            .unwrap_or(0.0)
    }

    /// # Satisfy
    ///
    /// Reserves on-hand goods for desires and records that as satisfaction.
    ///
    /// Basic, then common, then luxury. Basic and common stop after one full
    /// level. Luxury repeats one level at a time, and a new level starts only
    /// after every luxury desire has reached the current one.
    ///
    /// Within a desire, one target is chosen at random from the bucket. It is
    /// capped at `amount * cap`. The walk stops if that target cannot be taken
    /// in full. That partial reserve is kept. Later desires are left untouched.
    ///
    /// Claimed units are added to `reserved` and stay in `quantity`.
    ///
    /// Starts at the first basic desire. A stop is recorded for
    /// [`Pop::satisfy_continue`].
    ///
    /// Returns a copy of the desire that stopped the walk. `None` means basic
    /// and common are at one level and no luxury desire is waiting on another.
    pub fn satisfy(&mut self, rng: &mut dyn rand::RngCore) -> Option<Desire> {
        self.satisfy_from(SatisfyCursor::start(), rng)
    }

    /// # Satisfy Continue
    ///
    /// Reserves goods from the desire and target [`Pop::satisfy`] stopped on.
    ///
    /// The open target keeps the satisfaction it had already recorded, and only
    /// the cap still left on that target can be reserved. With no bookmark,
    /// this starts at the first basic desire, same as [`Pop::satisfy`].
    pub fn satisfy_continue(&mut self, rng: &mut dyn rand::RngCore) -> Option<Desire> {
        let cursor = self.satisfy_cursor.unwrap_or_else(SatisfyCursor::start);
        self.satisfy_from(cursor, rng)
    }

    /// Walk from `cursor` until a target cannot be filled or every open level is done.
    fn satisfy_from(&mut self, mut cursor: SatisfyCursor, rng: &mut dyn rand::RngCore) -> Option<Desire> {
        debug_assert!(
            self.desires.len() >= 3,
            "pop desires must have basic, common, and luxury tiers"
        );
        loop {
            if cursor.tier >= 3 {
                self.satisfy_cursor = None;
                return None;
            }
            if cursor.tier == 2 && self.desires[2].is_empty() {
                self.satisfy_cursor = None;
                return None;
            }
            if let Some(blocked) = self.satisfy_tier_from(&mut cursor, rng) {
                self.satisfy_cursor = Some(cursor);
                return Some(blocked);
            }
            if cursor.tier < 2 {
                cursor.tier += 1;
                cursor.desire_index = 0;
                cursor.target_index = 0;
                cursor.target_start_satisfaction = None;
                cursor.iter_target = if cursor.tier == 2 {
                    self.next_luxury_level()
                } else {
                    1.0
                };
            } else {
                cursor.iter_target += 1.0;
                cursor.desire_index = 0;
                cursor.target_index = 0;
                cursor.target_start_satisfaction = None;
            }
        }
    }

    /// The luxury level a fresh pass should open: one above the lowest desire.
    fn next_luxury_level(&self) -> f64 {
        let Some(min_level) = self.desires[2]
            .iter()
            .map(Desire::tiers_satisfied)
            .reduce(f64::min)
        else {
            return 1.0;
        };
        debug_assert!(min_level.is_finite(), "luxury satisfaction must be finite");
        min_level.floor() + 1.0
    }

    /// # Satisfy Tier
    ///
    /// Reserves `tier` toward `iter_target` full levels, in list order.
    ///
    /// Stops at the first desire that cannot reach the target. Returns a copy
    /// of that desire. `None` means every desire in the tier reached it.
    /// Does not move the bookmark used by [`Pop::satisfy_continue`].
    pub fn satisfy_tier(
        &mut self,
        tier: usize,
        iter_target: f64,
        rng: &mut dyn rand::RngCore,
    ) -> Option<Desire> {
        let mut cursor = SatisfyCursor {
            tier,
            desire_index: 0,
            target_index: 0,
            target_start_satisfaction: None,
            iter_target,
        };
        self.satisfy_tier_from(&mut cursor, rng)
    }

    /// Reserves the tier at `cursor`, starting at its desire and target.
    ///
    /// On a stop, `cursor` is updated to that desire and target.
    fn satisfy_tier_from(
        &mut self,
        cursor: &mut SatisfyCursor,
        rng: &mut dyn rand::RngCore,
    ) -> Option<Desire> {
        let mut desires = std::mem::take(&mut self.desires[cursor.tier]);
        let mut blocked = None;
        let mut resume = Some((cursor.target_index, cursor.target_start_satisfaction));
        for (index, desire) in desires.iter_mut().enumerate().skip(cursor.desire_index) {
            if desire.tiers_satisfied() + 1e-9 >= cursor.iter_target {
                continue;
            }
            let (target_index, target_start) = if index == cursor.desire_index {
                resume.take().unwrap_or((0, None))
            } else {
                (0, None)
            };
            match self.satisfy_one_desire(
                desire,
                cursor.iter_target,
                target_index,
                target_start,
                rng,
            ) {
                SatisfyProgress::Done => {}
                SatisfyProgress::Blocked {
                    target_index,
                    target_start_satisfaction,
                } => {
                    cursor.desire_index = index;
                    cursor.target_index = target_index;
                    cursor.target_start_satisfaction = Some(target_start_satisfaction);
                    blocked = Some(desire.clone());
                    break;
                }
            }
        }
        self.desires[cursor.tier] = desires;
        blocked
    }

    /// # Satisfy One Desire
    ///
    /// Reserves free stock (`quantity - reserved`) toward `iter_target` levels.
    ///
    /// A resumed target is finished first. Further targets are picked at random
    /// from the bucket. A target that cannot be taken in full stops the desire,
    /// after reserving whatever of that target is free.
    fn satisfy_one_desire(
        &mut self,
        desire: &mut Desire,
        iter_target: f64,
        target_index: usize,
        target_start_satisfaction: Option<f64>,
        rng: &mut dyn rand::RngCore,
    ) -> SatisfyProgress {
        debug_assert!(desire.amount > 0.0, "desire amount must be positive");
        debug_assert!(
            iter_target.is_finite() && iter_target > 0.0,
            "iteration target must be positive"
        );
        let mut remaining = iter_target * desire.amount - desire.satisfaction;
        if remaining <= 1e-9 {
            return SatisfyProgress::Done;
        }
        // Targets whose cap share this call already took in full.
        let mut filled = vec![false; desire.target.len()];
        if let Some(start) = target_start_satisfaction {
            if let Some(blocked) = self.fill_target(desire, target_index, Some(start), &mut remaining)
            {
                return blocked;
            }
            if let Some(done) = filled.get_mut(target_index) {
                *done = true;
            }
        }
        while remaining > 1e-9 {
            let open: Vec<usize> = (0..desire.target.len())
                .filter(|&index| !filled[index] && target_cap_left(desire, index, None) > 1e-9)
                .collect();
            if open.is_empty() {
                return SatisfyProgress::Blocked {
                    target_index: desire.target.len(),
                    target_start_satisfaction: desire.satisfaction,
                };
            }
            let index = open[crate::game::util::random_index(rng, open.len())];
            if let Some(blocked) = self.fill_target(desire, index, None, &mut remaining) {
                return blocked;
            }
            filled[index] = true;
        }
        if desire.tiers_satisfied() + 1e-9 >= iter_target {
            SatisfyProgress::Done
        } else {
            SatisfyProgress::Blocked {
                target_index: desire.target.len(),
                target_start_satisfaction: desire.satisfaction,
            }
        }
    }

    /// Reserve one bucket target. `None` means its cap share was taken in full.
    fn fill_target(
        &mut self,
        desire: &mut Desire,
        index: usize,
        resumed_start: Option<f64>,
        remaining: &mut f64,
    ) -> Option<SatisfyProgress> {
        let Some(target) = desire.target.get(index).cloned() else {
            return Some(SatisfyProgress::Blocked {
                target_index: index,
                target_start_satisfaction: desire.satisfaction,
            });
        };
        if target.efficiency <= 0.0 || *remaining <= 1e-9 {
            return None;
        }
        let start_satisfaction = resumed_start.unwrap_or(desire.satisfaction);
        let cap_room = target_cap_left(desire, index, resumed_start);
        if cap_room <= 1e-9 {
            return None;
        }
        let needed = (*remaining).min(cap_room) / target.efficiency;
        if needed <= 1e-9 {
            return None;
        }
        let available = self
            .property
            .get(&target.good)
            .map(|row| row.available())
            .unwrap_or(0.0)
            .max(0.0);
        let take = needed.min(available);
        if take > 0.0 {
            if let Some(row) = self.property.get_mut(&target.good) {
                row.reserved += take;
                let sat_gained = take * target.efficiency;
                desire.satisfaction += sat_gained;
                *remaining -= sat_gained;
            }
        }
        if needed - take > 1e-9 {
            Some(SatisfyProgress::Blocked {
                target_index: index,
                target_start_satisfaction: start_satisfaction,
            })
        } else {
            None
        }
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
    ///   until no further progress can be made with remaining goods. A desire that
    ///   misses the current level is benched and keeps its place in the tier.
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
        self.consume_tier(&mut working_desires); // satisfy them
        self.desires.insert(0, working_desires); // put back

        // second do common needs, only one pass needed.
        working_desires = self.desires.remove(1); // pop off
        self.consume_tier(&mut working_desires); // satisfy
        self.desires.insert(1, working_desires); // put back

        // Last is Luxury Needs, do until we produce no more satisfaction.
        let mut iter_target = 1.0;
        let mut luxury = std::mem::take(&mut self.desires[2]);
        // Desires still filling. One that misses `iter_target` is benched in place.
        let mut filling = vec![true; luxury.len()];
        while filling.iter().any(|&open| open) {
            for (desire, open) in luxury.iter_mut().zip(filling.iter_mut()) {
                if !*open {
                    continue;
                }
                self.consume_one_desire(desire);
                if desire.tiers_satisfied() < iter_target {
                    *open = false;
                }
            }
            iter_target += 1.0;
        }
        self.desires[2] = luxury; // put back
    }

    /// # Consume Tier
    /// 
    /// Takes a list of desires (presumably a tier) and tries to satisfy each desire
    /// in order.
    /// 
    /// Will consume desires for satisfaction.
    /// 
    /// Returns the highest success rate, useful for checking if any desire reached the 
    /// next full level.
    pub fn consume_tier(&mut self, desires: &mut Vec<Desire>) -> f64 {
        let mut success: f64 = 0.0;
        for desire in desires.iter_mut() {
            let result = self.consume_one_desire(desire);
            success = success.max(result);
        }
        success
    }

    /// # Consume One Desire
    /// 
    /// A helper which takes a single desire and tries to satisfy it to one level. It 
    /// returns final satisfaction level.
    pub(crate) fn consume_one_desire(&mut self, desire: &mut Desire) -> f64 {
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
            if let Some(row) = self.property.get_mut(&target.good) && row.quantity > 0.0 {
                // remaining (Capped at the cap amount of the desire) divided by 
                // efficiency is how much is needed.
                let needed = remaining.min(desire.amount * target.cap) / target.efficiency;
                let take = needed.min(row.quantity);

                // remove from quantity and reserve. Output made this morning
                // may not have been reserved, so the claim stops at zero.
                row.quantity -= take;
                row.reserved = (row.reserved - take).max(0.0);
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

    /// Applies structural demographic rates, then same-day birth and mortality
    /// effects, then [`Household::update`](crate::game::household::Household::update).
    ///
    /// Pops with household `count < 1` are left alone. Desire satisfaction does
    /// not change the rates. [`Self::rescale_desires`] matches desires to the
    /// size this returns.
    pub fn growth_phase(&mut self, factuals: &Factuals) {
        if self.demographics.household.count < 1.0 {
            return;
        }
        let mut rates = factuals.get_demographic_rates(self.demographics);
        rates = rates.add(&self.take_stored_growth_mods());
        self.demographics.household.update(&rates);
    }

    /// # Rescale Desires
    ///
    /// Matches each linked desire to this pop's current size.
    ///
    /// `factuals` supplies the demographic desire. Amount becomes that demo's
    /// amount times [`Self::get_scaling_factor`] of the desire's scalar.
    /// Additive effects take that same pop scale. Birth, mortality, sentiment,
    /// and satisfaction arms stay at the demographic values. Satisfaction
    /// already recorded is multiplied by `new_amount / old_amount`, so the
    /// fraction met stays. A desire with no stored demographic source is left
    /// alone. A zero old amount leaves satisfaction unchanged.
    pub fn rescale_desires(&mut self, factuals: &Factuals) {
        let scales: Vec<f64> = self
            .desires
            .iter()
            .flat_map(|tier| {
                tier.iter()
                    .map(|desire| self.get_scaling_factor(desire.scalar))
            })
            .collect();
        let mut index = 0;
        for tier in &mut self.desires {
            for desire in tier.iter_mut() {
                let pop_scale = scales[index];
                index += 1;
                let Some(demo) = factuals.source_demo_desire(desire) else {
                    continue;
                };
                let old_amount = desire.amount;
                desire.amount = demo.amount * pop_scale;
                desire.effect = demo.scaled_effects(pop_scale);
                if old_amount > 0.0 {
                    desire.satisfaction *= desire.amount / old_amount;
                }
            }
        }
    }

    /// Drains birth and mortality arms from `stored_effects`. Other arms stay.
    fn take_stored_growth_mods(&mut self) -> DemographicRates {
        let mut mods = DemographicRates::zero();
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

    /// # Reset Day
    ///
    /// Clears yesterday's satisfaction and same-day reserves.
    ///
    /// Each desire's satisfaction is set to 0. The satisfy bookmark is
    /// dropped. `reserved`, `fresh`, and `produced` on every property row
    /// are set to 0. Quantity, consumed, and used stay for today's walk and
    /// for decay. The job drops its claimed-input list and its shopping
    /// list. Line targets stay, so last night's plan is what this morning runs.
    pub fn reset_day(&mut self) {
        self.satisfy_cursor = None;
        for tier in &mut self.desires {
            for desire in tier.iter_mut() {
                desire.satisfaction = 0.0;
            }
        }
        for row in self.property.values_mut() {
            row.reserved = 0.0;
            row.fresh = 0.0;
            row.produced = 0.0;
        }
        self.job.reset_day();
    }

    /// End-of-day bookkeeping. The record struct is empty, so this is a no-op.
    pub fn record_keeping(&mut self, _factuals: &Factuals, _history: &MarketHistory) {}

    /// # Plan
    ///
    /// Sets the job's targets for the next morning.
    ///
    /// `factuals` supplies processes. `history` is yesterday's market board.
    /// For each desire, the highest-efficiency target is the good to make.
    /// Units wanted are `amount / efficiency`, one full tier, summed when
    /// several desires share that good. Satisfaction is ignored because this
    /// runs after consumption and before the morning reset. Stock on hand is
    /// `quantity`. The job writes each line's target, then the complexity
    /// cost of those targets. The modifier is [`Self::complexity_cost`].
    pub fn plan(&mut self, factuals: &Factuals, history: &MarketHistory) {
        let wanted = self.output_wanted();
        let on_hand = self.quantities_on_hand();
        let modifier = self.complexity_cost(factuals);
        self.job.plan(&wanted, &on_hand, factuals, history, modifier);
    }

    /// # Output Wanted
    ///
    /// Units of each good that would fill one tier of the desires it best serves.
    ///
    /// Each desire contributes its highest-efficiency target with a positive
    /// efficiency. A tie keeps the later target. The units are
    /// `amount / efficiency`. Desires with no such target add nothing.
    fn output_wanted(&self) -> HashMap<usize, f64> {
        let mut wanted = HashMap::new();
        for tier in &self.desires {
            for desire in tier {
                if desire.amount <= 0.0 {
                    continue;
                }
                let Some(target) = desire
                    .target
                    .iter()
                    .filter(|target| target.efficiency > 0.0)
                    .max_by(|a, b| {
                        a.efficiency
                            .partial_cmp(&b.efficiency)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                else {
                    continue;
                };
                *wanted.entry(target.good).or_insert(0.0) += desire.amount / target.efficiency;
            }
        }
        wanted
    }

    /// # Quantities On Hand
    ///
    /// `quantity` of each good the pop holds, skipping empty rows.
    ///
    /// Reserve is included. The plan compares this with [`Pop::output_wanted`].
    fn quantities_on_hand(&self) -> HashMap<usize, f64> {
        self.property
            .iter()
            .filter(|(_, row)| row.quantity > 0.0)
            .map(|(&good, row)| (good, row.quantity))
            .collect()
    }

    /// # Store Process Effect
    ///
    /// Maps one [`ProcessEffect`] from the job onto `stored_effects`.
    ///
    /// Research, culture, faith, authority, and legitimacy keep their amounts.
    /// Birth and mortality become this pop's growth arms.
    fn store_process_effect(&mut self, effect: ProcessEffect) {
        self.stored_effects.push(match effect {
            ProcessEffect::Research(v) => PopEffect::Research(v),
            ProcessEffect::Culture(v) => PopEffect::Culture(v),
            ProcessEffect::Faith(v) => PopEffect::Faith(v),
            ProcessEffect::Authority(v) => PopEffect::Authority(v),
            ProcessEffect::Legitimacy(v) => PopEffect::Legitimacy(v),
            ProcessEffect::BirthRate(v) => PopEffect::Birthrate(v),
            ProcessEffect::MortalityRate(target, v) => PopEffect::Mortality(target, v),
        });
    }

    /// End-of-day decay.
    ///
    /// 1. Return `used` to `quantity`.
    /// 2. Decay the aging part of `quantity` by the good's rate.
    ///    [`PopPRow::fresh`] is left in place.
    ///    [`GoodTag::Exposure`] skips this while owned.
    /// 3. Destroy `consumed` outright and credit byproducts.
    /// 4. Pay [`PopEffect::BonusGood`] from `stored_effects` and drop those arms.
    ///
    /// Returns `(decayed, volume)` per good. Volume is on-hand after `used`
    /// returns, plus `consumed`, including fresh output. Eaten stock and
    /// fresh output are not counted as rot.
    pub fn decay_goods(&mut self, factuals: &Factuals) -> HashMap<usize, (f64, f64)> {
        let mut gains: HashMap<usize, f64> = HashMap::new();
        let mut rot: HashMap<usize, (f64, f64)> = HashMap::new();

        for (&good_id, row) in self.property.iter_mut() {
            if row.used != 0.0 {
                row.quantity += row.used;
                row.used = 0.0;
            }

            let volume = (row.quantity.max(0.0) + row.consumed.max(0.0)).max(0.0);
            let good = factuals.find_good(good_id);
            let exposure = good.tags.contains(&GoodTag::Exposure);
            // The fresh portion stays whole. Only the older pile rots.
            let fresh = row.fresh.max(0.0).min(row.quantity.max(0.0));
            let aging = (row.quantity - fresh).max(0.0);
            let mut lost = 0.0;
            if !exposure && good.decay_rate > 0.0 && aging > 0.0 {
                lost = aging * good.decay_rate;
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

            if row.consumed > 0.0 {
                let eaten = row.consumed;
                row.consumed = 0.0;
                for (&byproduct, &ratio) in &good.decay_result {
                    if ratio != 0.0 && eaten != 0.0 {
                        *gains.entry(byproduct).or_insert(0.0) += eaten * ratio;
                    }
                }
            }
        }

        for (good_id, amount) in gains {
            if amount == 0.0 {
                continue;
            }
            self.property
                .entry(good_id)
                .or_insert_with(|| PopPRow::new(0.0))
                .quantity += amount;
        }

        let mut kept = Vec::new();
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
                other => kept.push(other),
            }
        }
        self.stored_effects = kept;
        rot
    }

    /// # Calculate Standard of Living
    /// 
    /// Calculates the standard of living for the pop, based on the satisfaction of
    /// their desires (tieres satisfied, not straight units of satisfaction.)
    /// 
    /// If a pop has no desires, then we return +inf to denote that.
    /// 
    /// Currently, this is a direct summation of satisfaction from all desires, plus
    /// bonus satisfaction from the desire and pop effects.
    pub fn calculate_sol(&mut self, factuals: &Factuals) -> f64 {
        // sanity check, if a pop has no desires, then return +inf
        if self.desires.iter().all(|tier| tier.is_empty()) {
            self.records.satisfaction = f64::INFINITY;
            return f64::INFINITY;
        }
        let mut sum = 0.0;
        // get sum from desire satsifactino directly
        for tier in self.desires.iter() {
            for desire in tier {
                sum += desire.tiers_satisfied();
                sum += desire.get_bonus_satisfaction();
            }
        }

        // get satisfaction from pop effects.
        sum += self.satisfaction_from_pop_effects();
        self.records.satisfaction = sum;
        sum
    }

    /// # Satisfaction from Pop Effects
    /// 
    /// Extracts the bonus satisfaction the pop should gain from it's stored effects.
    /// 
    /// This currently does not take the satisfaction tier into account, just adding directly
    /// to the output instead.
    fn satisfaction_from_pop_effects(&mut self) -> f64 {
        let mut sum = 0.0;
        // collect satisfaction bonuses from effects and
        // remove bonus satisfaction from effects
        let mut kept = Vec::new();
        for effect in &self.stored_effects {
            if let PopEffect::Satisfaction { amount, .. } = *effect {
                sum += amount;
            } else {
                kept.push(*effect);
            }
        }
        self.stored_effects = kept;
        sum
    }

    /// Buy order for the desire [`Pop::satisfy`] stopped on.
    ///
    /// The good is that desire's current target. The amount is the whole units
    /// still needed to finish this level, and no more than that target's
    /// remaining cap. Payment is chosen later, when a seller's book is known.
    ///
    /// `None` when nothing is stopped on, or the gap is under one unit.
    fn buy_for_stopped_desire(&self) -> Option<MarketOrder> {
        let cursor = self.satisfy_cursor?;
        let desire = self.desires.get(cursor.tier)?.get(cursor.desire_index)?;
        let target = desire.target.get(cursor.target_index).cloned()?;
        if target.efficiency <= 0.0 {
            return None;
        }

        // Satisfaction already recorded against this target since it was opened.
        let already = cursor
            .target_start_satisfaction
            .map(|start| (desire.satisfaction - start).max(0.0))
            .unwrap_or(0.0);
        let cap_left = desire.amount * target.cap - already;
        let level_left = cursor.iter_target * desire.amount - desire.satisfaction;
        let units = (cap_left.min(level_left) / target.efficiency).floor();
        if units < 1.0 {
            return None;
        }
        Some(MarketOrder::buy(Actor::Pop(self.id), target.good, units))
    }

    /// Seller requests first, then the buyer's other free goods.
    ///
    /// A good that still feeds the lowest open tier is left out. Higher tiers
    /// can be spent on that tier. Free goods go highest monetary rating first,
    /// then lowest id. Reserved stock is already excluded by `available`.
    /// Transport is included here and skipped later unless the seller requested it.
    fn payment_goods(
        &self,
        book: &SellerBook,
        history: &MarketHistory,
        avoid: usize,
    ) -> Vec<usize> {
        let mut goods: Vec<usize> = book
            .requests
            .iter()
            .map(|order| order.target)
            .filter(|good| *good != avoid && !self.feeds_open_tier(*good))
            .collect();
        let mut extras: Vec<(usize, f64)> = self
            .property
            .iter()
            .filter(|(good, row)| {
                **good != avoid
                    && !goods.contains(*good)
                    && row.available().floor() >= 1.0
                    && !self.feeds_open_tier(**good)
            })
            .map(|(good, _)| {
                (
                    *good,
                    crate::game::market::monetary_rating(history.salability(*good)),
                )
            })
            .collect();
        extras.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        goods.extend(extras.into_iter().map(|(good, _)| good));
        goods
    }

    /// True when free units of `good` still feed the lowest tier that has room.
    ///
    /// Those goods are not sold and are not used as payment. A higher tier
    /// can be given in exchange for this one.
    fn feeds_open_tier(&self, good: usize) -> bool {
        let Some(floor) = self.desires.iter().enumerate().find_map(|(tier, desires)| {
            let open = desires.iter().any(|desire| {
                desire.target.iter().any(|target| {
                    if target.efficiency <= 0.0 {
                        return false;
                    }
                    let room = desire.amount * target.cap - desire.satisfaction;
                    (room / target.efficiency).floor() >= 1.0
                })
            });
            open.then_some(tier)
        }) else {
            return false;
        };
        self.end_uses(good, self.free_units(good))
            .iter()
            .any(|use_| use_.tier == floor)
    }

    /// Whole units of `good` placed on the earliest desire that still has room.
    fn end_uses(&self, good: usize, qty: f64) -> Vec<EndUse> {
        let mut left = if qty.is_finite() && qty > 0.0 {
            qty.floor()
        } else {
            0.0
        };
        let mut found = Vec::new();
        for (tier, desires) in self.desires.iter().enumerate() {
            for (order, desire) in desires.iter().enumerate() {
                if left < 1.0 {
                    return found;
                }
                let Some(target) = desire.target.iter().find(|target| target.good == good) else {
                    continue;
                };
                if target.efficiency <= 0.0 {
                    continue;
                }
                let room = desire.amount * target.cap - desire.satisfaction;
                if room <= 0.0 {
                    continue;
                }
                let take = (room / target.efficiency).floor().min(left);
                if take < 1.0 {
                    continue;
                }
                found.push(EndUse {
                    tier,
                    order,
                    satisfaction: take * target.efficiency,
                });
                left -= take;
            }
        }
        found
    }

    /// Pop verdict on giving `given` and receiving `received`.
    ///
    /// Both maps are positive quantities. Firms do not use this.
    fn exchange_ok(
        &self,
        given: &HashMap<usize, f64>,
        received: &HashMap<usize, f64>,
        history: &MarketHistory,
    ) -> bool {
        let mut given_uses = Vec::new();
        for (&good, &qty) in given {
            given_uses.extend(self.end_uses(good, qty));
        }
        let mut received_uses = Vec::new();
        for (&good, &qty) in received {
            received_uses.extend(self.end_uses(good, qty));
        }
        let best = received_uses.iter().min_by(|a, b| {
            a.tier.cmp(&b.tier).then(a.order.cmp(&b.order))
        });
        for use_ in &given_uses {
            let blocked = match best {
                None => true,
                Some(best) => {
                    use_.tier < best.tier
                        || (use_.tier == best.tier && use_.order < best.order)
                }
            };
            if blocked {
                return false;
            }
        }
        let direct = if let Some(best) = best {
            let gain: f64 = received_uses
                .iter()
                .filter(|use_| use_.tier == best.tier)
                .map(|use_| use_.satisfaction)
                .sum();
            let loss: f64 = given_uses
                .iter()
                .filter(|use_| use_.tier == best.tier)
                .map(|use_| use_.satisfaction)
                .sum();
            if gain <= loss {
                return false;
            }
            gain - loss
        } else {
            0.0
        };
        let cost: f64 = given
            .iter()
            .map(|(&good, &qty)| history.holding_per_unit(good) * qty)
            .sum();
        let mut credit = 0.0;
        for (&good, &qty) in received {
            if self.end_uses(good, qty).is_empty() {
                credit += history.holding_per_unit(good) * qty;
            } else {
                credit += history.price(good) * qty;
            }
        }
        if direct == 0.0 {
            credit > cost
        } else {
            cost <= credit * LOSS_LIMIT
        }
    }

    fn move_good(&mut self, good: usize, delta: f64) {
        let row = self.property.entry(good).or_insert_with(|| PopPRow::new(0.0));
        row.quantity = (row.quantity + delta).max(0.0);
    }

    /// # Move Fresh
    ///
    /// Adds `delta` to `fresh` for `good`.
    ///
    /// Creates the row when it is missing. The result is clamped at `0`.
    fn move_fresh(&mut self, good: usize, delta: f64) {
        if delta == 0.0 {
            return;
        }
        let row = self.property.entry(good).or_insert_with(|| PopPRow::new(0.0));
        row.fresh = (row.fresh + delta).max(0.0);
    }

    /// Buy transport the seller is offering when that lowers the unpaid freight.
    ///
    /// Returns `None` when freight remains and no further purchase helps.
    /// Returns `Some(())` when stock plus transport in the basket can pay it.
    fn buy_transport_for_freight(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        book: &SellerBook,
        goods: &mut HashMap<usize, f64>,
        covered: &mut f64,
    ) -> Option<()> {
        let free = self.free_stock();
        loop {
            if crate::game::deal::freight_shortfall(factuals, history.friction, goods, &free)
                <= 0.0
            {
                return Some(());
            }
            let before =
                crate::game::deal::freight_shortfall(factuals, history.friction, goods, &free);
            let mut bought = false;
            for order in &book.offers {
                let Some(row) = factuals.goods.get(&order.target) else {
                    continue;
                };
                if row.transport_efficiency() <= 0.0 {
                    continue;
                }
                let listed = (-order.target_amount).floor();
                let already = goods.get(&order.target).copied().unwrap_or(0.0).max(0.0);
                if already + 1.0 > listed {
                    continue;
                }
                let mut trial = goods.clone();
                *trial.entry(order.target).or_insert(0.0) += 1.0;
                let mut trial_covered = *covered;
                let price = history.holding_per_unit(order.target);
                if price <= 0.0
                    || !self.add_payment(
                        factuals,
                        history,
                        book,
                        order.target,
                        &mut trial,
                        &mut trial_covered,
                        price,
                    )
                {
                    continue;
                }
                let after =
                    crate::game::deal::freight_shortfall(factuals, history.friction, &trial, &free);
                if after >= before {
                    continue;
                }
                *goods = trial;
                *covered = trial_covered;
                bought = true;
                break;
            }
            if !bought {
                return None;
            }
        }
    }

    /// Add `need` more holding value of payment. Skips transport goods unless
    /// the seller requested them, so transport stock stays available for freight.
    ///
    /// `covered` is holding value already offered. Returns when `covered`
    /// reaches `covered + need`. A later step adds one more unit when that
    /// tie would leave a seller with no desires unmoved.
    fn add_payment(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        book: &SellerBook,
        avoid: usize,
        goods: &mut HashMap<usize, f64>,
        covered: &mut f64,
        need: f64,
    ) -> bool {
        let target = *covered + need;
        for good in self.payment_goods(book, history, avoid) {
            if *covered >= target {
                return true;
            }
            if self.skips_transport(factuals, book, good) {
                continue;
            }
            let per = history.holding_per_unit(good);
            if per <= 0.0 {
                continue;
            }
            let free = self.payable_units(book, goods, good);
            if free < 1.0 {
                continue;
            }
            let take = ((target - *covered) / per).ceil().min(free);
            if take < 1.0 {
                continue;
            }
            *goods.entry(good).or_insert(0.0) -= take;
            *covered += take * per;
        }
        *covered >= target
    }

    /// One more unit of the first payable good. Used to break a holding-value tie.
    fn add_one_payment(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        book: &SellerBook,
        avoid: usize,
        goods: &mut HashMap<usize, f64>,
        covered: &mut f64,
    ) -> bool {
        for good in self.payment_goods(book, history, avoid) {
            if self.skips_transport(factuals, book, good) {
                continue;
            }
            let per = history.holding_per_unit(good);
            if per <= 0.0 || self.payable_units(book, goods, good) < 1.0 {
                continue;
            }
            *goods.entry(good).or_insert(0.0) -= 1.0;
            *covered += per;
            return true;
        }
        false
    }

    fn skips_transport(&self, factuals: &Factuals, book: &SellerBook, good: usize) -> bool {
        let is_transport = factuals
            .goods
            .get(&good)
            .is_some_and(|row| row.is_transport());
        let requested = book.requests.iter().any(|order| order.target == good);
        is_transport && !requested
    }

    /// Free whole units of `good` still available to put in `goods`, capped
    /// by the seller's request when they asked for it.
    fn payable_units(&self, book: &SellerBook, goods: &HashMap<usize, f64>, good: usize) -> f64 {
        let already = goods.get(&good).copied().unwrap_or(0.0).min(0.0).abs();
        let mut free = self.free_units(good).floor() - already;
        if let Some(request) = book.requests.iter().find(|order| order.target == good) {
            free = free.min(request.target_amount.floor() - already).max(0.0);
        }
        free
    }

    /// Holding value of the positive side minus holding value of the payment.
    fn holding_shortfall(history: &MarketHistory, goods: &HashMap<usize, f64>) -> f64 {
        let mut cost = 0.0;
        let mut credit = 0.0;
        for (&good, &qty) in goods {
            let per = history.holding_per_unit(good);
            if qty > 0.0 {
                cost += per * qty;
            } else if qty < 0.0 {
                credit += per * -qty;
            }
        }
        cost - credit
    }

    /// Buyer side of the same verdict `evaluate` uses for the seller.
    fn buyer_accepts(&self, goods: &HashMap<usize, f64>, history: &MarketHistory) -> bool {
        let mut given = HashMap::new();
        let mut received = HashMap::new();
        for (&good, &qty) in goods {
            if qty > 0.0 {
                received.insert(good, qty);
            } else if qty < 0.0 {
                given.insert(good, -qty);
            }
        }
        self.exchange_ok(&given, &received, history)
    }

    fn free_stock(&self) -> HashMap<usize, f64> {
        self.property
            .iter()
            .map(|(&good, row)| (good, row.available().max(0.0)))
            .collect()
    }

    /// Spend transport the buyer holds after the basket has moved.
    ///
    /// Units spent are recorded as consumed.
    fn pay_freight(&mut self, amount: f64, factuals: &Factuals) {
        let mut ids: Vec<usize> = self.property.keys().copied().collect();
        ids.sort_unstable();
        let mut left = amount;
        for id in ids {
            if left <= 0.0 {
                break;
            }
            let good = factuals.find_good(id);
            let efficiency = good.transport_efficiency();
            if efficiency <= 0.0 {
                continue;
            }
            let free = self.free_units(id);
            if free <= 0.0 {
                continue;
            }
            let take = (left / efficiency).min(free);
            self.property
                .get_mut(&id)
                .expect("free stock vanished before it could be spent")
                .spend_aged_first(take);
            self.move_good(id, -take);
            self.property
                .get_mut(&id)
                .expect("free stock vanished before it could be spent")
                .consumed += take;
            left -= take * efficiency;
        }
    }
}

impl DealMaker for Pop {
    fn actor(&self) -> Actor {
        Actor::Pop(self.id)
    }

    fn sell_orders(&self, _history: &MarketHistory) -> Vec<MarketOrder> {
        let mut orders = Vec::new();
        for (&good, row) in &self.property {
            let units = row.available().floor();
            if units >= 1.0 && !self.feeds_open_tier(good) {
                orders.push(MarketOrder::sell(self.actor(), good, units));
            }
        }
        orders
    }

    fn buy_orders(&self, _history: &MarketHistory) -> Vec<MarketOrder> {
        let mut orders: Vec<MarketOrder> = self.buy_for_stopped_desire().into_iter().collect();
        for order in self.job.buy_orders(self.actor()) {
            if let Some(existing) = orders.iter_mut().find(|held| held.target == order.target) {
                existing.target_amount += order.target_amount;
            } else {
                orders.push(order);
            }
        }
        orders
    }

    fn free_units(&self, good: usize) -> f64 {
        self.property
            .get(&good)
            .map(|row| row.available().max(0.0))
            .unwrap_or(0.0)
    }

    /// # Fresh Share
    ///
    /// `good`'s [`PopPRow::fresh_share`], or `0` when this pop does not hold it.
    fn fresh_share(&self, good: usize) -> f64 {
        self.property
            .get(&good)
            .map(PopPRow::fresh_share)
            .unwrap_or(0.0)
    }

    fn propose(
        &self,
        match_good: usize,
        book: &SellerBook,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> Option<ProposedDeal> {
        let offer = book
            .offers
            .iter()
            .find(|order| order.target == match_good && order.target_amount < 0.0)?;
        // This meeting is for one good. Use the buy for that good: the open
        // desire, the job's input shortfall, or the two added together.
        let wanted = self
            .buy_orders(history)
            .into_iter()
            .find(|order| order.target == match_good)
            .map(|order| order.target_amount)?;
        if wanted < 1.0 {
            return None;
        }
        let qty = wanted.min((-offer.target_amount).floor());
        if qty < 1.0 {
            return None;
        }
        if history.price(match_good) == 0.0 {
            return None;
        }
        let mut goods = HashMap::from([(match_good, qty)]);
        let mut covered = 0.0;
        let need = history.holding_per_unit(match_good) * qty;
        if need > 0.0
            && !self.add_payment(
                factuals,
                history,
                book,
                match_good,
                &mut goods,
                &mut covered,
                need,
            )
        {
            return None;
        }
        self.buy_transport_for_freight(factuals, history, book, &mut goods, &mut covered)?;
        if Self::holding_shortfall(history, &goods) >= 0.0
            && !self.add_one_payment(
                factuals,
                history,
                book,
                match_good,
                &mut goods,
                &mut covered,
            )
        {
            return None;
        }
        if crate::game::deal::freight_shortfall(
            factuals,
            history.friction,
            &goods,
            &self.free_stock(),
        ) > 0.0
        {
            return None;
        }
        if !self.buyer_accepts(&goods, history) {
            return None;
        }
        Some(ProposedDeal {
            buyer: self.actor(),
            seller: book.seller,
            match_good,
            freight: crate::game::deal::freight_bill(factuals, history.friction, &goods),
            goods,
            fresh: HashMap::new(),
        })
    }

    fn evaluate(
        &self,
        proposal: &ProposedDeal,
        history: &MarketHistory,
        _factuals: &Factuals,
    ) -> DealResponse {
        let mut given = HashMap::new();
        let mut received = HashMap::new();
        for (&good, &qty) in &proposal.goods {
            if qty > 0.0 {
                given.insert(good, qty);
            } else if qty < 0.0 {
                received.insert(good, -qty);
            }
        }
        let match_qty = given.get(&proposal.match_good).copied().unwrap_or(0.0);
        if match_qty < 1.0 || received.is_empty() {
            return DealResponse::Reject;
        }
        for (&good, &qty) in &given {
            if self.free_units(good) < qty {
                return DealResponse::Reject;
            }
        }
        if self.exchange_ok(&given, &received, history) {
            DealResponse::Accept
        } else {
            DealResponse::Reject
        }
    }

    fn finalize(&mut self, proposal: &ProposedDeal, factuals: &Factuals) {
        let id = self.actor();
        let sign = if proposal.buyer == id {
            1.0
        } else if proposal.seller == id {
            -1.0
        } else {
            return;
        };
        for (&good, &qty) in &proposal.goods {
            let delta = sign * qty;
            let fresh_units = delta * proposal.fresh_share(good);
            // Aged and fresh both sit in quantity. fresh marks the spared share.
            self.move_good(good, delta);
            self.move_fresh(good, fresh_units);
            // Goods just received count against the job's shopping list.
            if sign > 0.0 && qty > 0.0 {
                self.job.note_purchase(good, qty);
            }
        }
        if proposal.buyer == id {
            self.pay_freight(proposal.freight, factuals);
        }
    }

    fn reevaluate(&mut self, history: &MarketHistory, rng: &mut dyn rand::RngCore) {
        let _ = history;
        if self.satisfy_cursor.is_some() {
            self.satisfy_continue(rng);
        }
    }

    fn reset_day(&mut self) {
        Pop::reset_day(self);
    }

    fn reserve(&mut self, factuals: &Factuals, rng: &mut dyn rand::RngCore) {
        // Desires claim first. The job takes only what is still free.
        self.satisfy(rng);
        self.apply_craft(factuals);
        self.job.reserve(&mut self.property, factuals);
    }

    fn produce(&mut self, factuals: &Factuals) {
        for effect in self.job.produce(&mut self.property, factuals) {
            self.store_process_effect(effect);
        }
    }

    fn consume(&mut self) {
        Pop::consume(self);
    }

    fn decay_goods(&mut self, factuals: &Factuals) -> HashMap<usize, (f64, f64)> {
        Pop::decay_goods(self, factuals)
    }

    fn record_keeping(&mut self, factuals: &Factuals, history: &MarketHistory) {
        Pop::record_keeping(self, factuals, history);
    }

    fn plan(&mut self, factuals: &Factuals, history: &MarketHistory) {
        Pop::plan(self, factuals, history);
    }
}

fn target_cap_left(desire: &Desire, index: usize, resumed_start: Option<f64>) -> f64 {
    let Some(target) = desire.target.get(index) else {
        return 0.0;
    };
    let already = resumed_start
        .map(|start| (desire.satisfaction - start).max(0.0))
        .unwrap_or(0.0);
    desire.amount * target.cap - already
}

#[cfg(test)]
mod pop {
    use std::collections::{HashMap, HashSet};

    use crate::game::actor::Actor;
    use crate::game::actors::Actors;
    use crate::game::deal::{DealMaker, DealResponse, MeetingOutcome, ProposedDeal};
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{DemoDesire, Desire, DesireEffect, DesireTarget, DesireTargetType};
    use crate::game::scalingfactor::ScalingFactor;
    use crate::game::species::Species;
    use crate::game::effects::PopEffect;
    use crate::game::craft::Craft;
    use crate::game::factuals::Factuals;
    use crate::game::market::{Market, MarketGood, MarketHistory};
    use crate::game::good::{Good, GoodTag};
    use crate::game::household::Household;
    use crate::game::job::{Job, JobLine};
    use crate::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
    use crate::game::process::{InputType, Process, ProcessEffect, ProcessInput, ProcessOutput};
    use crate::game::sentiment::Sentiment;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn do_satisfy(pop: &mut Pop) -> Option<crate::game::desire::Desire> {
        let mut rng = StdRng::seed_from_u64(1);
        pop.satisfy(&mut rng)
    }

    fn do_continue(pop: &mut Pop) -> Option<crate::game::desire::Desire> {
        let mut rng = StdRng::seed_from_u64(1);
        pop.satisfy_continue(&mut rng)
    }

    fn do_match(
        market: &mut Market,
        actors: &mut Actors,
        factuals: &Factuals,
    ) -> Vec<crate::game::deal::ProposedDeal> {
        let mut rng = StdRng::seed_from_u64(1);
        market
            .match_deals(actors, factuals, &mut rng)
            .into_iter()
            .filter(|meeting| meeting.outcome == MeetingOutcome::Accepted)
            .filter_map(|meeting| meeting.proposal)
            .collect()
    }

    /// Makes pop for testing
    /// no specific data attached, just boiler plate.
    fn make_pop() -> Pop {
        Pop {
            id: 1,
            job: Job::none(),
            property: HashMap::new(),
            desires: vec![vec![]; 3],
            demographics: DemoRow {
                household: Household::new(),
                species: 0,
                culture: 0,
                class: 0,
                religion: 0,
            },
            stored_effects: vec![],
            sentiment: Sentiment::new(),
            records: PopRecords::default(),
            satisfy_cursor: None,
        }
    }

    fn make_good(id: usize, name: &str, decay_rate: f64) -> Good {
        Good {
            id,
            name: name.to_string(),
            class: None,
            decay_rate,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: Vec::new(),
        }
    }

    #[test]
    fn decays_on_hand_quantity_and_destroys_consumed() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(10.0).with_consumed(4.0));
        let mut grain = make_good(1, "grain", 0.5);
        grain.decay_result.insert(2, 0.25);
        let factuals = Factuals::new()
            .with_good(grain)
            .with_good(make_good(2, "chaff", 0.0));

        let rot = pop.decay_goods(&factuals);

        assert!((pop.property[&1].quantity - 5.0).abs() < 1e-9);
        assert_eq!(pop.property[&1].consumed, 0.0);
        // 10 * 0.5 rot, plus the eaten 4, each yielding 0.25 chaff.
        assert!((pop.property[&2].quantity - 2.25).abs() < 1e-9);
        assert!((rot[&1].0 - 5.0).abs() < 1e-9);
    }

    #[test]
    fn decay_spares_todays_fresh() {
        let mut pop = make_pop();
        let mut row = PopPRow::new(10.0);
        row.fresh = 4.0;
        pop.property.insert(1, row);
        let factuals = Factuals::new().with_good(make_good(1, "bread", 0.5));

        let rot = pop.decay_goods(&factuals);

        // 6 aging units at half the rate. The 4 baked today stay.
        assert_eq!(pop.property[&1].quantity, 7.0);
        assert_eq!(rot[&1].0, 3.0);
        assert_eq!(rot[&1].1, 10.0);
    }

    #[test]
    fn job_reserve_keeps_the_input_off_sell_orders() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, Some(2.0), vec![])]);
        pop.property.insert(1, PopPRow::new(2.0));
        pop.property.insert(9, PopPRow::new(5.0));
        let factuals = Factuals::new().with_process(bake());

        pop.job.reserve(&mut pop.property, &factuals);
        let sells = pop.sell_orders(&MarketHistory::new());

        assert!(sells.iter().all(|order| order.target != 1));
        assert!(sells.iter().any(|order| order.target == 9));
    }

    #[test]
    fn job_buy_orders_cover_the_shortfall() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        pop.property.insert(1, PopPRow::new(1.0));
        let factuals = Factuals::new().with_process(bake());

        pop.job.reserve(&mut pop.property, &factuals);
        let orders = pop.buy_orders(&MarketHistory::new());

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 1);
        assert_eq!(orders[0].target_amount, 3.0);
    }

    #[test]
    fn buy_orders_add_the_job_shortfall_onto_the_open_desire() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        // The open desire wants 4 bread. The job also needs 2 bread it does not hold.
        pop.job = Job::new(
            1,
            vec![JobLine::new(
                8,
                Some(2.0),
                vec![],
            )],
        );
        let process = Process::new(8, "slice", 0)
            .with_input(ProcessInput::new(2, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(3, 1.0, true));
        let factuals = Factuals::new().with_process(process);

        do_satisfy(&mut pop);
        pop.job.reserve(&mut pop.property, &factuals);
        let orders = pop.buy_orders(&MarketHistory::new());

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 2);
        assert_eq!(orders[0].target_amount, 6.0);
    }

    #[test]
    fn plan_covers_one_full_tier_of_the_best_target() {
        let mut pop = make_pop();
        pop.job = Job::new(
            1,
            vec![
                JobLine::new(7, None, vec![]),
                JobLine::new(9, None, vec![]),
            ],
        );
        // Bread covers the meal fully. Grain is the worse substitute.
        // Satisfaction is already one tier; the plan still asks for the next morning.
        let mut meal = desire(
            1,
            vec![
                DesireTarget::new(2, DesireTargetType::Consume, 1.0),
                DesireTarget::new(1, DesireTargetType::Consume, 0.5),
            ],
            4.0,
        );
        meal.satisfaction = 4.0;
        pop.desires[0].push(meal);
        pop.property.insert(2, PopPRow::new(1.0));
        let factuals = Factuals::new()
            .with_process(bake())
            .with_process(
                Process::new(9, "eat grain", 0)
                    .with_output(ProcessOutput::new(1, 1.0, true)),
            );
        let mut history = MarketHistory::new();
        history.prices.insert(1, 0.0);

        pop.plan(&factuals, &history);

        // 4 bread wanted, 1 on hand. Grain is not the best target, and its price is 0.
        assert_eq!(pop.job.lines[0].target, Some(3.0));
        assert_eq!(pop.job.lines[1].target, Some(0.0));
    }

    #[test]
    fn produce_stores_process_effects_on_the_pop() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        pop.property.insert(1, PopPRow::new(1.0));
        let factuals = Factuals::new().with_process(bake().with_effect(ProcessEffect::Culture(2.0)));
        let mut rng = StdRng::seed_from_u64(1);

        DealMaker::reserve(&mut pop, &factuals, &mut rng);
        DealMaker::produce(&mut pop, &factuals);

        assert_eq!(pop.stored_effects, vec![PopEffect::Culture(2.0)]);
        assert_eq!(pop.property[&2].quantity, 1.0);
        assert_eq!(pop.property[&2].fresh, 1.0);
        assert_eq!(pop.property[&2].produced, 1.0);
    }

    #[test]
    fn complexity_cost_follows_the_culture_overlay() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(1, Some(0.0), vec![])]);
        pop.demographics.culture = 2;
        let base = Factuals::new().with_craft(
            Craft::new(1, "subsistence")
                .with_process(1)
                .with_complexity_modifier(0.4),
        );

        assert!((pop.complexity_cost(&base) - 0.4).abs() < 1e-12);

        let mut cultured = base.with_craft(
            Craft::new(1, "subsistence")
                .with_origin(Some(DemographicSource::Culture(2)))
                .with_complexity_modifier(0.5),
        );
        assert!((pop.complexity_cost(&cultured) - 0.2).abs() < 1e-12);

        pop.job.lines.push(JobLine::new(2, Some(0.0), vec![]));
        cultured.config.pop.craft_distance = 0.2;
        assert!((pop.complexity_cost(&cultured) - 0.4).abs() < 1e-12);

        pop.job.craft = 0;
        assert!((pop.complexity_cost(&cultured) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn pops_in_craft_groups_pops_doing_the_same_work() {
        let mut actors = Actors::new();
        let mut welsh = Pop::new(2);
        welsh.job = Job::new(4, vec![]);
        let mut english = Pop::new(1);
        english.job = Job::new(4, vec![]);
        let mut miller = Pop::new(3);
        miller.job = Job::new(5, vec![]);
        actors.pops.insert(2, welsh);
        actors.pops.insert(1, english);
        actors.pops.insert(3, miller);
        actors.pops.insert(9, Pop::new(9));

        assert_eq!(actors.pops_in_craft(4), vec![1, 2]);
        assert_eq!(actors.pops_in_craft(5), vec![3]);
        assert!(actors.pops_in_craft(0).is_empty());
        assert!(actors.pops_in_craft(6).is_empty());
    }

    #[test]
    fn market_day_applies_the_craft_then_runs_it() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(4, Some(1.0), vec![])]);
        pop.demographics.culture = 2;
        pop.demographics.religion = 3;
        pop.property.insert(1, PopPRow::new(4.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));

        let factuals = Factuals::new()
            .with_good(make_good(1, "grain", 0.0))
            .with_good(make_good(2, "bread", 0.0))
            .with_process(bake())
            .with_craft(Craft::new(1, "subsistence").with_process(7).with_process(8))
            .with_craft(
                Craft::new(1, "subsistence")
                    .with_origin(Some(DemographicSource::Culture(2)))
                    .with_remove(8)
                    .with_process(9),
            )
            .with_craft(
                Craft::new(1, "subsistence")
                    .with_origin(Some(DemographicSource::Religion(3)))
                    .with_process(8),
            );
        let mut market = Market::new(1);
        market.pops.insert(pop.id);
        let mut actors = Actors::new();
        actors.pops.insert(pop.id, pop);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let pop = actors.pop(1);
        let processes: Vec<usize> = pop.job.lines.iter().map(|line| line.process).collect();
        assert_eq!(processes, vec![4, 7, 9, 8]);
        assert_eq!(pop.job.lines[1].target, Some(4.0));
        assert_eq!(pop.property.get(&2).map(|row| row.quantity).unwrap_or(0.0), 0.0);

        market.market_day(&mut actors, &factuals, &mut rng);

        let pop = actors.pop(1);
        assert_eq!(pop.property[&1].quantity, 0.0);
        assert_eq!(pop.property[&2].quantity, 0.0);
        assert_eq!(pop.property[&2].fresh, 4.0);
        assert_eq!(pop.property[&2].produced, 4.0);
        assert_eq!(pop.desires[0][0].satisfaction, 4.0);
    }

    fn bake() -> Process {
        Process::new(7, "bake", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true))
    }

    fn desire(id: usize, targets: Vec<DesireTarget>, amount: f64) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: id,
            priority: 0,
            target: targets,
            amount,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    #[test]
    fn consume_keeps_luxury_list_order() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(3.0));
        pop.property.insert(2, PopPRow::new(1.0));
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            1.0,
        ));
        pop.desires[2].push(desire(
            2,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            1.0,
        ));

        pop.consume();

        let ids: Vec<usize> = pop.desires[2].iter().map(|d| d.demo_desire_id).collect();
        assert_eq!(ids, vec![1, 2]);
        assert!((pop.desires[2][0].satisfaction - 3.0).abs() < 1e-9);
        assert!((pop.desires[2][1].satisfaction - 1.0).abs() < 1e-9);
    }

    #[test]
    fn satisfy_stops_at_the_first_desire_it_cannot_finish() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(10.0));
        pop.property.insert(2, PopPRow::new(3.0));
        pop.property.insert(3, PopPRow::new(10.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            10.0,
        ));
        pop.desires[0].push(desire(
            2,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            10.0,
        ));
        pop.desires[1].push(desire(
            3,
            vec![DesireTarget::new(3, DesireTargetType::Use, 1.0)],
            10.0,
        ));

        let blocked = do_satisfy(&mut pop).expect("second basic desire is short");

        assert_eq!(blocked.demo_desire_id, 2);
        assert!((blocked.satisfaction - 3.0).abs() < 1e-9);
        assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
        assert!((pop.desires[0][1].satisfaction - 3.0).abs() < 1e-9);
        assert_eq!(pop.desires[1][0].satisfaction, 0.0);
        assert!((pop.property[&1].reserved - 10.0).abs() < 1e-9);
        assert!((pop.property[&2].reserved - 3.0).abs() < 1e-9);
        assert_eq!(pop.property[&3].reserved, 0.0);
        assert!((pop.property[&1].quantity - 10.0).abs() < 1e-9);
        assert_eq!(pop.property[&1].consumed, 0.0);
        assert_eq!(pop.property[&2].used, 0.0);
    }

    #[test]
    fn satisfy_stops_at_the_first_target_it_cannot_fill() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(3.0));
        pop.property.insert(2, PopPRow::new(100.0));
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(1, DesireTargetType::Consume, 2.0),
                DesireTarget::new(2, DesireTargetType::Consume, 1.0),
            ],
            10.0,
        ));

        match do_satisfy(&mut pop) {
            Some(blocked) => {
                assert!((blocked.satisfaction - 6.0).abs() < 1e-9);
                assert!((pop.property[&1].reserved - 3.0).abs() < 1e-9);
                assert_eq!(pop.property[&2].reserved, 0.0);
            }
            None => {
                assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
                assert_eq!(pop.property[&1].reserved, 0.0);
                assert!((pop.property[&2].reserved - 10.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn satisfy_continues_through_a_target_whose_cap_is_filled() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(100.0));
        pop.property.insert(2, PopPRow::new(100.0));
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(1, DesireTargetType::Use, 1.0).with_cap(0.5),
                DesireTarget::new(2, DesireTargetType::Consume, 1.0),
            ],
            10.0,
        ));

        assert!(do_satisfy(&mut pop).is_none());
        assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
        let reserved = pop.property[&1].reserved + pop.property[&2].reserved;
        assert!((reserved - 10.0).abs() < 1e-9);
    }

    #[test]
    fn satisfy_does_not_refill_a_capped_target() {
        // Seeds cover both pick orders, so a capped target picked first is
        // never picked again past its cap.
        for seed in 0..20 {
            let mut pop = make_pop();
            pop.property.insert(1, PopPRow::new(100.0));
            pop.property.insert(2, PopPRow::new(100.0));
            pop.desires[0].push(desire(
                1,
                vec![
                    DesireTarget::new(1, DesireTargetType::Use, 1.0).with_cap(0.5),
                    DesireTarget::new(2, DesireTargetType::Consume, 1.0).with_cap(0.5),
                ],
                10.0,
            ));

            let mut rng = StdRng::seed_from_u64(seed);
            assert!(pop.satisfy(&mut rng).is_none());
            assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
            assert!((pop.property[&1].reserved - 5.0).abs() < 1e-9);
            assert!((pop.property[&2].reserved - 5.0).abs() < 1e-9);
        }
    }

    #[test]
    fn satisfy_stops_when_every_cap_is_spent() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(100.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0).with_cap(0.5)],
            10.0,
        ));

        let blocked = do_satisfy(&mut pop).expect("one capped target cannot fill the level");
        assert!((blocked.satisfaction - 5.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 5.0).abs() < 1e-9);
    }

    #[test]
    fn satisfy_repeats_luxury_until_a_level_cannot_be_finished() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(15.0));
        pop.property.insert(2, PopPRow::new(15.0));
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            10.0,
        ));
        pop.desires[2].push(desire(
            2,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            10.0,
        ));

        let blocked = do_satisfy(&mut pop).expect("second luxury level runs out");

        assert_eq!(blocked.demo_desire_id, 1);
        assert!((pop.desires[2][0].satisfaction - 15.0).abs() < 1e-9);
        assert!((pop.desires[2][1].satisfaction - 10.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 15.0).abs() < 1e-9);
        assert!((pop.property[&2].reserved - 10.0).abs() < 1e-9);
    }

    #[test]
    fn satisfy_resumes_a_partial_desire() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(4.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            10.0,
        ));

        let blocked = do_satisfy(&mut pop).expect("stock is short of one level");
        assert!((blocked.satisfaction - 4.0).abs() < 1e-9);

        pop.property.get_mut(&1).unwrap().quantity += 6.0;
        assert!(do_satisfy(&mut pop).is_none());
        assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
        assert!((pop.property[&1].quantity - 10.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 10.0).abs() < 1e-9);
    }

    #[test]
    fn satisfy_continue_keeps_the_cap_already_spent() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(2.0));
        pop.property.insert(2, PopPRow::new(0.0));
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(1, DesireTargetType::Consume, 1.0).with_cap(0.5),
                DesireTarget::new(2, DesireTargetType::Consume, 1.0),
            ],
            10.0,
        ));

        let blocked = do_satisfy(&mut pop).expect("a target is short");
        let first = blocked.satisfaction;

        pop.property.get_mut(&1).unwrap().quantity += 100.0;
        pop.property.get_mut(&2).unwrap().quantity += 100.0;
        assert!(do_continue(&mut pop).is_none());
        assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
        if (first - 2.0).abs() < 1e-9 {
            assert!((pop.property[&1].reserved - 5.0).abs() < 1e-9);
            assert!((pop.property[&2].reserved - 5.0).abs() < 1e-9);
        } else {
            assert_eq!(pop.property[&1].reserved, 0.0);
            assert!((pop.property[&2].reserved - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn satisfy_continue_moves_on_after_the_open_desire() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(10.0));
        pop.property.insert(2, PopPRow::new(4.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            10.0,
        ));
        pop.desires[0].push(desire(
            2,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            10.0,
        ));

        let blocked = do_satisfy(&mut pop).expect("second desire is short");
        assert_eq!(blocked.demo_desire_id, 2);

        pop.property.get_mut(&1).unwrap().quantity += 50.0;
        pop.property.get_mut(&2).unwrap().quantity += 6.0;
        assert!(do_continue(&mut pop).is_none());
        assert!((pop.property[&1].reserved - 10.0).abs() < 1e-9);
        assert!((pop.property[&2].reserved - 10.0).abs() < 1e-9);
        assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
        assert!((pop.desires[0][1].satisfaction - 10.0).abs() < 1e-9);
    }

    #[test]
    fn match_deals_buys_the_open_desire_from_a_seller() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.property.insert(9, PopPRow::new(10.0));
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        do_satisfy(&mut buyer).expect("buyer still wants bread");

        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));
        market.goods.insert(9, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);

        let deals = do_match(&mut market, &mut actors, &Factuals::new());

        assert_eq!(deals.len(), 1);
        assert_eq!(deals[0].goods.get(&2), Some(&4.0));
        assert_eq!(deals[0].goods.get(&9), Some(&-9.0));
        assert!((actors.pops[&1].property[&2].quantity - 4.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&9].quantity - 1.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&2].quantity - 6.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&9].quantity - 9.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&2].reserved - 4.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - 2.0).abs() < 1e-12);
        assert!((market.goods[&9].amv - 1.0).abs() < 1e-12);
        assert!((market.goods[&2].traded - 4.0).abs() < 1e-9);
        assert_eq!(market.goods[&2].paid, 0.0);
        assert!((market.goods[&9].paid - 9.0).abs() < 1e-9);
    }

    #[test]
    fn match_deals_buys_bread_the_seller_does_not_use() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.property.insert(9, PopPRow::new(10.0));
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        do_satisfy(&mut buyer).expect("buyer wants bread");

        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));
        seller.desires[0].push(desire(
            2,
            vec![DesireTarget::new(3, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        do_satisfy(&mut seller).expect("seller wants tools");

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));
        market.goods.insert(9, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);

        let deals = do_match(&mut market, &mut actors, &Factuals::new());

        assert_eq!(deals.len(), 1);
        assert_eq!(deals[0].goods.get(&2), Some(&4.0));
        assert_eq!(deals[0].goods.get(&9), Some(&-9.0));
    }

    #[test]
    fn match_deals_pays_a_higher_tier_good_for_a_lower_tier_good() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.property.insert(4, PopPRow::new(10.0));
        buyer.property.insert(8, PopPRow::new(1.0));
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            1.0,
        ));
        buyer.desires[0].push(desire(
            2,
            vec![DesireTarget::new(4, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        buyer.desires[1].push(desire(
            4,
            vec![DesireTarget::new(6, DesireTargetType::Consume, 1.0)],
            1.0,
        ));
        buyer.desires[2].push(desire(
            3,
            vec![DesireTarget::new(8, DesireTargetType::Consume, 10.0)],
            10.0,
        ));
        do_satisfy(&mut buyer).expect("buyer wants bread");

        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.goods.insert(2, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.goods.insert(4, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market.goods.insert(8, MarketGood::new().with_amv(2.0).with_salability(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);

        let deals = do_match(&mut market, &mut actors, &Factuals::new());

        assert_eq!(deals.len(), 1);
        assert_eq!(deals[0].goods.get(&2), Some(&1.0));
        assert_eq!(deals[0].goods.get(&8), Some(&-1.0));
        assert!(deals[0].goods.get(&4).is_none());
        assert_eq!(actors.pops[&1].property[&4].quantity, 10.0);
        assert_eq!(actors.pops[&1].property[&8].quantity, 0.0);
        assert_eq!(actors.pops[&2].property[&8].quantity, 1.0);
        assert_eq!(actors.pops[&1].property[&2].quantity, 1.0);
    }

    #[test]
    fn match_deals_rejects_bread_that_still_feeds_a_later_desire() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.property.insert(9, PopPRow::new(10.0));
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        do_satisfy(&mut buyer).expect("buyer wants bread");

        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));
        seller.desires[0].push(desire(
            3,
            vec![DesireTarget::new(3, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        seller.desires[0].push(desire(
            2,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        do_satisfy(&mut seller).expect("seller is stuck on tools");

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));
        market.goods.insert(9, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);

        let deals = do_match(&mut market, &mut actors, &Factuals::new());

        assert!(deals.is_empty());
        assert!((actors.pops[&1].property[&9].quantity - 10.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&2].quantity - 10.0).abs() < 1e-9);
    }

    fn time_good() -> Good {
        let mut time = make_good(0, "time", 0.0);
        time.tags.insert(GoodTag::transport(1.0));
        time
    }

    #[test]
    fn match_deals_refuses_when_freight_cannot_be_covered() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.property.insert(9, PopPRow::new(10.0));
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        do_satisfy(&mut buyer).expect("buyer wants bread");

        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));
        market.goods.insert(9, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);

        let deals = do_match(&mut market, &mut actors, &Factuals::new().with_good(time_good()));

        assert!(deals.is_empty());
        assert!((actors.pops[&1].property[&9].quantity - 10.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&2].quantity - 10.0).abs() < 1e-9);
    }

    #[test]
    fn match_deals_spends_the_buyers_transport_inside_the_basket() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.property.insert(9, PopPRow::new(10.0));
        buyer.property.insert(0, PopPRow::new(5.0));
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        do_satisfy(&mut buyer).expect("buyer wants bread");

        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));
        market.goods.insert(9, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);

        let deals = do_match(&mut market, &mut actors, &Factuals::new().with_good(time_good()));

        assert_eq!(deals.len(), 1);
        assert!((deals[0].freight - 1.0).abs() < 1e-9);
        assert!(deals[0].goods.get(&0).is_none());
        assert!(actors.pops[&2].property.get(&0).is_none());
        assert!((actors.pops[&1].property[&0].quantity - 4.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&0].consumed - 1.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&9].quantity - 1.0).abs() < 1e-9);
    }

    #[test]
    fn match_deals_buys_transport_to_cover_freight() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.property.insert(9, PopPRow::new(20.0));
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        do_satisfy(&mut buyer).expect("buyer wants bread");

        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));
        seller.property.insert(0, PopPRow::new(4.0));

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.goods.insert(2, MarketGood::new().with_amv(2.0));
        market.goods.insert(9, MarketGood::new().with_amv(1.0));
        market.goods.insert(0, MarketGood::new().with_amv(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);

        let deals = do_match(&mut market, &mut actors, &Factuals::new().with_good(time_good()));

        assert_eq!(deals.len(), 1);
        assert_eq!(deals[0].goods.get(&0), Some(&1.0));
        assert!(actors.pops[&1].property.get(&0).is_none_or(|row| row.quantity < 1e-9));
        assert!((actors.pops[&1].property[&0].consumed - 1.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&0].quantity - 3.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&9].quantity - 10.0).abs() < 1e-9);
    }

    /// Yesterday's card. Each row is `(good, AMV, salability)`.
    fn card(rows: &[(usize, f64, f64)]) -> MarketHistory {
        let mut history = MarketHistory::new();
        for &(good, amv, salability) in rows {
            history.prices.insert(good, amv);
            history.salability.insert(good, salability);
        }
        history
    }

    /// A basket from the seller's side. Positive quantity is what the seller
    /// gives. Negative quantity is what the seller receives.
    fn proposal(match_good: usize, goods: HashMap<usize, f64>) -> ProposedDeal {
        ProposedDeal {
            buyer: Actor::Pop(1),
            seller: Actor::Pop(2),
            match_good,
            goods,
            fresh: HashMap::new(),
            freight: 0.0,
        }
    }

    #[test]
    fn finalize_keeps_the_givers_fresh_share() {
        let mut seller = make_pop();
        seller.id = 2;
        let mut bread = PopPRow::new(10.0);
        bread.fresh = 3.0;
        bread.produced = 3.0;
        seller.property.insert(2, bread);

        let mut buyer = make_pop();
        buyer.id = 1;
        let mut coin = PopPRow::new(10.0);
        coin.fresh = 5.0;
        coin.produced = 5.0;
        buyer.property.insert(9, coin);

        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut deal = proposal(2, HashMap::from([(2, 4.0), (9, -9.0)]));
        deal.stamp_fresh_shares(|actor, good| actors.get(actor).fresh_share(good));

        assert!((deal.fresh_share(2) - 0.3).abs() < 1e-12);
        assert!((deal.fresh_share(9) - 0.5).abs() < 1e-12);

        actors
            .pops
            .get_mut(&1)
            .expect("buyer")
            .finalize(&deal, &Factuals::new());
        actors
            .pops
            .get_mut(&2)
            .expect("seller")
            .finalize(&deal, &Factuals::new());

        let buyer = &actors.pops[&1];
        let seller = &actors.pops[&2];
        assert!((buyer.property[&2].quantity - 4.0).abs() < 1e-12);
        assert!((buyer.property[&2].fresh - 1.2).abs() < 1e-12);
        assert_eq!(buyer.property[&2].produced, 0.0);
        assert!((seller.property[&2].quantity - 6.0).abs() < 1e-12);
        assert!((seller.property[&2].fresh - 1.8).abs() < 1e-12);
        assert_eq!(seller.property[&2].produced, 3.0);
        assert!((buyer.property[&9].quantity - 1.0).abs() < 1e-12);
        assert!((buyer.property[&9].fresh - 0.5).abs() < 1e-12);
        assert_eq!(buyer.property[&9].produced, 5.0);
        assert!((seller.property[&9].quantity - 9.0).abs() < 1e-12);
        assert!((seller.property[&9].fresh - 4.5).abs() < 1e-12);
        assert_eq!(seller.property[&9].produced, 0.0);
    }

    /// Freight is own consumption, not a transfer. Aged stock is spent
    /// before fresh, and the aged remainder can still rot that night.
    ///
    /// 10 on hand, 4 fresh. Spending 4 leaves fresh at 4; all 4 came from
    /// the 6 aged. Decay at half the rate then takes 1 off the 2 aged units
    /// left. Spending 8 instead leaves fresh at 2. Produced stays.
    #[test]
    fn freight_spends_aged_stock_before_fresh() {
        let mut time_good = make_good(0, "time", 0.5);
        time_good.set_transport_efficiency(1.0);
        let factuals = Factuals::new().with_good(time_good);

        let mut buyer = make_pop();
        let mut time = PopPRow::new(10.0);
        time.fresh = 4.0;
        time.produced = 4.0;
        buyer.property.insert(0, time);
        let mut deal = proposal(2, HashMap::new());
        deal.freight = 4.0;

        buyer.finalize(&deal, &factuals);

        assert!((buyer.property[&0].quantity - 6.0).abs() < 1e-12);
        assert!((buyer.property[&0].consumed - 4.0).abs() < 1e-12);
        assert!((buyer.property[&0].fresh - 4.0).abs() < 1e-12);
        assert_eq!(buyer.property[&0].produced, 4.0);

        let rot = buyer.decay_goods(&factuals);

        assert!((buyer.property[&0].quantity - 5.0).abs() < 1e-9);
        assert!((buyer.property[&0].fresh - 4.0).abs() < 1e-9);
        assert!((rot[&0].0 - 1.0).abs() < 1e-9);
        assert!((rot[&0].1 - 10.0).abs() < 1e-9);

        let mut heavy = make_pop();
        let mut time = PopPRow::new(10.0);
        time.fresh = 4.0;
        time.produced = 4.0;
        heavy.property.insert(0, time);
        let mut deal = proposal(2, HashMap::new());
        deal.freight = 8.0;

        heavy.finalize(&deal, &factuals);

        assert!((heavy.property[&0].quantity - 2.0).abs() < 1e-12);
        assert!((heavy.property[&0].consumed - 8.0).abs() < 1e-12);
        assert!((heavy.property[&0].fresh - 2.0).abs() < 1e-12);
        assert_eq!(heavy.property[&0].produced, 4.0);
    }

    #[test]
    fn fresh_share_caps_when_fresh_exceeds_quantity() {
        let mut row = PopPRow::new(10.0);
        row.fresh = 15.0;
        assert!((row.fresh_share() - 1.0).abs() < 1e-12);

        row.fresh = 0.0;
        assert_eq!(row.fresh_share(), 0.0);

        let mut empty = PopPRow::new(0.0);
        empty.fresh = 4.0;
        assert_eq!(empty.fresh_share(), 0.0);
    }

    /// Seller gives 4 of good 2 and receives 9 of good 9.
    ///
    /// Good 2 still fills the seller's basic desire, 4 satisfaction at
    /// efficiency 1. Good 9 feeds no desire. At salability 1 the payment is
    /// a holding gain, cost 8 against credit 9, which would be enough if the
    /// seller wanted nothing. A good that still feeds a desire is given up
    /// when the goods received feed that desire or an earlier one. Good 9
    /// feeds nothing, so the seller rejects.
    #[test]
    fn evaluate_rejects_a_good_that_still_feeds_the_open_desire() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));
        seller.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let history = card(&[(2, 2.0, 1.0), (9, 1.0, 1.0)]);
        let deal = proposal(2, HashMap::from([(2, 4.0), (9, -9.0)]));
        assert_eq!(
            seller.evaluate(&deal, &history, &Factuals::new()),
            DealResponse::Reject
        );
    }

    /// Both goods feed basic desires. Good 2 is earlier in the tier than good 9.
    /// The seller gives 4 of good 9, which is 4 satisfaction.
    ///
    /// On the tier of the best good received, satisfaction gained has to
    /// exceed satisfaction given up. Receiving 2 of good 2 puts 2 satisfaction
    /// in place of 4, so the seller rejects. Receiving 5 puts 5 in place of 4,
    /// so the seller accepts. Giving the later good is allowed because good 2
    /// feeds the earlier desire on that same tier.
    #[test]
    fn evaluate_requires_same_tier_satisfaction_to_increase() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(9, PopPRow::new(10.0));
        seller.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            10.0,
        ));
        seller.desires[0].push(desire(
            2,
            vec![DesireTarget::new(9, DesireTargetType::Consume, 1.0)],
            10.0,
        ));
        let history = card(&[(2, 1.0, 1.0), (9, 1.0, 1.0)]);
        // 4 satisfaction given up, 2 gained.
        let short = proposal(9, HashMap::from([(9, 4.0), (2, -2.0)]));
        assert_eq!(
            seller.evaluate(&short, &history, &Factuals::new()),
            DealResponse::Reject
        );
        // 4 satisfaction given up, 5 gained.
        let ahead = proposal(9, HashMap::from([(9, 4.0), (2, -5.0)]));
        assert_eq!(
            seller.evaluate(&ahead, &history, &Factuals::new()),
            DealResponse::Accept
        );
    }

    /// No desires at first. Good 2 is AMV 2 and good 9 is AMV 1, both at
    /// salability 1, so holding value equals face AMV.
    ///
    /// With no satisfaction change, holding credit has to be strictly above
    /// holding cost. Giving 4 of good 2 costs 8. Receiving 8 of good 9
    /// credits 8, a tie, so the seller rejects. Receiving 9 credits 9, a
    /// gain, so the seller accepts.
    ///
    /// Good 2 then feeds a basic desire. The seller gives 100 of good 9
    /// (holding cost 100, no desire on it) and receives 1 of good 2
    /// (satisfaction rises by 1, and a desired good is credited at face
    /// AMV, so credit is 2). A satisfaction gain may cost up to 4 times
    /// that credit. 100 is past 8, so the seller rejects.
    #[test]
    fn evaluate_rejects_a_holding_tie_and_a_loss_past_the_limit() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));
        seller.property.insert(9, PopPRow::new(100.0));
        let history = card(&[(2, 2.0, 1.0), (9, 1.0, 1.0)]);
        // Holding 8 for 8.
        let tie = proposal(2, HashMap::from([(2, 4.0), (9, -8.0)]));
        assert_eq!(
            seller.evaluate(&tie, &history, &Factuals::new()),
            DealResponse::Reject
        );
        // Holding 8 for 9.
        let cleared = proposal(2, HashMap::from([(2, 4.0), (9, -9.0)]));
        assert_eq!(
            seller.evaluate(&cleared, &history, &Factuals::new()),
            DealResponse::Accept
        );

        seller.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        // Satisfaction +1, holding cost 100 against a limit of 8.
        let fortune = proposal(9, HashMap::from([(9, 100.0), (2, -1.0)]));
        assert_eq!(
            seller.evaluate(&fortune, &history, &Factuals::new()),
            DealResponse::Reject
        );
    }

    /// No desires. The seller gives 1 of good 9 (AMV -0.1) and receives 1 of
    /// good 8 (AMV -0.2). Both salabilities are 0.1.
    ///
    /// A negative AMV is the cost of holding the good. Salability would
    /// shrink a positive AMV, and it leaves a negative AMV at its face
    /// value. Holding moves from -0.1 to -0.2. With no satisfaction change
    /// that is a loss, so the seller rejects.
    #[test]
    fn evaluate_does_not_soften_a_negative_amv() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(9, PopPRow::new(4.0));
        let history = card(&[(9, -0.1, 0.1), (8, -0.2, 0.1)]);
        let deal = proposal(9, HashMap::from([(9, 1.0), (8, -1.0)]));
        assert_eq!(
            seller.evaluate(&deal, &history, &Factuals::new()),
            DealResponse::Reject
        );
    }

    /// No desires. The seller gives 4 of good 2 (AMV 2, salability 1) and
    /// receives 5 of good 9 (AMV 1, salability 2).
    ///
    /// Salability of 1 or more prices a positive AMV at face, so good 9's
    /// salability of 2 leaves its holding value at 1. Holding falls from 8
    /// to 5. With no satisfaction change the seller rejects that loss.
    #[test]
    fn evaluate_rejects_an_amv_loss_when_salability_is_at_least_par() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(4.0));
        let history = card(&[(2, 2.0, 1.0), (9, 1.0, 2.0)]);
        let deal = proposal(2, HashMap::from([(2, 4.0), (9, -5.0)]));
        assert_eq!(
            seller.evaluate(&deal, &history, &Factuals::new()),
            DealResponse::Reject
        );
    }

    /// No desires. The seller gives 1 of good 2 (AMV 1) and receives 1 of
    /// good 9 (AMV 2). At face value the seller gains. Salability below 1
    /// multiplies a positive AMV down to a floor of 0.05.
    ///
    /// Both goods at salability 1 keep holding equal to face AMV, cost 1
    /// against credit 2, so the seller accepts. Good 9 at salability 0.4
    /// has holding credit 0.8 against a cost of 1, so the seller rejects.
    /// Good 2 at 0.5 and good 9 at 0.2 make the cost 0.5 and the credit 0.4,
    /// so the seller rejects again.
    #[test]
    fn evaluate_rejects_a_face_gain_that_salability_discounts_away() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));
        let deal = proposal(2, HashMap::from([(2, 1.0), (9, -1.0)]));
        // Face gain, and salability is high enough to keep it.
        let at_par = card(&[(2, 1.0, 1.0), (9, 2.0, 1.0)]);
        assert_eq!(
            seller.evaluate(&deal, &at_par, &Factuals::new()),
            DealResponse::Accept
        );
        // Same quantities. The received good's salability cuts its holding below the cost.
        let received_illiquid = card(&[(2, 1.0, 1.0), (9, 2.0, 0.4)]);
        assert_eq!(
            seller.evaluate(&deal, &received_illiquid, &Factuals::new()),
            DealResponse::Reject
        );
        // Both below par, and the dearer good is discounted harder: 0.4 against 0.5.
        let both_illiquid = card(&[(2, 1.0, 0.5), (9, 2.0, 0.2)]);
        assert_eq!(
            seller.evaluate(&deal, &both_illiquid, &Factuals::new()),
            DealResponse::Reject
        );
    }

    /// The seller gives 1 of good 8 and receives 1 of good 2. Good 8 is a
    /// luxury desire, and one unit yields 10 satisfaction. Good 2 is a basic
    /// desire, and one unit yields 1. Both AMVs are 1 at salability 1.
    ///
    /// A higher tier may be given for a lower tier. Satisfaction is compared
    /// on the basic tier only: the seller gains 1 there and gives up 0. The
    /// luxury's 10 satisfaction is a higher tier, so it stays out of that
    /// comparison. Holding cost is 1, and a satisfaction gain may cost up to
    /// 4 times the basic good's face credit of 1, so the seller accepts.
    #[test]
    fn evaluate_accepts_a_higher_tier_good_for_a_lower_tier_good() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(8, PopPRow::new(1.0));
        seller.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            1.0,
        ));
        seller.desires[2].push(desire(
            2,
            vec![DesireTarget::new(8, DesireTargetType::Consume, 10.0)],
            10.0,
        ));
        let history = card(&[(2, 1.0, 1.0), (8, 1.0, 1.0)]);
        let deal = proposal(8, HashMap::from([(8, 1.0), (2, -1.0)]));
        assert_eq!(
            seller.evaluate(&deal, &history, &Factuals::new()),
            DealResponse::Accept
        );
    }

    /// The seller gives 1 of good 9 and receives 1 of good 2. Both AMVs are
    /// 1 at salability 1, so holding value is unchanged.
    ///
    /// With no desires the exchange is a holding tie, and a tie is rejected.
    /// A basic desire for good 2 is then added. The seller gains 1
    /// satisfaction from good 2 and gives up none, because good 9 feeds no
    /// desire. That gain allows a holding cost up to 4 times good 2's face
    /// credit, and a cost of 1 is inside the limit, so the seller accepts.
    #[test]
    fn evaluate_accepts_satisfaction_gained_when_none_is_lost() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(9, PopPRow::new(4.0));
        let history = card(&[(2, 1.0, 1.0), (9, 1.0, 1.0)]);
        let deal = proposal(9, HashMap::from([(9, 1.0), (2, -1.0)]));
        // No desire on either good: holding 1 for 1.
        assert_eq!(
            seller.evaluate(&deal, &history, &Factuals::new()),
            DealResponse::Reject
        );
        seller.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        // Good 2 now gains 1 satisfaction. Good 9 still feeds nothing.
        assert_eq!(
            seller.evaluate(&deal, &history, &Factuals::new()),
            DealResponse::Accept
        );
    }

    /// One basic desire is half met and carries a satisfaction bonus of 6,
    /// so it contributes 0.5 tiers and 3 bonus. One common desire is fully
    /// met and carries a malus of 2, so it contributes 1 tier and no malus.
    /// A culture effect on the common desire is not satisfaction.
    #[test]
    fn standard_of_living_sums_tiers_and_desire_satisfaction_effects() {
        let mut pop = make_pop();
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 2.0;
        basic.effect.push(DesireEffect::Satisfaction(6.0, true));
        let mut common = desire(
            2,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        common.satisfaction = 4.0;
        common
            .effect
            .push(DesireEffect::Satisfaction(2.0, false));
        common.effect.push(DesireEffect::Culture(5.0, true));
        pop.desires[0].push(basic);
        pop.desires[1].push(common);

        let sol = pop.calculate_sol(&Factuals::new());

        assert!((sol - 4.5).abs() < 1e-9);
    }

    /// A stored satisfaction effect is added once, then removed. A birth-rate
    /// effect stays on the pop.
    #[test]
    fn standard_of_living_adds_pop_satisfaction_and_drops_that_effect() {
        let mut pop = make_pop();
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);
        pop.stored_effects
            .push(PopEffect::Satisfaction { tier: 1, amount: 1.5 });
        pop.stored_effects.push(PopEffect::Birthrate(0.01));

        let sol = pop.calculate_sol(&Factuals::new());

        assert!((sol - 2.5).abs() < 1e-9);
        assert_eq!(pop.stored_effects, vec![PopEffect::Birthrate(0.01)]);
    }

    /// Yesterday's satisfaction is 25 and good 1 is fully reserved. The
    /// desire is for a good the pop does not hold. Good 1 does not rot and
    /// cannot be sold. The market still has yesterday's payment on good 1.
    ///
    /// The morning reset clears that satisfaction and reserve, and drops the
    /// old tape, so the night does not treat yesterday's payment as today's.
    /// The only good is restated at 1.
    #[test]
    fn market_day_clears_yesterdays_satisfaction_reserve_and_tape() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(8.0).with_reserve(8.0));
        let mut stale = desire(
            1,
            vec![DesireTarget::new(99, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        stale.satisfaction = 25.0;
        pop.desires[0].push(stale);

        let mut market = Market::new(1);
        market.pops.insert(pop.id);
        let mut good = MarketGood::new().with_amv(2.0).with_salability(0.5);
        good.traded = 4.0;
        good.paid = 4.0;
        market.goods.insert(1, good);
        let mut actors = Actors::new();
        actors.pops.insert(pop.id, pop);
        let mut grain = make_good(1, "grain", 0.0);
        grain.tags.insert(GoodTag::Untradeable);
        let factuals = Factuals::new().with_good(grain);
        let mut rng = StdRng::seed_from_u64(1);

        let _meetings = market.market_day(&mut actors, &factuals, &mut rng);

        let pop = actors.pop(1);
        assert_eq!(pop.desires[0][0].satisfaction, 0.0);
        assert_eq!(pop.property[&1].reserved, 0.0);
        assert!((pop.property[&1].quantity - 8.0).abs() < 1e-9);
        assert_eq!(market.goods[&1].decayed, 0.0);
        assert!((market.goods[&1].salability - 0.5).abs() < 1e-9);
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
    }

    /// Good 1 rots completely. The pop has no desire for it, so the loss
    /// is the whole stock and [`MarketGood::decayed`] keeps that loss.
    #[test]
    fn market_day_reports_units_lost_to_rot() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(8.0));
        let mut market = Market::new(1);
        market.pops.insert(pop.id);
        let mut actors = Actors::new();
        actors.pops.insert(pop.id, pop);
        let factuals = Factuals::new().with_good(make_good(1, "grain", 1.0));
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        assert!((market.goods[&1].decayed - 8.0).abs() < 1e-9);
    }

    /// The buyer wants 4 of good 2 and pays with good 9. The seller holds
    /// 10 of good 2 and wants nothing. Neither good rots.
    ///
    /// Day 1 buys and eats the bread. The next morning clears that
    /// satisfaction, so day 2 buys again from what the seller still holds.
    #[test]
    fn market_day_buys_again_after_satisfaction_is_reset() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.property.insert(9, PopPRow::new(20.0));
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market
            .goods
            .insert(2, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(9, MarketGood::new().with_amv(1.0).with_salability(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let factuals = Factuals::new()
            .with_good(make_good(2, "bread", 0.0))
            .with_good(make_good(9, "gold", 0.0));
        let mut rng = StdRng::seed_from_u64(1);

        let day1 = market.market_day(&mut actors, &factuals, &mut rng);
        assert!(day1
            .iter()
            .any(|meeting| meeting.outcome == MeetingOutcome::Accepted));
        assert!(actors.pop(1).desires[0][0].satisfaction > 0.0);

        let day2 = market.market_day(&mut actors, &factuals, &mut rng);
        assert!(day2
            .iter()
            .any(|meeting| meeting.outcome == MeetingOutcome::Accepted));
        assert!((actors.pop(1).property[&2].quantity).abs() < 1e-9);
        assert!((actors.pop(2).property[&2].quantity - 2.0).abs() < 1e-9);
    }

    #[test]
    fn rescale_desires_matches_the_new_household_and_keeps_the_met_fraction() {
        let demo = DemoDesire::new(1)
            .with_tier(0)
            .with_scalar(ScalingFactor::Household(1.0))
            .with_amount(2.0)
            .with_effect(DesireEffect::Culture(3.0, true))
            .with_effect(DesireEffect::Birthrate(0.2, true));
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_desire(demo));
        let mut pop = make_pop();
        let linked = factuals.species[&0]
            .find_desire(1)
            .unwrap()
            .create_desire(&pop, DemographicSource::Species(0));
        pop.desires[0].push(linked);
        pop.desires[0][0].satisfaction = 1.0;
        pop.desires[1].push(desire(
            9,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            5.0,
        ));
        pop.desires[1][0].satisfaction = 1.0;
        pop.demographics.household.count = 4.0;

        pop.rescale_desires(&factuals);

        assert!((pop.desires[0][0].amount - 8.0).abs() < 1e-12);
        assert!((pop.desires[0][0].satisfaction - 4.0).abs() < 1e-12);
        assert_eq!(
            pop.desires[0][0].effect,
            vec![
                DesireEffect::Culture(24.0, true),
                DesireEffect::Birthrate(0.4, true),
            ]
        );
        assert_eq!(pop.desires[1][0].amount, 5.0);
        assert_eq!(pop.desires[1][0].satisfaction, 1.0);
    }

    #[test]
    fn market_day_rescales_satisfaction_after_growth() {
        let demo = DemoDesire::new(1)
            .with_tier(0)
            .with_scalar(ScalingFactor::Household(1.0))
            .with_amount(2.0);
        let factuals = Factuals::new()
            .with_species(Species::new(0, "human").with_desire(demo))
            .with_good(make_good(2, "bread", 0.0));
        let mut pop = make_pop();
        pop.property.insert(2, PopPRow::new(2.0));
        let linked = factuals.species[&0]
            .find_desire(1)
            .unwrap()
            .create_desire(&pop, DemographicSource::Species(0));
        pop.desires[0].push(linked);
        pop.desires[0][0].amount = 99.0;
        let mut market = Market::new(1);
        market.pops.insert(pop.id);
        let mut actors = Actors::new();
        actors.pops.insert(pop.id, pop);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let pop = actors.pop(1);
        let count = pop.demographics.household.count;
        assert!((pop.desires[0][0].amount - 2.0 * count).abs() < 1e-9);
    }
}
