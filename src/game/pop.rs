use std::collections::{HashMap, HashSet, VecDeque};

use crate::game::{
    actor::Actor, config::TIME_PER_LABOR, deal::{DealMaker, DealResponse, ProposedDeal, SellerBook}, demographic_source::DemographicSource, desire::{Desire, DesireTarget, DesireTargetType}, effects::{DesireEffect, ProcessEffect}, factuals::Factuals, good::{Good, GoodTag, TIME}, household::{DemographicRates, Household}, job::Job, market::{Market, MarketHistory}, marketorder::MarketOrder, scalingfactor::ScalingFactor, sentiment::{Sentiment, SentimentKind, SentimentMod}, util::whole_units_up,
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

    /// Species, culture, stratum, and religion for this slice.
    pub demographics: DemoRow,

    /// Share of the market population ceiling, in people. `None` leaves this pop out.
    pub population_cap: Option<f64>,

    /// Net households gained from internal migration this day.
    ///
    /// Negative is households that left. [`Market::internal_migration`]
    /// writes it and clears it at the start of that pass. A pop that
    /// stays is `0`. This is the household count, not people.
    pub migration: f64,

    /// Same-day effects waiting for growth or decay.
    pub stored_effects: Vec<PopEffect>,

    /// Mood shares. [`Self::growth_phase`] scales births per woman from these.
    pub sentiment: Sentiment,

    /// Day-end records. The struct is a placeholder.
    pub records: PopRecords,

    /// Recent standards of living, oldest first.
    ///
    /// [`Self::sol_shift`] reads these before [`Self::record_sol`] appends today.
    sol_samples: VecDeque<f64>,

    /// Where [`Pop::satisfy`] stopped. [`Pop::satisfy_continue`] walks from here.
    satisfy_cursor: Option<SatisfyCursor>,

    /// Desire source, id, and amount bits at the last [`Self::plan`].
    ///
    /// A difference is a new or removed desire, or a growth change of an
    /// amount. That night snaps cover. An empty stamp is no plan yet.
    planned_desires: Vec<(DemographicSource, usize, u64)>,

    /// Complete luxury levels at the last [`Self::plan`].
    ///
    /// The morning buy uses this many levels of a stopped luxury desire,
    /// and at least one. `0` is no plan yet, or a night that finished none.
    /// A saved luxury day is not included.
    luxury_reached: f64,

    /// Units of each good held back from eating, selling, and payment.
    ///
    /// One extra day for each desire [`Self::plan`] is saving, on that
    /// desire's winning good. The morning reset leaves the map, so today's
    /// match uses last night's pile. The job leaves the same pile. See
    /// [`Job::leave`](crate::game::job::Job::leave).
    savings: HashMap<usize, f64>,

    /// Luxury desires' part of [`Self::savings`].
    ///
    /// Empty when no luxury desire is saving.
    luxury_savings: HashMap<usize, f64>,

    /// Last [`Self::plan`] reached the luxury savings tier.
    ///
    /// Starts true, before any plan. [`Self::savings_piles`] clears it when
    /// an earlier short day stops the walk before that tier.
    luxury_savings_open: bool,

    /// Cover units cheaper to buy than the line that would make them.
    ///
    /// [`Self::plan`] writes the map. The morning buy raises that good's
    /// order to this amount. The job still aims at the cover. A purchase
    /// on hand shrinks the next night's gap.
    cheaper_cover: HashMap<usize, f64>,

    /// Units this pop posted to buy today, by good.
    ///
    /// Cleared at the morning reset. [`Self::savings_fill`] is the reader.
    buy_posted: HashMap<usize, f64>,

    /// Units this pop received as the buyer today, by good.
    ///
    /// Cleared at the morning reset.
    buy_received: HashMap<usize, f64>,
}

/// # Satisfaction Change
///
/// Total satisfaction received and total satisfaction given up.
///
/// Each list is the end uses of one side. Every tier is included. Returns
/// `(gain, loss)`.
fn satisfaction_change(received: &[EndUse], given: &[EndUse]) -> (f64, f64) {
    let gain = received.iter().map(|use_| use_.satisfaction).sum();
    let loss = given.iter().map(|use_| use_.satisfaction).sum();
    (gain, loss)
}

/// # Raw AMV
///
/// Face AMV of `goods`, ignoring salability.
///
/// `history` supplies [`MarketHistory::price`]. Quantities are positive.
/// Returns the sum.
fn raw_amv(history: &MarketHistory, goods: &HashMap<usize, f64>) -> f64 {
    goods
        .iter()
        .map(|(&good, &qty)| history.price(good) * qty)
        .sum()
}

/// # Dictated Price
///
/// Raw AMV per unit `order` will trade at.
///
/// A non-zero [`MarketOrder::accept_price`] is that price. Otherwise the
/// card's raw AMV is used. Salability is not applied. Returns the price.
fn dictated_price(order: &MarketOrder, history: &MarketHistory) -> f64 {
    if order.accept_price != 0.0 && order.accept_price.is_finite() {
        order.accept_price
    } else {
        history.price(order.target)
    }
}

/// Saved days held above the day that will be eaten.
///
/// Fixed for this tester. Demographics and sentiment do not change it.
const SAVINGS_DAYS: f64 = 1.0;

/// Share of the pop moved into anger and into fear for each full share of
/// basic needs left unmet. Fixed for this tester.
const BASIC_UNMET_SENTIMENT: f64 = 0.04;

/// Share of the pop moved into contentment for each full share of common
/// needs met. A share above one still counts. Fixed for this tester.
const COMMON_SENTIMENT: f64 = 0.02;

/// Share of the pop moved into hope and into happiness for each luxury
/// level met. Further levels still count. Fixed for this tester.
const LUXURY_SENTIMENT: f64 = 0.01;

/// Share of the pop moved per point of wealth per household.
///
/// Positive wealth moves that share into hope. Negative wealth moves the
/// absolute share into fear. Fixed for this tester.
const WEALTH_SENTIMENT: f64 = 0.01;

/// Market days kept for the living average. Fixed for this tester.
const SOL_AVERAGE_DAYS: usize = 5;

/// Share of the pop moved per point of living above or below its recent average.
///
/// One point is larger than the tier flats and a point of wealth above.
/// Fixed for this tester.
const SOL_CHANGE_SENTIMENT: f64 = 0.25;

/// Share of a leaving household-fraction whose property travels with them.
///
/// Half the people take a quarter of the stock. The rest of that share is
/// the cost of the move and stays, or is dropped when the pop is removed.
const MIGRATION_PROPERTY_SHARE: f64 = 0.5;

/// # Migrant Group
///
/// Households and goods taken off one pop by [`Pop::take_migrants`].
#[derive(Debug, Clone)]
pub(crate) struct MigrantGroup {
    /// Households in this slice.
    pub households: f64,
    /// Age, sex, labor, and partnership averages of the pop they left.
    pub shape: Household,
    /// `quantity` taken, by good.
    pub quantity: HashMap<usize, f64>,
    /// `used` taken, by good. Not included in `quantity`.
    pub used: HashMap<usize, f64>,
}

impl MigrantGroup {
    /// # Empty
    ///
    /// A group with no households and no goods.
    fn empty() -> Self {
        Self {
            households: 0.0,
            shape: Household::new(),
            quantity: HashMap::new(),
            used: HashMap::new(),
        }
    }
}

/// Fraction of each other emotion moved into contentment.
///
/// A larger share moves more. A content pop moves nothing. Fixed for
/// this tester.
const CONTENTMENT_PULL: f64 = 0.15;

/// Cover gaps and the savings pile for one night.
struct NightNeeds {
    /// One day of a good this job makes, before stock. See [`Pop::night_needs`].
    meal: HashMap<usize, f64>,
    /// Short meal, after stock. The saved day is [`Self::save_gap`].
    cover: HashMap<usize, f64>,
    /// Short saved day this job can make, after the meal has taken its stock.
    save_gap: HashMap<usize, f64>,
    savings: HashMap<usize, f64>,
    luxury_savings: HashMap<usize, f64>,
    /// The walk reached the luxury tier.
    luxury_savings_open: bool,
}

/// Satisfaction one basket side places on a single desire.
struct EndUse {
    tier: usize,
    order: usize,
    satisfaction: f64,
    /// Whole units of the good placed on this desire.
    units: f64,
}

/// Bookmark for one satisfaction walk.
///
/// `target_index` is into `desire.target` as stored, the target the walk
/// stopped on. `target_start_satisfaction` is the desire's satisfaction
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
                stratum: 0,
                religion: 0,
            },
            population_cap: None,
            migration: 0.0,
            stored_effects: vec![],
            sentiment: Sentiment::new(),
            records: PopRecords::default(),
            sol_samples: VecDeque::new(),
            satisfy_cursor: None,
            planned_desires: Vec::new(),
            luxury_reached: 0.0,
            savings: HashMap::new(),
            luxury_savings: HashMap::new(),
            luxury_savings_open: true,
            cheaper_cover: HashMap::new(),
            buy_posted: HashMap::new(),
            buy_received: HashMap::new(),
        }
    }

    /// # Calculate Migratory Pressure
    ///
    /// Emigration pressure for the later day phase.
    ///
    /// [`Market::internal_migration`](crate::game::market::Market::internal_migration)
    /// moves households inside one market. This per-pop step stays unwritten.
    pub fn calculate_migratory_pressure(&mut self, factuals: &Factuals, _region: &Market) {
        let _ = (self, factuals);
        todo!("Pop calculate migratory pressure")
    }

    /// # Process Internal Migration
    ///
    /// Job-to-job moves for the later day phase.
    ///
    /// The market pass is
    /// [`Market::internal_migration`](crate::game::market::Market::internal_migration).
    /// This per-pop step stays unwritten.
    pub fn process_internal_migration(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Pop process internal migration")
    }

    /// # Take Migrants
    ///
    /// Removes `households` from this pop and the property that leaves with them.
    ///
    /// `households` is clamped to the count on hand. The property share is half
    /// that fraction of the pop. `quantity` and `used` are reduced by that
    /// share and returned in the group. `fresh`, `produced`, and `reserved`
    /// shrink by the same share and stay. Savings shrink by that share and
    /// stay. Averages on this pop stay. The group's household shape is those
    /// averages from before the count dropped. A non-positive ask returns an
    /// empty group and changes nothing.
    pub(crate) fn take_migrants(&mut self, households: f64) -> MigrantGroup {
        let count = self.demographics.household.count;
        if !households.is_finite() || households <= 0.0 || count <= 0.0 {
            return MigrantGroup::empty();
        }
        let households = households.min(count);
        let goods_fraction = MIGRATION_PROPERTY_SHARE * households / count;
        let shape = self.demographics.household;
        self.demographics.household.count = count - households;
        let mut quantity = HashMap::new();
        let mut used = HashMap::new();
        for (&good, row) in &mut self.property {
            let take_qty = row.quantity * goods_fraction;
            let take_used = row.used * goods_fraction;
            row.quantity -= take_qty;
            row.used -= take_used;
            row.fresh *= 1.0 - goods_fraction;
            row.produced *= 1.0 - goods_fraction;
            row.reserved *= 1.0 - goods_fraction;
            if take_qty != 0.0 {
                quantity.insert(good, take_qty);
            }
            if take_used != 0.0 {
                used.insert(good, take_used);
            }
        }
        for amount in self.savings.values_mut() {
            *amount *= 1.0 - goods_fraction;
        }
        for amount in self.luxury_savings.values_mut() {
            *amount *= 1.0 - goods_fraction;
        }
        MigrantGroup {
            households,
            shape,
            quantity,
            used,
        }
    }

    /// # Receive Migrants
    ///
    /// Adds `group` onto this pop.
    ///
    /// Household averages fold in the arrivals. `quantity` and `used` add.
    /// Savings stay as they are. The arrivals do not bring a savings claim.
    /// An empty group changes nothing.
    pub(crate) fn receive_migrants(&mut self, group: &MigrantGroup) {
        if group.households <= 0.0 {
            return;
        }
        self.demographics.household.absorb(&group.shape, group.households);
        for (&good, &qty) in &group.quantity {
            if qty != 0.0 {
                self.property.entry(good).or_default().quantity += qty;
            }
        }
        for (&good, &qty) in &group.used {
            if qty != 0.0 {
                self.property.entry(good).or_default().used += qty;
            }
        }
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

    /// # Update Desires
    ///
    /// Adds species, culture, stratum, and religion desires this pop does not already have.
    ///
    /// `factuals` supplies those demographics. Does not shop, and does not
    /// rewrite satisfaction. Culture, stratum, and religion id `0` are
    /// skipped. Day-end size matching is [`Self::rescale_desires`].
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

        if self.demographics.stratum != 0 {
            if let Some(stratum) = factuals.get_stratum(self.demographics.stratum) {
                for demo in stratum.desires.values() {
                    let source = DemographicSource::Stratum(stratum.id);
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
    /// Adds the processes of this pop's effective craft that the job does not run.
    ///
    /// `factuals` supplies the craft, including a culture or religion attachment.
    /// A line already on the job keeps its target. A process added here starts
    /// at no iterations. This does not remove a line. A later call adds a
    /// craft process that is missing again.
    pub fn apply_craft(&mut self, factuals: &Factuals) {
        let processes = factuals.craft_processes(
            self.job.craft,
            self.demographics.culture,
            self.demographics.religion,
        );
        self.job.ensure_lines(&processes);
        self.prepare_skills(factuals);
    }

    /// Household floor for one attached skill. Flat, not per person.
    const SKILL_PER_HOUSEHOLD: f64 = 0.1;

    /// # Prepare Skills
    ///
    /// Attaches the job's skills and raises each pile to the household floor.
    ///
    /// `factuals` supplies which inputs are skills. The floor is
    /// [`Self::SKILL_PER_HOUSEHOLD`] times the household count. A pile
    /// already at or above that floor stays. A missing row is created at
    /// the floor. A floor of zero does nothing.
    pub fn prepare_skills(&mut self, factuals: &Factuals) {
        self.job.attach_skills(factuals);
        let floor = Self::SKILL_PER_HOUSEHOLD * self.demographics.household.count.max(0.0);
        if floor <= 0.0 {
            return;
        }
        let skills = self.job.skills.clone();
        for skill in skills {
            let row = self
                .property
                .entry(skill)
                .or_insert_with(|| PopPRow::new(0.0));
            if row.quantity < floor {
                row.quantity = floor;
            }
        }
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
    /// Within a desire, a target that can be taken in full comes first. It is
    /// capped at `amount * cap` minus what that target already contributed
    /// toward the open level. The walk stops if that target cannot be taken
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
        self.satisfy_from(SatisfyCursor::start(), rng, None)
    }

    /// # Satisfy What Was Made
    ///
    /// Reserves goods produced after the morning [`Self::satisfy`].
    ///
    /// Continues from the bookmark. Luxury stops after one level, so a unit
    /// above that level stays free to sell. No bookmark means the morning
    /// walk finished, and this does nothing.
    pub fn satisfy_made(&mut self, rng: &mut dyn rand::RngCore) {
        let Some(cursor) = self.satisfy_cursor else {
            return;
        };
        let _ = self.satisfy_from(cursor, rng, Some(1.0));
    }

    /// # Settle Shopping
    ///
    /// Rebuilds the job's buy list from stock still free after this morning's make.
    ///
    /// The saved day is not part of that stock. `factuals` supplies decay.
    /// Morning reserve shopped before that make
    /// and before [`Self::satisfy_made`] reserved any of it. An input the
    /// make left free is not bought again. Capital set aside in `used` still
    /// counts. The buy this posts is recorded for [`Self::savings_fill`].
    pub fn settle_shopping(&mut self, factuals: &Factuals) {
        self.job.leave(&self.savings);
        self.job.settle_shopping(&self.property, factuals);
        self.note_posted_buys();
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
        self.satisfy_from(cursor, rng, None)
    }

    /// Walk from `cursor` until a target cannot be filled or every open level is done.
    ///
    /// `luxury_limit` stops the walk before a luxury level above that number
    /// is opened. `None` walks every level the stock can fill.
    fn satisfy_from(
        &mut self,
        mut cursor: SatisfyCursor,
        rng: &mut dyn rand::RngCore,
        luxury_limit: Option<f64>,
    ) -> Option<Desire> {
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
            // The post-production walk sells anything above one luxury level.
            if cursor.tier == 2 {
                if let Some(limit) = luxury_limit {
                    if cursor.iter_target > limit {
                        self.satisfy_cursor = None;
                        return None;
                    }
                }
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
    /// A resumed target is finished first when its open cap can be taken in
    /// full. When that cap is still short and another target can be taken in
    /// full, that other target is used instead. The short bookmark stops the
    /// desire only when nothing else can. Further targets take a fillable
    /// cap below one level before a substitute, then one that can be taken
    /// in full, then a high-priority target, then higher efficiency. Each
    /// target's cap is `amount * cap` minus what that
    /// target already contributed toward the open level (`target_taken`). A
    /// target that cannot be taken in full stops the desire, after reserving
    /// whatever of that target is free. Completing the level clears
    /// `target_taken` so the next luxury level can use its caps again.
    fn satisfy_one_desire(
        &mut self,
        desire: &mut Desire,
        iter_target: f64,
        target_index: usize,
        target_start_satisfaction: Option<f64>,
        _rng: &mut dyn rand::RngCore,
    ) -> SatisfyProgress {
        debug_assert!(desire.amount > 0.0, "desire amount must be positive");
        debug_assert!(
            iter_target.is_finite() && iter_target > 0.0,
            "iteration target must be positive"
        );
        let mut remaining = iter_target * desire.amount - desire.satisfaction;
        if remaining <= 1e-9 {
            desire.target_taken.clear();
            return SatisfyProgress::Done;
        }
        while desire.target_taken.len() < desire.target.len() {
            desire.target_taken.push(0.0);
        }
        // Targets whose cap share this call already took in full.
        let mut filled = vec![false; desire.target.len()];
        if let Some(start) = target_start_satisfaction {
            // A short bookmark must not hide a good that can finish the level.
            if self.target_can_fill(desire, target_index, remaining) {
                if let Some(blocked) =
                    self.fill_target(desire, target_index, Some(start), &mut remaining)
                {
                    return blocked;
                }
                if let Some(done) = filled.get_mut(target_index) {
                    *done = true;
                }
            }
        }
        while remaining > 1e-9 {
            let open: Vec<usize> = (0..desire.target.len())
                .filter(|&index| !filled[index] && target_cap_left(desire, index) > 1e-9)
                .collect();
            if open.is_empty() {
                return SatisfyProgress::Blocked {
                    target_index: desire.target.len(),
                    target_start_satisfaction: desire.satisfaction,
                };
            }
            let index = self.pick_open_target(desire, &open, remaining);
            if let Some(blocked) = self.fill_target(desire, index, None, &mut remaining) {
                return blocked;
            }
            filled[index] = true;
        }
        if desire.tiers_satisfied() + 1e-9 >= iter_target {
            desire.target_taken.clear();
            SatisfyProgress::Done
        } else {
            SatisfyProgress::Blocked {
                target_index: desire.target.len(),
                target_start_satisfaction: desire.satisfaction,
            }
        }
    }

    /// # Pick Open Target
    ///
    /// The next bucket index [`Self::satisfy_one_desire`] should take.
    ///
    /// `desire` is the open desire. `open` is the indexes whose caps are not
    /// full. `remaining` is the satisfaction still needed for this level.
    /// A target whose cap is below one level, and whose open cap can be
    /// taken in full, comes first. Then a target that can be taken in full.
    /// Among those, a high-priority target comes first, then the higher
    /// efficiency, then the earlier index. When none can be filled, the
    /// target with the most satisfaction on hand wins, then that same tie
    /// break. Returns that index.
    fn pick_open_target(&self, desire: &Desire, open: &[usize], remaining: f64) -> usize {
        let key = |index: usize| -> (bool, bool, bool, f64, f64) {
            let target = &desire.target[index];
            let available = self
                .property
                .get(&target.good)
                .map(|row| row.available().max(0.0))
                .unwrap_or(0.0);
            let fills = self.target_can_fill(desire, index, remaining);
            let share = target.cap < 1.0 && fills;
            let on_hand = if target.efficiency > 0.0 {
                available * target.efficiency
            } else {
                0.0
            };
            (share, fills, target.high_priority, on_hand, target.efficiency)
        };
        open.iter().copied().max_by(|&a, &b| {
            let (share_a, fill_a, pri_a, hand_a, eff_a) = key(a);
            let (share_b, fill_b, pri_b, hand_b, eff_b) = key(b);
            let order = share_a
                .cmp(&share_b)
                .then(fill_a.cmp(&fill_b))
                .then(pri_a.cmp(&pri_b));
            if order != std::cmp::Ordering::Equal {
                return order;
            }
            // Both can finish the level: take the better good. Neither can:
            // take whichever stock satisfies more.
            let ranked = if fill_a {
                eff_a.partial_cmp(&eff_b).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                hand_a
                    .partial_cmp(&hand_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(eff_a.partial_cmp(&eff_b).unwrap_or(std::cmp::Ordering::Equal))
            };
            ranked.then(b.cmp(&a))
        }).unwrap_or(open[0])
    }

    /// # Target Can Fill
    ///
    /// True when free stock can take `index`'s open cap in full.
    ///
    /// `remaining` is the satisfaction still needed for this level. The cap
    /// is what that target may still contribute. A missing target, a
    /// non-positive efficiency, or a need of zero is false.
    fn target_can_fill(&self, desire: &Desire, index: usize, remaining: f64) -> bool {
        let Some(target) = desire.target.get(index) else {
            return false;
        };
        if target.efficiency <= 0.0 {
            return false;
        }
        let available = self
            .property
            .get(&target.good)
            .map(|row| row.available().max(0.0))
            .unwrap_or(0.0);
        let needed = remaining.min(target_cap_left(desire, index)).max(0.0) / target.efficiency;
        available >= needed && needed > 0.0
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
        let cap_room = target_cap_left(desire, index);
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
                if let Some(taken) = desire.target_taken.get_mut(index) {
                    *taken += sat_gained;
                }
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
    /// - Luxury (2) desires are cycled one level at a time. A desire that
    ///   misses the current level is benched and keeps its place in the tier.
    ///   A further level runs only when [`Self::luxury_buffer_ready`] is
    ///   true, and that level leaves [`Self::savings`] in quantity.
    ///
    /// For desires with a bucket of goods, higher-efficiency goods are preferred.
    ///
    /// The results of the consumption is stored in the desires as satisfaction.
    /// Every desire starts this call at zero satisfaction, so the level
    /// [`Pop::satisfy`] recorded for the reserved goods is counted once, when
    /// those goods are eaten.
    ///
    /// Also mutates `self.property` (reduces `quantity`, increases `consumed`).
    /// 
    /// The function assumes that all desires are currently in `self.desires` and
    /// none are in `self.working_desires`.
    pub fn consume(&mut self) {
        // Drop the satisfaction the morning reserve recorded.
        for tier in &mut self.desires {
            for desire in tier.iter_mut() {
                desire.satisfaction = 0.0;
            }
        }
        // first do basic desires, only one pass needed.
        let mut working_desires = self.desires.remove(0); // pop off front
        self.consume_tier(&mut working_desires); // satisfy them
        self.desires.insert(0, working_desires); // put back

        // second do common needs, only one pass needed.
        working_desires = self.desires.remove(1); // pop off
        self.consume_tier(&mut working_desires); // satisfy
        self.desires.insert(1, working_desires); // put back

        // Last is Luxury Needs. One level is eaten. A further level waits
        // until the saved luxury day is on hand, and leaves that day.
        let mut iter_target = 1.0;
        let mut luxury = std::mem::take(&mut self.desires[2]);
        // Desires still filling. One that misses `iter_target` is benched in place.
        let mut filling = vec![true; luxury.len()];
        let mut level = 0u32;
        while filling.iter().any(|&open| open) {
            if level > 0 && !self.luxury_buffer_ready() {
                break;
            }
            for (desire, open) in luxury.iter_mut().zip(filling.iter_mut()) {
                if !*open {
                    continue;
                }
                self.consume_one_desire_from(desire, level > 0);
                if desire.tiers_satisfied() < iter_target {
                    *open = false;
                }
            }
            level += 1;
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
        self.consume_one_desire_from(desire, false)
    }

    /// # Consume One Desire From
    ///
    /// Satisfies `desire` by one level and returns that level.
    ///
    /// Targets are taken highest efficiency first. `leave_savings` keeps
    /// [`Self::savings`] in quantity. The take still comes out of `quantity`
    /// and `reserved`, and moves into `consumed` or `used`.
    fn consume_one_desire_from(&mut self, desire: &mut Desire, leave_savings: bool) -> f64 {
        // Clone + sort by efficiency descending (best substitutes first)
        let mut targets = desire.target.clone();
        targets.sort_by(|a, b| b.efficiency.partial_cmp(&a.efficiency)
            .unwrap_or(std::cmp::Ordering::Equal));

        let mut remaining = desire.amount;

        for target in targets.iter() {
            if remaining <=  0.0 {
                break;
            }
            let floor = if leave_savings {
                self.savings_of(target.good)
            } else {
                0.0
            };
            // get the target good, or continue on to the next target.
            if let Some(row) = self.property.get_mut(&target.good) && row.quantity > 0.0 {
                // remaining (Capped at the cap amount of the desire) divided by 
                // efficiency is how much is needed.
                let needed = remaining.min(desire.amount * target.cap) / target.efficiency;
                let spare = (row.quantity - floor).max(0.0);
                let take = needed.min(spare);
                if take <= 0.0 {
                    continue;
                }

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

    /// # Growth Phase
    ///
    /// Applies structural demographic rates, then same-day birth and mortality
    /// effects, then scales births per woman by the demographic sentiment
    /// schedule, then [`Household::update`](crate::game::household::Household::update).
    ///
    /// Pops with household `count < 1` are left alone. The scale is
    /// [`Factuals::birth_sentiment`](crate::game::factuals::Factuals::birth_sentiment)
    /// times this pop's current sentiment. Desire satisfaction does not change
    /// the rates by itself. A market day may then scale this household with
    /// the other capped pops before [`Self::rescale_desires`] matches desires
    /// to the size that remains.
    pub fn growth_phase(&mut self, factuals: &Factuals) {
        if self.demographics.household.count < 1.0 {
            return;
        }
        let mut rates = factuals.get_demographic_rates(self.demographics);
        rates = rates.add(&self.take_stored_growth_mods());
        // Scale births by the stacked sentiment schedule. Mortality stays.
        let birth = factuals.birth_sentiment(self.demographics);
        let scale = birth.multiplier(
            self.sentiment.hope(),
            self.sentiment.happiness(),
            self.sentiment.fear(),
            self.sentiment.anger(),
        );
        rates.birth_per_woman *= scale;
        self.demographics.household.update(&rates);
    }

    /// # Update Sentiments
    ///
    /// Applies this day's sentiment arms, then drops the stored ones.
    ///
    /// Every desire contributes its sentiment effects.
    /// [`DesireEffect::signed_strength`] of that desire's
    /// [`Desire::tiers_satisfied`](crate::game::desire::Desire::tiers_satisfied)
    /// is the amount. A flat arm shifts that share of the whole pop. A
    /// relative arm scales that emotion's current share. Stored
    /// [`PopEffect::SentimentFlat`] and [`PopEffect::SentimentRelative`]
    /// are already amounts and are removed. Other stored effects stay.
    ///
    /// After those arms are collected, tier satisfaction and
    /// [`Self::wealth_per_household`] add their own flats, and the change in
    /// living since its recent average adds a larger one. Each other emotion
    /// then moves [`CONTENTMENT_PULL`] of its current share into contentment.
    /// `history` prices the wealth. The whole list is applied together through
    /// [`Sentiment::apply_mods_simultaniously`]. With nothing to apply,
    /// the sentiment stays. Today's living is recorded after that.
    pub fn update_sentiments(&mut self, history: &MarketHistory) {
        let mut mods = Vec::new();
        // Desire arms, from the satisfaction consume wrote.
        for tier in &self.desires {
            for desire in tier {
                let sat = desire.tiers_satisfied();
                if !sat.is_finite() {
                    continue;
                }
                for effect in &desire.effect {
                    match *effect {
                        DesireEffect::SentimentFlat(kind, _, _) => {
                            mods.push(SentimentMod::Flat {
                                kind,
                                delta: effect.signed_strength(sat),
                            });
                        }
                        DesireEffect::SentimentRelative(kind, _, _) => {
                            mods.push(SentimentMod::Relative {
                                kind,
                                relative: effect.signed_strength(sat),
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
        // Stored arms are spent here. Other same-day effects stay.
        let mut kept = Vec::new();
        for effect in self.stored_effects.drain(..) {
            match effect {
                PopEffect::SentimentFlat { kind, delta } => {
                    mods.push(SentimentMod::Flat { kind, delta });
                }
                PopEffect::SentimentRelative { kind, relative } => {
                    mods.push(SentimentMod::Relative { kind, relative });
                }
                other => kept.push(other),
            }
        }
        self.stored_effects = kept;
        // Tier satisfaction, wealth, and the living shift, after the arms above.
        self.push_living_mods(history, &mut mods);
        // Move the shares already stored toward contentment.
        self.push_contentment_pull(&mut mods);
        if !mods.is_empty() {
            self.sentiment.apply_mods_simultaniously(mods);
        }
        // Keep today's living for the next comparison.
        self.record_sol();
    }

    /// # Standard of Living
    ///
    /// The same sum [`Self::calculate_sol`] records, without removing
    /// stored satisfaction or writing the record.
    ///
    /// No desires returns `+inf`. Otherwise each desire adds
    /// [`Desire::tiers_satisfied`](crate::game::desire::Desire::tiers_satisfied)
    /// and [`Desire::get_bonus_satisfaction`](crate::game::desire::Desire::get_bonus_satisfaction),
    /// then each stored [`PopEffect::Satisfaction`] amount is added.
    pub(crate) fn standard_of_living(&self) -> f64 {
        if self.desires.iter().all(|tier| tier.is_empty()) {
            return f64::INFINITY;
        }
        let mut sum = 0.0;
        for tier in &self.desires {
            for desire in tier {
                sum += desire.tiers_satisfied();
                sum += desire.get_bonus_satisfaction();
            }
        }
        for effect in &self.stored_effects {
            if let PopEffect::Satisfaction { amount, .. } = *effect {
                sum += amount;
            }
        }
        sum
    }

    /// # Wealth per Household
    ///
    /// Holding value of what this pop still has, divided by its household count.
    ///
    /// Each good adds `quantity` times
    /// [`MarketHistory::holding_per_unit`](MarketHistory::holding_per_unit).
    /// Time is the day's labor and is left out. A household count at or below
    /// 0, or a non-finite result, returns 0.
    pub(crate) fn wealth_per_household(&self, history: &MarketHistory) -> f64 {
        let households = self.demographics.household.count;
        if households <= 0.0 {
            return 0.0;
        }
        let mut value = 0.0;
        for (&good, row) in &self.property {
            if good == TIME || row.quantity == 0.0 {
                continue;
            }
            value += row.quantity * history.holding_per_unit(good);
        }
        let per = value / households;
        if per.is_finite() { per } else { 0.0 }
    }

    /// # Tier Met
    ///
    /// Mean levels met on `tier`, plus stored satisfaction for that tier.
    ///
    /// Each desire adds its tiers satisfied and its bonus satisfaction.
    /// A non-finite desire is skipped. Stored [`PopEffect::Satisfaction`]
    /// on this tier is added once, after the mean. No desires and no stored
    /// satisfaction returns 0.
    fn tier_met(&self, tier: usize) -> f64 {
        let mut sum = 0.0;
        let mut count = 0.0;
        if let Some(desires) = self.desires.get(tier) {
            for desire in desires {
                let levels = desire.tiers_satisfied();
                let bonus = desire.get_bonus_satisfaction();
                if !levels.is_finite() || !bonus.is_finite() {
                    continue;
                }
                sum += levels + bonus;
                count += 1.0;
            }
        }
        let mut stored = 0.0;
        for effect in &self.stored_effects {
            if let PopEffect::Satisfaction { tier: effect_tier, amount } = *effect {
                if effect_tier == tier && amount.is_finite() {
                    stored += amount;
                }
            }
        }
        let mean = if count > 0.0 { sum / count } else { 0.0 };
        mean + stored
    }

    /// # Basic Unmet
    ///
    /// Mean shortfall of the basic desires, reduced by stored basic satisfaction.
    ///
    /// Each desire's shortfall is how far its levels plus bonus sit below one
    /// full level, and not below 0. One desire met past one level does not
    /// fill another desire's gap. The mean of those shortfalls is then
    /// reduced by stored satisfaction on tier 0, and not below 0. No basic
    /// desires returns 0.
    fn basic_unmet(&self) -> f64 {
        let Some(desires) = self.desires.get(0) else {
            return 0.0;
        };
        let mut shortfall = 0.0;
        let mut count = 0.0;
        for desire in desires {
            let levels = desire.tiers_satisfied();
            let bonus = desire.get_bonus_satisfaction();
            if !levels.is_finite() || !bonus.is_finite() {
                continue;
            }
            shortfall += (1.0 - (levels + bonus)).max(0.0);
            count += 1.0;
        }
        if count == 0.0 {
            return 0.0;
        }
        let mut stored = 0.0;
        for effect in &self.stored_effects {
            if let PopEffect::Satisfaction { tier, amount } = *effect {
                if tier == 0 && amount.is_finite() {
                    stored += amount;
                }
            }
        }
        (shortfall / count - stored).max(0.0)
    }

    /// # Push Living Mods
    ///
    /// Appends the tier, wealth, and living-shift flats to `mods`.
    ///
    /// Unmet basic needs move [`BASIC_UNMET_SENTIMENT`] per full share into
    /// anger and the same share into fear. That is the largest tier rate.
    /// Common satisfaction moves [`COMMON_SENTIMENT`] per full share into
    /// contentment, including a share above one. Luxury levels move
    /// [`LUXURY_SENTIMENT`] per level into hope and the same share into
    /// happiness, and further levels keep counting. That is the smallest
    /// tier rate.
    /// Wealth per household moves [`WEALTH_SENTIMENT`] per point into hope
    /// when positive and fear when negative. A rise in living since its
    /// recent average moves [`SOL_CHANGE_SENTIMENT`] per point into hope and
    /// the same share into happiness. A fall moves that share into anger and
    /// the same share into fear. A zero level adds nothing.
    fn push_living_mods(&self, history: &MarketHistory, mods: &mut Vec<SentimentMod>) {
        let basic = self.basic_unmet();
        if basic > 0.0 {
            let delta = basic * BASIC_UNMET_SENTIMENT;
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Anger,
                delta,
            });
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Fear,
                delta,
            });
        }
        let common = self.tier_met(1);
        if common > 0.0 {
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Contentment,
                delta: common * COMMON_SENTIMENT,
            });
        }
        let luxury = self.tier_met(2);
        if luxury > 0.0 {
            let delta = luxury * LUXURY_SENTIMENT;
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Hope,
                delta,
            });
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Happiness,
                delta,
            });
        }
        let wealth = self.wealth_per_household(history);
        if wealth != 0.0 {
            let delta = wealth * WEALTH_SENTIMENT;
            if delta > 0.0 {
                mods.push(SentimentMod::Flat {
                    kind: SentimentKind::Hope,
                    delta,
                });
            } else if delta < 0.0 {
                mods.push(SentimentMod::Flat {
                    kind: SentimentKind::Fear,
                    delta: -delta,
                });
            }
        }
        let shift = self.sol_shift();
        if shift > 0.0 {
            let delta = shift * SOL_CHANGE_SENTIMENT;
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Hope,
                delta,
            });
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Happiness,
                delta,
            });
        } else if shift < 0.0 {
            let delta = -shift * SOL_CHANGE_SENTIMENT;
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Anger,
                delta,
            });
            mods.push(SentimentMod::Flat {
                kind: SentimentKind::Fear,
                delta,
            });
        }
    }

    /// # Push Contentment Pull
    ///
    /// Appends a transfer into contentment for each other emotion.
    ///
    /// The amount is that emotion's current share times [`CONTENTMENT_PULL`].
    /// A zero share adds nothing.
    fn push_contentment_pull(&self, mods: &mut Vec<SentimentMod>) {
        for kind in [
            SentimentKind::Happiness,
            SentimentKind::Anger,
            SentimentKind::Fear,
            SentimentKind::Hope,
        ] {
            let share = self.sentiment.get(kind);
            if share > 0.0 {
                mods.push(SentimentMod::Transfer {
                    from: kind,
                    to: SentimentKind::Contentment,
                    amount: share * CONTENTMENT_PULL,
                });
            }
        }
    }

    /// # Standard of Living Shift
    ///
    /// Today's living minus the mean of the recent samples.
    ///
    /// The samples are earlier days only. No samples, or a non-finite
    /// living, returns 0.
    fn sol_shift(&self) -> f64 {
        let today = self.standard_of_living();
        if !today.is_finite() || self.sol_samples.is_empty() {
            return 0.0;
        }
        let average = self.sol_samples.iter().sum::<f64>() / self.sol_samples.len() as f64;
        let shift = today - average;
        if shift.is_finite() { shift } else { 0.0 }
    }

    /// # Record Standard of Living
    ///
    /// Appends today's living to the samples [`Self::sol_shift`] reads.
    ///
    /// A non-finite living is left out. The list keeps the most recent
    /// [`SOL_AVERAGE_DAYS`] readings.
    fn record_sol(&mut self) {
        let today = self.standard_of_living();
        if !today.is_finite() {
            return;
        }
        if self.sol_samples.len() == SOL_AVERAGE_DAYS {
            self.sol_samples.pop_front();
        }
        self.sol_samples.push_back(today);
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
                    let ratio = desire.amount / old_amount;
                    desire.satisfaction *= ratio;
                    for taken in &mut desire.target_taken {
                        *taken *= ratio;
                    }
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
    /// Applies desire decay and clears same-day reserves.
    ///
    /// Each desire's satisfaction is multiplied by [`Desire::decay`]. A decay
    /// of `0.0` clears it. Per-target taken is cleared, and the satisfy
    /// bookmark is dropped. `reserved`, `fresh`, `produced`, and `lost` on
    /// every property row are set to 0. Quantity, consumed, and used stay for
    /// today's walk and for decay. The job drops its claimed-input list and
    /// its shopping list. Line targets stay, so last night's plan is what
    /// this morning runs.
    pub fn reset_day(&mut self) {
        self.satisfy_cursor = None;
        self.buy_posted.clear();
        self.buy_received.clear();
        for tier in &mut self.desires {
            for desire in tier.iter_mut() {
                desire.satisfaction *= desire.decay;
                desire.target_taken.clear();
            }
        }
        for row in self.property.values_mut() {
            row.reserved = 0.0;
            row.fresh = 0.0;
            row.produced = 0.0;
            row.lost = 0.0;
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
    /// [`Self::night_needs`] is the short meal, the short saved day, and the
    /// savings pile. [`crate::game::job::desire_cover`] keeps the saved day
    /// when the line that takes the good pays, and leaves it off when that
    /// line does not. [`Self::planning_property`] is the stock the job spends,
    /// `produced` and `lost` included. The modifier is [`Self::complexity_cost`].
    /// [`Self::desire_stamp`] differs from the last plan when the household
    /// grew into an amount or a desire was added or removed, and that night
    /// snaps the cover. Cover units [`crate::game::job::buys_cheaper_than_making`]
    /// returns are stored for the morning order.
    pub fn plan(&mut self, factuals: &Factuals, history: &MarketHistory) {
        self.luxury_reached = self.finished_luxury_levels();
        let needs = self.night_needs(factuals, history);
        self.savings = needs.savings;
        self.luxury_savings = needs.luxury_savings;
        self.luxury_savings_open = needs.luxury_savings_open;
        let modifier = self.complexity_cost(factuals);
        let property = self.planning_property();
        let stamp = self.desire_stamp();
        let snap_cover = stamp != self.planned_desires;
        self.planned_desires = stamp;
        // A paying line keeps the saved day. A losing line keeps the meal.
        let cover = crate::game::job::desire_cover(
            &self.job.lines,
            &needs.cover,
            &needs.save_gap,
            factuals,
            history,
        );
        self.cheaper_cover = crate::game::job::buys_cheaper_than_making(
            &self.job.lines,
            &cover,
            &needs.meal,
            factuals,
            history,
        );
        self.job
            .plan(&cover, &property, factuals, history, modifier, snap_cover);
    }

    /// # Finished Luxury Levels
    ///
    /// Complete luxury levels every luxury desire has reached.
    ///
    /// The result is the floor of the lowest luxury [`Desire::tiers_satisfied`].
    /// An empty luxury tier, or a non-finite level, is `0`.
    fn finished_luxury_levels(&self) -> f64 {
        let Some(tier) = self.desires.get(2) else {
            return 0.0;
        };
        if tier.is_empty() {
            return 0.0;
        }
        let lowest = tier
            .iter()
            .map(Desire::tiers_satisfied)
            .fold(f64::INFINITY, f64::min);
        if lowest.is_finite() {
            lowest.floor().max(0.0)
        } else {
            0.0
        }
    }

    /// # Desire Stamp
    ///
    /// Source, id, and amount bits of every desire, in tier order.
    ///
    /// Used by [`Self::plan`] to see growth and added or removed desires.
    /// The amount is [`f64::to_bits`], so an unchanged amount compares equal.
    fn desire_stamp(&self) -> Vec<(DemographicSource, usize, u64)> {
        let mut stamp = Vec::new();
        for tier in &self.desires {
            for desire in tier {
                stamp.push((desire.source, desire.demo_desire_id, desire.amount.to_bits()));
            }
        }
        stamp
    }

    /// # Planning Property
    ///
    /// Stock the night plan may spend the next morning.
    ///
    /// Copies the property rows. Adds this pop's morning time grant on
    /// [`TIME`]: [`Self::get_scaling_factor`] of [`ScalingFactor::Labor`] at
    /// [`TIME_PER_LABOR`], the amount [`crate::game::actors::Actors::start_day`]
    /// adds before produce. Time still in `quantity` stays, and the grant is
    /// added on top. `produced` and `lost` are unchanged.
    fn planning_property(&self) -> HashMap<usize, PopPRow> {
        let mut property = self.property.clone();
        let time = self.get_scaling_factor(ScalingFactor::Labor(TIME_PER_LABOR));
        if time > 0.0 {
            property
                .entry(TIME)
                .and_modify(|row| row.quantity += time)
                .or_insert(PopPRow::new(time));
        }
        property
    }

    /// # Night Needs
    ///
    /// Cover gaps and the savings pile for the next morning.
    ///
    /// `factuals` supplies processes and durability. `history` supplies
    /// salability and holding value. Satisfaction is not read. This runs
    /// after consumption and before the morning reset, so stock is
    /// `quantity`, plus the morning time grant on [`TIME`]. One level of a
    /// desire is `amount / efficiency` of a single target. A target whose
    /// cap is below 1 cannot finish that level. Goods are not mixed.
    /// [`Self::best_target`] picks the target. A desire stock can finish is
    /// spent. Every tier is walked, so a later desire is planned while an
    /// earlier one is still short.
    ///
    /// Each short desire adds the highest-efficiency target some line
    /// outputs. A desire no line can make adds nothing.
    /// [`Self::savings_piles`] then finds one saved day on each desire it
    /// allows. That check uses the stock on hand, before tomorrow's meal is
    /// set aside. A day this job makes is kept apart from the meal. Stock
    /// left after a covered meal is spent on the meal first, and what remains
    /// shortens the saved day. [`crate::game::job::desire_cover`] decides
    /// whether that day joins the line. [`Job::plan`] adds one output unit
    /// above the gap it is given.
    ///
    /// The meal is one level of a good this job makes, recorded before stock
    /// is subtracted and still recorded when that stock already covers it.
    /// A desire covered by some other good adds no meal. The saved day is
    /// not part of it.
    ///
    /// Returns that meal, the meal gap, the saved-day gap, the units to
    /// withhold, and the luxury part of that pile. An empty meal gap means
    /// every producible meal is covered.
    fn night_needs(&self, factuals: &Factuals, history: &MarketHistory) -> NightNeeds {
        let mut producible = HashSet::new();
        for line in &self.job.lines {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            for output in &process.outputs {
                if output.amount > 0.0 {
                    producible.insert(output.good);
                }
            }
        }

        // Planning copy. Covered desires spend it. Short desires do not.
        // Tomorrow's time is included, since the grant arrives before satisfy.
        let mut stock: HashMap<usize, f64> = self
            .property
            .iter()
            .filter(|(_, row)| row.quantity > 0.0)
            .map(|(&good, row)| (good, row.quantity))
            .collect();
        let time = self.get_scaling_factor(ScalingFactor::Labor(TIME_PER_LABOR));
        if time > 0.0 {
            *stock.entry(TIME).or_insert(0.0) += time;
        }

        // The saved day is this stock. The meal loop below sets tomorrow's
        // meal aside for the production gap only.
        let mut saving_stock = stock.clone();

        let mut meal: HashMap<usize, f64> = HashMap::new();
        let mut gross: HashMap<usize, f64> = HashMap::new();
        for tier in &self.desires {
            for desire in tier {
                if desire.amount <= 0.0 {
                    continue;
                }
                let covered = Self::best_target(&desire.target, |target| {
                    target.cap >= 1.0
                        && stock.get(&target.good).copied().unwrap_or(0.0)
                            >= desire.amount / target.efficiency
                });
                if let Some(target) = covered {
                    let units = desire.amount / target.efficiency;
                    // The day's meal, before this stock is spent.
                    if producible.contains(&target.good) {
                        *meal.entry(target.good).or_insert(0.0) += units;
                    }
                    let left = stock.get(&target.good).copied().unwrap_or(0.0) - units;
                    if left > 0.0 {
                        stock.insert(target.good, left);
                    } else {
                        stock.remove(&target.good);
                    }
                    continue;
                }
                let Some(target) = Self::best_target(&desire.target, |target| {
                    target.cap >= 1.0 && producible.contains(&target.good)
                }) else {
                    continue;
                };
                let units = desire.amount / target.efficiency;
                *meal.entry(target.good).or_insert(0.0) += units;
                *gross.entry(target.good).or_insert(0.0) += units;
            }
        }
        // The saved day stays off the meal. Stock covers the meal first.
        let (savings, luxury_savings, luxury_savings_open) =
            self.savings_piles(factuals, history, &mut saving_stock);
        let mut save_gross: HashMap<usize, f64> = HashMap::new();
        for (&good, &units) in &savings {
            if units > 0.0 && self.makes_good(factuals, good) {
                save_gross.insert(good, units);
            }
        }
        let mut goods: Vec<usize> = gross
            .keys()
            .copied()
            .chain(save_gross.keys().copied())
            .collect();
        goods.sort_unstable();
        goods.dedup();
        let mut cover = HashMap::new();
        let mut save_gap = HashMap::new();
        for good in goods {
            let meal = gross.get(&good).copied().unwrap_or(0.0);
            let save = save_gross.get(&good).copied().unwrap_or(0.0);
            let have = stock.get(&good).copied().unwrap_or(0.0);
            let meal_short = (meal - have).max(0.0);
            let left = (have - meal).max(0.0);
            let save_short = (save - left).max(0.0);
            if meal_short > 0.0 {
                cover.insert(good, meal_short);
            }
            if save_short > 0.0 {
                save_gap.insert(good, save_short);
            }
        }
        NightNeeds {
            meal,
            cover,
            save_gap,
            savings,
            luxury_savings,
            luxury_savings_open,
        }
    }

    /// # Savings Piles
    ///
    /// Units to hold back, the luxury part of that pile, and whether the walk reached luxury.
    ///
    /// `stock` is the supply each saved day is compared with. A later tier
    /// adds nothing while that stock is short of one ungrossed day of the
    /// earlier tier's winning good. The pile stored, and the stock set
    /// aside for it, is still the larger grossed hold. Desires in the short
    /// tier are still saved. Basic and common pass `in_bucket`, so the day
    /// stays a good that desire eats. Luxury does not. Both maps sum the
    /// desires that were allowed onto each winning good. The flag is false
    /// when that stop comes before the luxury tier.
    fn savings_piles(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        stock: &mut HashMap<usize, f64>,
    ) -> (HashMap<usize, f64>, HashMap<usize, f64>, bool) {
        let mut pile = HashMap::new();
        let mut luxury = HashMap::new();
        let mut luxury_open = true;
        for (tier, desires) in self.desires.iter().enumerate() {
            let mut short = false;
            for desire in desires {
                // Basic and common stay in the bucket, so the day is a meal
                // that can be on hand at dawn. Luxury may leave the bucket.
                let Some((good, day, units)) =
                    self.chosen_buffer(desire, factuals, history, tier < 2)
                else {
                    continue;
                };
                let have = stock.get(&good).copied().unwrap_or(0.0);
                if have < day {
                    short = true;
                }
                let left = have - units;
                if left > 0.0 {
                    stock.insert(good, left);
                } else {
                    stock.remove(&good);
                }
                *pile.entry(good).or_insert(0.0) += units;
                if tier == 2 {
                    *luxury.entry(good).or_insert(0.0) += units;
                }
            }
            if short {
                if tier < 2 {
                    luxury_open = false;
                }
                break;
            }
        }
        (pile, luxury, luxury_open)
    }

    /// # Chosen Buffer
    ///
    /// The good that holds one saved day of `desire`, that day's units, and the whole units to hold.
    ///
    /// `factuals` supplies durability. `history` supplies salability and holding
    /// value. `in_bucket` keeps the search on goods that satisfy `desire`.
    /// The winner is the highest [`Self::savings_rating`]. The eaten good
    /// keeps a tie. The day's units are of the winning good and are not
    /// grossed. The hold is. Fill is scored on that hold. Returns `None`
    /// when every candidate rates at zero.
    fn chosen_buffer(
        &self,
        desire: &Desire,
        factuals: &Factuals,
        history: &MarketHistory,
        in_bucket: bool,
    ) -> Option<(usize, f64, f64)> {
        if desire.amount <= 0.0 {
            return None;
        }
        let eaten = Self::eaten_target(desire)?;
        let mut best: Option<(f64, usize, f64, f64)> = None;
        for good in factuals.each_good() {
            // A skill is practice on the pop, not a good that holds a saved day.
            if good.is_skill() {
                continue;
            }
            if in_bucket
                && !desire
                    .target
                    .iter()
                    .any(|target| target.good == good.id && target.efficiency > 0.0)
            {
                continue;
            }
            let Some((day, units)) = Self::buffer_units(desire, good, eaten, history) else {
                continue;
            };
            let fill = self.savings_fill(factuals, good.id, units);
            let rating = Self::savings_rating(history.salability(good.id), good.durability(), fill);
            if rating <= 0.0 || !Self::rating_beats(rating, good.id, eaten.good, best) {
                continue;
            }
            best = Some((rating, good.id, day, units));
        }
        best.map(|(_, id, day, units)| (id, day, units))
    }

    /// # Rating Beats
    ///
    /// True when `rating` on `id` should replace `best`.
    ///
    /// A higher rating wins. An equal rating keeps the eaten good, then the
    /// lower id. `eaten` is that good's id.
    fn rating_beats(
        rating: f64,
        id: usize,
        eaten: usize,
        best: Option<(f64, usize, f64, f64)>,
    ) -> bool {
        let Some((was, best_id, _, _)) = best else {
            return true;
        };
        match rating.partial_cmp(&was) {
            Some(std::cmp::Ordering::Greater) => true,
            Some(std::cmp::Ordering::Equal) => {
                let eaten_now = id == eaten;
                let eaten_best = best_id == eaten;
                if eaten_now != eaten_best {
                    eaten_now
                } else {
                    id < best_id
                }
            }
            _ => false,
        }
    }

    /// # Savings Rating
    ///
    /// How well one good holds a saved day.
    ///
    /// `salability` is liquidity. `durability` is one minus decay. `fill` is
    /// how much of a try to obtain the good succeeded. The rating is the
    /// product. A good that decays completely, or that this pop did not
    /// obtain, rates 0.
    fn savings_rating(salability: f64, durability: f64, fill: f64) -> f64 {
        if durability <= 0.0 || fill <= 0.0 || salability <= 0.0 || !salability.is_finite() {
            return 0.0;
        }
        salability * durability * fill
    }

    /// # Savings Fill
    ///
    /// How obtainable `good` is for a saved day of `units`.
    ///
    /// Returns 1 when this pop's job makes the good, or `quantity` already
    /// holds `units`. Otherwise returns the share of today's posted buy that
    /// arrived, or 0 when nothing was posted.
    fn savings_fill(&self, factuals: &Factuals, good: usize, units: f64) -> f64 {
        if self.makes_good(factuals, good) {
            return 1.0;
        }
        let held = self
            .property
            .get(&good)
            .map(|row| row.quantity)
            .unwrap_or(0.0);
        if units > 0.0 && held >= units {
            return 1.0;
        }
        let posted = self.buy_posted.get(&good).copied().unwrap_or(0.0);
        if posted > 0.0 {
            let got = self.buy_received.get(&good).copied().unwrap_or(0.0);
            return (got / posted).clamp(0.0, 1.0);
        }
        0.0
    }

    /// # Makes Good
    ///
    /// True when a job line outputs `good`.
    ///
    /// `factuals` supplies the process. A missing process does not count.
    /// An output of zero does not count.
    fn makes_good(&self, factuals: &Factuals, good: usize) -> bool {
        self.job.lines.iter().any(|line| {
            factuals.get_process(line.process).is_some_and(|process| {
                process
                    .outputs
                    .iter()
                    .any(|output| output.good == good && output.amount > 0.0)
            })
        })
    }

    /// # Eaten Target
    ///
    /// The good `desire` would eat for one level.
    ///
    /// Prefers a target whose cap can finish the level. Otherwise the highest
    /// efficiency. Returns `None` when every efficiency is at or below zero.
    fn eaten_target(desire: &Desire) -> Option<&DesireTarget> {
        Self::best_target(&desire.target, |target| target.cap >= 1.0)
            .or_else(|| Self::best_target(&desire.target, |_| true))
    }

    /// # Buffer Units
    ///
    /// The ungrossed day of `good` for `desire`, and the whole units to hold.
    ///
    /// `eaten` is the good that day would have been. `history` prices an
    /// outside good off that day's holding value. The day is [`SAVINGS_DAYS`]
    /// long, in this good's units. The hold divides that day by durability
    /// so one day remains after rot. A good that decays completely returns
    /// `None`. An outside day whose holding value is not positive returns
    /// `None`. An exact outside total adds one unit to the hold.
    fn buffer_units(
        desire: &Desire,
        good: &Good,
        eaten: &DesireTarget,
        history: &MarketHistory,
    ) -> Option<(f64, f64)> {
        let durability = good.durability();
        if durability <= 0.0 {
            return None;
        }
        let bucket = desire
            .target
            .iter()
            .filter(|target| target.good == good.id && target.efficiency > 0.0)
            .max_by(|a, b| {
                a.efficiency
                    .partial_cmp(&b.efficiency)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        let outside = bucket.is_none();
        let day = if let Some(target) = bucket {
            SAVINGS_DAYS * desire.amount / target.efficiency
        } else {
            let eaten_units = SAVINGS_DAYS * desire.amount / eaten.efficiency;
            let eaten_hold = history.holding_per_unit(eaten.good);
            let save_hold = history.holding_per_unit(good.id);
            if eaten_units <= 0.0 || eaten_hold <= 0.0 || save_hold <= 0.0 {
                return None;
            }
            eaten_units * eaten_hold / save_hold
        };
        if !day.is_finite() || day <= 0.0 {
            return None;
        }
        let gross = day / durability;
        if !gross.is_finite() || gross <= 0.0 {
            return None;
        }
        let whole = whole_units_up(gross);
        let units = if outside && gross == gross.floor() {
            whole + 1.0
        } else {
            whole
        };
        if units >= 1.0 {
            Some((day, units))
        } else {
            None
        }
    }

    /// # Savings Of
    ///
    /// Units of `good` in [`Self::savings`], or 0 when it is not saved.
    fn savings_of(&self, good: usize) -> f64 {
        self.savings.get(&good).copied().unwrap_or(0.0).max(0.0)
    }

    /// # Luxury Buffer Ready
    ///
    /// True when a further luxury level may be eaten.
    ///
    /// False when [`Self::luxury_savings_open`] is false. Otherwise every
    /// entry in [`Self::luxury_savings`] is still in `quantity`. An empty
    /// map passes.
    fn luxury_buffer_ready(&self) -> bool {
        if !self.luxury_savings_open {
            return false;
        }
        self.luxury_savings.iter().all(|(&good, &units)| {
            self.property
                .get(&good)
                .map(|row| row.quantity)
                .unwrap_or(0.0)
                >= units
        })
    }

    /// # Add Savings Buys
    ///
    /// Adds the whole units of each saved good still short of [`Self::savings`].
    ///
    /// `orders` is the desire buy already posted. A good already on that list
    /// has the shortfall added. Stock [`Self::free_units`] already holds counts
    /// toward the pile. A shortfall under one unit is skipped.
    fn add_savings_buys(&self, orders: &mut Vec<MarketOrder>) {
        let mut goods: Vec<usize> = self.savings.keys().copied().collect();
        goods.sort_unstable();
        for good in goods {
            let short = (self.savings_of(good) - self.free_units(good)).max(0.0);
            let units = whole_units_up(short);
            if units < 1.0 {
                continue;
            }
            if let Some(existing) = orders.iter_mut().find(|order| order.target == good) {
                existing.target_amount += units;
            } else {
                orders.push(MarketOrder::buy(Actor::Pop(self.id), good, units));
            }
        }
    }

    /// # Add Upgrade Buys
    ///
    /// Moves a desire's level onto a cheaper, higher-efficiency substitute.
    ///
    /// `history` is yesterday's card. `orders` is the stopped desire's buy,
    /// before a saved day is added. Each desire that still holds a
    /// lower-efficiency target shops [`Self::upgrade_good`] for the units
    /// that finish that desire's levels, minus units of it already on hand.
    /// Basic and common ask for one level. Luxury asks for
    /// [`Self::luxury_reached`] levels, and at least one. On the desire
    /// [`Self::satisfy`] stopped on, the other targets of that desire come
    /// off `orders`. A good already listed is raised when this amount is
    /// larger. The held good stays in the bucket.
    fn add_upgrade_buys(&self, history: &MarketHistory, orders: &mut Vec<MarketOrder>) {
        let mut swaps = Vec::new();
        for (tier, desires) in self.desires.iter().enumerate() {
            for (index, desire) in desires.iter().enumerate() {
                let Some(good) = self.upgrade_good(desire, history) else {
                    continue;
                };
                let Some(target) = desire.target.iter().find(|target| target.good == good) else {
                    continue;
                };
                let units = self.swap_units(tier, desire, target);
                if units < 1.0 {
                    continue;
                }
                let stopped = self.satisfy_cursor.is_some_and(|cursor| {
                    cursor.tier == tier && cursor.desire_index == index
                });
                let replaced: Vec<usize> = desire
                    .target
                    .iter()
                    .map(|target| target.good)
                    .filter(|held| *held != good)
                    .collect();
                swaps.push((good, units, replaced, stopped));
            }
        }
        // The level's other options come off before the substitute is written.
        for (_, _, replaced, stopped) in &swaps {
            if !stopped {
                continue;
            }
            orders.retain(|order| !replaced.contains(&order.target));
        }
        let mut placed = HashSet::new();
        for (good, units, _, _) in swaps {
            if let Some(existing) = orders.iter_mut().find(|order| order.target == good) {
                if placed.contains(&good) {
                    existing.target_amount += units;
                } else {
                    existing.target_amount = existing.target_amount.max(units);
                    placed.insert(good);
                }
            } else {
                orders.push(MarketOrder::buy(Actor::Pop(self.id), good, units));
                placed.insert(good);
            }
        }
    }

    /// # Swap Units
    ///
    /// Whole units of `target` that cover the levels `desire` is shopping.
    ///
    /// `tier` is the desire's tier. Basic and common cover one level. Luxury
    /// covers [`Self::luxury_reached`] levels, and at least one. Units of
    /// `target` already in `quantity` are subtracted. The result is rounded
    /// away from zero. A covered level is `0`.
    fn swap_units(&self, tier: usize, desire: &Desire, target: &DesireTarget) -> f64 {
        if target.efficiency <= 0.0 {
            return 0.0;
        }
        let levels = if tier == 2 {
            self.luxury_reached.max(1.0)
        } else {
            1.0
        };
        let need = levels * desire.amount * target.cap / target.efficiency;
        let held = self
            .property
            .get(&target.good)
            .map(|row| row.quantity)
            .unwrap_or(0.0);
        whole_units_up((need - held).max(0.0))
    }

    /// # Upgrade Good
    ///
    /// One higher-efficiency substitute that is cheaper per point of satisfaction.
    ///
    /// `desire` is one desire. `history` prices each target that is on the
    /// card. The pop must hold at least one unit of a target. That held
    /// target with the lowest efficiency is the one a swap would replace. A
    /// target of higher efficiency is chosen when its AMV divided by
    /// efficiency is strictly lower. Among those, the lowest cost wins, then
    /// the higher efficiency. Returns that good, or nothing when no swap is
    /// cheaper.
    fn upgrade_good(&self, desire: &Desire, history: &MarketHistory) -> Option<usize> {
        if desire.amount <= 0.0 {
            return None;
        }
        // The least efficient good already on hand.
        let mut incumbent: Option<(f64, f64)> = None;
        for target in &desire.target {
            let Some(cost) = Self::priced_satisfaction(history, target.good, target.efficiency) else {
                continue;
            };
            if !self.holds_one(target.good) {
                continue;
            }
            let take = match incumbent {
                None => true,
                Some((efficiency, held)) => {
                    target.efficiency < efficiency
                        || (target.efficiency == efficiency && cost > held)
                }
            };
            if take {
                incumbent = Some((target.efficiency, cost));
            }
        }
        let (floor_efficiency, floor_cost) = incumbent?;
        // The cheaper substitute of higher efficiency.
        let mut best: Option<(usize, f64, f64)> = None;
        for target in &desire.target {
            if target.efficiency <= floor_efficiency {
                continue;
            }
            let Some(cost) = Self::priced_satisfaction(history, target.good, target.efficiency) else {
                continue;
            };
            if cost >= floor_cost {
                continue;
            }
            let take = match best {
                None => true,
                Some((_, efficiency, held)) => {
                    cost < held || (cost == held && target.efficiency > efficiency)
                }
            };
            if take {
                best = Some((target.good, target.efficiency, cost));
            }
        }
        best.map(|(good, _, _)| good)
    }

    /// # Priced Satisfaction
    ///
    /// AMV of one point of satisfaction from `good` at `efficiency`.
    ///
    /// `history` supplies the AMV. A good missing from the card, a
    /// non-positive AMV, or a non-positive efficiency returns nothing.
    fn priced_satisfaction(history: &MarketHistory, good: usize, efficiency: f64) -> Option<f64> {
        if efficiency <= 0.0 || !efficiency.is_finite() || !history.prices.contains_key(&good) {
            return None;
        }
        let amv = history.price(good);
        if amv <= 0.0 || !amv.is_finite() {
            return None;
        }
        let cost = amv / efficiency;
        cost.is_finite().then_some(cost)
    }

    /// # Holds One
    ///
    /// True when `quantity` of `good` is at least one unit.
    ///
    /// Reserved units still count. A missing row is false.
    fn holds_one(&self, good: usize) -> bool {
        self.property
            .get(&good)
            .map(|row| row.quantity >= 1.0)
            .unwrap_or(false)
    }

    /// # Add Cheaper Cover
    ///
    /// Raises each order to the units in [`Self::cheaper_cover`].
    ///
    /// `orders` is the buy already posted. A good already listed stays at
    /// the larger amount. A good not listed is added. A stored amount under
    /// one unit is skipped.
    fn add_cheaper_cover(&self, orders: &mut Vec<MarketOrder>) {
        let mut goods: Vec<usize> = self.cheaper_cover.keys().copied().collect();
        goods.sort_unstable();
        for good in goods {
            let units = whole_units_up(self.cheaper_cover.get(&good).copied().unwrap_or(0.0));
            if units < 1.0 {
                continue;
            }
            if let Some(existing) = orders.iter_mut().find(|order| order.target == good) {
                if units > existing.target_amount {
                    existing.target_amount = units;
                }
            } else {
                orders.push(MarketOrder::buy(Actor::Pop(self.id), good, units));
            }
        }
    }

    /// # Note Posted Buys
    ///
    /// Raises today's posted buy to the amounts [`Self::buy_orders`] would post.
    ///
    /// A later call keeps the larger amount. The morning reset clears the tape.
    fn note_posted_buys(&mut self) {
        for order in self.buy_orders(&MarketHistory::new()) {
            if order.target_amount <= 0.0 {
                continue;
            }
            let posted = self.buy_posted.entry(order.target).or_insert(0.0);
            if order.target_amount > *posted {
                *posted = order.target_amount;
            }
        }
    }

    /// # Best Target
    ///
    /// The highest-efficiency target in `targets` that `keep` accepts.
    ///
    /// A target with efficiency at or below zero is skipped. A tie keeps
    /// the later target. Returns that target, or `None` when none are kept.
    fn best_target<'a>(
        targets: &'a [DesireTarget],
        keep: impl Fn(&DesireTarget) -> bool,
    ) -> Option<&'a DesireTarget> {
        targets
            .iter()
            .filter(|target| target.efficiency > 0.0 && keep(target))
            .max_by(|a, b| {
                a.efficiency
                    .partial_cmp(&b.efficiency)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
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
    /// fresh output are not counted as rot. The rot is stored on
    /// [`PopPRow::lost`].
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
            row.lost = lost;
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
    /// bonus satisfaction from the desire and pop effects. The sum is
    /// [`Self::standard_of_living`]. Stored satisfaction is then removed.
    pub fn calculate_sol(&mut self, factuals: &Factuals) -> f64 {
        // sanity check, if a pop has no desires, then return +inf
        if self.desires.iter().all(|tier| tier.is_empty()) {
            self.records.satisfaction = f64::INFINITY;
            return f64::INFINITY;
        }
        let sum = self.standard_of_living();
        // Drop the stored satisfaction now that the sum includes it.
        let _ = self.satisfaction_from_pop_effects();
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

    /// # Buys For Stopped Desire
    ///
    /// One buy for each good that can still fill the desire [`Pop::satisfy`] stopped on.
    ///
    /// Each amount is the whole units of that good that finish the open level,
    /// rounded away from zero, and no more than that good's remaining cap on
    /// the level. A luxury desire that finished more than one level last
    /// night ([`Self::luxury_reached`]) buys that many levels, each capped
    /// the same way. Payment is chosen later, when a seller's book is known.
    ///
    /// Empty when nothing is stopped on, or every gap is under one unit.
    fn buys_for_stopped_desire(&self) -> Vec<MarketOrder> {
        let Some(cursor) = self.satisfy_cursor else {
            return Vec::new();
        };
        let Some(desire) = self.desires.get(cursor.tier).and_then(|tier| tier.get(cursor.desire_index)) else {
            return Vec::new();
        };
        let level_left = cursor.iter_target * desire.amount - desire.satisfaction;
        if level_left <= 1e-9 {
            return Vec::new();
        }
        let levels = if cursor.tier == 2 {
            self.luxury_reached.max(1.0)
        } else {
            1.0
        };
        let mut orders = Vec::new();
        for (index, target) in desire.target.iter().enumerate() {
            if target.efficiency <= 0.0 {
                continue;
            }
            let cap_left = target_cap_left(desire, index);
            if cap_left <= 1e-9 {
                continue;
            }
            // This level's open share, then one full cap for each later level.
            let open = cap_left.min(level_left).max(0.0) / target.efficiency;
            let later = (levels - 1.0).max(0.0) * desire.amount * target.cap / target.efficiency;
            let units = whole_units_up(open + later);
            if units < 1.0 {
                continue;
            }
            orders.push(MarketOrder::buy(Actor::Pop(self.id), target.good, units));
        }
        orders
    }

    /// # Payment Goods
    ///
    /// Seller requests first, then the buyer's other free goods.
    ///
    /// `factuals` distinguishes transport. `avoid` is the good being bought.
    /// `skip` ids are left out. A good is included when [`Self::spendable`]
    /// for that purchase is at least one unit. Free goods go highest monetary
    /// rating first, then lowest id. Unrequested transport is listed after
    /// the other free goods. [`Self::add_payment`] then keeps a freight
    /// reserve of that transport.
    fn payment_goods(
        &self,
        factuals: &Factuals,
        book: &SellerBook,
        history: &MarketHistory,
        avoid: usize,
        skip: &[usize],
    ) -> Vec<usize> {
        let mut goods: Vec<usize> = book
            .requests
            .iter()
            .map(|order| order.target)
            .filter(|good| {
                *good != avoid && !skip.contains(good) && self.spendable(*good, avoid) >= 1.0
            })
            .collect();
        let mut ordinary = Vec::new();
        let mut transport = Vec::new();
        for (good, row) in &self.property {
            if *good == avoid || goods.contains(good) || skip.contains(good) {
                continue;
            }
            if row.available().floor() < 1.0 || self.spendable(*good, avoid) < 1.0 {
                continue;
            }
            let rating = crate::game::market::monetary_rating(history.salability(*good));
            if self.is_unrequested_transport(factuals, book, *good) {
                transport.push((*good, rating));
            } else {
                ordinary.push((*good, rating));
            }
        }
        let by_pay = |a: &(usize, f64), b: &(usize, f64)| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        };
        ordinary.sort_by(by_pay);
        transport.sort_by(by_pay);
        goods.extend(ordinary.into_iter().map(|(good, _)| good));
        goods.extend(transport.into_iter().map(|(good, _)| good));
        goods
    }

    /// # Desire Units
    ///
    /// Whole units of `good` that still feed a desire.
    ///
    /// Each direct target contributes the whole units that fill its remaining
    /// room ([`desire_room`]). An input the job consumes to make a desired
    /// output contributes the input units for that same room. A full cap
    /// contributes nothing. The result is not limited by stock on hand.
    fn desire_units(&self, good: usize) -> f64 {
        let mut units = 0.0;
        for desires in &self.desires {
            for desire in desires {
                for (index, target) in desire.target.iter().enumerate() {
                    if target.good != good || target.efficiency <= 0.0 {
                        continue;
                    }
                    let room = desire_room(desire, index);
                    units += (room / target.efficiency).floor();
                }
            }
        }
        for (output, per_output) in self.job.feeds_of(good) {
            if per_output <= 0.0 {
                continue;
            }
            for desires in &self.desires {
                for desire in desires {
                    for (index, target) in desire.target.iter().enumerate() {
                        if target.good != output || target.efficiency <= 0.0 {
                            continue;
                        }
                        let output_units = (desire_room(desire, index) / target.efficiency).floor();
                        units += (output_units * per_output).floor();
                    }
                }
            }
        }
        units
    }

    /// # Surplus For Sale
    ///
    /// Whole free units of `good` above [`Self::desire_units`] and [`Self::savings`].
    ///
    /// Free units are [`Self::free_units`]. Units a desire reserved are
    /// already in its satisfaction, so they are out of the room. Units the
    /// job claimed ([`Job::claimed_of`]) count toward the desire they feed,
    /// so they are not held back a second time. Sell orders and payment
    /// both use this amount.
    fn surplus_for_sale(&self, good: usize) -> f64 {
        let free = self.free_units(good).floor();
        let still = (self.desire_units(good) - self.job.claimed_of(good)).max(0.0);
        (free - still - self.savings_of(good)).max(0.0).floor()
    }

    /// # Spendable
    ///
    /// Whole free units of `good` the buyer may offer while buying `buying`.
    ///
    /// Surplus above every desire is always included, via
    /// [`Self::surplus_for_sale`], when `buying` feeds no open desire.
    /// [`Self::savings`] is taken out of the free units first. When `buying`
    /// still feeds a desire, units of `good` placed on that tier or an
    /// earlier one stay. Units placed only on a later tier, and any units
    /// past every desire, are spendable. Same-tier goods are not spent on
    /// each other. Sell orders stay on the surplus alone.
    fn spendable(&self, good: usize, buying: usize) -> f64 {
        let offerable = (self.free_units(good).floor() - self.savings_of(good))
            .max(0.0)
            .floor();
        if offerable < 1.0 {
            return 0.0;
        }
        let Some(limit) = self.earliest_use(buying) else {
            return self.surplus_for_sale(good);
        };
        let keep: f64 = self
            .end_uses(good, offerable)
            .into_iter()
            .filter(|use_| tier_through(use_.tier, limit.0))
            .map(|use_| use_.units)
            .sum();
        (offerable - keep).max(0.0).floor()
    }

    /// # Earliest Use
    ///
    /// Tier and list index of the first open desire `good` still feeds.
    ///
    /// A missing use means the good fills nothing that still has room.
    /// The walk is the same one [`Self::end_uses`] uses.
    fn earliest_use(&self, good: usize) -> Option<(usize, usize)> {
        self.end_uses(good, f64::MAX)
            .into_iter()
            .find(|use_| use_.units >= 1.0)
            .map(|use_| (use_.tier, use_.order))
    }

    /// # End Uses
    ///
    /// Whole units of `good` placed on the earliest desire that still has room.
    ///
    /// `good` is the good id. `qty` is the units to place. Direct targets are
    /// placed first. Units still left are then placed on a desire whose good
    /// this input is consumed to make. [`Job::reserve`] records those links.
    /// Returns one [`EndUse`] per desire that received units.
    fn end_uses(&self, good: usize, qty: f64) -> Vec<EndUse> {
        let mut left = if qty.is_finite() && qty > 0.0 {
            qty.floor()
        } else {
            0.0
        };
        let mut found = Vec::new();
        for (tier, desires) in self.desires.iter().enumerate() {
            if left < 1.0 {
                break;
            }
            for (order, desire) in desires.iter().enumerate() {
                if left < 1.0 {
                    break;
                }
                let Some(index) = desire.target.iter().position(|target| target.good == good) else {
                    continue;
                };
                let target = &desire.target[index];
                if target.efficiency <= 0.0 {
                    continue;
                }
                let room = desire_room(desire, index);
                let take = (room / target.efficiency).floor().min(left);
                if take < 1.0 {
                    continue;
                }
                found.push(EndUse {
                    tier,
                    order,
                    satisfaction: take * target.efficiency,
                    units: take,
                });
                left -= take;
            }
        }
        self.place_input_uses(good, &mut left, &mut found);
        found
    }

    /// # Place Input Uses
    ///
    /// Places leftover units of an input onto the desire its output feeds.
    ///
    /// `good` is the input. `left` is whole units not yet placed on a direct
    /// target. `found` collects the uses. One output takes `per_output` input
    /// units. Satisfaction is the output units made times that desire's
    /// efficiency. `left` falls by the input units spent.
    fn place_input_uses(&self, good: usize, left: &mut f64, found: &mut Vec<EndUse>) {
        let feeds = self.job.feeds_of(good);
        if feeds.is_empty() {
            return;
        }
        for (tier, desires) in self.desires.iter().enumerate() {
            for (order, desire) in desires.iter().enumerate() {
                if *left < 1.0 {
                    return;
                }
                for (index, target) in desire.target.iter().enumerate() {
                    if target.efficiency <= 0.0 {
                        continue;
                    }
                    let Some(per_output) = feeds
                        .iter()
                        .find(|(output, _)| *output == target.good)
                        .map(|(_, per)| *per)
                    else {
                        continue;
                    };
                    if per_output <= 0.0 {
                        continue;
                    }
                    let output_room = (desire_room(desire, index) / target.efficiency).floor();
                    let made = (*left / per_output).floor().min(output_room);
                    if made < 1.0 {
                        continue;
                    }
                    let spent = made * per_output;
                    found.push(EndUse {
                        tier,
                        order,
                        satisfaction: made * target.efficiency,
                        units: spent,
                    });
                    *left -= spent;
                }
            }
        }
    }

    /// # Exchange Ok
    ///
    /// Pop verdict on giving `given` and receiving `received`.
    ///
    /// Both maps are positive quantities. Firms do not use this. A unit that
    /// still feeds an earlier desire than the best end received is refused.
    /// Satisfaction is then one total: every end received against every end
    /// given up. A strict satisfaction gain accepts, and the raw AMV of the
    /// basket does not veto it. With no satisfaction on either side, the
    /// raw AMV received must meet the raw AMV given. Equality meets that
    /// price. Returns whether the pop accepts.
    fn exchange_ok(
        &self,
        given: &HashMap<usize, f64>,
        received: &HashMap<usize, f64>,
        history: &MarketHistory,
    ) -> bool {
        let (given_uses, received_uses) = self.exchange_uses(given, received);
        if self.exchange_blocked(&given_uses, &received_uses) {
            return false;
        }
        let (gain, loss) = satisfaction_change(&received_uses, &given_uses);
        if gain > loss {
            return true;
        }
        if gain == 0.0 && loss == 0.0 {
            return raw_amv(history, received) >= raw_amv(history, given);
        }
        false
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

    /// # Buy Transport For Freight
    ///
    /// Buy transport the seller is offering when that lowers the unpaid freight.
    ///
    /// `skip` goods are not used to pay for that transport. Returns `None`
    /// when freight remains and no further purchase helps. Returns `Some(())`
    /// when stock plus transport in the basket can pay it.
    fn buy_transport_for_freight(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        book: &SellerBook,
        goods: &mut HashMap<usize, f64>,
        covered: &mut f64,
        skip: &[usize],
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
                let Some(row) = factuals.get_good(order.target) else {
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
                let price = history.price(order.target);
                if price <= 0.0
                    || !self.add_payment(
                        factuals,
                        history,
                        book,
                        order.target,
                        &mut trial,
                        &mut trial_covered,
                        price,
                        skip,
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

    /// # Can Propose One
    ///
    /// Whether [`Self::propose_quantity`] accepts one unit of `good`.
    ///
    /// `offers` and `requests` are the seller's book. `history` prices the
    /// goods. `factuals` supplies transport. A stored price of zero returns
    /// false. The deal itself is discarded.
    pub(crate) fn can_propose_one(
        &self,
        good: usize,
        offers: &[MarketOrder],
        requests: &[MarketOrder],
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> bool {
        if history.price(good) == 0.0 {
            return false;
        }
        let book = SellerBook {
            seller: self.actor(),
            offers: offers.to_vec(),
            requests: requests.to_vec(),
        };
        self.propose_quantity(good, 1.0, &book, history, factuals)
            .is_ok()
    }

    /// # Add Payment
    ///
    /// Add `need` more raw AMV of payment. Unrequested transport keeps a
    /// freight reserve, so the bill can still be paid from what remains.
    ///
    /// `covered` is raw AMV already offered. Each payment good counts at
    /// [`MarketHistory::price`], not at its salability-scaled holding.
    /// `skip` goods are not offered. Returns when `covered` reaches
    /// `covered + need`. The whole unit that crosses that price is the last
    /// unit added.
    fn add_payment(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        book: &SellerBook,
        avoid: usize,
        goods: &mut HashMap<usize, f64>,
        covered: &mut f64,
        need: f64,
        skip: &[usize],
    ) -> bool {
        let target = *covered + need;
        for good in self.payment_goods(factuals, book, history, avoid, skip) {
            if *covered >= target {
                return true;
            }
            let per = history.price(good);
            if per <= 0.0 {
                continue;
            }
            let free = self.payable_after_freight(factuals, history, book, goods, good, avoid);
            let free = self.skill_offer_room(factuals, goods, good, free);
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

    /// # Skill Offer Room
    ///
    /// How many units of `good` this basket may still offer.
    ///
    /// `factuals` says whether `good` is a skill. `goods` is the basket so
    /// far, and payment is a negative amount. `free` is the units otherwise
    /// payable. A skill is offered once: the basket takes at most one unit.
    /// Any other good returns `free`.
    fn skill_offer_room(
        &self,
        factuals: &Factuals,
        goods: &HashMap<usize, f64>,
        good: usize,
        free: f64,
    ) -> f64 {
        if !factuals.get_good(good).is_some_and(|row| row.is_skill()) {
            return free;
        }
        let offered = (-goods.get(&good).copied().unwrap_or(0.0)).max(0.0);
        (1.0 - offered).max(0.0).min(free)
    }

    /// True when `good` is transport and the seller did not request it.
    ///
    /// `factuals` supplies the transport tag. `book` is the seller's requests.
    /// Requested transport is ordinary payment. Unrequested transport is spent
    /// only above [`Self::freight_reserve`].
    fn is_unrequested_transport(
        &self,
        factuals: &Factuals,
        book: &SellerBook,
        good: usize,
    ) -> bool {
        let is_transport = factuals
            .get_good(good)
            .is_some_and(|row| row.is_transport());
        let requested = book.requests.iter().any(|order| order.target == good);
        is_transport && !requested
    }

    /// Whole transport units `goods` must leave in stock to cover its freight.
    ///
    /// `factuals` prices the bill and the efficiency. `history` supplies
    /// friction. `good` is the transport good. A missing efficiency is `0`,
    /// and then the reserve is `0`. The bill is [`crate::game::deal::freight_bill`]
    /// of the basket so far. The result is that bill divided by efficiency,
    /// rounded up.
    fn freight_reserve(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        goods: &HashMap<usize, f64>,
        good: usize,
    ) -> f64 {
        let efficiency = factuals
            .get_good(good)
            .map(|row| row.transport_efficiency())
            .unwrap_or(0.0);
        if efficiency <= 0.0 {
            return 0.0;
        }
        let bill = crate::game::deal::freight_bill(factuals, history.friction, goods);
        (bill / efficiency).ceil()
    }

    /// Units of `good` still free to pay with, after the freight reserve.
    ///
    /// `factuals` and `history` price the reserve. `book` and `goods` are the
    /// seller's book and the basket so far. `buying` is the good being bought.
    /// Unrequested transport subtracts [`Self::freight_reserve`]. Other goods
    /// return [`Self::payable_units`].
    fn payable_after_freight(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        book: &SellerBook,
        goods: &HashMap<usize, f64>,
        good: usize,
        buying: usize,
    ) -> f64 {
        let free = self.payable_units(book, goods, good, buying);
        if !self.is_unrequested_transport(factuals, book, good) {
            return free;
        }
        (free - self.freight_reserve(factuals, history, goods, good)).max(0.0)
    }

    /// # Payable Units
    ///
    /// Units of `good` still available to put in `goods` while buying `buying`.
    ///
    /// `book` is the seller's offers and requests. `goods` is the basket so
    /// far. Units already in the basket are subtracted from
    /// [`Self::spendable`]. A seller request caps the amount.
    fn payable_units(
        &self,
        book: &SellerBook,
        goods: &HashMap<usize, f64>,
        good: usize,
        buying: usize,
    ) -> f64 {
        let already = goods.get(&good).copied().unwrap_or(0.0).min(0.0).abs();
        let mut free = self.spendable(good, buying) - already;
        if let Some(request) = book.requests.iter().find(|order| order.target == good) {
            free = free.min(request.target_amount.floor() - already).max(0.0);
        }
        free
    }

    /// # Buyer Accepts
    ///
    /// Buyer side of the verdict [`Self::evaluate`] uses for the seller.
    ///
    /// `goods` is the basket, positive toward the buyer. `history` supplies
    /// raw AMV. The tier and satisfaction checks match the seller. A basket
    /// that only misses because whole units paid more raw AMV than they
    /// received is still offered: the seller's price was the target, and the
    /// seller still requires that price or a satisfaction gain. Returns
    /// whether the buyer puts the basket on the table.
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
            || self.exchange_why(&given, &received, history) == "price_short"
    }

    /// # Exchange Uses
    ///
    /// End uses of the goods given and the goods received.
    ///
    /// `given` and `received` are positive quantities. Surplus is given
    /// before units a desire still needs, so a gift inside the surplus has
    /// no end use. Returns the two lists.
    fn exchange_uses(
        &self,
        given: &HashMap<usize, f64>,
        received: &HashMap<usize, f64>,
    ) -> (Vec<EndUse>, Vec<EndUse>) {
        let mut given_uses = Vec::new();
        for (&good, &qty) in given {
            let cutting = (qty - self.surplus_for_sale(good)).max(0.0);
            given_uses.extend(self.end_uses(good, cutting));
        }
        let mut received_uses = Vec::new();
        for (&good, &qty) in received {
            received_uses.extend(self.end_uses(good, qty));
        }
        (given_uses, received_uses)
    }

    /// # Exchange Blocked
    ///
    /// Whether a given end outranks the best end received.
    ///
    /// Earlier is a lower tier, or the same tier and an earlier list index.
    /// A given end with nothing received is blocked. Returns that answer.
    fn exchange_blocked(&self, given_uses: &[EndUse], received_uses: &[EndUse]) -> bool {
        let best = received_uses.iter().min_by(|a, b| {
            a.tier.cmp(&b.tier).then(a.order.cmp(&b.order))
        });
        given_uses.iter().any(|use_| match best {
            None => true,
            Some(best) => {
                use_.tier < best.tier || (use_.tier == best.tier && use_.order < best.order)
            }
        })
    }

    /// # Exchange Why
    ///
    /// Names the check [`Self::exchange_ok`] is on.
    ///
    /// `given` and `received` are positive quantities. `history` supplies
    /// raw AMV. A refusal is `"blocked"`, `"sat_short"`, or `"price_short"`.
    /// A pass is `"sat_gain"` or `"price_met"`.
    fn exchange_why(
        &self,
        given: &HashMap<usize, f64>,
        received: &HashMap<usize, f64>,
        history: &MarketHistory,
    ) -> &'static str {
        let (given_uses, received_uses) = self.exchange_uses(given, received);
        if self.exchange_blocked(&given_uses, &received_uses) {
            return "blocked";
        }
        let (gain, loss) = satisfaction_change(&received_uses, &given_uses);
        if gain > loss {
            return "sat_gain";
        }
        if gain == 0.0 && loss == 0.0 && raw_amv(history, received) >= raw_amv(history, given) {
            return "price_met";
        }
        if gain > 0.0 || loss > 0.0 {
            "sat_short"
        } else {
            "price_short"
        }
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

    /// # Bulky Payment
    ///
    /// The non-transport good in `goods` that adds the most bulk.
    ///
    /// `factuals` supplies bulk and the transport tag. Only negative
    /// quantities are payment. A missing good counts as bulk `0`. Equal
    /// bulk selects the higher id. Returns `None` when every payment good
    /// is transport.
    fn bulky_payment(factuals: &Factuals, goods: &HashMap<usize, f64>) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for (&id, &qty) in goods {
            if qty >= 0.0 {
                continue;
            }
            let row = factuals.get_good(id);
            if row.is_some_and(|good| good.is_transport()) {
                continue;
            }
            let bulk = row.map(|good| good.bulk()).unwrap_or(0.0);
            let replace = match best {
                None => true,
                Some((held, score)) => bulk > score || (bulk == score && id > held),
            };
            if replace {
                best = Some((id, bulk));
            }
        }
        best.map(|(id, _)| id)
    }

    /// # Propose Quantity
    ///
    /// One basket for `qty` of `match_good`, or why that size was refused.
    ///
    /// `book` is the seller's offers and requests. `history` prices the
    /// goods. `factuals` supplies transport. Payment, freight, and the
    /// buyer's own check all have to pass. When the freight bill is the
    /// check that fails, the bulkiest non-transport good in that basket is
    /// left out and the same quantity is tried again. That stops when the
    /// basket pays or every remaining payment good is transport. Any other
    /// failure is returned as it is. The `Err` string is the check that
    /// stopped the last try.
    fn propose_quantity(
        &self,
        match_good: usize,
        qty: f64,
        book: &SellerBook,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> Result<ProposedDeal, &'static str> {
        let mut skip = Vec::new();
        loop {
            match self.propose_skipped(match_good, qty, book, history, factuals, &skip) {
                Ok(deal) => return Ok(deal),
                Err((reason, goods)) if reason == "freight" || reason == "freight_left" => {
                    match Self::bulky_payment(factuals, &goods) {
                        Some(drop) if !skip.contains(&drop) => skip.push(drop),
                        _ => return Err(reason),
                    }
                }
                Err((reason, _)) => return Err(reason),
            }
        }
    }

    /// # Propose Skipped
    ///
    /// One basket for `qty` of `match_good`, leaving out `skip`.
    ///
    /// Same checks as [`Self::propose_quantity`]. On failure the basket that
    /// failed is returned with the check that failed.
    fn propose_skipped(
        &self,
        match_good: usize,
        qty: f64,
        book: &SellerBook,
        history: &MarketHistory,
        factuals: &Factuals,
        skip: &[usize],
    ) -> Result<ProposedDeal, (&'static str, HashMap<usize, f64>)> {
        let mut goods = HashMap::from([(match_good, qty)]);
        let mut covered = 0.0;
        let ask = book
            .offers
            .iter()
            .find(|order| order.target == match_good && order.target_amount < 0.0)
            .map(|order| dictated_price(order, history))
            .unwrap_or_else(|| history.price(match_good));
        let need = if ask > 0.0 { ask * qty } else { 0.0 };
        if need > 0.0
            && !self.add_payment(
                factuals,
                history,
                book,
                match_good,
                &mut goods,
                &mut covered,
                need,
                skip,
            )
        {
            return Err(("cannot_pay", goods));
        }
        if self
            .buy_transport_for_freight(factuals, history, book, &mut goods, &mut covered, skip)
            .is_none()
        {
            return Err(("freight", goods));
        }
        if crate::game::deal::freight_shortfall(
            factuals,
            history.friction,
            &goods,
            &self.free_stock(),
        ) > 0.0
        {
            return Err(("freight_left", goods));
        }
        if !self.buyer_accepts(&goods, history) {
            return Err(("buyer_rejects", goods));
        }
        Ok(ProposedDeal {
            buyer: self.actor(),
            seller: book.seller,
            match_good,
            freight: crate::game::deal::freight_bill(factuals, history.friction, &goods),
            goods,
            fresh: HashMap::new(),
        })
    }
}

impl DealMaker for Pop {
    fn actor(&self) -> Actor {
        Actor::Pop(self.id)
    }

    fn sell_orders(&self, history: &MarketHistory) -> Vec<MarketOrder> {
        let mut orders = Vec::new();
        for (&good, _) in &self.property {
            let units = self.surplus_for_sale(good);
            if units >= 1.0 {
                let price = history.price(good);
                orders.push(MarketOrder::sell(self.actor(), good, units).with_accept_price(price));
            }
        }
        orders
    }

    fn buy_orders(&self, history: &MarketHistory) -> Vec<MarketOrder> {
        let mut orders = self.buys_for_stopped_desire();
        // The level moves onto a cheaper substitute before a saved day is added back.
        self.add_upgrade_buys(history, &mut orders);
        self.add_savings_buys(&mut orders);
        self.add_cheaper_cover(&mut orders);
        for order in self.job.buy_orders(self.actor()) {
            if let Some(existing) = orders.iter_mut().find(|held| held.target == order.target) {
                existing.target_amount += order.target_amount;
            } else {
                orders.push(order);
            }
        }
        for order in &mut orders {
            order.accept_price = history.price(order.target);
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
            .find(|order| order.target == match_good && order.target_amount < 0.0);
        let Some(offer) = offer else {
            return None;
        };
        // This meeting is for one good. Use the buy for that good: the open
        // desire, the job's input shortfall, or the two added together.
        let wanted = self
            .buy_orders(history)
            .into_iter()
            .find(|order| order.target == match_good)
            .map(|order| order.target_amount);
        let Some(wanted) = wanted else {
            return None;
        };
        if wanted < 1.0 {
            return None;
        }
        let ask = dictated_price(offer, history);
        let bid = self
            .buy_orders(history)
            .into_iter()
            .find(|order| order.target == match_good)
            .map(|order| dictated_price(&order, history))
            .unwrap_or_else(|| history.price(match_good));
        // The buyer will not pay a higher raw price than the one they posted.
        if ask > 0.0 && bid > 0.0 && bid < ask {
            return None;
        }
        let qty = wanted.min((-offer.target_amount).floor());
        if qty < 1.0 || history.price(match_good) == 0.0 {
            return None;
        }
        // The whole pile may not be payable. Keep the largest whole quantity that is.
        let mut left = qty.floor();
        while left >= 1.0 {
            if let Ok(deal) = self.propose_quantity(match_good, left, book, history, factuals) {
                return Some(deal);
            }
            left -= 1.0;
        }
        None
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
                // Count the units this buy brought in.
                *self.buy_received.entry(good).or_insert(0.0) += qty;
            }
        }
        if proposal.buyer == id {
            self.pay_freight(proposal.freight, factuals);
        }
    }

    fn reevaluate(&mut self, history: &MarketHistory, rng: &mut dyn rand::RngCore) {
        let _ = history;
        // Goods just received can fill the open desire before sell orders are read again.
        if self.satisfy_cursor.is_some() {
            self.satisfy_continue(rng);
        }
        // A desire that just opened may post a larger buy.
        self.note_posted_buys();
    }

    fn reset_day(&mut self) {
        Pop::reset_day(self);
    }

    fn reserve(&mut self, factuals: &Factuals, rng: &mut dyn rand::RngCore) {
        // Desires claim first. The job takes only what is still free, and
        // leaves the saved day in quantity. A craft process that is not
        // already on the job stays off.
        self.satisfy(rng);
        self.job.leave(&self.savings);
        self.job.reserve(&mut self.property, factuals);
    }

    fn produce(&mut self, factuals: &Factuals) {
        self.job.leave(&self.savings);
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

/// # Desire Room
///
/// Satisfaction still open on the bucket target at `index`.
///
/// The room is that target's cap left ([`target_cap_left`]), and no more
/// than the gap to one level (`amount - satisfaction`). Another target's
/// take fills the level, not this target's cap. A full cap, or a met
/// level, is `0`.
/// # Tier Through
///
/// True when `tier` is `limit` or an earlier tier.
///
/// A later tier is false. List index is not compared, so a good on the
/// same tier is not spent on another desire of that tier.
fn tier_through(tier: usize, limit: usize) -> bool {
    tier <= limit
}

fn desire_room(desire: &Desire, index: usize) -> f64 {
    let level_left = desire.amount - desire.satisfaction;
    target_cap_left(desire, index).min(level_left).max(0.0)
}

fn target_cap_left(desire: &Desire, index: usize) -> f64 {
    let Some(target) = desire.target.get(index) else {
        return 0.0;
    };
    let already = desire.target_taken.get(index).copied().unwrap_or(0.0);
    desire.amount * target.cap - already
}

#[cfg(test)]
mod pop {
    use std::collections::{HashMap, HashSet, VecDeque};

    use crate::game::actor::Actor;
    use crate::game::actors::Actors;
    use crate::game::deal::{DealMaker, DealResponse, MeetingOutcome, ProposedDeal, SellerBook};
    use crate::game::marketorder::MarketOrder;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{DemoDesire, Desire, DesireEffect, DesireTarget, DesireTargetType};
    use crate::game::scalingfactor::ScalingFactor;
    use crate::game::culture::Culture;
    use crate::game::species::Species;
    use crate::game::stratum::Stratum;
    use crate::game::effects::PopEffect;
    use crate::game::craft::Craft;
    use crate::game::factuals::Factuals;
    use crate::game::market::{Market, MarketGood, MarketHistory};
    use crate::game::good::{Good, GoodTag, TIME};
    use crate::game::household::{BirthSentiment, DemographicRates, Household};
    use crate::game::job::{Job, JobLine};
    use crate::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
    use crate::game::process::{InputType, Process, ProcessEffect, ProcessInput, ProcessOutput};
    use crate::game::sentiment::{Sentiment, SentimentKind};
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
                stratum: 0,
                religion: 0,
            },
            population_cap: None,
            migration: 0.0,
            stored_effects: vec![],
            sentiment: Sentiment::new(),
            records: PopRecords::default(),
            sol_samples: VecDeque::new(),
            satisfy_cursor: None,
            planned_desires: Vec::new(),
            luxury_reached: 0.0,
            savings: HashMap::new(),
            luxury_savings: HashMap::new(),
            luxury_savings_open: true,
            cheaper_cover: HashMap::new(),
            buy_posted: HashMap::new(),
            buy_received: HashMap::new(),
        }
    }

    /// # Population At
    ///
    /// Headcount after one year of baseline rates.
    ///
    /// `birth_add` is added to births per woman, then that sum is multiplied
    /// by `scale`. Mortality stays at the baseline.
    fn population_at(birth_add: f64, scale: f64) -> f64 {
        let mut household = Household::new();
        let mut rates = DemographicRates::baseline();
        rates.birth_per_woman = (rates.birth_per_woman + birth_add) * scale;
        household.update(&rates);
        household.total_count()
    }

    #[test]
    fn growth_phase_raises_births_for_a_hopeful_pop() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::from_parts(0.0, 0.0, 0.0, 0.0, 1.0);
        let factuals = Factuals::new().with_species(Species::new(0, "human"));

        pop.growth_phase(&factuals);

        let scale = BirthSentiment::human().multiplier(1.0, 0.0, 0.0, 0.0);
        let got = pop.demographics.household.total_count();
        assert!((got - population_at(0.0, scale)).abs() < 1e-9);
    }

    #[test]
    fn growth_phase_lowers_births_for_a_fearful_pop() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::from_parts(0.0, 0.0, 0.0, 1.0, 0.0);
        let factuals = Factuals::new().with_species(Species::new(0, "human"));

        pop.growth_phase(&factuals);

        let scale = BirthSentiment::human().multiplier(0.0, 0.0, 1.0, 0.0);
        let got = pop.demographics.household.total_count();
        assert!((got - population_at(0.0, scale)).abs() < 1e-9);
    }

    #[test]
    fn growth_phase_leaves_a_content_pop_on_the_baseline() {
        let mut pop = make_pop();
        let factuals = Factuals::new().with_species(Species::new(0, "human"));

        pop.growth_phase(&factuals);

        let got = pop.demographics.household.total_count();
        assert!((got - population_at(0.0, 1.0)).abs() < 1e-9);
    }

    #[test]
    fn growth_phase_mixes_every_sentiment_share() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::from_parts(0.10, 0.20, 0.30, 0.15, 0.25);
        let factuals = Factuals::new().with_species(Species::new(0, "human"));
        let scale = BirthSentiment::human().multiplier(
            pop.sentiment.hope(),
            pop.sentiment.happiness(),
            pop.sentiment.fear(),
            pop.sentiment.anger(),
        );

        pop.growth_phase(&factuals);

        let got = pop.demographics.household.total_count();
        assert!((got - population_at(0.0, scale)).abs() < 1e-9);
    }

    #[test]
    fn growth_phase_scales_stored_births_after_the_add() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::from_parts(0.0, 0.0, 0.0, 0.0, 1.0);
        pop.stored_effects.push(PopEffect::Birthrate(0.10));
        let factuals = Factuals::new().with_species(Species::new(0, "human"));

        pop.growth_phase(&factuals);

        let scale = BirthSentiment::human().multiplier(1.0, 0.0, 0.0, 0.0);
        let got = pop.demographics.household.total_count();
        assert!((got - population_at(0.10, scale)).abs() < 1e-9);
        assert!(pop.stored_effects.is_empty());
    }

    #[test]
    fn growth_phase_ignores_sentiment_without_a_species_schedule() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::from_parts(0.0, 0.0, 0.0, 0.0, 1.0);

        pop.growth_phase(&Factuals::new());

        let got = pop.demographics.household.total_count();
        assert!((got - population_at(0.0, 1.0)).abs() < 1e-9);
    }

    #[test]
    fn rebalance_scales_every_capped_pop_down_to_the_shared_ceiling() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut large = make_pop();
        large.id = 1;
        large.demographics.household.count = 4.0;
        large.population_cap = Some(10.0);
        let mut small = make_pop();
        small.id = 2;
        small.demographics.household.count = 2.0;
        small.population_cap = Some(10.0);
        let adult = large.demographics.household.adult;
        market.pops.insert(large.id);
        market.pops.insert(small.id);
        actors.pops.insert(large.id, large);
        actors.pops.insert(small.id, small);

        market.rebalance_population_cap(&mut actors);

        let large = actors.pop(1);
        let small = actors.pop(2);
        let factor = 20.0 / 30.0;
        assert!((large.demographics.total_population() - 20.0 * factor).abs() < 1e-9);
        assert!((small.demographics.total_population() - 10.0 * factor).abs() < 1e-9);
        assert!((large.demographics.household.adult - adult).abs() < 1e-12);
        assert!((small.demographics.household.adult - adult).abs() < 1e-12);
    }

    #[test]
    fn rebalance_shrinks_a_pop_that_is_under_its_own_share() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut under = make_pop();
        under.id = 1;
        under.demographics.household.count = 2.0;
        under.population_cap = Some(12.0);
        let mut over = make_pop();
        over.id = 2;
        over.demographics.household.count = 2.0;
        over.population_cap = Some(4.0);
        market.pops.insert(under.id);
        market.pops.insert(over.id);
        actors.pops.insert(under.id, under);
        actors.pops.insert(over.id, over);

        market.rebalance_population_cap(&mut actors);

        let under = actors.pop(1).demographics.total_population();
        let over = actors.pop(2).demographics.total_population();
        assert!((under - 8.0).abs() < 1e-9);
        assert!((over - 8.0).abs() < 1e-9);
    }

    #[test]
    fn rebalance_leaves_an_uncapped_pop_at_its_size() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut capped = make_pop();
        capped.id = 1;
        capped.demographics.household.count = 4.0;
        capped.population_cap = Some(10.0);
        let mut open = make_pop();
        open.id = 2;
        open.demographics.household.count = 4.0;
        market.pops.insert(capped.id);
        market.pops.insert(open.id);
        actors.pops.insert(capped.id, capped);
        actors.pops.insert(open.id, open);

        market.rebalance_population_cap(&mut actors);

        assert!((actors.pop(1).demographics.total_population() - 10.0).abs() < 1e-9);
        assert!((actors.pop(2).demographics.total_population() - 20.0).abs() < 1e-9);
    }

    #[test]
    fn rebalance_does_nothing_while_the_sum_is_under_the_ceiling() {
        let mut market = Market::new(1);
        let mut actors = Actors::new();
        let mut pop = make_pop();
        pop.population_cap = Some(pop.demographics.total_population() * 1000.0);
        let before = pop.demographics.household.count;
        market.pops.insert(pop.id);
        actors.pops.insert(pop.id, pop);

        market.rebalance_population_cap(&mut actors);

        assert!((actors.pop(1).demographics.household.count - before).abs() < 1e-12);
    }

    #[test]
    fn update_desires_copies_a_stratum_desire() {
        let mut pop = make_pop();
        pop.demographics.culture = 1;
        pop.demographics.stratum = 2;
        let factuals = Factuals::new()
            .with_culture(Culture::new(1, "farmers"))
            .with_stratum(Stratum::new(2, "tenants", 1).with_desire(DemoDesire::new(4).with_tier(0)));

        pop.update_desires(&factuals);

        assert_eq!(pop.desires[0].len(), 1);
        assert_eq!(pop.desires[0][0].source, DemographicSource::Stratum(2));
        assert_eq!(pop.desires[0][0].demo_desire_id, 4);
        assert!(factuals.find_culture(1).strata.contains(&2));
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
    fn decay_records_lost_and_reset_clears_it() {
        let mut pop = make_pop();
        let mut row = PopPRow::new(10.0);
        row.fresh = 4.0;
        row.consumed = 4.0;
        pop.property.insert(1, row);
        let factuals = Factuals::new().with_good(make_good(1, "bread", 0.5));

        let rot = pop.decay_goods(&factuals);

        // 6 aging units rot. The eaten 4 are destroyed and are not lost.
        assert_eq!(pop.property[&1].lost, 3.0);
        assert_eq!(rot[&1].0, 3.0);
        assert_eq!(pop.property[&1].consumed, 0.0);

        pop.reset_day();

        assert_eq!(pop.property[&1].lost, 0.0);
    }

    /// Satisfaction carries by [`Desire::decay`] on the morning reset. A
    /// decay of `0.0` clears it. Per-target taken always clears.
    #[test]
    fn reset_day_scales_satisfaction_by_desire_decay() {
        let mut pop = make_pop();
        let mut kept = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        kept.satisfaction = 8.0;
        kept.decay = 0.5;
        kept.target_taken = vec![3.0];
        let mut cleared = desire(
            2,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        cleared.satisfaction = 6.0;
        cleared.decay = 0.0;
        cleared.target_taken = vec![2.0];
        pop.desires[0].push(kept);
        pop.desires[0].push(cleared);
        pop.satisfy_cursor = Some(super::SatisfyCursor {
            tier: 0,
            desire_index: 0,
            target_index: 0,
            target_start_satisfaction: Some(0.0),
            iter_target: 1.0,
        });

        pop.reset_day();

        assert!((pop.desires[0][0].satisfaction - 4.0).abs() < 1e-12);
        assert!(pop.desires[0][0].target_taken.is_empty());
        assert_eq!(pop.desires[0][1].satisfaction, 0.0);
        assert!(pop.desires[0][1].target_taken.is_empty());
        assert!(pop.satisfy_cursor.is_none());
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
    fn sell_orders_offer_the_units_above_the_desire() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(1, PopPRow::new(10.0));

        let grain = pop
            .sell_orders(&MarketHistory::new())
            .into_iter()
            .find(|order| order.target == 1)
            .expect("grain");
        assert!((grain.target_amount + 6.0).abs() < 1e-9);

        // The buyer receives five more. The next read offers that surplus too.
        let deal = ProposedDeal {
            buyer: Actor::Pop(pop.id),
            seller: Actor::Pop(2),
            match_good: 1,
            goods: HashMap::from([(1, 5.0)]),
            fresh: HashMap::new(),
            freight: 0.0,
        };
        pop.finalize(&deal, &Factuals::new());
        let grain = pop
            .sell_orders(&MarketHistory::new())
            .into_iter()
            .find(|order| order.target == 1)
            .expect("grain");
        assert!((grain.target_amount + 11.0).abs() < 1e-9);
    }

    /// Ten bread. The first desire reserves four, then the walk stops on
    /// tools. A later desire still takes four bread, so two are for sale.
    #[test]
    fn sell_orders_hold_back_a_later_desire_after_an_earlier_reserve() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[0].push(desire(
            2,
            vec![DesireTarget::new(3, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[0].push(desire(
            3,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(1, PopPRow::new(10.0));
        do_satisfy(&mut pop).expect("stuck on tools");
        assert!((pop.property[&1].reserved - 4.0).abs() < 1e-9);

        let bread = pop
            .sell_orders(&MarketHistory::new())
            .into_iter()
            .find(|order| order.target == 1)
            .expect("bread");
        assert!((bread.target_amount + 2.0).abs() < 1e-9);
    }

    /// Ten units from a bucket, each target capped at half. Two cheese are
    /// already reserved toward the level. Bread still has its whole cap of
    /// five, so five of ten bread are for sale.
    #[test]
    fn sell_orders_hold_back_a_bucket_target_by_its_own_cap() {
        let mut pop = make_pop();
        let mut meal = desire(
            1,
            vec![
                DesireTarget::new(1, DesireTargetType::Consume, 1.0).with_cap(0.5),
                DesireTarget::new(2, DesireTargetType::Consume, 1.0).with_cap(0.5),
            ],
            10.0,
        );
        meal.satisfaction = 2.0;
        meal.target_taken = vec![0.0, 2.0];
        pop.desires[0].push(meal);
        pop.property.insert(1, PopPRow::new(10.0));
        let mut cheese = PopPRow::new(2.0);
        cheese.reserved = 2.0;
        pop.property.insert(2, cheese);

        let bread = pop
            .sell_orders(&MarketHistory::new())
            .into_iter()
            .find(|order| order.target == 1)
            .expect("bread");
        assert!((bread.target_amount + 5.0).abs() < 1e-9);
    }

    #[test]
    fn an_input_counts_as_the_desire_it_feeds() {
        let mut pop = make_pop();
        // Target 0 still records the recipe and does not claim the gold.
        pop.job = Job::new(1, vec![JobLine::new(6, Some(0.0), vec![])]);
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(6, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(4, PopPRow::new(10.0));
        pop.property.insert(8, PopPRow::new(5.0));
        let jewelry = Process::new(6, "make jewelry", 0)
            .with_input(ProcessInput::new(4, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(6, 1.0, true));
        let factuals = Factuals::new()
            .with_process(jewelry)
            .with_good(make_good(4, "gold", 0.0));

        pop.job.reserve(&mut pop.property, &factuals);

        let gold = pop
            .sell_orders(&MarketHistory::new())
            .into_iter()
            .find(|order| order.target == 4)
            .expect("gold");
        // Four gold feed the jewelry desire. The other six are for sale.
        assert!((gold.target_amount + 6.0).abs() < 1e-9);
        // Stone feeds nothing. Gold feeds the jewelry desire, so giving stone is allowed.
        assert!(pop.exchange_ok(
            &HashMap::from([(8, 2.0)]),
            &HashMap::from([(4, 2.0)]),
            &MarketHistory::new(),
        ));
    }

    #[test]
    fn cabin_wood_above_the_recipe_is_for_sale() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(8, Some(0.0), vec![])]);
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(8, DesireTargetType::Consume, 1.0)],
            2.0,
        ));
        pop.property.insert(7, PopPRow::new(10.0));
        let cabins = Process::new(8, "make cabins", 0)
            .with_input(ProcessInput::new(7, 3.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(8, 1.0, true));
        let factuals = Factuals::new().with_process(cabins);

        pop.job.reserve(&mut pop.property, &factuals);

        let wood = pop
            .sell_orders(&MarketHistory::new())
            .into_iter()
            .find(|order| order.target == 7)
            .expect("wood");
        // Two cabins take six wood. The other four are for sale.
        assert!((wood.target_amount + 4.0).abs() < 1e-9);
    }

    /// No water, and the six wood sit inside a later desire, so none of it is surplus.
    /// The buyer still pays with that wood, because water is the earlier desire.
    /// Four water are still wanted: amount 8 at efficiency 2. Those four units
    /// place 8 satisfaction. Four wood place 4. The gain is larger, so the
    /// later good pays for the earlier one.
    #[test]
    fn propose_pays_for_an_earlier_desire_with_a_later_good() {
        let mut buyer = make_pop();
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 2.0)],
            8.0,
        ));
        buyer.desires[1].push(desire(
            2,
            vec![DesireTarget::new(7, DesireTargetType::Consume, 1.0)],
            6.0,
        ));
        buyer.property.insert(7, PopPRow::new(6.0));
        do_satisfy(&mut buyer);
        let book = SellerBook {
            seller: Actor::Pop(2),
            offers: vec![MarketOrder::sell(Actor::Pop(2), 2, 4.0)],
            requests: vec![],
        };
        let history = card(&[(2, 1.0, 1.0), (7, 1.0, 1.0)]);

        let deal = buyer
            .propose(2, &book, &history, &Factuals::new())
            .expect("basket");

        assert_eq!(deal.goods.get(&2).copied(), Some(4.0));
        assert_eq!(deal.goods.get(&7).copied(), Some(-4.0));
    }

    /// Water is priced at 3. One skill is offered, and it does not cover
    /// that price, so wood pays the rest. The skill is still offered once.
    #[test]
    fn propose_offers_a_skill_once() {
        let mut buyer = make_pop();
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            1.0,
        ));
        buyer.property.insert(40, PopPRow::new(8.0));
        buyer.property.insert(7, PopPRow::new(4.0));
        do_satisfy(&mut buyer);
        let mut skill = make_good(40, "survival", 0.05);
        skill.mass = 0.0;
        skill.tags.insert(GoodTag::Skill);
        let factuals = Factuals::new()
            .with_good(skill)
            .with_good(make_good(7, "wood", 0.0))
            .with_good(make_good(2, "water", 0.0));
        let book = SellerBook {
            seller: Actor::Pop(2),
            offers: vec![MarketOrder::sell(Actor::Pop(2), 2, 1.0)],
            requests: vec![],
        };
        let history = card(&[(2, 3.0, 1.0), (40, 1.0, 1.5), (7, 1.0, 1.0)]);

        let deal = buyer
            .propose(2, &book, &history, &factuals)
            .expect("basket");

        assert_eq!(deal.goods.get(&2).copied(), Some(1.0));
        assert_eq!(deal.goods.get(&40).copied(), Some(-1.0));
        assert_eq!(deal.goods.get(&7).copied(), Some(-2.0));
    }

    /// Ten time, freight bill 1. Four water take four time at the raw price.
    /// The reserve stays out of the basket. One time is only the reserve, so
    /// that buyer offers nothing.
    #[test]
    fn propose_pays_with_time_above_the_freight_reserve() {
        let mut buyer = make_pop();
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        buyer.property.insert(0, PopPRow::new(10.0));
        do_satisfy(&mut buyer);
        let factuals = Factuals::new().with_good(time_good());
        let book = SellerBook {
            seller: Actor::Pop(2),
            offers: vec![MarketOrder::sell(Actor::Pop(2), 2, 4.0)],
            requests: vec![],
        };
        let history = card(&[(2, 1.0, 1.0), (0, 1.0, 1.0)]);

        let deal = buyer
            .propose(2, &book, &history, &factuals)
            .expect("basket");

        assert_eq!(deal.goods.get(&2).copied(), Some(4.0));
        assert_eq!(deal.goods.get(&0).copied(), Some(-4.0));

        buyer.property.insert(0, PopPRow::new(1.0));
        assert!(buyer.propose(2, &book, &history, &factuals).is_none());
    }

    /// Eight water are on offer. Five time covers four once the freight
    /// reserve is held back, and the buyer keeps those four.
    #[test]
    fn propose_steps_down_to_the_quantity_it_can_pay() {
        let mut buyer = make_pop();
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            8.0,
        ));
        buyer.property.insert(0, PopPRow::new(5.0));
        do_satisfy(&mut buyer);
        let factuals = Factuals::new().with_good(time_good());
        let book = SellerBook {
            seller: Actor::Pop(2),
            offers: vec![MarketOrder::sell(Actor::Pop(2), 2, 8.0)],
            requests: vec![],
        };
        let history = card(&[(2, 1.0, 1.0), (0, 1.0, 1.0)]);

        let deal = buyer
            .propose(2, &book, &history, &factuals)
            .expect("basket");

        assert_eq!(deal.goods.get(&2).copied(), Some(4.0));
    }

    /// One wood. The cabin covers the price, then its bulk makes the freight
    /// larger than the ten time on hand. Leaving the cabin out, that time
    /// covers the wood, so the buyer pays with time.
    #[test]
    fn propose_drops_a_bulky_payment_when_freight_fails() {
        let mut buyer = make_pop();
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(7, DesireTargetType::Consume, 1.0)],
            1.0,
        ));
        buyer.property.insert(8, PopPRow::new(1.0));
        buyer.property.insert(0, PopPRow::new(10.0));
        do_satisfy(&mut buyer);
        let factuals = Factuals::new()
            .with_good(time_good())
            .with_good(make_good(7, "wood", 0.0).with_mass(2.0).with_volume(0.003))
            .with_good(make_good(8, "cabins", 0.0).with_mass(20.0).with_volume(0.05));
        let book = SellerBook {
            seller: Actor::Pop(2),
            offers: vec![MarketOrder::sell(Actor::Pop(2), 7, 1.0)],
            requests: vec![],
        };
        let mut history = card(&[(7, 1.0, 1.0), (8, 1.0, 1.0), (0, 1.0, 1.0)]);
        history.friction = 1.0;

        let deal = buyer
            .propose(7, &book, &history, &factuals)
            .expect("basket");

        assert_eq!(deal.goods.get(&7).copied(), Some(1.0));
        assert!(deal.goods.get(&8).is_none());
        assert!(deal.goods.get(&0).copied().unwrap_or(0.0) < 0.0);
    }

    /// The meal is already full, so the grain is only a job input. Stone feeds
    /// nothing. The priced basket is a holding loss, and the buyer still offers it.
    #[test]
    fn propose_offers_a_job_input_at_a_holding_loss() {
        let mut buyer = make_pop();
        buyer.job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        buyer.property.insert(2, PopPRow::new(4.0));
        buyer.property.insert(8, PopPRow::new(10.0));
        do_satisfy(&mut buyer);
        let factuals = Factuals::new().with_process(bake());
        buyer.job.reserve(&mut buyer.property, &factuals);
        let book = SellerBook {
            seller: Actor::Pop(2),
            offers: vec![MarketOrder::sell(Actor::Pop(2), 1, 4.0)],
            requests: vec![],
        };
        let history = card(&[(1, 1.0, 1.0), (8, 1.0, 1.0)]);

        let deal = buyer
            .propose(1, &book, &history, &factuals)
            .expect("basket");

        assert_eq!(deal.goods.get(&1).copied(), Some(4.0));
        assert!(deal.goods.get(&8).copied().unwrap_or(0.0) <= -4.0);
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
        assert_eq!(orders[0].target_amount, 4.0);
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

        // Bread is the meal. The grain line makes that input this morning, so
        // it runs first. Four bread runs need four grain, plus one grain.
        // The bread line takes those four runs. It pays, and the leftover
        // grain is one surplus run, so the target steps a quarter toward it.
        assert_eq!(pop.job.lines[0].process, 9);
        assert_eq!(pop.job.lines[0].target, Some(5.0));
        assert_eq!(pop.job.lines[1].process, 7);
        assert_eq!(pop.job.lines[1].target, Some(4.25));
    }

    #[test]
    fn plan_covers_the_short_common_tier_when_basic_is_stocked() {
        let mut pop = make_pop();
        pop.job = Job::new(
            1,
            vec![JobLine::new(7, None, vec![]), JobLine::new(9, None, vec![])],
        );
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[1].push(desire(
            2,
            vec![DesireTarget::new(4, DesireTargetType::Consume, 1.0)],
            2.0,
        ));
        pop.property.insert(2, PopPRow::new(4.0));
        let factuals = Factuals::new()
            .with_process(bake())
            .with_process(Process::new(9, "weave", 0).with_output(ProcessOutput::new(4, 1.0, true)));
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        history.prices.insert(4, 0.0);

        pop.plan(&factuals, &history);

        // Basic bread is on hand, and bread is not worth holding, so that line
        // keeps a zero target and moves behind the cloth line.
        assert_eq!(pop.job.lines[0].process, 9);
        assert_eq!(pop.job.lines[0].target, Some(3.0));
        assert_eq!(pop.job.lines[1].process, 7);
        assert_eq!(pop.job.lines[1].target, Some(0.0));
    }

    #[test]
    fn plan_makes_the_producible_target_when_the_best_is_not() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(9, None, vec![])]);
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(2, DesireTargetType::Consume, 1.0),
                DesireTarget::new(1, DesireTargetType::Consume, 0.5),
            ],
            4.0,
        ));
        let factuals = Factuals::new()
            .with_process(Process::new(9, "thresh", 0).with_output(ProcessOutput::new(1, 1.0, true)));
        let mut history = MarketHistory::new();
        history.prices.insert(1, 0.0);

        pop.plan(&factuals, &history);

        // Bread would take 4. The job makes grain, and grain covers the meal at half efficiency, plus one grain.
        assert_eq!(pop.job.lines[0].target, Some(9.0));
    }

    #[test]
    fn plan_covers_a_later_tier_while_basic_is_short() {
        let mut pop = make_pop();
        pop.job = Job::new(
            1,
            vec![JobLine::new(7, None, vec![]), JobLine::new(9, None, vec![])],
        );
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[2].push(desire(
            2,
            vec![DesireTarget::new(4, DesireTargetType::Consume, 1.0)],
            3.0,
        ));
        // Five grain finishes the five bread runs. The gem line has no input.
        pop.property.insert(1, PopPRow::new(5.0));
        let factuals = Factuals::new()
            .with_process(bake())
            .with_process(Process::new(9, "cut gem", 0).with_output(ProcessOutput::new(4, 1.0, true)));
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);
        history.prices.insert(4, 0.0);

        pop.plan(&factuals, &history);

        // Basic is short, and the gem tier is short too. Both are written.
        // Bread stays in front. 4 bread plus one unit, and 3 gems plus one.
        assert_eq!(pop.job.lines[0].process, 7);
        assert_eq!(pop.job.lines[0].target, Some(5.0));
        assert_eq!(pop.job.lines[1].process, 9);
        assert_eq!(pop.job.lines[1].target, Some(4.0));
    }

    #[test]
    fn plan_subtracts_shared_stock_once() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, None, vec![])]);
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[0].push(desire(
            2,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(2, PopPRow::new(1.0));
        // Eight grain finishes the eight bread runs.
        pop.property.insert(1, PopPRow::new(8.0));
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);

        pop.plan(&factuals, &history);

        // Eight bread across the two desires, one loaf already on hand, plus one unit.
        assert_eq!(pop.job.lines[0].target, Some(8.0));
    }

    #[test]
    fn plan_spends_the_next_mornings_time_on_top_of_time_still_held() {
        let mut pop = make_pop();
        pop.demographics.household.count = 1.0;
        pop.demographics.household.adult = 1.0;
        pop.demographics.household.elder = 0.0;
        pop.demographics.household.child = 0.0;
        pop.property.insert(TIME, PopPRow::new(10.0));
        pop.job = Job::new(1, vec![JobLine::new(1, Some(100.0), vec![])]);
        let factuals = Factuals::new().with_process(
            Process::new(1, "make grain", 0)
                .with_input(ProcessInput::new(TIME, 0.5, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(1, 6.0, true)),
        );
        let mut history = MarketHistory::new();
        history.prices.insert(TIME, 0.0);

        pop.plan(&factuals, &history);

        // One adult's grant plus the 10 still held would fund (64 + 10) / 0.5 runs.
        // The line is at 100, so this night steps a quarter of the way up, not onto that budget.
        assert_eq!(pop.job.lines[0].target, Some(125.0));
    }

    #[test]
    fn plan_snaps_a_new_desire_and_steps_when_only_stock_changes() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        // Nine grain finishes the largest cover this test snaps to.
        pop.property.insert(1, PopPRow::new(9.0));
        let factuals = Factuals::new().with_process(bake());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 0.0);

        pop.plan(&factuals, &history);

        // The desire is new, so the cover of 5 is written whole: 4 short plus one unit.
        // Bread is not worth holding, so the snap raises 1 to that cover and does not add them.
        assert_eq!(pop.job.lines[0].target, Some(5.0));

        pop.property.insert(2, PopPRow::new(4.0));
        pop.plan(&factuals, &history);

        // The desire is unchanged. Stock covers it, and the line still does not pay, so 5 steps toward 0.
        assert_eq!(pop.job.lines[0].target, Some(3.75));

        pop.desires[0][0].amount = 8.0;
        pop.property.remove(&2);
        pop.plan(&factuals, &history);

        // The amount changed, so the cover snaps to 9. The line does not pay, so 3.75 rises to 9.
        assert_eq!(pop.job.lines[0].target, Some(9.0));
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

    /// # Market Day Leaves A Craft Process Off The Job
    ///
    /// The craft lists bake. The job does not run it. Two market days leave
    /// the job on the line it already had.
    #[test]
    fn market_day_leaves_a_craft_process_off_the_job() {
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
        market.market_day(&mut actors, &factuals, &mut rng);

        let pop = actors.pop(1);
        let processes: Vec<usize> = pop.job.lines.iter().map(|line| line.process).collect();
        assert_eq!(processes, vec![4]);
        assert_eq!(pop.job.lines[0].target, Some(1.0));
    }

    /// # Apply Craft Adds The Attached Processes Once
    ///
    /// Calling [`Pop::apply_craft`] adds the culture and religion lists behind
    /// the line the job already runs. New lines start at no iterations.
    #[test]
    fn apply_craft_adds_the_attached_processes_once() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(4, Some(1.0), vec![])]);
        pop.demographics.culture = 2;
        pop.demographics.religion = 3;
        let factuals = Factuals::new()
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

        pop.apply_craft(&factuals);

        let processes: Vec<usize> = pop.job.lines.iter().map(|line| line.process).collect();
        assert_eq!(processes, vec![4, 7, 9, 8]);
        assert_eq!(pop.job.lines[0].target, Some(1.0));
        assert!(pop.job.lines[1..].iter().all(|line| line.target == Some(0.0)));
    }

    fn bake() -> Process {
        Process::new(7, "bake", 0)
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true))
    }

    fn desire(id: usize, targets: Vec<DesireTarget>, amount: f64) -> Desire {
        let n = targets.len();
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: id,
            priority: 0,
            target: targets,
            amount,
            satisfaction: 0.0,
            target_taken: vec![0.0; n],
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
    fn consume_counts_a_reserved_level_once() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(10.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));

        assert!(do_satisfy(&mut pop).is_none());
        assert!((pop.desires[0][0].satisfaction - 4.0).abs() < 1e-9);

        pop.consume();

        assert!((pop.desires[0][0].satisfaction - 4.0).abs() < 1e-9);
        assert!((pop.property[&1].quantity - 6.0).abs() < 1e-9);
        assert!((pop.property[&1].consumed - 4.0).abs() < 1e-9);
        assert!(pop.property[&1].reserved.abs() < 1e-9);
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
    fn satisfy_fills_the_target_it_can_finish() {
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

        // The efficient good cannot finish the level. The other good can,
        // so the walk takes that one and does not stop on the short pile.
        assert!(do_satisfy(&mut pop).is_none());
        assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
        assert_eq!(pop.property[&1].reserved, 0.0);
        assert!((pop.property[&2].reserved - 10.0).abs() < 1e-9);
    }

    #[test]
    fn satisfy_takes_the_higher_efficiency_when_both_can_finish() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(6.0));
        pop.property.insert(2, PopPRow::new(100.0));
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(2, DesireTargetType::Consume, 1.0),
                DesireTarget::new(1, DesireTargetType::Consume, 2.0),
            ],
            10.0,
        ));

        // Both goods can finish the level. The better one is reserved,
        // even though it is later in the bucket and the other pile is larger.
        assert!(do_satisfy(&mut pop).is_none());
        assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 5.0).abs() < 1e-9);
        assert_eq!(pop.property[&2].reserved, 0.0);
    }

    #[test]
    fn satisfy_stops_when_no_target_can_be_filled() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(3.0));
        pop.property.insert(2, PopPRow::new(4.0));
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(2, DesireTargetType::Consume, 1.0),
                DesireTarget::new(1, DesireTargetType::Consume, 2.0),
            ],
            10.0,
        ));

        // Neither pile finishes the level. The one that satisfies more is
        // reserved, and the walk stops there.
        let blocked = do_satisfy(&mut pop).expect("no target finishes the level");
        assert!((blocked.satisfaction - 6.0).abs() < 1e-9);
        assert!((pop.desires[0][0].satisfaction - 6.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 3.0).abs() < 1e-9);
        assert_eq!(pop.property[&2].reserved, 0.0);
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
        // Each cap is half the level. A target is taken once, up to that cap.
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
    fn satisfy_keeps_a_capped_target_across_calls() {
        // First call takes 2 of a 5-cap and stops. A second satisfy with more
        // stock only fills the 3 still open on that cap, not another full 5.
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(2.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0).with_cap(0.5)],
            10.0,
        ));

        let blocked = do_satisfy(&mut pop).expect("stock is short of the cap");
        assert!((blocked.satisfaction - 2.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 2.0).abs() < 1e-9);

        pop.property.get_mut(&1).unwrap().quantity = 100.0;
        let blocked = do_satisfy(&mut pop).expect("one capped target cannot fill the level");
        assert!((blocked.satisfaction - 5.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 5.0).abs() < 1e-9);
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

    /// Three grain cannot finish a level of 10. Five bread then can, at
    /// efficiency 2. Continue reserves 3.5 bread and leaves the grain reserve.
    #[test]
    fn satisfy_continue_uses_a_good_that_can_finish_when_the_bookmark_cannot() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(3.0));
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(1, DesireTargetType::Consume, 1.0),
                DesireTarget::new(2, DesireTargetType::Consume, 2.0),
            ],
            10.0,
        ));

        let blocked = do_satisfy(&mut pop).expect("grain is short of the level");
        assert!((blocked.satisfaction - 3.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 3.0).abs() < 1e-9);

        pop.property.insert(2, PopPRow::new(5.0));
        assert!(do_continue(&mut pop).is_none());
        assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 3.0).abs() < 1e-9);
        assert!((pop.property[&2].reserved - 3.5).abs() < 1e-9);
    }

    /// Time's cap is half a level and can be filled. Cabins are the better
    /// substitute and can fill the level alone. The cap is reserved first,
    /// and cabins take only what that cap left open.
    #[test]
    fn satisfy_takes_a_fillable_cap_before_a_better_substitute() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(100.0));
        pop.property.insert(2, PopPRow::new(100.0));
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(2, DesireTargetType::Consume, 2.0),
                DesireTarget::new(1, DesireTargetType::Consume, 1.0).with_cap(0.5),
            ],
            10.0,
        ));

        assert!(do_satisfy(&mut pop).is_none());
        assert!((pop.desires[0][0].satisfaction - 10.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 5.0).abs() < 1e-9);
        assert!((pop.property[&2].reserved - 2.5).abs() < 1e-9);
    }

    /// The capped good fills its share. The better good is missing, so the
    /// buy is the whole units that finish the level, for every good that
    /// still has room.
    #[test]
    fn satisfy_buys_the_goods_that_finish_a_capped_level() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(100.0));
        pop.desires[2].push(desire(
            1,
            vec![
                DesireTarget::new(2, DesireTargetType::Consume, 1.0),
                DesireTarget::new(3, DesireTargetType::Consume, 2.0),
                DesireTarget::new(1, DesireTargetType::Consume, 1.0).with_cap(0.5),
            ],
            10.0,
        ));

        let blocked = do_satisfy(&mut pop).expect("the level is only half filled");
        assert!((blocked.satisfaction - 5.0).abs() < 1e-9);
        assert!((pop.property[&1].reserved - 5.0).abs() < 1e-9);

        let orders = pop.buy_orders(&MarketHistory::new());
        let units = |good: usize| {
            orders
                .iter()
                .find(|order| order.target == good)
                .map(|order| order.target_amount)
                .unwrap_or(0.0)
        };
        // Five satisfaction is left. Efficiency 1 needs 5. Efficiency 2 needs 3.
        assert!((units(2) - 5.0).abs() < 1e-9);
        assert!((units(3) - 3.0).abs() < 1e-9);
        assert_eq!(units(1), 0.0);
    }

    /// Last night finished two luxury levels, so today's buy asks for two.
    #[test]
    fn luxury_buy_repeats_levels_finished_last_night() {
        let mut pop = make_pop();
        pop.luxury_reached = 2.0;
        pop.property.insert(1, PopPRow::new(100.0));
        pop.desires[2].push(desire(
            1,
            vec![
                DesireTarget::new(2, DesireTargetType::Consume, 2.0),
                DesireTarget::new(1, DesireTargetType::Consume, 1.0).with_cap(0.5),
            ],
            10.0,
        ));

        assert!(do_satisfy(&mut pop).is_some());
        let orders = pop.buy_orders(&MarketHistory::new());
        let cabins = orders
            .iter()
            .find(|order| order.target == 2)
            .expect("cabins finish the open share");
        // This level needs 2.5, and the next level's cap is another 5.
        assert!((cabins.target_amount - 8.0).abs() < 1e-9);
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
        assert_eq!(deals[0].goods.get(&9), Some(&-8.0));
        assert!((actors.pops[&1].property[&2].quantity - 4.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&9].quantity - 2.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&2].quantity - 6.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&9].quantity - 8.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&2].reserved - 4.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - 2.0).abs() < 1e-12);
        assert!((market.goods[&9].amv - 1.0).abs() < 1e-12);
        assert!((market.goods[&2].traded - 4.0).abs() < 1e-9);
        assert_eq!(market.goods[&2].paid, 0.0);
        assert!((market.goods[&9].paid - 8.0).abs() < 1e-9);
    }

    /// The buyer wants four bread and holds nothing that can pay for one.
    /// Two sellers offer bread. No meeting is opened. The buy stays sought.
    #[test]
    fn match_deals_skips_a_buyer_who_cannot_pay() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        do_satisfy(&mut buyer).expect("buyer wants bread");

        let mut seller_a = make_pop();
        seller_a.id = 2;
        seller_a.property.insert(2, PopPRow::new(10.0));
        let mut seller_b = make_pop();
        seller_b.id = 3;
        seller_b.property.insert(2, PopPRow::new(10.0));

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.pops.insert(3);
        market
            .goods
            .insert(2, MarketGood::new().with_amv(1.0).with_salability(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller_a);
        actors.pops.insert(3, seller_b);

        let mut rng = StdRng::seed_from_u64(1);
        let meetings = market.match_deals(&mut actors, &Factuals::new(), &mut rng);

        assert!(meetings.is_empty());
        assert!((market.goods[&2].sought_unmet - 4.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&2].quantity - 10.0).abs() < 1e-9);
        assert!((actors.pops[&3].property[&2].quantity - 10.0).abs() < 1e-9);
    }

    /// One seller will only take a single coin, which does not cover the
    /// bread. The other seller asks for nothing. The coin sits on a later
    /// desire, so it is not offered for sale, and it can still pay for
    /// bread. One bread places 4 satisfaction. The three coins given up
    /// place 3, so the buyer spends them. The buyer meets the open seller
    /// and does not open an empty meeting with the capped one.
    #[test]
    fn match_deals_skips_a_seller_whose_request_blocks_payment() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.property.insert(9, PopPRow::new(3.0));
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 4.0)],
            4.0,
        ));
        buyer.desires[1].push(desire(
            3,
            vec![DesireTarget::new(9, DesireTargetType::Consume, 1.0)],
            3.0,
        ));
        do_satisfy(&mut buyer).expect("buyer wants bread");

        let mut capped = make_pop();
        capped.id = 2;
        capped.property.insert(2, PopPRow::new(10.0));
        capped.desires[0].push(desire(
            2,
            vec![DesireTarget::new(9, DesireTargetType::Consume, 1.0)],
            1.0,
        ));
        do_satisfy(&mut capped).expect("capped seller wants one coin");

        let mut open = make_pop();
        open.id = 3;
        open.property.insert(2, PopPRow::new(10.0));

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.pops.insert(3);
        market
            .goods
            .insert(2, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(9, MarketGood::new().with_amv(0.4).with_salability(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, capped);
        actors.pops.insert(3, open);

        let mut rng = StdRng::seed_from_u64(1);
        let meetings = market.match_deals(&mut actors, &Factuals::new(), &mut rng);

        assert!(meetings.iter().all(|meeting| meeting.outcome == MeetingOutcome::Accepted));
        assert!(meetings.iter().any(|meeting| {
            meeting.buyer == Actor::Pop(1)
                && meeting.seller == Actor::Pop(3)
                && meeting.match_good == 2
        }));
        assert!(meetings.iter().all(|meeting| meeting.seller != Actor::Pop(2)));
        assert!((actors.pops[&1].property[&2].quantity - 1.0).abs() < 1e-9);
        assert!(actors.pops[&1].property.get(&9).is_none_or(|row| row.quantity < 1e-9));
    }

    /// The bread buyer holds one wood, and wood's holding does not cover one
    /// bread. They sell that wood for a good that does cover it, then buy
    /// the bread from the other seller in the same call.
    #[test]
    fn match_deals_retries_a_buyer_after_they_are_paid() {
        let mut buyer = make_pop();
        buyer.id = 1;
        buyer.property.insert(7, PopPRow::new(1.0));
        buyer.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            1.0,
        ));
        do_satisfy(&mut buyer).expect("buyer wants bread");

        let mut wood_buyer = make_pop();
        wood_buyer.id = 2;
        wood_buyer.property.insert(9, PopPRow::new(4.0));
        wood_buyer.desires[0].push(desire(
            2,
            vec![DesireTarget::new(7, DesireTargetType::Consume, 1.0)],
            1.0,
        ));
        do_satisfy(&mut wood_buyer).expect("wood buyer wants wood");

        let mut bread_seller = make_pop();
        bread_seller.id = 3;
        bread_seller.property.insert(2, PopPRow::new(5.0));

        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.pops.insert(3);
        market
            .goods
            .insert(2, MarketGood::new().with_amv(1.0).with_salability(1.0));
        market
            .goods
            .insert(7, MarketGood::new().with_amv(0.5).with_salability(1.0));
        market
            .goods
            .insert(9, MarketGood::new().with_amv(1.5).with_salability(1.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, wood_buyer);
        actors.pops.insert(3, bread_seller);

        let mut rng = StdRng::seed_from_u64(1);
        let meetings = market.match_deals(&mut actors, &Factuals::new(), &mut rng);

        assert!(meetings.iter().all(|meeting| meeting.outcome == MeetingOutcome::Accepted));
        assert_eq!(meetings.len(), 2);
        assert!((actors.pops[&1].property[&2].quantity - 1.0).abs() < 1e-9);
        assert!(actors.pops[&1].property.get(&9).is_none_or(|row| row.quantity < 1e-9));
        assert!((actors.pops[&3].property[&2].quantity - 4.0).abs() < 1e-9);
        assert!((actors.pops[&3].property[&9].quantity - 1.0).abs() < 1e-9);
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
        assert_eq!(deals[0].goods.get(&9), Some(&-8.0));
    }

    /// The buyer wants one bread. They hold ten of a basic good the desire
    /// still takes four of, and one luxury unit that fills its desire.
    ///
    /// Payment uses surplus. Six of the basic good are above the desire, so
    /// the basket pays one of those, the raw price of the bread. The luxury
    /// unit is the one that fills its desire, so it stays.
    #[test]
    fn match_deals_pays_surplus_of_a_desired_good() {
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
        assert_eq!(deals[0].goods.get(&4), Some(&-1.0));
        assert!(deals[0].goods.get(&8).is_none());
        assert_eq!(actors.pops[&1].property[&4].quantity, 9.0);
        assert_eq!(actors.pops[&1].property[&8].quantity, 1.0);
        assert_eq!(actors.pops[&2].property[&4].quantity, 1.0);
        assert!(actors.pops[&2].property.get(&8).is_none());
        assert_eq!(actors.pops[&1].property[&2].quantity, 1.0);
    }

    /// The seller is stuck on tools. Bread is a later desire on that same
    /// tier and still has room for four. Ten on hand leaves six above that
    /// room, and those six can be sold.
    #[test]
    fn match_deals_sells_bread_above_a_later_desire() {
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

        // Four bread move. Payment is eight of good 9, the raw price.
        assert_eq!(deals.len(), 1);
        assert_eq!(deals[0].goods.get(&2), Some(&4.0));
        assert_eq!(deals[0].goods.get(&9), Some(&-8.0));
        assert!((actors.pops[&1].property[&2].quantity - 4.0).abs() < 1e-9);
        assert!((actors.pops[&1].property[&9].quantity - 2.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&2].quantity - 6.0).abs() < 1e-9);
        assert!((actors.pops[&2].property[&9].quantity - 8.0).abs() < 1e-9);
        // The four the later desire can still take stay. Two surplus remain.
        let bread = actors.pops[&2]
            .sell_orders(&market.history())
            .into_iter()
            .find(|order| order.target == 2)
            .expect("bread");
        assert!((bread.target_amount + 2.0).abs() < 1e-9);
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

    /// The buyer holds the time, so freight stays out of the basket. Four
    /// bread at raw AMV 2 take eight coins. Two coins remain.
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
        assert!((actors.pops[&1].property[&9].quantity - 2.0).abs() < 1e-9);
    }

    /// The buyer holds no time, so one unit is bought with the bread. Four
    /// bread at raw AMV 2 and one time at raw AMV 1 take nine coins. Eleven
    /// remain.
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
        assert!((actors.pops[&1].property[&9].quantity - 11.0).abs() < 1e-9);
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

    /// Seller holds 10 of good 2. The desire still takes 4, so 6 are surplus.
    ///
    /// Giving 4 stays inside that surplus. Good 9 feeds no desire. At
    /// salability 1 the payment is a holding gain, cost 8 against credit 9,
    /// so the seller accepts.
    ///
    /// Giving 8 cuts 2 units into the desire. Those units still feed it, and
    /// good 9 feeds nothing, so the seller rejects. Seventeen of good 9 would
    /// be a holding gain against that gift (credit 17, cost 16) if the units
    /// were surplus. A unit that still feeds a desire is given up when the
    /// goods received feed that desire or an earlier one.
    #[test]
    fn evaluate_accepts_surplus_and_rejects_a_cut_into_the_desire() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));
        seller.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let history = card(&[(2, 2.0, 1.0), (9, 1.0, 1.0)]);
        let within = proposal(2, HashMap::from([(2, 4.0), (9, -9.0)]));
        assert_eq!(
            seller.evaluate(&within, &history, &Factuals::new()),
            DealResponse::Accept
        );
        let cutting = proposal(2, HashMap::from([(2, 8.0), (9, -17.0)]));
        assert_eq!(
            seller.evaluate(&cutting, &history, &Factuals::new()),
            DealResponse::Reject
        );
    }

    /// Both goods feed basic desires. Good 2 is earlier in the tier than good 9.
    /// The seller gives 4 of good 9, which is 4 satisfaction.
    ///
    /// Satisfaction gained has to exceed satisfaction given up, one for one,
    /// across the goods. Receiving 2 of good 2 puts 2 satisfaction in place
    /// of 4, so the seller rejects. Receiving 5 puts 5 in place of 4, so the
    /// seller accepts. Giving the later good is allowed because good 2 feeds
    /// the earlier desire.
    #[test]
    fn evaluate_requires_a_strict_satisfaction_gain() {
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
    /// salability 1. With no satisfaction on either side the raw price is
    /// enforced, and meeting it accepts.
    ///
    /// Giving 4 of good 2 costs 8. Receiving 7 of good 9 credits 7, short of
    /// the price, so the seller rejects. Receiving 8 credits 8 and accepts.
    ///
    /// Good 2 then feeds a basic desire. The seller gives 100 of good 9
    /// (no desire on it) and receives 1 of good 2. Satisfaction rises by 1
    /// against nothing given up, so the seller accepts that AMV loss.
    #[test]
    fn evaluate_meets_the_raw_price_and_accepts_a_satisfaction_gain() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));
        seller.property.insert(9, PopPRow::new(100.0));
        let history = card(&[(2, 2.0, 1.0), (9, 1.0, 1.0)]);
        // Raw 8 for 7.
        let short = proposal(2, HashMap::from([(2, 4.0), (9, -7.0)]));
        assert_eq!(
            seller.evaluate(&short, &history, &Factuals::new()),
            DealResponse::Reject
        );
        // Raw 8 for 8.
        let met = proposal(2, HashMap::from([(2, 4.0), (9, -8.0)]));
        assert_eq!(
            seller.evaluate(&met, &history, &Factuals::new()),
            DealResponse::Accept
        );

        seller.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        // Satisfaction +1 against an AMV loss of 100 for 2.
        let fortune = proposal(9, HashMap::from([(9, 100.0), (2, -1.0)]));
        assert_eq!(
            seller.evaluate(&fortune, &history, &Factuals::new()),
            DealResponse::Accept
        );
    }

    /// No desires. The seller gives 1 of good 9 (AMV -0.1) and receives 1 of
    /// good 8 (AMV -0.2). Both salabilities are 0.1.
    ///
    /// Raw AMV moves from -0.1 to -0.2. Salability does not scale it. With no
    /// satisfaction on either side that is a loss, so the seller rejects.
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

    /// No desires. The seller gives 4 of good 2 (AMV 2) and receives 5 of
    /// good 9 (AMV 1). Good 9's salability is 2.
    ///
    /// Raw AMV falls from 8 to 5. Salability does not scale either price.
    /// With no satisfaction on either side the seller rejects that loss.
    #[test]
    fn evaluate_rejects_a_raw_loss_with_no_satisfaction() {
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
    /// good 9 (AMV 2). The raw price is a gain, so the seller accepts.
    /// Salability does not scale that price. Low salability on either good
    /// leaves the same acceptance.
    #[test]
    fn evaluate_accepts_a_raw_gain_at_any_salability() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(10.0));
        let deal = proposal(2, HashMap::from([(2, 1.0), (9, -1.0)]));
        let at_par = card(&[(2, 1.0, 1.0), (9, 2.0, 1.0)]);
        assert_eq!(
            seller.evaluate(&deal, &at_par, &Factuals::new()),
            DealResponse::Accept
        );
        let received_illiquid = card(&[(2, 1.0, 1.0), (9, 2.0, 0.4)]);
        assert_eq!(
            seller.evaluate(&deal, &received_illiquid, &Factuals::new()),
            DealResponse::Accept
        );
        let both_illiquid = card(&[(2, 1.0, 0.5), (9, 2.0, 0.2)]);
        assert_eq!(
            seller.evaluate(&deal, &both_illiquid, &Factuals::new()),
            DealResponse::Accept
        );
    }

    /// The seller gives 1 of good 8 and receives 1 of good 2. Good 8 is a
    /// luxury desire, and one unit yields 10 satisfaction. Good 2 is a basic
    /// desire, and one unit yields 1. Both AMVs are 1 at salability 1.
    ///
    /// A later tier may be given for an earlier one. Satisfaction is compared
    /// one for one across both goods, so 10 given against 1 received is not
    /// a gain and the seller rejects.
    ///
    /// One unit of a luxury worth 1 satisfaction, given for two units of the
    /// basic good, gains satisfaction. The luxury's raw AMV is 5 and the
    /// basic good's is 1, so the exchange is an AMV loss, and the seller
    /// accepts it.
    #[test]
    fn evaluate_compares_satisfaction_across_tiers() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(8, PopPRow::new(1.0));
        seller.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
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
            DealResponse::Reject
        );

        seller.property.insert(8, PopPRow::new(1.0));
        seller.desires[2][0].target[0].efficiency = 1.0;
        let traded = proposal(8, HashMap::from([(8, 1.0), (2, -2.0)]));
        let loss = card(&[(2, 1.0, 1.0), (8, 5.0, 1.0)]);
        assert_eq!(
            seller.evaluate(&traded, &loss, &Factuals::new()),
            DealResponse::Accept
        );
    }

    /// The seller gives 1 of good 9 (AMV 1) and receives 1 of good 2 (AMV 0.5).
    ///
    /// With no desires the raw price is short, so the seller rejects. A basic
    /// desire for good 2 is then added. The seller gains 1 satisfaction from
    /// good 2 and gives up none, because good 9 feeds no desire. That gain
    /// accepts the AMV loss.
    #[test]
    fn evaluate_accepts_satisfaction_gained_when_none_is_lost() {
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(9, PopPRow::new(4.0));
        let history = card(&[(2, 0.5, 1.0), (9, 1.0, 1.0)]);
        let deal = proposal(9, HashMap::from([(9, 1.0), (2, -1.0)]));
        // No desire on either good: raw 1 given for 0.5 received.
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

    /// A fully met basic desire carries a happiness bonus of 0.2. Nothing
    /// basic is unmet, and the pop holds nothing. Shares are then brought
    /// back to 1.
    #[test]
    fn update_sentiments_shifts_happiness_from_a_met_desire() {
        let mut pop = make_pop();
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        basic
            .effect
            .push(DesireEffect::SentimentFlat(SentimentKind::Happiness, 0.2, true));
        pop.desires[0].push(basic);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.happiness() - 0.2 / 1.2).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.2).abs() < 1e-9);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);
        assert_eq!(pop.sentiment.hope(), 0.0);
    }

    /// The basic desire is unmet, so its whole shortfall moves 0.04 into
    /// anger and 0.04 into fear. Its anger arm adds another 0.2 of anger.
    /// A stored hope arm is applied in the same batch and then removed.
    /// The birth arm stays.
    #[test]
    fn update_sentiments_applies_stored_hope_and_drops_it() {
        let mut pop = make_pop();
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 0.0;
        basic.effect.push(DesireEffect::SentimentFlat(
            SentimentKind::Anger,
            -0.2,
            false,
        ));
        pop.desires[0].push(basic);
        pop.stored_effects
            .push(PopEffect::SentimentFlat {
                kind: SentimentKind::Hope,
                delta: 0.2,
            });
        pop.stored_effects.push(PopEffect::Birthrate(0.01));

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.anger() - 0.24 / 1.48).abs() < 1e-9);
        assert!((pop.sentiment.fear() - 0.04 / 1.48).abs() < 1e-9);
        assert!((pop.sentiment.hope() - 0.2 / 1.48).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.48).abs() < 1e-9);
        assert_eq!(pop.stored_effects, vec![PopEffect::Birthrate(0.01)]);
    }

    /// Hope is an even fifth. A met basic desire scales that share by half.
    /// 15 percent of each other share moves into contentment. The basic need
    /// is met, so the tier adds nothing before the partition is brought
    /// back to 1.
    #[test]
    fn update_sentiments_scales_one_emotion_from_its_current_share() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::even();
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        basic.effect.push(DesireEffect::SentimentRelative(
            SentimentKind::Hope,
            0.5,
            true,
        ));
        pop.desires[0].push(basic);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.hope() - 0.27 / 1.1).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - 0.17 / 1.1).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 0.32 / 1.1).abs() < 1e-9);
    }

    /// No desires, so the tiers add nothing. Ten grain at a price and
    /// salability of 1 is 10 of holding value. The pop is one household,
    /// so wealth per household is 10 and hope takes 0.10 of the pop.
    #[test]
    fn update_sentiments_moves_hope_from_wealth_per_household() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(10.0));
        let mut history = MarketHistory::new();
        history.prices.insert(1, 1.0);
        history.salability.insert(1, 1.0);

        pop.update_sentiments(&history);

        assert!((pop.sentiment.hope() - 0.10 / 1.10).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.10).abs() < 1e-9);
        assert_eq!(pop.sentiment.happiness(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);
    }

    /// One basic desire is two levels met and the other is empty. The
    /// overshot desire does not fill the empty one, so half of basic is
    /// unmet. That half moves 0.02 into anger and 0.02 into fear.
    #[test]
    fn update_sentiments_splits_basic_shortfalls_across_desires() {
        let mut pop = make_pop();
        let mut met = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        met.satisfaction = 8.0;
        let unmet = desire(
            2,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        pop.desires[0].push(met);
        pop.desires[0].push(unmet);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.anger() - 0.02 / 1.04).abs() < 1e-9);
        assert!((pop.sentiment.fear() - 0.02 / 1.04).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.04).abs() < 1e-9);
    }

    /// The common desire is one level met and its bonus adds another half,
    /// so the tier is 1.5. Contentment takes 0.03 of the pop.
    #[test]
    fn update_sentiments_lets_common_satisfaction_pass_one_level() {
        let mut pop = make_pop();
        let mut common = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        common.satisfaction = 4.0;
        common
            .effect
            .push(DesireEffect::Satisfaction(0.5, true));
        pop.desires[1].push(common);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.contentment() - 1.03 / 1.03).abs() < 1e-9);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sentiment.hope(), 0.0);
    }

    /// Both luxury desires are met twice. The tier level is 2, so hope and
    /// happiness each take 0.02 of the pop.
    #[test]
    fn update_sentiments_counts_each_luxury_level() {
        let mut pop = make_pop();
        for id in [1, 2] {
            let mut luxury = desire(
                id,
                vec![DesireTarget::new(id, DesireTargetType::Consume, 1.0)],
                4.0,
            );
            luxury.satisfaction = 8.0;
            pop.desires[2].push(luxury);
        }

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.hope() - 0.02 / 1.04).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - 0.02 / 1.04).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.04).abs() < 1e-9);
    }

    /// The common desire is unmet. A stored satisfaction of 1.5 on that tier
    /// is the tier's level, so contentment takes 0.03 of the pop. Sentiment
    /// leaves that stored satisfaction in place. The birth arm stays.
    #[test]
    fn update_sentiments_adds_stored_satisfaction_on_its_tier() {
        let mut pop = make_pop();
        pop.desires[1].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.stored_effects
            .push(PopEffect::Satisfaction { tier: 1, amount: 1.5 });
        pop.stored_effects.push(PopEffect::Birthrate(0.01));

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.contentment() - 1.03 / 1.03).abs() < 1e-9);
        assert_eq!(
            pop.stored_effects,
            vec![
                PopEffect::Satisfaction { tier: 1, amount: 1.5 },
                PopEffect::Birthrate(0.01),
            ]
        );
    }

    /// One basic level is met and no earlier living is stored. The shift
    /// adds nothing. Today's living is kept for the next day.
    #[test]
    fn update_sentiments_records_living_before_it_can_shift() {
        let mut pop = make_pop();
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(&MarketHistory::new());

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sol_samples.len(), 1);
        assert!((pop.sol_samples[0] - 1.0).abs() < 1e-9);
    }

    /// Yesterday's living was 0 and today one level is met. The rise of 1
    /// moves 0.25 into hope and 0.25 into happiness.
    #[test]
    fn update_sentiments_moves_hope_and_happiness_from_a_rising_living() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(0.0);
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.hope() - 0.25 / 1.50).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - 0.25 / 1.50).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.50).abs() < 1e-9);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);
    }

    /// The recent living was 2 and today one level is met. The fall of 1
    /// moves 0.25 into anger and 0.25 into fear. The basic need is met, so
    /// the tier adds nothing.
    #[test]
    fn update_sentiments_moves_anger_and_fear_from_a_falling_living() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(2.0);
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.anger() - 0.25 / 1.50).abs() < 1e-9);
        assert!((pop.sentiment.fear() - 0.25 / 1.50).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.50).abs() < 1e-9);
        assert_eq!(pop.sentiment.hope(), 0.0);
        assert_eq!(pop.sentiment.happiness(), 0.0);
    }

    /// Living rose by 1 and the household holds 10 of wealth. Hope takes
    /// 0.25 from the rise and 0.10 from the wealth. The rise is the larger add.
    #[test]
    fn update_sentiments_lets_a_rising_living_outweigh_wealth() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(0.0);
        pop.property.insert(1, PopPRow::new(10.0));
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);
        let mut history = MarketHistory::new();
        history.prices.insert(1, 1.0);
        history.salability.insert(1, 1.0);

        pop.update_sentiments(&history);

        assert!((pop.sentiment.hope() - 0.35 / 1.60).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - 0.25 / 1.60).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.60).abs() < 1e-9);
    }

    /// Five earlier days are stored, the oldest at 0 and the rest at 1.
    /// Today's living is 1, so the gap is 0.2 and each of hope and happiness
    /// takes 0.05. The oldest day is dropped and today is kept.
    #[test]
    fn update_sentiments_drops_the_oldest_living_sample() {
        let mut pop = make_pop();
        pop.sol_samples = VecDeque::from([0.0, 1.0, 1.0, 1.0, 1.0]);
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.hope() - 0.05 / 1.10).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - 0.05 / 1.10).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.10).abs() < 1e-9);
        assert_eq!(pop.sol_samples.len(), 5);
        assert!(pop.sol_samples.iter().all(|sample| (sample - 1.0).abs() < 1e-9));
    }

    /// A pop with no desires has infinite living. That reading is not a
    /// rise, and it is not stored over the earlier finite day.
    #[test]
    fn update_sentiments_skips_an_infinite_living() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(1.0);

        pop.update_sentiments(&MarketHistory::new());

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sol_samples, VecDeque::from([1.0]));
    }

    /// Five households hold 10 grain and a pile of time. Wealth is 2 per
    /// household. Time is left out, and hope takes 0.02 of the pop.
    #[test]
    fn update_sentiments_divides_wealth_by_households_and_skips_time() {
        let mut pop = make_pop();
        pop.demographics.household.count = 5.0;
        pop.property.insert(TIME, PopPRow::new(100.0));
        pop.property.insert(1, PopPRow::new(10.0));
        let mut history = MarketHistory::new();
        history.prices.insert(1, 1.0);
        history.salability.insert(1, 1.0);

        pop.update_sentiments(&history);

        assert!((pop.sentiment.hope() - 0.02 / 1.02).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.02).abs() < 1e-9);
        assert_eq!(pop.sentiment.fear(), 0.0);
    }

    /// Ten units priced at -1 cost 10 to hold. Salability does not shrink
    /// that cost. Fear takes 0.10 of the pop.
    #[test]
    fn update_sentiments_moves_fear_from_negative_wealth() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(10.0));
        let mut history = MarketHistory::new();
        history.prices.insert(1, -1.0);
        history.salability.insert(1, 0.1);

        pop.update_sentiments(&history);

        assert!((pop.sentiment.fear() - 0.10 / 1.10).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.10).abs() < 1e-9);
        assert_eq!(pop.sentiment.hope(), 0.0);
        assert_eq!(pop.sentiment.anger(), 0.0);
    }

    /// No households. The grain on hand does not move hope.
    #[test]
    fn update_sentiments_skips_wealth_when_there_are_no_households() {
        let mut pop = make_pop();
        pop.demographics.household.count = 0.0;
        pop.property.insert(1, PopPRow::new(10.0));
        let mut history = MarketHistory::new();
        history.prices.insert(1, 1.0);
        history.salability.insert(1, 1.0);

        pop.update_sentiments(&history);

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sentiment.hope(), 0.0);
    }

    /// The basic desire is unmet. Stored satisfaction of 0.5 on that tier
    /// cuts the gap in half, so anger and fear each take 0.02.
    #[test]
    fn update_sentiments_reduces_basic_unmet_by_stored_satisfaction() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.stored_effects
            .push(PopEffect::Satisfaction { tier: 0, amount: 0.5 });

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.anger() - 0.02 / 1.04).abs() < 1e-9);
        assert!((pop.sentiment.fear() - 0.02 / 1.04).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.04).abs() < 1e-9);
        assert_eq!(
            pop.stored_effects,
            vec![PopEffect::Satisfaction { tier: 0, amount: 0.5 }]
        );
    }

    /// Stored basic satisfaction larger than the gap clears it. Anger and
    /// fear stay empty, and the stored satisfaction stays.
    #[test]
    fn update_sentiments_lets_stored_basic_satisfaction_clear_the_gap() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.stored_effects
            .push(PopEffect::Satisfaction { tier: 0, amount: 1.5 });

        pop.update_sentiments(&MarketHistory::new());

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);
        assert_eq!(
            pop.stored_effects,
            vec![PopEffect::Satisfaction { tier: 0, amount: 1.5 }]
        );
    }

    /// An unmet basic desire carries a satisfaction malus of 0.5. The gap
    /// is 1.5, so anger and fear each take 0.06.
    #[test]
    fn update_sentiments_deepens_basic_unmet_with_a_satisfaction_malus() {
        let mut pop = make_pop();
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic
            .effect
            .push(DesireEffect::Satisfaction(0.5, false));
        pop.desires[0].push(basic);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.anger() - 0.06 / 1.12).abs() < 1e-9);
        assert!((pop.sentiment.fear() - 0.06 / 1.12).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.12).abs() < 1e-9);
    }

    /// The basic desire is met and its bonus adds half a level. Basic
    /// satisfaction does not move hope or happiness, and nothing is unmet.
    #[test]
    fn update_sentiments_leaves_a_met_basic_bonus_alone() {
        let mut pop = make_pop();
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        basic
            .effect
            .push(DesireEffect::Satisfaction(0.5, true));
        pop.desires[0].push(basic);

        pop.update_sentiments(&MarketHistory::new());

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sentiment.hope(), 0.0);
        assert_eq!(pop.sentiment.happiness(), 0.0);
        assert_eq!(pop.sentiment.anger(), 0.0);
    }

    /// Sentiment starts even, so a contentment add is visible. The common
    /// desire is one level met and its bonus adds another half. 15 percent
    /// of each other share also moves into contentment.
    #[test]
    fn update_sentiments_shows_common_satisfaction_above_one_level() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::even();
        let mut common = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        common.satisfaction = 4.0;
        common
            .effect
            .push(DesireEffect::Satisfaction(0.5, true));
        pop.desires[1].push(common);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.contentment() - 0.35 / 1.03).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - 0.17 / 1.03).abs() < 1e-9);
        assert!((pop.sentiment.hope() - 0.17 / 1.03).abs() < 1e-9);
        assert!((pop.sentiment.anger() - 0.17 / 1.03).abs() < 1e-9);
        assert!((pop.sentiment.fear() - 0.17 / 1.03).abs() < 1e-9);
    }

    /// One luxury desire is met twice and its bonus adds one more level,
    /// so the tier is 3. Hope and happiness each take 0.03.
    #[test]
    fn update_sentiments_adds_luxury_bonus_levels() {
        let mut pop = make_pop();
        let mut luxury = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        luxury.satisfaction = 8.0;
        luxury
            .effect
            .push(DesireEffect::Satisfaction(0.5, true));
        pop.desires[2].push(luxury);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.hope() - 0.03 / 1.06).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - 0.03 / 1.06).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.06).abs() < 1e-9);
    }

    /// Sentiment starts even. The common desire is unmet and its malus
    /// pulls the tier below zero, which adds nothing. 15 percent of each
    /// other share still moves into contentment.
    #[test]
    fn update_sentiments_ignores_a_negative_common_level() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::even();
        let mut common = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        common
            .effect
            .push(DesireEffect::Satisfaction(0.5, false));
        pop.desires[1].push(common);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.contentment() - 0.32).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - 0.17).abs() < 1e-9);
        assert!((pop.sentiment.anger() - 0.17).abs() < 1e-9);
        assert!((pop.sentiment.fear() - 0.17).abs() < 1e-9);
        assert!((pop.sentiment.hope() - 0.17).abs() < 1e-9);
    }

    /// No desires sit on the common tier. Stored satisfaction of 1.5 there
    /// is the whole level. Sentiment starts even, so the add is visible.
    /// 15 percent of each other share also moves into contentment.
    #[test]
    fn update_sentiments_counts_stored_satisfaction_on_an_empty_tier() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::even();
        pop.stored_effects
            .push(PopEffect::Satisfaction { tier: 1, amount: 1.5 });

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.contentment() - 0.35 / 1.03).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - 0.17 / 1.03).abs() < 1e-9);
        assert!((pop.sentiment.hope() - 0.17 / 1.03).abs() < 1e-9);
        assert!(pop.sol_samples.is_empty());
        assert_eq!(
            pop.stored_effects,
            vec![PopEffect::Satisfaction { tier: 1, amount: 1.5 }]
        );
    }

    /// Today's living matches the one day already stored. The shift adds
    /// nothing, and both days stay on the list.
    #[test]
    fn update_sentiments_adds_nothing_when_living_matches_the_average() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(1.0);
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(&MarketHistory::new());

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sol_samples, VecDeque::from([1.0, 1.0]));
    }

    /// Living rises from 0 to 1, then stays at 1. The second day is still
    /// half a point above the average, so hope and happiness are pushed
    /// again. 15 percent of the first day's hope and happiness moves into
    /// contentment.
    #[test]
    fn update_sentiments_adds_the_living_shift_again_on_the_next_day() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(0.0);
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.satisfaction = 4.0;
        pop.desires[0].push(basic);

        pop.update_sentiments(&MarketHistory::new());
        pop.update_sentiments(&MarketHistory::new());

        let carried_hope = 0.25 / 1.50;
        let carried_contentment = 1.0 / 1.50;
        let hope = carried_hope * (1.0 - 0.15) + 0.125;
        let contentment = carried_contentment + 0.15 * carried_hope * 2.0;
        let mass = hope + hope + contentment;
        assert!((pop.sentiment.hope() - hope / mass).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - hope / mass).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - contentment / mass).abs() < 1e-9);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sol_samples, VecDeque::from([0.0, 1.0, 1.0]));
    }

    /// Living fell from 1 to 0, and the basic desire is unmet. Anger and
    /// fear take 0.25 from the fall and 0.04 from the gap. The fall is larger.
    #[test]
    fn update_sentiments_lets_a_falling_living_outweigh_unmet_basic() {
        let mut pop = make_pop();
        pop.sol_samples.push_back(1.0);
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.anger() - 0.29 / 1.58).abs() < 1e-9);
        assert!((pop.sentiment.fear() - 0.29 / 1.58).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.58).abs() < 1e-9);
        assert_eq!(pop.sentiment.hope(), 0.0);
        assert_eq!(pop.sentiment.happiness(), 0.0);
    }

    /// No desires and no stock. The pop that is mostly angry moves more
    /// of itself into contentment than the pop that is mostly content.
    #[test]
    fn update_sentiments_pulls_a_larger_share_harder_toward_contentment() {
        let mut far = make_pop();
        far.sentiment = Sentiment::from_parts(0.0, 0.2, 0.8, 0.0, 0.0);
        let mut near = make_pop();
        near.sentiment = Sentiment::from_parts(0.0, 0.8, 0.2, 0.0, 0.0);

        far.update_sentiments(&MarketHistory::new());
        near.update_sentiments(&MarketHistory::new());

        assert!((far.sentiment.anger() - 0.68).abs() < 1e-9);
        assert!((far.sentiment.contentment() - 0.32).abs() < 1e-9);
        assert!((near.sentiment.anger() - 0.17).abs() < 1e-9);
        assert!((near.sentiment.contentment() - 0.83).abs() < 1e-9);
    }

    /// No desires and no stock. A fully angry pop moves 15 percent of that
    /// share into contentment, then 15 percent of what remains.
    #[test]
    fn update_sentiments_pulls_again_when_the_cause_is_gone() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::from_parts(0.0, 0.0, 1.0, 0.0, 0.0);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.anger() - 0.85).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 0.15).abs() < 1e-9);
        assert_eq!(pop.sentiment.hope(), 0.0);
        assert_eq!(pop.sentiment.happiness(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.anger() - 0.7225).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 0.2775).abs() < 1e-9);
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
    /// satisfaction, so day 2 buys again. Day 2 also buys the saved day,
    /// eats one level, and holds what is left of that day.
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
        assert!((actors.pop(1).property[&2].quantity - 2.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&2].quantity).abs() < 1e-9);
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

    /// The pop holds 4 grain and the basic desire wants 4. Consume meets it.
    /// The desire's happiness bonus of 0.2 then shifts that share of the pop
    /// out of contentment.
    #[test]
    fn market_day_shifts_sentiment_from_a_met_desire() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(4.0));
        let mut basic = desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        );
        basic.effect.push(DesireEffect::SentimentFlat(
            SentimentKind::Happiness,
            0.2,
            true,
        ));
        pop.desires[0].push(basic);
        let factuals = Factuals::new().with_good(make_good(1, "grain", 0.0));
        let mut market = Market::new(1);
        market.pops.insert(pop.id);
        let mut actors = Actors::new();
        actors.pops.insert(pop.id, pop);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let pop = actors.pop(1);
        // The grain is eaten, so wealth adds nothing. The basic need is met.
        assert!((pop.sentiment.happiness() - 0.2 / 1.2).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.2).abs() < 1e-9);
    }

    /// The first day eats the grain and records living of 1. The next morning
    /// clears that satisfaction and nothing is left to eat, so living is 0.
    /// Anger and fear take 0.25 from the fall and 0.04 from the unmet need.
    #[test]
    fn market_day_shifts_anger_after_living_falls() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(4.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let factuals = Factuals::new().with_good(make_good(1, "grain", 0.0));
        let mut market = Market::new(1);
        market.pops.insert(pop.id);
        let mut actors = Actors::new();
        actors.pops.insert(pop.id, pop);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);
        let pop = actors.pop(1);
        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sol_samples, VecDeque::from([1.0]));

        market.market_day(&mut actors, &factuals, &mut rng);
        let pop = actors.pop(1);
        assert!((pop.sentiment.anger() - 0.29 / 1.58).abs() < 1e-9);
        assert!((pop.sentiment.fear() - 0.29 / 1.58).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.58).abs() < 1e-9);
        assert_eq!(pop.sol_samples, VecDeque::from([1.0, 0.0]));
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

    /// Half the leaving share's savings stay on the origin. The destination
    /// keeps the claim it already had.
    #[test]
    fn take_migrants_shrinks_savings_and_the_destination_keeps_its_own() {
        let mut origin = make_pop();
        origin.demographics.household.count = 10.0;
        origin.savings.insert(1, 40.0);
        origin.luxury_savings.insert(2, 20.0);
        let mut dest = make_pop();
        dest.id = 2;
        dest.demographics.household.count = 10.0;
        dest.savings.insert(1, 8.0);

        let group = origin.take_migrants(4.0);
        dest.receive_migrants(&group);

        assert!((origin.savings[&1] - 32.0).abs() < 1e-9);
        assert!((origin.luxury_savings[&2] - 16.0).abs() < 1e-9);
        assert!((dest.savings[&1] - 8.0).abs() < 1e-9);
        assert!(dest.luxury_savings.is_empty());
    }

    #[test]
    fn market_day_matches_desires_to_the_fitted_population() {
        let demo = DemoDesire::new(1)
            .with_tier(0)
            .with_scalar(ScalingFactor::Household(1.0))
            .with_amount(2.0);
        let factuals = Factuals::new()
            .with_species(Species::new(0, "human").with_desire(demo));
        let mut pop = make_pop();
        // One household fitted back under 1 is pruned. Two stay above that line.
        pop.demographics.household.count = 2.0;
        let start = pop.demographics.total_population();
        pop.population_cap = Some(start);
        let linked = factuals.species[&0]
            .find_desire(1)
            .unwrap()
            .create_desire(&pop, DemographicSource::Species(0));
        pop.desires[0].push(linked);
        pop.desires[0][0].amount = 99.0;
        pop.desires[0][0].satisfaction = 49.5;
        let mut market = Market::new(1);
        market.pops.insert(pop.id);
        let mut actors = Actors::new();
        actors.pops.insert(pop.id, pop);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let pop = actors.pop(1);
        let count = pop.demographics.household.count;
        assert!((pop.demographics.total_population() - start).abs() < 1e-6);
        assert!((pop.desires[0][0].amount - 2.0 * count).abs() < 1e-9);
    }

    /// Growth raises people per household. The cap then scales that one
    /// household under 1, and the pop is removed.
    #[test]
    fn market_day_removes_a_one_household_pop_the_cap_leaves_under_one() {
        let factuals = Factuals::new().with_species(Species::new(0, "human"));
        let mut pop = make_pop();
        let start = pop.demographics.total_population();
        pop.population_cap = Some(start);
        let mut market = Market::new(1);
        market.pops.insert(pop.id);
        let mut actors = Actors::new();
        actors.pops.insert(pop.id, pop);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        assert!(actors.pops.get(&1).is_none());
        assert!(!market.pops.contains(&1));
    }

    #[test]
    fn rebalance_then_rescale_keeps_the_met_fraction() {
        let demo = DemoDesire::new(1)
            .with_tier(0)
            .with_scalar(ScalingFactor::Household(1.0))
            .with_amount(2.0);
        let factuals = Factuals::new()
            .with_species(Species::new(0, "human").with_desire(demo));
        let mut pop = make_pop();
        let start = pop.demographics.total_population();
        pop.population_cap = Some(start);
        pop.demographics.household.count = 2.0;
        let linked = factuals.species[&0]
            .find_desire(1)
            .unwrap()
            .create_desire(&pop, DemographicSource::Species(0));
        pop.desires[0].push(linked);
        pop.desires[0][0].amount = 99.0;
        pop.desires[0][0].satisfaction = 49.5;
        let mut market = Market::new(1);
        market.pops.insert(pop.id);
        let mut actors = Actors::new();
        actors.pops.insert(pop.id, pop);

        market.rebalance_population_cap(&mut actors);
        actors.pop_mut(1).rescale_desires(&factuals);

        let pop = actors.pop(1);
        let amount = pop.desires[0][0].amount;
        assert!((pop.demographics.total_population() - start).abs() < 1e-9);
        assert!((amount - 2.0 * pop.demographics.household.count).abs() < 1e-9);
        assert!((pop.desires[0][0].satisfaction / amount - 0.5).abs() < 1e-9);
    }

    /// A basic day stays on bread even when grain rates higher and is already held.
    #[test]
    fn savings_keeps_a_basic_day_in_the_bucket() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(1, PopPRow::new(5.0));
        let factuals = Factuals::new()
            .with_good(make_good(2, "bread", 0.5))
            .with_good(make_good(1, "grain", 0.0))
            .with_process(bake());
        let history = card(&[(1, 1.0, 1.0), (2, 1.0, 1.0)]);

        pop.plan(&factuals, &history);

        assert_eq!(pop.savings, HashMap::from([(2, 8.0)]));
    }

    /// Luxury may leave the bucket. Bread rots. Grain does not, and five
    /// grain is already the outside day.
    #[test]
    fn savings_moves_the_day_onto_the_higher_rated_good() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(1, PopPRow::new(5.0));
        let factuals = Factuals::new()
            .with_good(make_good(2, "bread", 0.5))
            .with_good(make_good(1, "grain", 0.0))
            .with_process(bake());
        let history = card(&[(1, 1.0, 1.0), (2, 1.0, 1.0)]);

        pop.plan(&factuals, &history);

        assert_eq!(pop.savings, HashMap::from([(1, 5.0)]));
    }

    #[test]
    fn savings_skips_a_good_that_decays_completely() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(TIME, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let factuals = Factuals::new()
            .with_good(make_good(TIME, "time", 1.0))
            .with_process(
                Process::new(1, "make time", 0).with_output(ProcessOutput::new(TIME, 1.0, true)),
            );

        pop.plan(&factuals, &MarketHistory::new());

        assert!(pop.savings.is_empty());
    }

    /// Luxury may leave the bucket. Gold that did not arrive rates 0.
    /// The same buy, filled, outrates bread.
    #[test]
    fn savings_ignores_a_good_yesterdays_buy_missed() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.buy_posted.insert(9, 10.0);
        let factuals = Factuals::new()
            .with_good(make_good(2, "bread", 0.5))
            .with_good(make_good(9, "gold", 0.0))
            .with_process(bake());
        let history = card(&[(2, 1.0, 1.0), (9, 1.0, 1.0)]);

        pop.plan(&factuals, &history);
        assert_eq!(pop.savings, HashMap::from([(2, 8.0)]));

        pop.buy_received.insert(9, 10.0);
        pop.plan(&factuals, &history);
        assert_eq!(pop.savings, HashMap::from([(9, 5.0)]));
    }

    #[test]
    fn savings_waits_to_save_a_later_tier() {
        let mut pop = make_pop();
        pop.job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(1.0), vec![]),
                JobLine::new(9, Some(1.0), vec![]),
            ],
        );
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[1].push(desire(
            2,
            vec![DesireTarget::new(4, DesireTargetType::Consume, 1.0)],
            2.0,
        ));
        let factuals = Factuals::new()
            .with_good(make_good(2, "bread", 0.0))
            .with_good(make_good(4, "cloth", 0.0))
            .with_process(bake())
            .with_process(
                Process::new(9, "weave", 0).with_output(ProcessOutput::new(4, 1.0, true)),
            );
        let history = card(&[(2, 1.0, 1.0), (4, 1.0, 1.0)]);

        pop.plan(&factuals, &history);
        assert_eq!(pop.savings, HashMap::from([(2, 4.0)]));

        // Three bread is short of the saved day. Four is that day.
        pop.property.insert(2, PopPRow::new(3.0));
        pop.plan(&factuals, &history);
        assert_eq!(pop.savings, HashMap::from([(2, 4.0)]));

        pop.property.insert(2, PopPRow::new(4.0));
        pop.plan(&factuals, &history);
        assert_eq!(pop.savings, HashMap::from([(2, 4.0), (4, 2.0)]));
    }

    /// Bread decays by half, so the day is 4 and the hold is 8. Four on hand
    /// opens the next tier. Three does not. The pile stays the hold.
    #[test]
    fn savings_opens_the_next_tier_once_one_day_is_on_hand() {
        let mut pop = make_pop();
        pop.job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(1.0), vec![]),
                JobLine::new(9, Some(1.0), vec![]),
            ],
        );
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[1].push(desire(
            2,
            vec![DesireTarget::new(4, DesireTargetType::Consume, 1.0)],
            2.0,
        ));
        let factuals = Factuals::new()
            .with_good(make_good(2, "bread", 0.5))
            .with_good(make_good(4, "cloth", 0.0))
            .with_process(bake())
            .with_process(
                Process::new(9, "weave", 0).with_output(ProcessOutput::new(4, 1.0, true)),
            );
        let history = card(&[(2, 1.0, 1.0), (4, 1.0, 1.0)]);

        pop.property.insert(2, PopPRow::new(3.0));
        pop.plan(&factuals, &history);
        assert_eq!(pop.savings, HashMap::from([(2, 8.0)]));

        pop.property.insert(2, PopPRow::new(4.0));
        pop.plan(&factuals, &history);
        assert_eq!(pop.savings, HashMap::from([(2, 8.0), (4, 2.0)]));
    }

    #[test]
    fn sell_orders_keep_the_saved_day() {
        let mut pop = make_pop();
        pop.property.insert(9, PopPRow::new(8.0));
        pop.savings.insert(9, 5.0);

        let orders = pop.sell_orders(&MarketHistory::new());

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 9);
        assert!((orders[0].target_amount + 3.0).abs() < 1e-9);
    }

    #[test]
    fn spendable_keeps_the_saved_day() {
        let mut pop = make_pop();
        pop.property.insert(9, PopPRow::new(8.0));
        pop.savings.insert(9, 5.0);

        assert!((pop.spendable(9, 1) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn buy_orders_add_the_saved_day_onto_the_open_desire() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.savings.insert(2, 4.0);

        do_satisfy(&mut pop);
        let orders = pop.buy_orders(&MarketHistory::new());

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 2);
        assert_eq!(orders[0].target_amount, 8.0);
    }

    /// Grain fills the meal. Bread satisfies twice as much per unit, and both
    /// prices are 1, so bread is the cheaper satisfaction. One level is 2
    /// bread. The grain stays in the bucket and on hand.
    #[test]
    fn buy_orders_asks_for_a_level_of_a_cheaper_substitute() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(1, DesireTargetType::Consume, 1.0),
                DesireTarget::new(2, DesireTargetType::Consume, 2.0),
            ],
            4.0,
        ));
        pop.property.insert(1, PopPRow::new(4.0));
        do_satisfy(&mut pop);
        let history = card(&[(1, 1.0, 1.0), (2, 1.0, 1.0)]);

        let orders = pop.buy_orders(&history);

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 2);
        assert_eq!(orders[0].target_amount, 2.0);
        assert_eq!(pop.property[&1].quantity, 4.0);
    }

    /// The meal is not filled yet. Grain is still on hand, and bread at the
    /// same AMV buys twice the satisfaction. The buy is one level of bread,
    /// 4 units, and the grain is not posted beside it.
    #[test]
    fn buy_orders_moves_a_short_level_onto_the_cheaper_substitute() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(1, DesireTargetType::Consume, 1.0),
                DesireTarget::new(2, DesireTargetType::Consume, 2.0),
            ],
            8.0,
        ));
        pop.property.insert(1, PopPRow::new(4.0));
        do_satisfy(&mut pop).expect("grain is short of the level");
        let history = card(&[(1, 1.0, 1.0), (2, 1.0, 1.0)]);

        let orders = pop.buy_orders(&history);

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 2);
        assert_eq!(orders[0].target_amount, 4.0);
        assert_eq!(pop.property[&1].quantity, 4.0);
    }

    /// Bread at 3 against grain at 1 costs more per point of satisfaction
    /// (1.5 against 1). The meal stays on the grain, and nothing is posted.
    #[test]
    fn buy_orders_keeps_grain_when_bread_costs_more_satisfaction() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(1, DesireTargetType::Consume, 1.0),
                DesireTarget::new(2, DesireTargetType::Consume, 2.0),
            ],
            4.0,
        ));
        pop.property.insert(1, PopPRow::new(4.0));
        do_satisfy(&mut pop);
        let history = card(&[(1, 1.0, 1.0), (2, 3.0, 1.0)]);

        assert!(pop.buy_orders(&history).is_empty());
    }

    #[test]
    fn buy_orders_skip_a_saved_day_already_free() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(2, PopPRow::new(8.0));
        pop.savings.insert(2, 4.0);

        assert!(do_satisfy(&mut pop).is_none());
        assert!(pop.buy_orders(&MarketHistory::new()).is_empty());
    }

    #[test]
    fn consume_eats_one_luxury_level_and_leaves_the_saved_day() {
        let mut pop = make_pop();
        pop.property.insert(8, PopPRow::new(8.0));
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(8, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.savings.insert(8, 4.0);
        pop.luxury_savings.insert(8, 4.0);

        pop.consume();

        assert!((pop.desires[2][0].satisfaction - 4.0).abs() < 1e-9);
        assert!((pop.property[&8].quantity - 4.0).abs() < 1e-9);

        let factuals = Factuals::new().with_good(make_good(8, "cabins", 0.0));
        pop.plan(&factuals, &MarketHistory::new());
        assert!((pop.luxury_reached - 1.0).abs() < 1e-9);
    }

    #[test]
    fn consume_stops_a_second_luxury_level_while_the_saved_day_is_short() {
        let mut pop = make_pop();
        pop.property.insert(8, PopPRow::new(6.0));
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(8, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.savings.insert(8, 4.0);
        pop.luxury_savings.insert(8, 4.0);

        pop.consume();

        assert!((pop.desires[2][0].satisfaction - 4.0).abs() < 1e-9);
        assert!((pop.property[&8].quantity - 2.0).abs() < 1e-9);
    }

    #[test]
    fn consume_still_eats_a_basic_level_held_as_savings() {
        let mut pop = make_pop();
        pop.property.insert(2, PopPRow::new(4.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.savings.insert(2, 4.0);

        pop.consume();

        assert!((pop.desires[0][0].satisfaction - 4.0).abs() < 1e-9);
        assert!(pop.property[&2].quantity.abs() < 1e-9);
    }

    #[test]
    fn consume_stops_further_luxury_when_an_earlier_day_was_short() {
        let mut pop = make_pop();
        pop.property.insert(8, PopPRow::new(8.0));
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(8, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.luxury_savings_open = false;

        pop.consume();

        assert!((pop.desires[2][0].satisfaction - 4.0).abs() < 1e-9);
        assert!((pop.property[&8].quantity - 4.0).abs() < 1e-9);
    }

    #[test]
    fn consume_eats_another_luxury_level_when_no_day_was_saved() {
        let mut pop = make_pop();
        pop.property.insert(8, PopPRow::new(8.0));
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(8, DesireTargetType::Consume, 1.0)],
            4.0,
        ));

        pop.consume();

        assert!((pop.desires[2][0].satisfaction - 8.0).abs() < 1e-9);
        assert!(pop.property[&8].quantity.abs() < 1e-9);
    }

    #[test]
    fn plan_closes_luxury_while_an_earlier_day_is_short() {
        let mut pop = make_pop();
        pop.job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(1.0), vec![]),
                JobLine::new(8, Some(1.0), vec![]),
            ],
        );
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[2].push(desire(
            3,
            vec![DesireTarget::new(8, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let factuals = Factuals::new()
            .with_good(make_good(2, "bread", 0.0))
            .with_good(make_good(8, "cabins", 0.0))
            .with_process(bake())
            .with_process(
                Process::new(8, "cabins", 0).with_output(ProcessOutput::new(8, 1.0, true)),
            );
        let history = card(&[(2, 1.0, 1.0), (8, 1.0, 1.0)]);

        pop.plan(&factuals, &history);

        assert!(!pop.luxury_savings_open);
        assert!(pop.luxury_savings.is_empty());

        // One saved day of bread is on hand. Tomorrow's meal is not a second day.
        pop.property.insert(2, PopPRow::new(4.0));
        pop.plan(&factuals, &history);

        assert!(pop.luxury_savings_open);
        assert_eq!(pop.luxury_savings, HashMap::from([(8, 4.0)]));
    }

    /// Ten time makes two grain, and the board prices grain at 1. The line
    /// loses money, so the quota is the meal and not the saved day. No
    /// faster grain process is known, so the meal is bought and still made.
    /// Once that grain is on hand, the buy stops and the farm target steps down.
    #[test]
    fn plan_buys_a_subsistence_cover_and_still_makes_it() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let factuals = Factuals::new()
            .with_good(make_good(TIME, "time", 1.0))
            .with_good(make_good(1, "grain", 0.0))
            .with_process(
                Process::new(29, "subsistence farm", 0)
                    .with_input(ProcessInput::new(TIME, 10.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(1, 2.0, true)),
            );
        let history = card(&[(1, 1.0, 1.0)]);

        pop.plan(&factuals, &history);

        // Meal of 4, plus one grain. The saved day of 4 stays off the quota.
        assert_eq!(pop.cheaper_cover, HashMap::from([(1, 4.0)]));
        let farm = pop
            .job
            .lines
            .iter()
            .find(|line| line.process == 29)
            .expect("farm line");
        assert!((farm.target.expect("farm target") - 2.5).abs() < 1e-9);
        let orders = pop.buy_orders(&history);
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 1);
        assert_eq!(orders[0].target_amount, 4.0);

        pop.property.insert(1, PopPRow::new(4.0));
        pop.plan(&factuals, &history);

        // The meal is on hand. The line does not pay, so 2.5 steps toward one run.
        assert!(pop.cheaper_cover.is_empty());
        let farm = pop
            .job
            .lines
            .iter()
            .find(|line| line.process == 29)
            .expect("farm line");
        assert!((farm.target.expect("farm target") - 1.875).abs() < 1e-9);
    }

    /// Make grain is known and cheaper per time than the farm. The farm is
    /// not on a job that runs it. The buy is the day's meal, and it stays
    /// posted once that grain is on hand. The farm keeps making the gap
    /// until then, then steps down.
    #[test]
    fn plan_keeps_a_worse_meal_until_the_grain_is_on_hand() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let factuals = Factuals::new()
            .with_good(make_good(TIME, "time", 1.0))
            .with_good(make_good(1, "grain", 0.0))
            .with_process(
                Process::new(29, "subsistence farm", 0)
                    .with_input(ProcessInput::new(TIME, 10.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(1, 2.0, true)),
            )
            .with_process(
                Process::new(1, "make grain", 0)
                    .with_input(ProcessInput::new(TIME, 0.5, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(1, 6.0, true)),
            );
        let history = card(&[(1, 1.0, 1.0)]);

        pop.plan(&factuals, &history);

        // Meal of 4, plus one grain. The morning grant can finish the 2.5 runs.
        assert_eq!(pop.cheaper_cover, HashMap::from([(1, 4.0)]));
        let farm = pop
            .job
            .lines
            .iter()
            .find(|line| line.process == 29)
            .expect("farm line");
        assert!((farm.target.expect("farm target") - 2.5).abs() < 1e-9);
        let orders = pop.buy_orders(&history);
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].target, 1);
        assert_eq!(orders[0].target_amount, 4.0);

        pop.property.insert(1, PopPRow::new(4.0));
        pop.plan(&factuals, &history);

        // The meal is on hand, so 2.5 steps toward one run. The buy stays
        // at that meal. This farm is worse than make grain.
        assert_eq!(pop.cheaper_cover, HashMap::from([(1, 4.0)]));
        let farm = pop
            .job
            .lines
            .iter()
            .find(|line| line.process == 29)
            .expect("farm line");
        assert!((farm.target.expect("farm target") - 1.875).abs() < 1e-9);
    }

    /// Two grain are on hand of a meal of four. The farm is worse than make
    /// grain, so the buy is the whole meal. The line still aims at the hole.
    #[test]
    fn plan_buys_the_whole_meal_while_some_grain_is_on_hand() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(29, Some(0.0), vec![])]);
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(1, PopPRow::new(2.0));
        let factuals = Factuals::new()
            .with_good(make_good(TIME, "time", 1.0))
            .with_good(make_good(1, "grain", 0.0))
            .with_process(
                Process::new(29, "subsistence farm", 0)
                    .with_input(ProcessInput::new(TIME, 10.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(1, 2.0, true)),
            )
            .with_process(
                Process::new(1, "make grain", 0)
                    .with_input(ProcessInput::new(TIME, 0.5, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(1, 6.0, true)),
            );
        let history = card(&[(1, 1.0, 1.0)]);

        pop.plan(&factuals, &history);

        // Hole of 2, plus one grain, over two grain a run. The buy is 4.
        assert_eq!(pop.cheaper_cover, HashMap::from([(1, 4.0)]));
        let farm = pop
            .job
            .lines
            .iter()
            .find(|line| line.process == 29)
            .expect("farm line");
        assert!((farm.target.expect("farm target") - 1.5).abs() < 1e-9);
    }

    /// Half a time makes six grain. That costs less than the board, so the
    /// specialty keeps the cover and subsistence is not the line that buys.
    #[test]
    fn plan_keeps_a_specialty_cover_when_making_costs_less() {
        let mut pop = make_pop();
        pop.job = Job::new(
            1,
            vec![
                JobLine::new(29, Some(0.0), vec![]),
                JobLine::new(1, Some(0.0), vec![]),
            ],
        );
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let factuals = Factuals::new()
            .with_good(make_good(TIME, "time", 1.0))
            .with_good(make_good(1, "grain", 0.0))
            .with_process(
                Process::new(29, "subsistence farm", 0)
                    .with_input(ProcessInput::new(TIME, 10.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(1, 2.0, true)),
            )
            .with_process(
                Process::new(1, "make grain", 0)
                    .with_input(ProcessInput::new(TIME, 0.5, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(1, 6.0, true)),
            );
        let history = card(&[(1, 1.0, 1.0)]);

        pop.plan(&factuals, &history);

        assert!(pop.cheaper_cover.is_empty());
        let specialty = pop
            .job
            .lines
            .iter()
            .find(|line| line.process == 1)
            .expect("specialty line");
        // Meal of 4 plus the saved day of 4, plus one grain, over six grain a run.
        // A paying line may step surplus on top of that cover.
        assert!((specialty.cover - 1.5).abs() < 1e-9);
        assert!(specialty.target.expect("specialty target") + 1e-9 >= 1.5);
        let farm = pop
            .job
            .lines
            .iter()
            .find(|line| line.process == 29)
            .expect("farm line");
        assert_eq!(farm.target, Some(0.0));
    }
}
