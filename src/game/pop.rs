use std::collections::{HashMap, HashSet};

use crate::game::{
    actor::Actor,
    deal::{DealMaker, DealResponse, ProposedDeal, SellerBook},
    desire::{Desire, DesireSource, DesireTargetType},
    factuals::Factuals,
    good::GoodTag,
    household::DemographicRates,
    market::{Market, MarketHistory, AMV_EPSILON},
    marketorder::MarketOrder,
    scalingfactor::ScalingFactor,
    sentiment::Sentiment,
};

pub use crate::game::effects::PopEffect;
pub use crate::game::pop_property::{DemoRow, PopPRow, PopRecords};

/// A population slice: one household block, its goods, and its desires.
#[derive(Debug, Clone)]
pub struct Pop {
    pub id: usize,

    // TODO: Job. A primitive process a pop runs without a firm (subsistence
    // and cottage work). The job changes over time. Not a stable id.

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

/// Bookmark for one satisfaction walk.
///
/// `target_index` is into the desire's targets in highest-efficiency-first
/// order. `target_start_satisfaction` is the desire's satisfaction when that
/// target began receiving goods, so a resume does not spend its cap twice.
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

    /// Adds the day's generated goods, each scaled by this pop's household.
    pub fn start_day(&mut self, new_goods: &[(usize, ScalingFactor)]) {
        for (good_id, scaling) in new_goods {
            let amount = self.get_scaling_factor(*scaling);
            self.property
                .entry(*good_id)
                .and_modify(|row| row.quantity += amount)
                .or_insert(PopPRow::new(amount));
        }
    }

    /// Adds species, culture, and religion desires that this pop does not already have.
    ///
    /// Does not shop, and does not rewrite satisfaction. Culture and religion id
    /// `0` are skipped. Class desires are not supported yet.
    pub fn update_desires(&mut self, factuals: &Factuals) {
        let mut existing = HashSet::new();
        for tier in &self.desires {
            for desire in tier {
                existing.insert(desire.source);
            }
        }
        self.add_missing_demographic_desires(factuals, &existing);
    }

    fn add_missing_demographic_desires(
        &mut self,
        factuals: &Factuals,
        existing: &HashSet<DesireSource>,
    ) {
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
        if let Some(start) = target_start_satisfaction {
            if let Some(blocked) = self.fill_target(desire, target_index, Some(start), &mut remaining)
            {
                return blocked;
            }
        }
        while remaining > 1e-9 {
            let open: Vec<usize> = (0..desire.target.len())
                .filter(|&index| target_cap_left(desire, index, None) > 1e-9)
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
        self.consume_tier(&mut working_desires); // satisfy them
        self.desires.insert(0, working_desires); // put back

        // second do common needs, only one pass needed.
        working_desires = self.desires.remove(1); // pop off
        self.consume_tier(&mut working_desires); // satisfy
        self.desires.insert(1, working_desires); // put back

        // Last is Luxury Needs, do until we produce no more satisfaction.
        let mut iter_target = 1.0;
        working_desires = self.desires.remove(2); // pop off
        let mut ordered_desires = vec![];
        loop {// loop over desires
            // satisfy the current working desires
            self.consume_tier(&mut working_desires);
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

    /// Applies structural demographic rates, then same-day birth and mortality
    /// effects, then [`Household::update`](crate::game::household::Household::update).
    ///
    /// Pops with household `count < 1` are left alone. Desire satisfaction does
    /// not change the rates.
    pub fn growth_phase(&mut self, factuals: &Factuals) {
        if self.demographics.household.count < 1.0 {
            return;
        }
        let mut rates = factuals.get_demographic_rates(self.demographics);
        rates = rates.add(&self.take_stored_growth_mods());
        self.demographics.household.update(&rates);
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

    /// End-of-day bookkeeping. The record struct is empty, so this is a no-op.
    pub fn record_keeping(&mut self, _factuals: &Factuals, _history: &MarketHistory) {}

    /// End-of-day decay.
    ///
    /// 1. Return `used` to `quantity`.
    /// 2. Decay `quantity` by the good's rate. [`GoodTag::Exposure`] skips this while owned.
    /// 3. Destroy `consumed` outright and credit byproducts.
    /// 4. Pay [`PopEffect::BonusGood`] from `stored_effects` and drop those arms.
    ///
    /// Returns `(decayed, volume)` per good. Volume is on-hand after `used`
    /// returns, plus `consumed`. Eaten stock is not counted as rot.
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

    /// Seller requests first, then the buyer's other free goods by salability.
    fn payment_goods(
        &self,
        book: &SellerBook,
        history: &MarketHistory,
        avoid: usize,
    ) -> Vec<usize> {
        let mut goods: Vec<usize> = book.requests.iter().map(|order| order.target).collect();
        let mut extras: Vec<(usize, f64)> = self
            .property
            .iter()
            .filter(|(good, row)| {
                **good != avoid && !goods.contains(*good) && row.available().floor() >= 1.0
            })
            .map(|(good, _)| (*good, history.salability(*good)))
            .collect();
        extras.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        goods.extend(extras.into_iter().map(|(good, _)| good));
        goods
    }

    fn move_good(&mut self, good: usize, delta: f64) {
        let row = self.property.entry(good).or_insert_with(|| PopPRow::new(0.0));
        row.quantity = (row.quantity + delta).max(0.0);
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
                let price = history.price(order.target).abs();
                if price < AMV_EPSILON
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

    /// Add `need` more AMV of payment. Skips transport goods unless the seller
    /// requested them, so transport stock stays available for freight.
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
            let is_transport = factuals
                .goods
                .get(&good)
                .is_some_and(|row| row.is_transport());
            let requested = book.requests.iter().any(|order| order.target == good);
            if is_transport && !requested {
                continue;
            }
            let pay_amv = history.price(good).abs();
            if pay_amv < AMV_EPSILON {
                continue;
            }
            let already = goods.get(&good).copied().unwrap_or(0.0).min(0.0).abs();
            let mut free = self.free_units(good).floor() - already;
            if let Some(request) = book.requests.iter().find(|order| order.target == good) {
                free = free.min(request.target_amount.floor() - already).max(0.0);
            }
            if free < 1.0 {
                continue;
            }
            let take = ((target - *covered) / pay_amv).ceil().min(free);
            if take < 1.0 {
                continue;
            }
            *goods.entry(good).or_insert(0.0) -= take;
            *covered += take * pay_amv;
        }
        *covered >= target
    }

    fn free_stock(&self) -> HashMap<usize, f64> {
        self.property
            .iter()
            .map(|(&good, row)| (good, row.available().max(0.0)))
            .collect()
    }

    /// Spend transport the buyer holds after the basket has moved.
    fn pay_freight(&mut self, amount: f64, factuals: &Factuals) {
        let mut ids: Vec<usize> = self.property.keys().copied().collect();
        ids.sort_unstable();
        let mut left = amount;
        for id in ids {
            if left <= 0.0 {
                break;
            }
            let Some(good) = factuals.goods.get(&id) else {
                continue;
            };
            let efficiency = good.transport_efficiency();
            if efficiency <= 0.0 {
                continue;
            }
            let free = self.free_units(id);
            if free <= 0.0 {
                continue;
            }
            let take = (left / efficiency).min(free);
            self.move_good(id, -take);
            if let Some(row) = self.property.get_mut(&id) {
                row.consumed += take;
            }
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
            if units >= 1.0 {
                orders.push(MarketOrder::sell(self.actor(), good, units));
            }
        }
        orders
    }

    fn buy_orders(&self, _history: &MarketHistory) -> Vec<MarketOrder> {
        self.buy_for_stopped_desire().into_iter().collect()
    }

    fn free_units(&self, good: usize) -> f64 {
        self.property
            .get(&good)
            .map(|row| row.available().max(0.0))
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
        let wanted = self.buy_for_stopped_desire()?.target_amount;
        let qty = wanted.min((-offer.target_amount).floor());
        if qty < 1.0 {
            return None;
        }
        let good_amv = history.price(match_good).abs();
        if good_amv < AMV_EPSILON {
            return None;
        }
        let mut goods = HashMap::from([(match_good, qty)]);
        let mut covered = 0.0;
        let need = qty * good_amv;
        if !self.add_payment(
            factuals,
            history,
            book,
            match_good,
            &mut goods,
            &mut covered,
            need,
        ) {
            return None;
        }
        self.buy_transport_for_freight(factuals, history, book, &mut goods, &mut covered)?;
        Some(ProposedDeal {
            buyer: self.actor(),
            seller: book.seller,
            match_good,
            freight: crate::game::deal::freight_bill(factuals, history.friction, &goods),
            goods,
        })
    }

    fn evaluate(
        &self,
        proposal: &ProposedDeal,
        history: &MarketHistory,
        _factuals: &Factuals,
    ) -> DealResponse {
        if !crate::game::deal::seller_can_accept(self, proposal, history) {
            return DealResponse::Reject;
        }
        let wants: Vec<usize> = self
            .buy_orders(history)
            .into_iter()
            .map(|order| order.target)
            .collect();
        if !wants.is_empty()
            && !proposal
                .goods
                .iter()
                .any(|(good, qty)| *qty < 0.0 && wants.contains(good))
        {
            return DealResponse::Reject;
        }
        DealResponse::Accept
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
            self.move_good(good, sign * qty);
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

    use crate::game::actors::Actors;
    use crate::game::desire::{Desire, DesireSource, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::market::{Market, MarketGood};
    use crate::game::good::{Good, GoodTag};
    use crate::game::household::Household;
    use crate::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
    use crate::game::scalingfactor::ScalingFactor;
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
        market: &Market,
        actors: &mut Actors,
        factuals: &Factuals,
    ) -> Vec<crate::game::deal::ProposedDeal> {
        let mut rng = StdRng::seed_from_u64(1);
        market.match_deals(actors, factuals, &mut rng)
    }

    fn make_pop() -> Pop {
        Pop {
            id: 1,
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

    fn desire(id: usize, targets: Vec<DesireTarget>, amount: f64) -> Desire {
        Desire {
            source: DesireSource::Species(0, id),
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

        assert_eq!(*blocked.source.demo_desire_id(), 2);
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

        assert_eq!(*blocked.source.demo_desire_id(), 1);
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
        assert_eq!(*blocked.source.demo_desire_id(), 2);

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

        let deals = do_match(&market, &mut actors, &Factuals::new());

        assert_eq!(deals.len(), 1);
        assert_eq!(deals[0].goods.get(&2), Some(&4.0));
        assert_eq!(deals[0].goods.get(&9), Some(&-8.0));
        assert!((actors.pops[&1].property[&2].quantity - 4.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&9].quantity - 2.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&2].quantity - 6.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&9].quantity - 8.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&2].reserved - 4.0).abs() < 1e-9);
    }

    #[test]
    fn match_deals_rejects_when_the_seller_wanted_a_different_good() {
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

        let deals = do_match(&market, &mut actors, &Factuals::new());

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

        let deals = do_match(&market, &mut actors, &Factuals::new().with_good(time_good()));

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

        let deals = do_match(&market, &mut actors, &Factuals::new().with_good(time_good()));

        assert_eq!(deals.len(), 1);
        assert!((deals[0].freight - 1.0).abs() < 1e-9);
        assert!(deals[0].goods.get(&0).is_none());
        assert!(actors.pops[&2].property.get(&0).is_none());
        assert!((actors.pops[&1].property[&0].quantity - 4.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&0].consumed - 1.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&9].quantity - 2.0).abs() < 1e-9);
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

        let deals = do_match(&market, &mut actors, &Factuals::new().with_good(time_good()));

        assert_eq!(deals.len(), 1);
        assert_eq!(deals[0].goods.get(&0), Some(&1.0));
        assert!(actors.pops[&1].property.get(&0).is_none_or(|row| row.quantity < 1e-9));
        assert!((actors.pops[&1].property[&0].consumed - 1.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&0].quantity - 3.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&9].quantity - 11.0).abs() < 1e-9);
    }
}
