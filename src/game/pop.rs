use std::collections::{HashMap, HashSet, VecDeque};

use crate::game::{
    actor::Actor,
    config::TIME_PER_LABOR,
    deal::{DealMaker, ProposedDeal},
    demographic_source::DemographicSource,
    desire::{DemoDesire, Desire, DesireTarget, DesireTargetType},
    effects::{DesireEffect, ProcessEffect},
    factuals::Factuals,
    good::{Good, GoodTag, TIME},
    household::{DemographicRates, Household},
    job::Job,
    market::{AMV_EPSILON, Market, MarketHistory},
    marketorder::MarketOrder,
    scalingfactor::ScalingFactor,
    sentiment::{Sentiment, SentimentKind, SentimentMod},
    util::whole_units_up,
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
    /// desire's winning good, walked forward by [`walk_pile`].
    /// The morning reset leaves the map, so today's
    /// match uses last night's pile. The job leaves the same pile. See
    /// [`Job::leave`](crate::game::job::Job::leave).
    savings: HashMap<usize, f64>,

    /// Units removed from [`Self::property`]. Not offered for sale.
    ///
    /// [`Self::meal`] and [`Job::input_hold`] are parts of this pile.
    /// The rest of a currency here is the savings target. Decay and
    /// migration see it. [`crate::game::ladder`] is the writer.
    pub(crate) saved: HashMap<usize, f64>,

    /// Part of [`Self::saved`] that consumption may eat.
    ///
    /// Reserved before the sell book. The rest of [`Self::saved`] stays out
    /// of the meal.
    pub(crate) meal: HashMap<usize, f64>,

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
    /// [`Self::plan`] writes the map. [`Self::mass_want`] buys the part of
    /// this cover that stock does not already hold, and
    /// [`Self::prepare_mass_buys`] funds that buy. A purchase on hand
    /// shrinks the next night's gap.
    cheaper_cover: HashMap<usize, f64>,

    /// Units funded for today's mass clear, by good.
    ///
    /// `None` until [`Self::prepare_mass_buys`] runs. The morning reset
    /// drops it. [`DealMaker::mass_order`] posts a listed amount and does
    /// not invent a buy for a good left out.
    mass_buys: Option<HashMap<usize, f64>>,

    /// Tender still allowed on today's sell book, by good.
    ///
    /// Empty until [`Self::note_currency_sale_room`]. The morning reset
    /// drops it. [`Self::mass_sell`] reads it.
    currency_sale_room: HashMap<usize, f64>,

    /// Units of each tender posted for sale today.
    ///
    /// [`Self::spend_currency_sale_room`] adds a post. The morning reset
    /// drops it. [`Self::plan`] notes it on the next sale.
    day_listed: HashMap<usize, f64>,

    /// Units this pop posted to buy today, by good.
    ///
    /// Cleared at the morning reset. [`Self::savings_fill`] is the reader.
    buy_posted: HashMap<usize, f64>,

    /// Units this pop received as the buyer today, by good.
    ///
    /// Cleared at the morning reset.
    buy_received: HashMap<usize, f64>,

    /// Payment given today, by good. [`Self::finalize`] writes it.
    ///
    /// Cleared at the morning reset.
    day_paid: HashMap<usize, f64>,

    /// Payment taken in today, by good. [`Self::finalize`] writes it.
    ///
    /// Cleared at the morning reset.
    day_received: HashMap<usize, f64>,

    /// The deal's good received today, by good. [`Self::finalize`] writes it.
    ///
    /// Cleared at the morning reset.
    day_got: HashMap<usize, f64>,

    /// The deal's good given today, by good. [`Self::finalize`] writes it.
    ///
    /// Cleared at the morning reset.
    day_gave: HashMap<usize, f64>,

    /// Transport spent as freight today, by good. [`Self::pay_freight`] writes it.
    ///
    /// Cleared at the morning reset.
    day_haul: HashMap<usize, f64>,

    /// Units eaten for a desire today, by good. [`Self::consume`] writes it.
    ///
    /// Cleared at the morning reset. A good only used, and freight, stay out.
    day_eaten: HashMap<usize, f64>,

    /// Units a desire used today, by good. [`Self::consume`] writes it.
    ///
    /// Cleared at the morning reset. These units return at decay.
    day_held: HashMap<usize, f64>,

    /// Units eaten as rest today, by good. [`Self::rest`] writes it.
    ///
    /// Cleared at the morning reset. Not part of the meal.
    rest_taken: HashMap<usize, f64>,

    /// Units created this morning, by good.
    ///
    /// [`Self::start_day`] replaces the map with this morning's grant.
    /// [`Self::reset_day`] leaves it. [`Self::day_books`] prices it as income.
    day_granted: HashMap<usize, f64>,

    /// Units posted to buy today, on the day tape.
    ///
    /// The buy ladder writes each bid. The morning reset drops it.
    tape_buy: HashMap<usize, f64>,

    /// Units posted to sell today, on the day tape.
    ///
    /// The sell book writes each offer. The morning reset drops it.
    tape_sell: HashMap<usize, f64>,

    /// The deal's good received today, on the day tape.
    ///
    /// [`Self::note_flow`] writes [`DayFlow::Got`]. The morning reset drops it.
    tape_bought: HashMap<usize, f64>,

    /// The deal's good given today, on the day tape.
    ///
    /// [`Self::note_flow`] writes [`DayFlow::Gave`]. The morning reset drops it.
    tape_sold: HashMap<usize, f64>,

    /// Tender given today, on the day tape.
    ///
    /// [`Self::note_flow`] writes [`DayFlow::Paid`]. The morning reset drops it.
    tape_paid: HashMap<usize, f64>,

    /// Tender taken today, on the day tape.
    ///
    /// [`Self::note_flow`] writes [`DayFlow::Received`]. The morning reset drops it.
    tape_earned: HashMap<usize, f64>,

    /// Change received today, on the day tape.
    ///
    /// The buy ladder writes it. The morning reset drops it.
    tape_change_in: HashMap<usize, f64>,

    /// Change given today, on the day tape.
    ///
    /// The buy ladder writes it. The morning reset drops it.
    tape_change_out: HashMap<usize, f64>,

    /// Meal units set aside today, on the day tape.
    ///
    /// [`Self::stash_working`] writes [`SaveHold::Meal`]. The morning reset drops it.
    tape_meal: HashMap<usize, f64>,

    /// Production inputs set aside today, on the day tape.
    ///
    /// [`Self::stash_working`] writes [`SaveHold::Input`]. The morning reset drops it.
    tape_input: HashMap<usize, f64>,

    /// Currency swept into savings today, on the day tape.
    ///
    /// [`Self::stash_working`] writes [`SaveHold::Currency`]. The morning reset drops it.
    tape_saved: HashMap<usize, f64>,

    /// Property that arrived with migrants today, on the day tape.
    ///
    /// [`Self::receive_migrants`] writes it. The morning reset drops it.
    tape_arrived: HashMap<usize, f64>,

    /// Property that left with migrants today, on the day tape.
    ///
    /// [`Self::take_migrants`] writes it. The morning reset drops it.
    tape_left: HashMap<usize, f64>,

    /// Saved stock that shrank with migrants and did not travel, on the day tape.
    ///
    /// [`Self::take_migrants`] writes it. The morning reset drops it.
    tape_dropped: HashMap<usize, f64>,

    /// Living [`Self::rest`] stored for [`Self::standard_of_living`].
    ///
    /// The morning reset clears it.
    rest_sol: f64,

    /// Private value of one unit, in the same units as the card.
    ///
    /// [`Self::revise_worth`] writes it from rot, purchases, makes, and
    /// units eaten or used. The buy reads it. It is not written onto the card.
    worth: HashMap<usize, f64>,

    /// Units eaten or used today, waiting for [`Self::revise_worth`].
    ///
    /// [`Self::decay_goods`] adds them. The revise clears the map.
    turned: HashMap<usize, f64>,

    /// Tonight's flows have not been folded into [`Self::worth`] yet.
    ///
    /// [`Self::revise_worth`] closes it. The morning reset opens it.
    worth_open: bool,
}

/// Saved days held above the day that will be eaten.
///
/// Fixed for this tester. Demographics and sentiment do not change it.
const SAVINGS_DAYS: f64 = 1.0;

/// Decay at or above this is too fast to hold as the saved day.
///
/// The saved-day choice keeps a slower good. A faster good is saved as a
/// more durable tender when one can be obtained, or left for the morning
/// that makes it.
const SAVINGS_EASY_DECAY: f64 = 0.10;

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

/// # Walk Pile
///
/// Moves `previous` one household toward `goal`.
///
/// `households` is the pop's household count. One household, or fewer,
/// covers the whole gap. Each good steps by [`walk_amount`]. A good at
/// its goal stays. A result at zero is dropped. Returns the new pile.
fn walk_pile(
    previous: &HashMap<usize, f64>,
    goal: &HashMap<usize, f64>,
    households: f64,
) -> HashMap<usize, f64> {
    let mut goods: Vec<usize> = previous
        .keys()
        .copied()
        .chain(goal.keys().copied())
        .collect();
    goods.sort_unstable();
    goods.dedup();
    let mut out = HashMap::new();
    for good in goods {
        let prev = previous.get(&good).copied().unwrap_or(0.0);
        let aim = goal.get(&good).copied().unwrap_or(0.0);
        let next = walk_amount(prev, aim, households);
        if next > 0.0 {
            out.insert(good, next);
        }
    }
    out
}

/// # Walk Amount
///
/// One household of the gap from `previous` toward `goal`.
///
/// `households` above one splits the gap. The step is that share, rounded
/// up, and at least one unit. The result does not pass `goal`.
fn walk_amount(previous: f64, goal: f64, households: f64) -> f64 {
    if (goal - previous).abs() <= 1e-9 {
        return goal;
    }
    let count = if households.is_finite() && households > 1.0 {
        households
    } else {
        1.0
    };
    let step = whole_units_up((goal - previous).abs() / count).max(1.0);
    if goal > previous {
        (previous + step).min(goal)
    } else {
        (previous - step).max(goal)
    }
}

/// # Scale Gap
///
/// The part of `gap` that matches `walked` against `full`.
///
/// A good missing from `walked`, or a zero pile, drops out. A walked pile
/// that has reached `full` keeps the whole gap. A partial pile keeps the
/// same share of the gap. Returns that shortfall.
fn scale_gap(
    gap: &HashMap<usize, f64>,
    full: &HashMap<usize, f64>,
    walked: &HashMap<usize, f64>,
) -> HashMap<usize, f64> {
    let mut out = HashMap::new();
    for (&good, &short) in gap {
        let whole = full.get(&good).copied().unwrap_or(0.0);
        let part = walked.get(&good).copied().unwrap_or(0.0);
        if short <= 0.0 || part <= 0.0 || whole <= 0.0 {
            continue;
        }
        let scaled = if part >= whole {
            short
        } else {
            short * part / whole
        };
        if scaled > 0.0 {
            out.insert(good, scaled);
        }
    }
    out
}

/// Which part of the saved pile a stash belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SaveHold {
    /// Consumption reserve.
    Meal,
    /// Production input. [`Job::input_hold`] records the same units.
    Input,
    /// Currency held toward the savings target.
    Currency,
}

/// Today's posts, trades, plan set-asides, and migration goods for one pop.
///
/// [`Pop::day_tape`] copies this. The morning reset clears the maps it reads.
#[derive(Debug, Clone, Default)]
pub struct DayTape {
    /// Units posted to buy.
    pub buy_offers: HashMap<usize, f64>,
    /// Units posted to sell.
    pub sell_offers: HashMap<usize, f64>,
    /// The deal's good received.
    pub bought: HashMap<usize, f64>,
    /// The deal's good given.
    pub sold: HashMap<usize, f64>,
    /// Tender given.
    pub paid: HashMap<usize, f64>,
    /// Tender taken.
    pub earned: HashMap<usize, f64>,
    /// Change received.
    pub change_in: HashMap<usize, f64>,
    /// Change given.
    pub change_out: HashMap<usize, f64>,
    /// Meal set aside.
    pub meal: HashMap<usize, f64>,
    /// Production inputs set aside.
    pub inputs: HashMap<usize, f64>,
    /// Currency swept into savings.
    pub saved: HashMap<usize, f64>,
    /// Tender left in working stock and off the sell book.
    pub unlisted: HashMap<usize, f64>,
    /// Property that arrived with migrants.
    pub arrived: HashMap<usize, f64>,
    /// Property that left with migrants.
    pub left: HashMap<usize, f64>,
    /// Saved stock that shrank and did not travel.
    pub dropped: HashMap<usize, f64>,
}

/// One of today's book maps on a pop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DayFlow {
    /// Payment given.
    Paid,
    /// Payment taken.
    Received,
    /// The good received.
    Got,
    /// The good given.
    Gave,
}

/// # Add Tape
///
/// Adds `units` of `good` onto `book`.
///
/// A non-positive or non-finite amount is ignored. Nothing is returned.
fn add_tape(book: &mut HashMap<usize, f64>, good: usize, units: f64) {
    if !units.is_finite() || units <= 0.0 {
        return;
    }
    *book.entry(good).or_insert(0.0) += units;
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
            saved: HashMap::new(),
            meal: HashMap::new(),
            luxury_savings: HashMap::new(),
            luxury_savings_open: true,
            cheaper_cover: HashMap::new(),
            mass_buys: None,
            currency_sale_room: HashMap::new(),
            day_listed: HashMap::new(),
            buy_posted: HashMap::new(),
            buy_received: HashMap::new(),
            day_paid: HashMap::new(),
            day_received: HashMap::new(),
            day_got: HashMap::new(),
            day_gave: HashMap::new(),
            day_haul: HashMap::new(),
            day_eaten: HashMap::new(),
            day_held: HashMap::new(),
            rest_taken: HashMap::new(),
            rest_sol: 0.0,
            day_granted: HashMap::new(),
            tape_buy: HashMap::new(),
            tape_sell: HashMap::new(),
            tape_bought: HashMap::new(),
            tape_sold: HashMap::new(),
            tape_paid: HashMap::new(),
            tape_earned: HashMap::new(),
            tape_change_in: HashMap::new(),
            tape_change_out: HashMap::new(),
            tape_meal: HashMap::new(),
            tape_input: HashMap::new(),
            tape_saved: HashMap::new(),
            tape_arrived: HashMap::new(),
            tape_left: HashMap::new(),
            tape_dropped: HashMap::new(),
            worth: HashMap::new(),
            turned: HashMap::new(),
            worth_open: true,
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
    /// share and returned in the group. `fresh`, `produced`, `spent`, and
    /// `reserved` shrink by the same share and stay. Today's payment, the
    /// deal's good, freight, the meal, the tender units posted for sale, and
    /// the morning grant shrink by that share and stay. Savings shrink
    /// by that share and stay. The day tape is left as posted: property that
    /// leaves is noted, and saved stock that shrinks without traveling is
    /// noted. Averages on this pop stay. The group's household
    /// shape is those averages from before the count dropped. A non-positive
    /// ask returns an empty group and changes nothing.
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
            row.spent *= 1.0 - goods_fraction;
            row.reserved *= 1.0 - goods_fraction;
            if take_qty != 0.0 {
                quantity.insert(good, take_qty);
                add_tape(&mut self.tape_left, good, take_qty);
            }
            if take_used != 0.0 {
                used.insert(good, take_used);
            }
        }
        for (&good, &amount) in &self.saved {
            add_tape(&mut self.tape_dropped, good, amount * goods_fraction);
        }
        for amount in self.savings.values_mut() {
            *amount *= 1.0 - goods_fraction;
        }
        for amount in self.luxury_savings.values_mut() {
            *amount *= 1.0 - goods_fraction;
        }
        for amount in self.saved.values_mut() {
            *amount *= 1.0 - goods_fraction;
        }
        for amount in self.meal.values_mut() {
            *amount *= 1.0 - goods_fraction;
        }
        for amount in self.job.input_hold.values_mut() {
            *amount *= 1.0 - goods_fraction;
        }
        let keep = 1.0 - goods_fraction;
        for flow in [
            &mut self.day_paid,
            &mut self.day_received,
            &mut self.day_got,
            &mut self.day_gave,
            &mut self.day_haul,
            &mut self.day_eaten,
            &mut self.day_listed,
            &mut self.day_granted,
        ] {
            for amount in flow.values_mut() {
                *amount *= keep;
            }
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
    /// Arriving quantity is noted on the day tape. Savings stay as they are.
    /// The arrivals do not bring a savings claim.
    /// An empty group changes nothing.
    pub(crate) fn receive_migrants(&mut self, group: &MigrantGroup) {
        if group.households <= 0.0 {
            return;
        }
        self.demographics
            .household
            .absorb(&group.shape, group.households);
        for (&good, &qty) in &group.quantity {
            if qty != 0.0 {
                self.property.entry(good).or_default().quantity += qty;
                add_tape(&mut self.tape_arrived, good, qty);
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
    /// created. A positive amount is this morning's creation of that good.
    /// The creation record is replaced by this call.
    ///
    /// Returns each good id and the amount added, in the same order.
    pub fn start_day(&mut self, new_goods: &[(usize, ScalingFactor)]) -> Vec<(usize, f64)> {
        let mut added = Vec::with_capacity(new_goods.len());
        self.day_granted.clear();
        for (good_id, scaling) in new_goods {
            let amount = self.get_scaling_factor(*scaling);
            self.property
                .entry(*good_id)
                .and_modify(|row| row.quantity += amount)
                .or_insert(PopPRow::new(amount));
            if amount > 0.0 {
                *self.day_granted.entry(*good_id).or_insert(0.0) += amount;
            }
            added.push((*good_id, amount));
        }
        added
    }

    /// # Update Desires
    ///
    /// Adds species, culture, stratum, and religion desires this pop does not already have.
    ///
    /// `factuals` supplies those demographics. Each owner's desires are
    /// copied in id order ([`Self::desires_in_id_order`]). Does not shop,
    /// and does not rewrite satisfaction. Culture, stratum, and religion
    /// id `0` are skipped. Day-end size matching is [`Self::rescale_desires`].
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
            for demo in Self::desires_in_id_order(&species.desires) {
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
                for demo in Self::desires_in_id_order(&culture.desires) {
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
                for demo in Self::desires_in_id_order(&stratum.desires) {
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
                for demo in Self::desires_in_id_order(&religion.desires) {
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

    /// # Desires In Id Order
    ///
    /// One demographic's desires, lowest id first.
    ///
    /// `desires` is that owner's map. The map's own order is not the copy
    /// order. [`Self::satisfy_tier`] walks a tier in the order these copies
    /// were pushed and stops at the first desire that cannot fill, so a
    /// lower id of the same owner is reserved before a higher one.
    ///
    /// Returns those desires in id order.
    fn desires_in_id_order(desires: &HashMap<usize, DemoDesire>) -> Vec<&DemoDesire> {
        let mut ordered: Vec<&DemoDesire> = desires.values().collect();
        ordered.sort_by_key(|desire| desire.id);
        ordered
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
    /// counts.
    pub fn settle_shopping(&mut self, factuals: &Factuals) {
        self.job.leave(&self.savings);
        self.job.settle_shopping(&self.property, factuals);
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
            (
                share,
                fills,
                target.high_priority,
                on_hand,
                target.efficiency,
            )
        };
        open.iter()
            .copied()
            .max_by(|&a, &b| {
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
                    eff_a
                        .partial_cmp(&eff_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    hand_a
                        .partial_cmp(&hand_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(
                            eff_a
                                .partial_cmp(&eff_b)
                                .unwrap_or(std::cmp::Ordering::Equal),
                        )
                };
                ranked.then(b.cmp(&a))
            })
            .unwrap_or(open[0])
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
    /// and `reserved`, and moves into `consumed` or `used`. A consumed take
    /// is added to [`Self::day_eaten`]. A used take is added to
    /// [`Self::day_held`].
    fn consume_one_desire_from(&mut self, desire: &mut Desire, leave_savings: bool) -> f64 {
        // Clone + sort by efficiency descending (best substitutes first)
        let mut targets = desire.target.clone();
        targets.sort_by(|a, b| {
            b.efficiency
                .partial_cmp(&a.efficiency)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut remaining = desire.amount;

        for target in targets.iter() {
            if remaining <= 0.0 {
                break;
            }
            let floor = if leave_savings {
                self.savings_of(target.good)
            } else {
                0.0
            };
            // get the target good, or continue on to the next target.
            let mut meal = 0.0;
            let mut held = 0.0;
            if let Some(row) = self.property.get_mut(&target.good)
                && row.quantity > 0.0
            {
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
                        meal = take;
                        let sat_gained = take * target.efficiency;
                        desire.satisfaction += sat_gained;
                        remaining -= sat_gained;
                    }
                    DesireTargetType::Use => {
                        row.used += take;
                        held = take;
                        let sat_gained = take * target.efficiency;
                        desire.satisfaction += sat_gained;
                        remaining -= sat_gained;
                    }
                }
            }
            // The meal reserved before the sell book sits outside property.
            if remaining > 0.0 && target.efficiency > 0.0 {
                let needed = remaining.min(desire.amount * target.cap) / target.efficiency;
                let used = matches!(target.desire_type, DesireTargetType::Use);
                let take = self.draw_saved_meal(target.good, needed, used);
                if take > 0.0 {
                    let sat_gained = take * target.efficiency;
                    desire.satisfaction += sat_gained;
                    remaining -= sat_gained;
                    if used {
                        held += take;
                    } else {
                        meal += take;
                    }
                }
            }
            if meal > 0.0 {
                *self.day_eaten.entry(target.good).or_insert(0.0) += meal;
            }
            if held > 0.0 {
                *self.day_held.entry(target.good).or_insert(0.0) += held;
            }
        }
        // The current satisfaction rate.
        desire.satisfaction / desire.amount
    }

    /// # Rest
    ///
    /// Eats leftover free stock named by this pop's species sinks.
    ///
    /// `factuals` supplies [`Factuals::species_sinks`]. Each sink takes free
    /// quantity above [`Self::savings`], and no reserved unit. The take leaves
    /// `quantity` and is recorded as consumed and as today's rest. The living
    /// gain is the sum of those takes times each sink's efficiency, divided
    /// by the household count. A missing species, a non-positive household
    /// count, or no spare stock adds nothing. Nothing is returned.
    pub fn rest(&mut self, factuals: &Factuals) {
        let sinks: Vec<(usize, f64)> = factuals
            .species_sinks(self.demographics.species)
            .iter()
            .map(|sink| (sink.good, sink.efficiency))
            .collect();
        let households = self.demographics.household.count;
        if !households.is_finite() || households <= 0.0 {
            return;
        }
        let mut satisfaction = 0.0;
        for (good, efficiency) in sinks {
            if !efficiency.is_finite() || efficiency <= 0.0 {
                continue;
            }
            let spare = (self.free_units(good) - self.savings_of(good)).max(0.0);
            if spare <= 0.0 {
                continue;
            }
            let take = {
                let Some(row) = self.property.get_mut(&good) else {
                    continue;
                };
                let take = spare.min(row.quantity.max(0.0));
                if take <= 0.0 {
                    continue;
                }
                row.quantity -= take;
                row.consumed += take;
                take
            };
            *self.rest_taken.entry(good).or_insert(0.0) += take;
            satisfaction += take * efficiency;
        }
        self.rest_sol = satisfaction / households;
    }

    /// # Desire Eaten
    ///
    /// Units a desire ate today, by good.
    pub(crate) fn desire_eaten(&self) -> &HashMap<usize, f64> {
        &self.day_eaten
    }

    /// # Desire Held
    ///
    /// Units a desire used today, by good.
    pub(crate) fn desire_held(&self) -> &HashMap<usize, f64> {
        &self.day_held
    }

    /// # Rest Taken
    ///
    /// Units eaten as rest today, by good.
    pub(crate) fn rest_taken(&self) -> &HashMap<usize, f64> {
        &self.rest_taken
    }

    /// # Growth Phase
    ///
    /// Applies structural demographic rates, then same-day birth and mortality
    /// effects, then scales births per woman by the demographic sentiment
    /// schedule, then [`Household::update`](crate::game::household::Household::update).
    ///
    /// Pops with household `count < 1` are left alone. The year split is
    /// [`crate::game::config::PopConfig::turn_length`]. The scale is
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
        self.demographics
            .household
            .update(&rates, factuals.config.pop.turn_length);
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
    /// Today's [`Self::rest`] adds its stored living gain.
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
        if self.rest_sol.is_finite() {
            sum += self.rest_sol;
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
        for (&good, &qty) in &self.saved {
            if good == TIME || qty == 0.0 {
                continue;
            }
            value += qty * history.holding_per_unit(good);
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
            if let PopEffect::Satisfaction {
                tier: effect_tier,
                amount,
            } = *effect
            {
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
    /// bookmark is dropped. `reserved`, `fresh`, `produced`, `spent`, and
    /// `lost` on every property row are set to 0. Quantity, consumed, and used
    /// stay for today's walk and for decay. Eaten and used units waiting on
    /// worth are cleared, and that gate reopens. Stored worth stays. The job
    /// drops its claimed-input list and its shopping list. The mass buy list,
    /// the tender sale rooms, the units posted from those rooms, posted buys,
    /// today's payment, deal good, freight, meal, and rest, and the day tape
    /// are dropped.
    /// The morning grant stays. Line targets stay,
    /// so last night's plan is what this morning runs.
    pub fn reset_day(&mut self) {
        self.satisfy_cursor = None;
        self.mass_buys = None;
        self.currency_sale_room.clear();
        self.day_listed.clear();
        self.buy_posted.clear();
        self.buy_received.clear();
        self.day_paid.clear();
        self.day_received.clear();
        self.day_got.clear();
        self.day_gave.clear();
        self.day_haul.clear();
        self.day_eaten.clear();
        self.day_held.clear();
        self.rest_taken.clear();
        self.rest_sol = 0.0;
        self.tape_buy.clear();
        self.tape_sell.clear();
        self.tape_bought.clear();
        self.tape_sold.clear();
        self.tape_paid.clear();
        self.tape_earned.clear();
        self.tape_change_in.clear();
        self.tape_change_out.clear();
        self.tape_meal.clear();
        self.tape_input.clear();
        self.tape_saved.clear();
        self.tape_arrived.clear();
        self.tape_left.clear();
        self.tape_dropped.clear();
        self.turned.clear();
        self.worth_open = true;
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
            row.spent = 0.0;
            row.lost = 0.0;
        }
        self.job.reset_day();
    }

    /// # Record Keeping
    ///
    /// Folds today's rot, purchases, makes, and meals into [`Self::worth`].
    ///
    /// `factuals` is unused. `history` is the card [`Self::revise_worth`]
    /// reads. Nothing is returned.
    pub fn record_keeping(&mut self, _factuals: &Factuals, history: &MarketHistory) {
        self.revise_worth(history);
    }

    /// # Day Books
    ///
    /// Today's production, trade, rot, and meal, priced on `history`.
    ///
    /// Income is units made, the morning grant [`Self::start_day`] recorded,
    /// payment taken in, and the deal's good received. Expenses are inputs a
    /// process destroyed, payment given, the deal's good given, transport
    /// spent as freight, and units lost to rot. The meal is
    /// [`Self::day_eaten`]. Each unit is [`MarketHistory::holding_per_unit`].
    /// Capital set aside is not a line. A grant that is spent or lost later
    /// is income on this line and an expense on that later line, so the two
    /// meet in the net. Returns `(income, expenses, meal)`.
    pub fn day_books(&self, history: &MarketHistory) -> (f64, f64, f64) {
        let mut income = 0.0;
        let mut expenses = 0.0;
        for (&good, row) in &self.property {
            let price = history.holding_per_unit(good);
            if row.produced > 0.0 {
                income += row.produced * price;
            }
            if row.spent > 0.0 {
                expenses += row.spent * price;
            }
            if row.lost > 0.0 {
                expenses += row.lost * price;
            }
        }
        let priced = |flow: &HashMap<usize, f64>| -> f64 {
            flow.iter()
                .map(|(&good, &qty)| qty.max(0.0) * history.holding_per_unit(good))
                .sum()
        };
        income += priced(&self.day_received);
        income += priced(&self.day_got);
        income += priced(&self.day_granted);
        expenses += priced(&self.day_paid);
        expenses += priced(&self.day_gave);
        expenses += priced(&self.day_haul);
        (income, expenses, priced(&self.day_eaten))
    }

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
    /// returns are stored for the morning order. The saved pile walks one
    /// household toward that ideal. The sale pile, today's sales, and the
    /// units of each tender posted today are noted first, so the next plan
    /// can follow what sold.
    pub fn plan(&mut self, factuals: &Factuals, history: &MarketHistory) {
        self.luxury_reached = self.finished_luxury_levels();
        let needs = self.night_needs(factuals, history);
        // One household of the saved day, toward last night's pile.
        let households = self.household_count();
        let savings = walk_pile(&self.savings, &needs.savings, households);
        let save_gap = scale_gap(&needs.save_gap, &needs.savings, &savings);
        self.savings = savings;
        self.luxury_savings = walk_pile(&self.luxury_savings, &needs.luxury_savings, households);
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
            &save_gap,
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
        // The sale pile and what sold today arm the next plan.
        let mut for_sale = HashMap::new();
        let mut sold = HashMap::new();
        for line in &self.job.lines {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            for output in &process.outputs {
                if output.amount <= 0.0 {
                    continue;
                }
                if factuals
                    .get_good(output.good)
                    .is_some_and(|good| good.is_skill())
                {
                    continue;
                }
                for_sale
                    .entry(output.good)
                    .or_insert_with(|| self.surplus_for_sale(output.good));
                sold.entry(output.good)
                    .or_insert_with(|| self.day_gave.get(&output.good).copied().unwrap_or(0.0));
            }
        }
        self.job.note_sale(&for_sale, &sold);
        // A tender's posted units are the offer that can sell out.
        for &tender in history.currencies() {
            if !for_sale.contains_key(&tender) {
                continue;
            }
            let listed = self.day_listed.get(&tender).copied().unwrap_or(0.0);
            self.job.note_listed(tender, listed);
        }
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
                stamp.push((
                    desire.source,
                    desire.demo_desire_id,
                    desire.amount.to_bits(),
                ));
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
    /// [`Self::best_target`] picks the target stock can finish. A desire
    /// stock can finish is spent. Every tier is walked, so a later desire
    /// is planned while an earlier one is still short.
    ///
    /// Each short desire adds the target [`Self::priced_target`] picks
    /// among goods some line outputs. A desire no line can make adds nothing.
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
                let Some(target) = self.priced_target(&desire.target, history, |target| {
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
    /// when that stop comes before the luxury tier. A day the choice refuses
    /// still closes a later tier while one ungrossed day of the eaten good
    /// is not on hand.
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
                    if Self::is_eaten_day_short(desire, stock) {
                        short = true;
                    }
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
    /// grossed. The hold is. Fill is scored on that hold. A winner that
    /// decays at [`SAVINGS_EASY_DECAY`] or faster is replaced by a more
    /// durable tender this pop can obtain. When this pop makes that winner
    /// and no tender qualifies, returns `None` so the morning make covers
    /// the day. Otherwise the rotting good is kept. Returns `None` when
    /// every candidate rates at zero.
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
        let (_, id, day, units) = best?;
        let Some(winner) = factuals.get_good(id) else {
            return Some((id, day, units));
        };
        if winner.decay_rate < SAVINGS_EASY_DECAY {
            return Some((id, day, units));
        }
        if let Some(money) =
            self.durable_tender(factuals, history, desire, eaten, winner.durability())
        {
            return Some(money);
        }
        if self.makes_good(factuals, id) {
            return None;
        }
        Some((id, day, units))
    }

    /// # Is Eaten Day Short
    ///
    /// True when `stock` holds less than one ungrossed day of the good `desire` eats.
    ///
    /// The day is [`SAVINGS_DAYS`] times the desire amount, divided by that
    /// good's efficiency. No eaten good, or a non-positive day, is false.
    fn is_eaten_day_short(desire: &Desire, stock: &HashMap<usize, f64>) -> bool {
        let Some(eaten) = Self::eaten_target(desire) else {
            return false;
        };
        if eaten.efficiency <= 0.0 {
            return false;
        }
        let day = SAVINGS_DAYS * desire.amount / eaten.efficiency;
        if !day.is_finite() || day <= 0.0 {
            return false;
        }
        stock.get(&eaten.good).copied().unwrap_or(0.0) < day
    }

    /// # Durable Tender
    ///
    /// A tender that holds `desire` in place of a good of `floor` durability.
    ///
    /// `history` lists the tenders and prices the outside day. `factuals`
    /// supplies decay. `eaten` is the good that day would have been. A
    /// tender at or below `floor`, or that this pop cannot obtain, is
    /// skipped. The most durable wins. A tie keeps the higher salability,
    /// then the lower id. Returns that good, the ungrossed day, and the
    /// whole units to hold.
    fn durable_tender(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        desire: &Desire,
        eaten: &DesireTarget,
        floor: f64,
    ) -> Option<(usize, f64, f64)> {
        let mut best: Option<(f64, f64, usize, f64, f64)> = None;
        for &id in history.currencies() {
            let Some(good) = factuals.get_good(id) else {
                continue;
            };
            if good.is_skill() {
                continue;
            }
            let durability = good.durability();
            if durability <= 0.0 || durability <= floor {
                continue;
            }
            let Some((day, units)) = Self::buffer_units(desire, good, eaten, history) else {
                continue;
            };
            if self.savings_fill(factuals, good.id, units) <= 0.0 {
                continue;
            }
            let salability = history.salability(good.id);
            if !Self::tender_beats(durability, salability, good.id, best) {
                continue;
            }
            best = Some((durability, salability, good.id, day, units));
        }
        best.map(|(_, _, id, day, units)| (id, day, units))
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

    /// # Tender Beats
    ///
    /// True when this tender should replace `best`.
    ///
    /// A higher durability wins. An equal durability keeps the higher
    /// salability, then the lower id.
    fn tender_beats(
        durability: f64,
        salability: f64,
        id: usize,
        best: Option<(f64, f64, usize, f64, f64)>,
    ) -> bool {
        let Some((was_durability, was_salability, was_id, _, _)) = best else {
            return true;
        };
        match durability.partial_cmp(&was_durability) {
            Some(std::cmp::Ordering::Greater) => true,
            Some(std::cmp::Ordering::Equal) => match salability.partial_cmp(&was_salability) {
                Some(std::cmp::Ordering::Greater) => true,
                Some(std::cmp::Ordering::Equal) => id < was_id,
                _ => false,
            },
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
    /// Every entry in [`Self::luxury_savings`] is still in `quantity`. An
    /// empty map passes. A short earlier tier does not close this level.
    fn luxury_buffer_ready(&self) -> bool {
        self.luxury_savings.iter().all(|(&good, &units)| {
            self.property
                .get(&good)
                .map(|row| row.quantity)
                .unwrap_or(0.0)
                >= units
        })
    }

    /// # Household Count
    ///
    /// Households in this pop, and at least one.
    ///
    /// A count above one is that count. Anything else is one household, so
    /// the saved day finishes in one night.
    pub(crate) fn household_count(&self) -> f64 {
        let count = self.demographics.household.count;
        if count.is_finite() && count > 1.0 {
            count
        } else {
            1.0
        }
    }

    /// # On Hand
    ///
    /// Whole units of `good` in `quantity`.
    ///
    /// Reserved units still count. A missing row is `0`.
    fn on_hand(&self, good: usize) -> f64 {
        self.property
            .get(&good)
            .map(|row| row.quantity.max(0.0).floor())
            .unwrap_or(0.0)
    }

    /// # Positive Card
    ///
    /// The listed AMV of `good` when that card is positive.
    ///
    /// `history` is the board. A missing good, a non-positive AMV, and a
    /// non-finite AMV return nothing. [`MarketHistory::price`]'s default
    /// of 1 is not a card.
    fn positive_card(history: &MarketHistory, good: usize) -> Option<f64> {
        let price = history.prices.get(&good).copied()?;
        (price.is_finite() && price > 0.0).then_some(price)
    }

    /// # Quote
    ///
    /// Private value of one unit of `good`, or the positive card.
    ///
    /// A stored [`Self::worth`] above zero is the quote. Otherwise
    /// [`Self::positive_card`] is the quote. A good with neither returns
    /// nothing.
    fn quote(&self, good: usize, history: &MarketHistory) -> Option<f64> {
        if let Some(worth) = self.worth.get(&good).copied() {
            if worth.is_finite() && worth > 0.0 {
                return Some(worth);
            }
        }
        Self::positive_card(history, good)
    }

    /// # Revise Worth
    ///
    /// Folds today's rot, purchases, makes, and units eaten or used into [`Self::worth`].
    ///
    /// `history` supplies the positive card. A good with no positive card is
    /// left alone. Evidence is units bought, made, eaten, or used, plus units
    /// lost. No evidence leaves stored worth where it is. The target is the
    /// card scaled by the share of evidence that succeeded, so a full success
    /// aims at twice the card and a total loss aims at zero. The step is that
    /// evidence over the pile it came from, and worth moves that far from its
    /// previous value, or from the card when none is stored. A make on a large
    /// pile therefore moves only partway. The result stays above
    /// [`AMV_EPSILON`] and at most twice the card. The gate closes, so a
    /// second call this night does nothing. Nothing is returned.
    fn revise_worth(&mut self, history: &MarketHistory) {
        if !self.worth_open {
            return;
        }
        self.worth_open = false;

        let mut goods: Vec<usize> = self.property.keys().copied().collect();
        goods.extend(self.buy_received.keys().copied());
        goods.extend(self.turned.keys().copied());
        goods.sort_unstable();
        goods.dedup();

        for good in goods {
            let Some(card) = Self::positive_card(history, good) else {
                continue;
            };
            let row = self.property.get(&good);
            let bought = self
                .buy_received
                .get(&good)
                .copied()
                .unwrap_or(0.0)
                .max(0.0);
            let produced = row.map(|row| row.produced.max(0.0)).unwrap_or(0.0);
            let turned = self.turned.get(&good).copied().unwrap_or(0.0).max(0.0);
            let lost = row.map(|row| row.lost.max(0.0)).unwrap_or(0.0);
            let up = bought + produced + turned;
            let evidence = up + lost;
            if evidence <= 0.0 {
                continue;
            }
            // Success pulls toward twice the card. Rot pulls toward zero.
            let on_hand = row.map(|row| row.quantity.max(0.0)).unwrap_or(0.0);
            let base = (on_hand + lost + turned).max(evidence);
            let tilt = ((up - lost) / evidence).clamp(-1.0, 1.0);
            let target = card * (1.0 + tilt);
            // Evidence over the pile, so old stock dilutes the move.
            let step = (evidence / base).clamp(0.0, 1.0);
            let prev = self
                .worth
                .get(&good)
                .copied()
                .filter(|worth| worth.is_finite() && *worth > 0.0)
                .unwrap_or(card);
            let mut next = prev * (1.0 - step) + target * step;
            if !next.is_finite() {
                next = AMV_EPSILON;
            }
            let ceiling = (2.0 * card).max(AMV_EPSILON);
            self.worth.insert(good, next.clamp(AMV_EPSILON, ceiling));
        }
        self.turned.clear();
    }

    /// # Portfolio
    ///
    /// Whole units of one level of `desire`, split across its targets.
    ///
    /// `tier` selects the level count. Luxury (`2`) repeats
    /// [`Self::luxury_reached`] levels, and at least one. Every other tier
    /// is one level. A target scores its efficiency per [`Self::quote`], or
    /// its efficiency when it has no quote. Satisfaction is shared
    /// in proportion to those scores, and no target takes more than its cap
    /// over those levels. Each share is floored to whole units. Leftover
    /// satisfaction goes to the highest score that still fits, as many whole
    /// units as that score can take, then the higher efficiency, then the
    /// lower good id. Returns goods of at least one unit.
    fn portfolio(
        &self,
        desire: &Desire,
        tier: usize,
        history: &MarketHistory,
    ) -> Vec<(usize, f64)> {
        if !desire.amount.is_finite() || desire.amount <= 0.0 {
            return Vec::new();
        }
        let levels = if tier == 2 {
            let reached = self.luxury_reached;
            if reached.is_finite() && reached > 1.0 {
                reached
            } else {
                1.0
            }
        } else {
            1.0
        };
        let budget = levels * desire.amount;
        if !budget.is_finite() || budget <= 0.0 {
            return Vec::new();
        }

        struct Share {
            good: usize,
            efficiency: f64,
            score: f64,
            cap_sat: f64,
            units: f64,
        }

        let mut shares: Vec<Share> = Vec::new();
        for target in &desire.target {
            if !target.efficiency.is_finite() || target.efficiency <= 0.0 {
                continue;
            }
            if !target.cap.is_finite() || target.cap <= 0.0 {
                continue;
            }
            let quote = self.quote(target.good, history).unwrap_or(1.0);
            let score = target.efficiency / quote;
            if !score.is_finite() || score <= 0.0 {
                continue;
            }
            shares.push(Share {
                good: target.good,
                efficiency: target.efficiency,
                score,
                cap_sat: budget * target.cap,
                units: 0.0,
            });
        }
        let total: f64 = shares.iter().map(|share| share.score).sum();
        if total <= 0.0 {
            return Vec::new();
        }

        // Share one level by score, then give leftover satisfaction to the best score that still fits.
        let mut used = 0.0;
        for share in &mut shares {
            let sat = (budget * share.score / total).min(share.cap_sat).max(0.0);
            let cap_units = (share.cap_sat / share.efficiency).floor().max(0.0);
            let units = (sat / share.efficiency).floor().max(0.0).min(cap_units);
            share.units = units;
            used += units * share.efficiency;
        }
        let mut leftover = (budget - used).max(0.0);
        while leftover > 0.0 {
            let mut best: Option<usize> = None;
            for (index, share) in shares.iter().enumerate() {
                if share.efficiency > leftover {
                    continue;
                }
                if share.units * share.efficiency + share.efficiency > share.cap_sat {
                    continue;
                }
                let take = match best {
                    None => true,
                    Some(held) => {
                        let other = &shares[held];
                        match share.score.partial_cmp(&other.score) {
                            Some(std::cmp::Ordering::Greater) => true,
                            Some(std::cmp::Ordering::Less) => false,
                            _ => match share.efficiency.partial_cmp(&other.efficiency) {
                                Some(std::cmp::Ordering::Greater) => true,
                                Some(std::cmp::Ordering::Less) => false,
                                _ => share.good < other.good,
                            },
                        }
                    }
                };
                if take {
                    best = Some(index);
                }
            }
            let Some(index) = best else {
                break;
            };
            let efficiency = shares[index].efficiency;
            let cap_units = (shares[index].cap_sat / efficiency).floor().max(0.0);
            let room = (cap_units - shares[index].units).max(0.0);
            let fit = (leftover / efficiency).floor().max(0.0);
            let add = room.min(fit);
            if add < 1.0 {
                break;
            }
            shares[index].units += add;
            leftover -= add * efficiency;
        }

        let mut units: HashMap<usize, f64> = HashMap::new();
        for share in shares {
            if share.units >= 1.0 {
                *units.entry(share.good).or_insert(0.0) += share.units;
            }
        }
        let mut out: Vec<(usize, f64)> = units.into_iter().collect();
        out.sort_unstable_by_key(|(good, _)| *good);
        out
    }

    /// # Consumption Plan
    ///
    /// Whole units of each good in today's consumption mix.
    ///
    /// Every desire contributes [`Self::portfolio`]. The same good on two
    /// desires is added. `history` prices the quotes. Returns that map.
    fn consumption_plan(&self, history: &MarketHistory) -> HashMap<usize, f64> {
        let mut plan = HashMap::new();
        for (tier, desires) in self.desires.iter().enumerate() {
            for desire in desires {
                for (good, units) in self.portfolio(desire, tier, history) {
                    *plan.entry(good).or_insert(0.0) += units;
                }
            }
        }
        plan
    }

    /// # Note Posted Buys
    ///
    /// Records the funded mass buys as today's posted buy.
    ///
    /// A later call keeps the larger amount. The morning reset clears the
    /// tape. [`Self::savings_fill`] reads it.
    fn note_posted_buys(&mut self) {
        let Some(plan) = self.mass_buys.clone() else {
            return;
        };
        for (good, amount) in plan {
            if amount <= 0.0 {
                continue;
            }
            let posted = self.buy_posted.entry(good).or_insert(0.0);
            if amount > *posted {
                *posted = amount;
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

    /// # Priced Target
    ///
    /// The target in `targets` with the most satisfaction per quote that `keep` accepts.
    ///
    /// `history` prices [`Self::quote`]. A missing quote counts as one.
    /// The score is efficiency divided by that quote. A target with
    /// efficiency at or below zero is skipped. A tie keeps the later
    /// target. Returns that target, or `None` when none are kept.
    fn priced_target<'a>(
        &self,
        targets: &'a [DesireTarget],
        history: &MarketHistory,
        keep: impl Fn(&DesireTarget) -> bool,
    ) -> Option<&'a DesireTarget> {
        targets
            .iter()
            .filter(|target| target.efficiency > 0.0 && keep(target))
            .max_by(|a, b| {
                let left = a.efficiency / self.quote(a.good, history).unwrap_or(1.0);
                let right = b.efficiency / self.quote(b.good, history).unwrap_or(1.0);
                left.partial_cmp(&right)
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
    /// 1. Record `used` and `consumed` on [`Self::turned`], then return `used` to `quantity`.
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
        let mut turned_now: HashMap<usize, f64> = HashMap::new();

        for (&good_id, row) in self.property.iter_mut() {
            if row.used > 0.0 {
                *turned_now.entry(good_id).or_insert(0.0) += row.used;
            }
            if row.consumed > 0.0 {
                *turned_now.entry(good_id).or_insert(0.0) += row.consumed;
            }
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

        // Saved stock is off the working ledger and still rots.
        let saved_ids: Vec<usize> = self.saved.keys().copied().collect();
        for good_id in saved_ids {
            let before = self.saved.get(&good_id).copied().unwrap_or(0.0);
            if before <= 0.0 {
                continue;
            }
            let Some(good) = factuals.get_good(good_id) else {
                continue;
            };
            let exposure = good.tags.contains(&GoodTag::Exposure);
            let mut lost = 0.0;
            if !exposure && good.decay_rate > 0.0 {
                lost = before * good.decay_rate;
                let keep = ((before - lost) / before).max(0.0);
                if let Some(saved) = self.saved.get_mut(&good_id) {
                    *saved = (*saved - lost).max(0.0);
                }
                if let Some(meal) = self.meal.get_mut(&good_id) {
                    *meal *= keep;
                }
                if let Some(held) = self.job.input_hold.get_mut(&good_id) {
                    *held *= keep;
                }
            }
            let row = self
                .property
                .entry(good_id)
                .or_insert_with(|| PopPRow::new(0.0));
            row.lost += lost;
            let entry = rot.entry(good_id).or_insert((0.0, 0.0));
            entry.0 += lost;
            entry.1 += before;
        }
        self.saved.retain(|_, qty| *qty > 1e-12);
        self.meal.retain(|_, qty| *qty > 1e-12);
        self.job.input_hold.retain(|_, qty| *qty > 1e-12);

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
        for (good, units) in turned_now {
            *self.turned.entry(good).or_insert(0.0) += units;
        }
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
    /// so they are not held back a second time. [`Self::units_on_offer`]
    /// reads this amount when the good is not a loss.
    fn surplus_for_sale(&self, good: usize) -> f64 {
        let free = self.free_units(good).floor();
        let still = (self.desire_units(good) - self.job.claimed_of(good)).max(0.0);
        (free - still - self.savings_of(good)).max(0.0).floor()
    }

    /// # Has Desire For
    ///
    /// True when a desire target lists `good` at a positive efficiency.
    fn has_desire_for(&self, good: usize) -> bool {
        self.desires.iter().flatten().any(|desire| {
            desire
                .target
                .iter()
                .any(|target| target.good == good && target.efficiency > 0.0)
        })
    }

    /// # Job Buy Units
    ///
    /// Whole units of `good` the job still shops for the next run.
    ///
    /// The amount is [`Job::buy_orders`]. A good the job is not shopping
    /// for is `0`.
    fn job_buy_units(&self, good: usize) -> f64 {
        self.job
            .buy_orders(self.actor())
            .into_iter()
            .find(|order| order.target == good)
            .map(|order| order.target_amount.max(0.0))
            .unwrap_or(0.0)
    }

    /// # Processes Making
    ///
    /// Job lines whose process outputs `good`.
    ///
    /// `factuals` supplies the process. A missing process is skipped. A line
    /// that does not output `good` is skipped. Returns those processes.
    fn processes_making<'a>(
        &self,
        factuals: &'a Factuals,
        good: usize,
    ) -> Vec<&'a crate::game::process::Process> {
        self.job
            .lines
            .iter()
            .filter_map(|line| {
                let process = factuals.get_process(line.process)?;
                let makes = process
                    .outputs
                    .iter()
                    .any(|output| output.good == good && output.amount > 0.0);
                makes.then_some(process)
            })
            .collect()
    }

    /// # Is Produced At A Loss
    ///
    /// True when every line that makes `good` loses holding on one run.
    ///
    /// `factuals` and `history` score [`crate::game::job::trade_score`]. A pop
    /// that does not make `good` is false. One line at or above zero keeps
    /// the good for its desire.
    fn is_produced_at_a_loss(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        good: usize,
    ) -> bool {
        let lines = self.processes_making(factuals, good);
        !lines.is_empty()
            && lines
                .iter()
                .all(|process| crate::game::job::trade_score(process, history) < 0.0)
    }

    /// # Accepts The Card
    ///
    /// True when this pop will sell one unit of `good` at `market`.
    ///
    /// `factuals` names the good and the processes. `history` prices
    /// [`crate::game::job::make_cost_per_unit`]. A non-finite or zero card
    /// is refused. A pop that does not make `good`, or whose make-cost is
    /// not finite, accepts the card. A maker accepts when
    /// [`crate::game::job::card_covers_cost`] says the card covers the
    /// lowest finite cost after [`crate::game::job::listed_decay`].
    fn accepts_the_card(
        &self,
        factuals: &Factuals,
        history: &MarketHistory,
        good: usize,
        market: f64,
    ) -> bool {
        if !market.is_finite() || market == 0.0 {
            return false;
        }
        let cost = self
            .processes_making(factuals, good)
            .iter()
            .filter_map(|process| crate::game::job::make_cost_per_unit(process, good, history))
            .filter(|cost| cost.is_finite())
            .reduce(f64::min);
        let Some(cost) = cost else {
            return true;
        };
        crate::game::job::card_covers_cost(
            cost,
            market,
            crate::game::job::listed_decay(factuals, good),
        )
    }

    /// # Liquid Tender
    ///
    /// Whole free units of `tender` above the saved day.
    ///
    /// Savings stay out of the purse. Returns that count, floored at zero.
    fn liquid_tender(&self, tender: usize) -> f64 {
        (self.free_units(tender).floor() - self.savings_of(tender))
            .max(0.0)
            .floor()
    }

    /// # Note Currency Sale Room
    ///
    /// Stores how much of each tender may still be sold today.
    ///
    /// `currencies` are the tender slots. Every filled slot is stored.
    /// The room is [`currency_sale_cap`] of that tender's surplus. A later
    /// pass reads the room instead of taking another share. Units already
    /// posted stay on [`Self::day_listed`]. Nothing is returned.
    pub(crate) fn note_currency_sale_room(&mut self, currencies: &[Option<usize>]) {
        self.currency_sale_room.clear();
        for tender in currencies.iter().copied().flatten() {
            if self.currency_sale_room.contains_key(&tender) {
                continue;
            }
            let room = currency_sale_cap(self.surplus_for_sale(tender));
            self.currency_sale_room.insert(tender, room);
        }
    }

    /// # Spend Currency Sale Room
    ///
    /// Removes `units` from the stored sale room of `good`.
    ///
    /// A good with no stored room is left alone. The room does not go
    /// below zero. Positive `units` are added to today's posted listing.
    /// Nothing is returned.
    pub(crate) fn spend_currency_sale_room(&mut self, good: usize, units: f64) {
        if let Some(room) = self.currency_sale_room.get_mut(&good) {
            *room = (*room - units).max(0.0);
            if units > 0.0 {
                *self.day_listed.entry(good).or_insert(0.0) += units;
            }
        }
    }

    /// # Mass Want
    ///
    /// Whole units of `good` this pop would buy on the mass market.
    ///
    /// `history` prices [`Self::consumption_plan`]. `factuals` tells
    /// [`Self::units_on_offer`]. The plan and the saved day are one pile,
    /// and stock on hand, reserved included, comes off that pile.
    /// The part of [`Self::cheaper_cover`] still above that stock replaces
    /// the shortfall when it is larger. The job's next-run shopping is
    /// added on top. A desire target stays at one unit when this pop is not
    /// already offering the good. A good with none of those is `0`.
    fn mass_want(&self, good: usize, history: &MarketHistory, factuals: &Factuals) -> f64 {
        let owned = self.on_hand(good);
        let stock = self
            .consumption_plan(history)
            .get(&good)
            .copied()
            .unwrap_or(0.0);
        let want = stock + self.savings_of(good);
        let short = (want - owned).max(0.0).floor();
        let cheaper = whole_units_up(self.cheaper_cover.get(&good).copied().unwrap_or(0.0));
        let cover_left = (cheaper - owned).max(0.0);
        let next = short.max(cover_left) + self.job_buy_units(good);
        if self.has_desire_for(good) && self.units_on_offer(good, factuals, history) < 1.0 {
            next.max(1.0)
        } else {
            next
        }
    }

    /// # Is Worth Below Card
    ///
    /// True when this pop's stored worth of `good` is below the positive card.
    ///
    /// `history` is the board. A missing worth, a non-positive worth, and a
    /// card that is not a positive listed AMV are false. The default price
    /// of 1 is not that card.
    fn is_worth_below_card(&self, good: usize, history: &MarketHistory) -> bool {
        let Some(card) = Self::positive_card(history, good) else {
            return false;
        };
        self.worth
            .get(&good)
            .copied()
            .is_some_and(|worth| worth.is_finite() && worth > 0.0 && worth < card)
    }

    /// # Units On Offer
    ///
    /// Whole units of `good` this pop will sell.
    ///
    /// `factuals` and `history` score a loss and the card. A good made at a
    /// loss offers free units above savings, including units that still feed
    /// a desire. Any other good offers [`Self::surplus_for_sale`]. When
    /// [`Self::is_worth_below_card`] holds, free units above today's
    /// consumption plan are offered as well, and the larger of the two
    /// counts. The plan is the meal that stays. Returns that count.
    fn units_on_offer(&self, good: usize, factuals: &Factuals, history: &MarketHistory) -> f64 {
        let free = self.free_units(good).floor();
        let ordinary = if self.is_produced_at_a_loss(factuals, history, good) {
            (free - self.savings_of(good)).max(0.0).floor()
        } else {
            self.surplus_for_sale(good)
        };
        if !self.is_worth_below_card(good, history) {
            return ordinary;
        }
        let meal = self
            .consumption_plan(history)
            .get(&good)
            .copied()
            .unwrap_or(0.0)
            .floor()
            .max(0.0);
        let priced = (free - meal).max(0.0).floor();
        ordinary.max(priced)
    }

    /// # Mass Buy Tier
    ///
    /// The earliest desire tier that names `good`, or [`usize::MAX`] when
    /// none does.
    ///
    /// A lower tier is funded first. A good no desire names sorts after
    /// every desire.
    fn mass_buy_tier(&self, good: usize) -> usize {
        self.desires
            .iter()
            .enumerate()
            .find_map(|(tier, desires)| {
                desires
                    .iter()
                    .any(|desire| {
                        desire
                            .target
                            .iter()
                            .any(|target| target.good == good && target.efficiency > 0.0)
                    })
                    .then_some(tier)
            })
            .unwrap_or(usize::MAX)
    }

    /// # Mass Buy Score
    ///
    /// Satisfaction per quote of `good` on its earliest desire tier.
    ///
    /// `history` prices [`Self::quote`]. A missing quote counts as one.
    /// The score is the highest efficiency on that tier divided by the
    /// quote. A good no desire names scores 0. Returns that score.
    fn mass_buy_score(&self, good: usize, history: &MarketHistory) -> f64 {
        let tier = self.mass_buy_tier(good);
        let Some(desires) = self.desires.get(tier) else {
            return 0.0;
        };
        let quote = self.quote(good, history).unwrap_or(1.0);
        let mut best = 0.0;
        for desire in desires {
            for target in &desire.target {
                if target.good != good || !target.efficiency.is_finite() || target.efficiency <= 0.0
                {
                    continue;
                }
                let score = target.efficiency / quote;
                if score > best {
                    best = score;
                }
            }
        }
        best
    }

    /// # Mass Candidates
    ///
    /// Goods this pop might buy, earliest desire tier first.
    ///
    /// `history` prices the quote. A desire target, a [`Self::cheaper_cover`]
    /// entry, and a job shopping line each qualify. One good is listed once.
    /// Within a tier, [`Self::mass_buy_score`] comes first and a tie keeps
    /// the lower id. Returns that order.
    fn mass_candidates(&self, history: &MarketHistory) -> Vec<usize> {
        let mut goods = HashSet::new();
        for desire in self.desires.iter().flatten() {
            for target in &desire.target {
                if target.efficiency > 0.0 {
                    goods.insert(target.good);
                }
            }
        }
        goods.extend(self.cheaper_cover.keys().copied());
        for order in self.job.buy_orders(self.actor()) {
            goods.insert(order.target);
        }
        let mut goods: Vec<usize> = goods.into_iter().collect();
        goods.sort_by(|left, right| {
            let tier = self.mass_buy_tier(*left).cmp(&self.mass_buy_tier(*right));
            if tier != std::cmp::Ordering::Equal {
                return tier;
            }
            let score = self
                .mass_buy_score(*right, history)
                .partial_cmp(&self.mass_buy_score(*left, history))
                .unwrap_or(std::cmp::Ordering::Equal);
            if score != std::cmp::Ordering::Equal {
                return score;
            }
            left.cmp(right)
        });
        goods
    }

    /// # Mass Buy
    ///
    /// A buy of `buy_units` of `good`, or `None` when no tender can start it.
    ///
    /// `currencies` are the tender slots. `market` is the card AMV of `good`,
    /// and it is the price this buy accepts. The amount is the most
    /// [`crate::game::market::mix_change`] can pay for `buy_units` at that
    /// card from [`Self::liquid_tender`]. A negative card is not paid by
    /// this pop, so `buy_units` is the amount. Returns the order.
    fn mass_buy(
        &self,
        good: usize,
        history: &MarketHistory,
        currencies: &[Option<usize>],
        market: f64,
        buy_units: f64,
    ) -> Option<MarketOrder> {
        let tenders = crate::game::market::payment_tenders(currencies, good);
        if tenders.is_empty() {
            return None;
        }
        // The seller pays a negative card. One tender purse covers the buy.
        let spec = if market > 0.0 {
            crate::game::market::priced_coins(&tenders, history, |id| self.liquid_tender(id))
        } else {
            let mut rows = Vec::new();
            for id in tenders {
                let amv = history.price(id);
                if !amv.is_finite() || amv <= 0.0 {
                    continue;
                }
                let per = (market.abs() / amv).ceil().max(1.0);
                rows.push((id, amv, (buy_units * per).max(per)));
                break;
            }
            rows
        };
        if spec.is_empty() {
            return None;
        }
        let (units, mix) = crate::game::market::mix_change(buy_units, market, &spec);
        if units < 1.0 || !mix.iter().any(|(_, qty)| *qty >= 1.0) {
            return None;
        }
        Some(MarketOrder::buy(self.actor(), good, units).with_accept_price(market))
    }

    /// # Mass Sell
    ///
    /// A sell of the units of `good` this pop will part with.
    ///
    /// `market` is the card AMV. `currencies` are the tender slots. The units
    /// start at [`Self::units_on_offer`]. A filled tender slot is then
    /// limited to its sale room, or to [`currency_sale_cap`] of
    /// [`Self::surplus_for_sale`] when that room was not stored. The price
    /// is the card. Under one unit, or a card [`Self::accepts_the_card`]
    /// refuses, returns `None`. A negative card is a real ask.
    fn mass_sell(
        &self,
        good: usize,
        factuals: &Factuals,
        history: &MarketHistory,
        market: f64,
        currencies: &[Option<usize>],
    ) -> Option<MarketOrder> {
        let mut units = self.units_on_offer(good, factuals, history);
        let is_tender = currencies.iter().copied().flatten().any(|id| id == good);
        if is_tender {
            let cap = self
                .currency_sale_room
                .get(&good)
                .copied()
                .unwrap_or_else(|| currency_sale_cap(self.surplus_for_sale(good)));
            units = units.min(cap);
        }
        if units < 1.0 || !self.accepts_the_card(factuals, history, good, market) {
            return None;
        }
        Some(MarketOrder::sell(self.actor(), good, units).with_accept_price(market))
    }

    fn move_good(&mut self, good: usize, delta: f64) {
        let row = self
            .property
            .entry(good)
            .or_insert_with(|| PopPRow::new(0.0));
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
        let row = self
            .property
            .entry(good)
            .or_insert_with(|| PopPRow::new(0.0));
        row.fresh = (row.fresh + delta).max(0.0);
    }

    /// # Pay Freight
    ///
    /// Spends `amount` of transport cover from free stock.
    ///
    /// `factuals` names the transport goods. Property ids are walked in
    /// order. A good with no transport efficiency is skipped. Units spent
    /// are recorded as consumed and as today's freight. Nothing is returned.
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
            let row = self
                .property
                .get_mut(&id)
                .expect("free stock vanished before it could be spent");
            row.consumed += take;
            *self.day_haul.entry(id).or_insert(0.0) += take;
            left -= take * efficiency;
        }
    }

    /// # Working Units
    ///
    /// Free units of `good` still in [`Self::property`].
    ///
    /// Saved stock is already outside that map. Returns the free quantity,
    /// floored at zero.
    pub(crate) fn working_units(&self, good: usize) -> f64 {
        self.free_units(good).max(0.0)
    }

    /// # Take Working
    ///
    /// Removes up to `units` of `good` from working stock.
    ///
    /// Aged units leave before fresh. Returns the units removed.
    pub(crate) fn take_working(&mut self, good: usize, units: f64) -> f64 {
        let take = units.min(self.working_units(good)).max(0.0);
        if take <= 0.0 {
            return 0.0;
        }
        if let Some(row) = self.property.get_mut(&good) {
            row.spend_aged_first(take);
            row.quantity = (row.quantity - take).max(0.0);
        }
        take
    }

    /// # Give Working
    ///
    /// Adds `units` of `good` to working stock.
    ///
    /// A non-positive amount does nothing.
    pub(crate) fn give_working(&mut self, good: usize, units: f64) {
        if !units.is_finite() || units <= 0.0 {
            return;
        }
        self.property
            .entry(good)
            .or_insert_with(|| PopPRow::new(0.0))
            .quantity += units;
    }

    /// # Stash Working
    ///
    /// Moves up to `units` of `good` from working stock into [`Self::saved`].
    ///
    /// `hold` records the meal or the production-input part. Currency savings
    /// are the rest of the pile. The units moved are noted on the day tape.
    /// Returns the units moved.
    pub(crate) fn stash_working(&mut self, good: usize, units: f64, hold: SaveHold) -> f64 {
        let took = self.take_working(good, units);
        if took <= 0.0 {
            return 0.0;
        }
        *self.saved.entry(good).or_insert(0.0) += took;
        match hold {
            SaveHold::Meal => {
                *self.meal.entry(good).or_insert(0.0) += took;
                add_tape(&mut self.tape_meal, good, took);
            }
            SaveHold::Input => {
                *self.job.input_hold.entry(good).or_insert(0.0) += took;
                add_tape(&mut self.tape_input, good, took);
            }
            SaveHold::Currency => add_tape(&mut self.tape_saved, good, took),
        }
        took
    }

    /// # Keep Craft Inputs
    ///
    /// Parks the inputs a paying line can finish from the stock just made.
    ///
    /// `factuals` names the processes. The units come from
    /// [`Job::craft_input_keep`]. They move into [`Job::input_hold`], so
    /// the meal and the sell book leave them for the next produce. Nothing
    /// is returned.
    pub(crate) fn keep_craft_inputs(&mut self, factuals: &Factuals) {
        let keep = self.job.craft_input_keep(&self.property, factuals);
        for (good, units) in keep {
            self.stash_working(good, units, SaveHold::Input);
        }
    }

    /// # Release Input Hold
    ///
    /// Moves [`Job::input_hold`] back into working stock.
    ///
    /// Produce spends those inputs from property. The saved pile drops the
    /// same units. Nothing is returned.
    pub(crate) fn release_input_hold(&mut self) {
        let hold = std::mem::take(&mut self.job.input_hold);
        for (good, units) in hold {
            if units <= 0.0 {
                continue;
            }
            if let Some(saved) = self.saved.get_mut(&good) {
                *saved = (*saved - units).max(0.0);
            }
            self.give_working(good, units);
        }
        self.saved.retain(|_, qty| *qty > 1e-12);
    }

    /// # Draw Saved Meal
    ///
    /// Takes up to `units` of `good` from the meal part of [`Self::saved`].
    ///
    /// `used` records a use on the property row. Otherwise the row records
    /// the take as consumed, so decay can still credit the byproduct. The
    /// caller records [`Self::day_eaten`] or [`Self::day_held`]. Returns
    /// the units taken.
    pub(crate) fn draw_saved_meal(&mut self, good: usize, units: f64, used: bool) -> f64 {
        let have = self.meal.get(&good).copied().unwrap_or(0.0);
        let take = units.min(have).max(0.0);
        if take <= 0.0 {
            return 0.0;
        }
        if let Some(meal) = self.meal.get_mut(&good) {
            *meal = (*meal - take).max(0.0);
        }
        if let Some(saved) = self.saved.get_mut(&good) {
            *saved = (*saved - take).max(0.0);
        }
        let row = self
            .property
            .entry(good)
            .or_insert_with(|| PopPRow::new(0.0));
        // The caller records the day book. The row is what decay credits.
        if used {
            row.used += take;
        } else {
            row.consumed += take;
        }
        self.meal.retain(|_, qty| *qty > 1e-12);
        self.saved.retain(|_, qty| *qty > 1e-12);
        take
    }

    /// # Saved Currency Units
    ///
    /// Saved units of `good` that are neither the meal nor a production input.
    pub(crate) fn saved_currency_units(&self, good: usize) -> f64 {
        let saved = self.saved.get(&good).copied().unwrap_or(0.0);
        let meal = self.meal.get(&good).copied().unwrap_or(0.0);
        let held = self.job.input_hold.get(&good).copied().unwrap_or(0.0);
        (saved - meal - held).max(0.0)
    }

    /// # Spend Saved Currency
    ///
    /// Removes up to `units` of saved currency `good`.
    ///
    /// The meal and the input hold stay. Returns the units removed.
    pub(crate) fn spend_saved_currency(&mut self, good: usize, units: f64) -> f64 {
        let take = units.min(self.saved_currency_units(good)).max(0.0);
        if take <= 0.0 {
            return 0.0;
        }
        if let Some(saved) = self.saved.get_mut(&good) {
            *saved = (*saved - take).max(0.0);
        }
        self.saved.retain(|_, qty| *qty > 1e-12);
        take
    }

    /// # Give Saved
    ///
    /// Adds `units` of `good` to the saved pile as currency savings.
    ///
    /// The meal and the input hold are unchanged.
    pub(crate) fn give_saved(&mut self, good: usize, units: f64) {
        if !units.is_finite() || units <= 0.0 {
            return;
        }
        *self.saved.entry(good).or_insert(0.0) += units;
    }

    /// # Note Flow
    ///
    /// Adds `units` to one of today's book maps.
    ///
    /// `flow` selects payment or the good. A non-positive amount is ignored.
    /// The same units are noted on the day tape. Freight stays on
    /// [`Self::pay_freight`].
    pub(crate) fn note_flow(&mut self, flow: DayFlow, good: usize, units: f64) {
        if !units.is_finite() || units <= 0.0 {
            return;
        }
        match flow {
            DayFlow::Paid => {
                *self.day_paid.entry(good).or_insert(0.0) += units;
                add_tape(&mut self.tape_paid, good, units);
            }
            DayFlow::Received => {
                *self.day_received.entry(good).or_insert(0.0) += units;
                add_tape(&mut self.tape_earned, good, units);
            }
            DayFlow::Got => {
                *self.day_got.entry(good).or_insert(0.0) += units;
                add_tape(&mut self.tape_bought, good, units);
            }
            DayFlow::Gave => {
                *self.day_gave.entry(good).or_insert(0.0) += units;
                add_tape(&mut self.tape_sold, good, units);
            }
        }
    }

    /// # Note Buy Offer
    ///
    /// Adds `units` of `good` to today's buy offers.
    ///
    /// A non-positive or non-finite amount is ignored. Nothing is returned.
    pub(crate) fn note_buy_offer(&mut self, good: usize, units: f64) {
        add_tape(&mut self.tape_buy, good, units);
    }

    /// # Note Sell Offer
    ///
    /// Adds `units` of `good` to today's sell offers.
    ///
    /// A non-positive or non-finite amount is ignored. Nothing is returned.
    pub(crate) fn note_sell_offer(&mut self, good: usize, units: f64) {
        add_tape(&mut self.tape_sell, good, units);
    }

    /// # Note Change In
    ///
    /// Adds `units` of `good` to change this pop received today.
    ///
    /// A non-positive or non-finite amount is ignored. Nothing is returned.
    pub(crate) fn note_change_in(&mut self, good: usize, units: f64) {
        add_tape(&mut self.tape_change_in, good, units);
    }

    /// # Note Change Out
    ///
    /// Adds `units` of `good` to change this pop gave today.
    ///
    /// A non-positive or non-finite amount is ignored. Nothing is returned.
    pub(crate) fn note_change_out(&mut self, good: usize, units: f64) {
        add_tape(&mut self.tape_change_out, good, units);
    }

    /// # Day Tape
    ///
    /// Today's posts, trades, plan set-asides, and migration goods.
    ///
    /// The unlisted tender is copied from the job. The other maps are the
    /// day tape. Returns that copy.
    pub fn day_tape(&self) -> DayTape {
        DayTape {
            buy_offers: self.tape_buy.clone(),
            sell_offers: self.tape_sell.clone(),
            bought: self.tape_bought.clone(),
            sold: self.tape_sold.clone(),
            paid: self.tape_paid.clone(),
            earned: self.tape_earned.clone(),
            change_in: self.tape_change_in.clone(),
            change_out: self.tape_change_out.clone(),
            meal: self.tape_meal.clone(),
            inputs: self.tape_input.clone(),
            saved: self.tape_saved.clone(),
            unlisted: self.job.unlisted.clone(),
            arrived: self.tape_arrived.clone(),
            left: self.tape_left.clone(),
            dropped: self.tape_dropped.clone(),
        }
    }

    /// # Transport Cover
    ///
    /// Transport units this pop can spend from working stock.
    ///
    /// `factuals` names the transport goods. A good that is missing there
    /// adds nothing.
    pub(crate) fn transport_cover(&self, factuals: &Factuals) -> f64 {
        let mut cover = 0.0;
        for (&id, row) in &self.property {
            let Some(good) = factuals.get_good(id) else {
                continue;
            };
            cover += good.transport_cover(row.available().max(0.0));
        }
        cover
    }

    /// # Spend Freight
    ///
    /// Pays `amount` of transport from working stock.
    ///
    /// `factuals` names the transport goods. The spend is [`Self::pay_freight`].
    pub(crate) fn spend_freight(&mut self, amount: f64, factuals: &Factuals) {
        self.pay_freight(amount, factuals);
    }

    /// # Meal To Keep
    ///
    /// Units of stock to move into the meal before the sell book.
    ///
    /// One level of each basic and common desire, and one level of each
    /// luxury desire. Desires in a tier follow [`Desire::cmp_order`]. Inside
    /// a desire, higher efficiency is taken first, up to that target's cap.
    /// Returns the units by good.
    pub(crate) fn meal_to_keep(&self) -> HashMap<usize, f64> {
        let mut stock: HashMap<usize, f64> = self
            .property
            .iter()
            .map(|(&good, row)| (good, row.quantity.max(0.0)))
            .collect();
        let mut locked = HashMap::new();
        for tier in &self.desires {
            let mut order: Vec<&Desire> = tier.iter().collect();
            order.sort_by(|left, right| left.cmp_order(right));
            for desire in order {
                if !desire.amount.is_finite() || desire.amount <= 0.0 {
                    continue;
                }
                let mut budget = desire.amount;
                let mut targets = desire.target.clone();
                targets.sort_by(|left, right| {
                    right
                        .efficiency
                        .partial_cmp(&left.efficiency)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                for target in targets {
                    if budget <= 0.0 || target.efficiency <= 0.0 {
                        break;
                    }
                    let have = stock.get(&target.good).copied().unwrap_or(0.0);
                    let cap_units =
                        desire.amount * target.cap.max(0.0) / target.efficiency;
                    let want = (budget / target.efficiency).min(cap_units.max(0.0));
                    let take = want.min(have).max(0.0);
                    if take <= 0.0 {
                        continue;
                    }
                    if let Some(left) = stock.get_mut(&target.good) {
                        *left = (*left - take).max(0.0);
                    }
                    *locked.entry(target.good).or_insert(0.0) += take;
                    budget -= take * target.efficiency;
                }
            }
        }
        locked
    }

    /// # Basic Gaps
    ///
    /// Satisfaction each tier-0 desire still lacks, and the goods that fill it.
    ///
    /// Stock on hand counts, and so does a meal already set aside. A desire
    /// that stock covers is left out. Returns each short desire as the goods
    /// with their satisfaction per unit, then the missing satisfaction.
    pub(crate) fn basic_gaps(&self) -> Vec<(Vec<(usize, f64)>, f64)> {
        let mut out = Vec::new();
        let Some(basic) = self.desires.first() else {
            return out;
        };
        for desire in basic {
            if !desire.amount.is_finite() || desire.amount <= 0.0 {
                continue;
            }
            let mut goods = Vec::new();
            let mut have = 0.0;
            for target in &desire.target {
                if !target.efficiency.is_finite() || target.efficiency <= 0.0 {
                    continue;
                }
                let stock = self.working_units(target.good)
                    + self.meal.get(&target.good).copied().unwrap_or(0.0);
                have += stock.max(0.0) * target.efficiency;
                goods.push((target.good, target.efficiency));
            }
            let gap = desire.amount - have;
            if gap > 1e-9 && !goods.is_empty() {
                out.push((goods, gap));
            }
        }
        out
    }

}

/// Share of a tender's stock, after desire and savings, offered in one day.
///
/// The remainder is left off the sell book.
const CURRENCY_SALE_SHARE: f64 = 0.1;

/// # Currency Sale Cap
///
/// Whole units of a tender's `available` stock that may be offered.
///
/// `available` is what remains after desire and savings. The cap is
/// [`CURRENCY_SALE_SHARE`] of that stock, floored. A non-positive or
/// non-finite stock caps at 0.
fn currency_sale_cap(available: f64) -> f64 {
    if !available.is_finite() || available <= 0.0 {
        0.0
    } else {
        (available * CURRENCY_SALE_SHARE).floor().max(0.0)
    }
}

impl DealMaker for Pop {
    fn actor(&self) -> Actor {
        Actor::Pop(self.id)
    }

    /// # Mass Market Order
    ///
    /// One buy or sell of `good`, or `None` when this pop sits out.
    ///
    /// `history` is the card. `currencies` are the market's tender slots.
    /// `factuals` names goods and processes. An untradeable good, a zero
    /// card, and a tender [`crate::game::market::payment_tenders`] will not
    /// pay sit out. A negative card is a real price. The want is
    /// [`Self::mass_want`]. A prepared [`Self::mass_buys`] list posts only
    /// the funded amount, and a good left off that list is not bought.
    /// With no list yet, the want is the amount. An empty bin does not
    /// block that buy. The posted price is the card. Fear and a short pile
    /// change whether this pop shows up and how many units, not that price.
    /// When the buy cannot start, a job input still short stays put unless
    /// every line that makes it loses holding. Otherwise [`Self::mass_sell`]
    /// is the offer, and a maker whose card is under the decay floor sits
    /// out. Returns that order.
    fn mass_order(
        &self,
        good: usize,
        history: &MarketHistory,
        currencies: &[Option<usize>],
        factuals: &Factuals,
    ) -> Option<MarketOrder> {
        if !crate::game::market::tradeable(factuals, good) {
            return None;
        }
        let market = history.price(good);
        if !market.is_finite() || market == 0.0 {
            return None;
        }
        let tenders = crate::game::market::payment_tenders(currencies, good);
        let is_tender = currencies.iter().copied().flatten().any(|id| id == good);
        if is_tender && tenders.is_empty() {
            return None;
        }
        let want = self.mass_want(good, history, factuals);
        let job_units = self.job_buy_units(good);
        let buy_units = match &self.mass_buys {
            Some(plan) => plan.get(&good).copied().unwrap_or(0.0),
            None => want,
        };
        if buy_units >= 1.0 {
            if let Some(order) = self.mass_buy(good, history, currencies, market, buy_units) {
                return Some(order);
            }
        }
        // The next run still needs this input, so it is not dumped here.
        if want >= 1.0 && job_units >= 1.0 && !self.is_produced_at_a_loss(factuals, history, good) {
            return None;
        }
        self.mass_sell(good, factuals, history, market, currencies)
    }

    /// # Mass Card Sell
    ///
    /// A sell of `good` at the card, or `None` when this pop will not sell.
    ///
    /// `history` is the card. `currencies` are the tender slots. `factuals`
    /// names goods and processes. An untradeable good and a zero card sit
    /// out. The units and the refusal are [`Self::mass_sell`]. Returns that
    /// order.
    fn mass_card_sell(
        &self,
        good: usize,
        history: &MarketHistory,
        currencies: &[Option<usize>],
        factuals: &Factuals,
    ) -> Option<MarketOrder> {
        if !crate::game::market::tradeable(factuals, good) {
            return None;
        }
        let market = history.price(good);
        if !market.is_finite() || market == 0.0 {
            return None;
        }
        self.mass_sell(good, factuals, history, market, currencies)
    }

    /// # Mass Conversion Buy
    ///
    /// A buy of `good` paid only from `tenders`, or `None`.
    ///
    /// `history` is the card. `factuals` names goods. The purse is
    /// [`Self::liquid_tender`] of each tender with a positive price. The
    /// amount is the whole units of `good` that purse can pay at the card,
    /// from [`crate::game::market::mix_change`]. A non-positive card, an
    /// empty purse, and a buy under one unit sit out. The accept price is
    /// the card. Returns that order.
    fn mass_conversion_buy(
        &self,
        good: usize,
        history: &MarketHistory,
        tenders: &[usize],
        factuals: &Factuals,
    ) -> Option<MarketOrder> {
        if !crate::game::market::tradeable(factuals, good) || tenders.is_empty() {
            return None;
        }
        let market = history.price(good);
        if !market.is_finite() || market <= 0.0 {
            return None;
        }
        let spec = crate::game::market::priced_coins(tenders, history, |id| self.liquid_tender(id));
        if spec.is_empty() {
            return None;
        }
        let mut purse = 0.0;
        for &(_, amv, free) in &spec {
            purse += amv * free;
        }
        let want = (purse / market).floor();
        if want < 1.0 {
            return None;
        }
        let (units, mix) = crate::game::market::mix_change(want, market, &spec);
        if units < 1.0 || !mix.iter().any(|(_, qty)| *qty >= 1.0) {
            return None;
        }
        Some(MarketOrder::buy(self.actor(), good, units).with_accept_price(market))
    }

    /// # Prepare Mass Buys
    ///
    /// Funds today's mass buys from the liquid tenders.
    ///
    /// `history` is the card. `currencies` are the tender slots. `factuals`
    /// names goods and processes. Each [`Self::mass_want`] of at least one
    /// unit is priced at the card. An earlier desire tier is funded before
    /// a later one. Within a tier, more satisfaction per quote is funded
    /// first, and a tie keeps the lower id. A good no desire names is
    /// funded after those. Each tender's purse is [`Self::liquid_tender`],
    /// and a funded buy subtracts that tender's part of the mix. A negative
    /// card is funded in full, because the seller pays. The result is
    /// stored on [`Self::mass_buys`], and those amounts are today's posted
    /// buys. Nothing is returned.
    fn prepare_mass_buys(
        &mut self,
        history: &MarketHistory,
        currencies: &[Option<usize>],
        factuals: &Factuals,
    ) {
        let mut funded = HashMap::new();
        let mut purses: HashMap<usize, f64> = HashMap::new();
        for good in self.mass_candidates(history) {
            if !crate::game::market::tradeable(factuals, good) {
                continue;
            }
            let market = history.price(good);
            if !market.is_finite() || market == 0.0 {
                continue;
            }
            let tenders = crate::game::market::payment_tenders(currencies, good);
            if tenders.is_empty() {
                continue;
            }
            let want = self.mass_want(good, history, factuals);
            if want < 1.0 {
                continue;
            }
            // A negative card is a disposal fee the seller pays.
            if market < 0.0 {
                let priced = tenders.iter().any(|&id| {
                    let amv = history.price(id);
                    amv.is_finite() && amv > 0.0
                });
                if priced {
                    funded.insert(good, want);
                }
                continue;
            }
            let spec = crate::game::market::priced_coins(&tenders, history, |id| {
                *purses.entry(id).or_insert_with(|| self.liquid_tender(id))
            });
            if spec.is_empty() {
                continue;
            }
            let (units, mix) = crate::game::market::mix_change(want, market, &spec);
            if units >= 1.0 && mix.iter().any(|(_, qty)| *qty >= 1.0) {
                funded.insert(good, units);
                for (id, qty) in mix {
                    if let Some(purse) = purses.get_mut(&id) {
                        *purse = (*purse - qty).max(0.0);
                    }
                }
            }
        }
        self.mass_buys = Some(funded);
        self.note_posted_buys();
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
            // The deal's good is stock in or out. Every other good is the payment.
            if good == proposal.match_good {
                if delta > 0.0 {
                    *self.day_got.entry(good).or_insert(0.0) += delta;
                } else if delta < 0.0 {
                    *self.day_gave.entry(good).or_insert(0.0) += -delta;
                }
            } else if delta > 0.0 {
                *self.day_received.entry(good).or_insert(0.0) += delta;
            } else if delta < 0.0 {
                *self.day_paid.entry(good).or_insert(0.0) += -delta;
            }
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

    /// # Pay Transport
    ///
    /// Spends `amount` of transport cover from free stock.
    ///
    /// `factuals` names the transport goods. The spend is [`Self::pay_freight`].
    /// Nothing is returned.
    fn pay_transport(&mut self, amount: f64, factuals: &Factuals) {
        self.pay_freight(amount, factuals);
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
        // The next produce spends these. The meal does not.
        self.keep_craft_inputs(factuals);
    }

    fn consume(&mut self) {
        Pop::consume(self);
    }

    fn rest(&mut self, factuals: &Factuals) {
        Pop::rest(self, factuals);
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
    use crate::game::config::TIME_PER_LABOR;
    use crate::game::craft::Craft;
    use crate::game::culture::Culture;
    use crate::game::deal::{DealMaker, ProposedDeal};
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{DemoDesire, Desire, DesireEffect, DesireTarget, DesireTargetType};
    use crate::game::effects::PopEffect;
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag, TIME};
    use crate::game::household::{BirthSentiment, DemographicRates, Household};
    use crate::game::job::{Job, JobLine};
    use crate::game::market::{Market, MarketGood, MarketHistory};
    use crate::game::pop::{DemoRow, Pop, PopPRow, PopRecords};
    use crate::game::process::{InputType, Process, ProcessEffect, ProcessInput, ProcessOutput};
    use crate::game::religion::Religion;
    use crate::game::scalingfactor::ScalingFactor;
    use crate::game::sentiment::{Sentiment, SentimentKind};
    use crate::game::species::Species;
    use crate::game::stratum::Stratum;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn do_satisfy(pop: &mut Pop) -> Option<crate::game::desire::Desire> {
        let mut rng = StdRng::seed_from_u64(1);
        pop.satisfy(&mut rng)
    }

    fn do_continue(pop: &mut Pop) -> Option<crate::game::desire::Desire> {
        let mut rng = StdRng::seed_from_u64(1);
        pop.satisfy_continue(&mut rng)
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
            saved: HashMap::new(),
            meal: HashMap::new(),
            luxury_savings: HashMap::new(),
            luxury_savings_open: true,
            cheaper_cover: HashMap::new(),
            mass_buys: None,
            currency_sale_room: HashMap::new(),
            day_listed: HashMap::new(),
            buy_posted: HashMap::new(),
            buy_received: HashMap::new(),
            day_paid: HashMap::new(),
            day_received: HashMap::new(),
            day_got: HashMap::new(),
            day_gave: HashMap::new(),
            day_haul: HashMap::new(),
            day_eaten: HashMap::new(),
            day_held: HashMap::new(),
            rest_taken: HashMap::new(),
            rest_sol: 0.0,
            day_granted: HashMap::new(),
            tape_buy: HashMap::new(),
            tape_sell: HashMap::new(),
            tape_bought: HashMap::new(),
            tape_sold: HashMap::new(),
            tape_paid: HashMap::new(),
            tape_earned: HashMap::new(),
            tape_change_in: HashMap::new(),
            tape_change_out: HashMap::new(),
            tape_meal: HashMap::new(),
            tape_input: HashMap::new(),
            tape_saved: HashMap::new(),
            tape_arrived: HashMap::new(),
            tape_left: HashMap::new(),
            tape_dropped: HashMap::new(),
            worth: HashMap::new(),
            turned: HashMap::new(),
            worth_open: true,
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
        household.update(&rates, 1.0);
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
    fn growth_phase_uses_the_configured_turn_length() {
        let mut pop = make_pop();
        let mut factuals = Factuals::new().with_species(Species::new(0, "human"));
        factuals.config.pop.turn_length = 12.0;

        pop.growth_phase(&factuals);

        let mut household = Household::new();
        household.update(&DemographicRates::baseline(), 12.0);
        let got = pop.demographics.household.total_count();
        assert!((got - household.total_count()).abs() < 1e-9);
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
            .with_stratum(
                Stratum::new(2, "tenants", 1).with_desire(DemoDesire::new(4).with_tier(0)),
            );

        pop.update_desires(&factuals);

        assert_eq!(pop.desires[0].len(), 1);
        assert_eq!(pop.desires[0][0].source, DemographicSource::Stratum(2));
        assert_eq!(pop.desires[0][0].demo_desire_id, 4);
        assert!(factuals.find_culture(1).strata.contains(&2));
    }

    /// Religion stores luxury id 2, then id 1. The copy is ids 1 then 2.
    #[test]
    fn update_desires_copies_religion_desires_in_id_order() {
        let mut pop = make_pop();
        pop.demographics.religion = 1;
        let factuals = Factuals::new().with_religion(
            Religion::new(1, "hearth")
                .with_desire(DemoDesire::new(2).with_tier(2))
                .with_desire(DemoDesire::new(1).with_tier(2)),
        );

        pop.update_desires(&factuals);

        let ids: Vec<usize> = pop.desires[2]
            .iter()
            .map(|desire| desire.demo_desire_id)
            .collect();
        assert_eq!(ids, vec![1, 2]);
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
        pop.property
            .insert(1, PopPRow::new(10.0).with_consumed(4.0));
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

    fn grain_meal(pop: &mut Pop, amount: f64) {
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            amount,
        ));
    }

    fn card_at(price: f64) -> MarketHistory {
        let mut history = MarketHistory::new();
        history.prices.insert(1, price);
        history.prices.insert(5, 1.0);
        history
    }

    /// Grain at `grain`. Time at `time` with salability 1.
    fn card_with_time(grain: f64, time: f64) -> MarketHistory {
        let mut history = card_at(grain);
        history.prices.insert(0, time);
        history.salability.insert(0, 1.0);
        history
    }

    /// Two time in, six water out.
    fn water_maker(target: f64) -> (Job, Factuals) {
        let job = Job::new(1, vec![JobLine::new(2, Some(target), vec![])]);
        let process = Process::new(2, "make water", 0)
            .with_input(ProcessInput::new(0, 2.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 6.0, true));
        let factuals = Factuals::new()
            .with_good(make_good(0, "time", 1.0))
            .with_good(make_good(2, "water", 0.0))
            .with_process(process);
        (job, factuals)
    }

    /// Five time in, six grain out. Time is loaded so it does not survive the night.
    fn grain_maker() -> (Job, Factuals) {
        let job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        let process = Process::new(1, "make grain", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(1, 6.0, true));
        let factuals = Factuals::new()
            .with_good(make_good(0, "time", 1.0))
            .with_process(process);
        (job, factuals)
    }

    /// Empty bin, desire of 4, card 2, ten tender. The buy is 4 at the card.
    #[test]
    fn mass_order_buys_the_short_need_up_to_the_tender() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(5, PopPRow::new(10.0));
        let order = pop
            .mass_order(
                1,
                &card_at(2.0),
                &[Some(5), None, None, None],
                &Factuals::new(),
            )
            .expect("buy");
        assert!((order.target_amount - 4.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);
    }

    /// Two of the four are already on hand. The buy is the other two at the card.
    #[test]
    fn mass_order_raises_the_buy_ceiling_with_the_share_still_short() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(2.0));
        pop.property.insert(5, PopPRow::new(10.0));
        let order = pop
            .mass_order(
                1,
                &card_at(2.0),
                &[Some(5), None, None, None],
                &Factuals::new(),
            )
            .expect("buy");
        assert!((order.target_amount - 2.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);
    }

    /// Tender covers two units at a card of 2. The want stays four.
    #[test]
    fn mass_order_buys_only_what_the_tender_can_pay() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(5, PopPRow::new(5.0));
        let order = pop
            .mass_order(
                1,
                &card_at(2.0),
                &[Some(5), None, None, None],
                &Factuals::new(),
            )
            .expect("buy");
        assert!((order.target_amount - 2.0).abs() < 1e-9);
    }

    /// The saved tender is not purse, so the short meal is not bought.
    #[test]
    fn mass_order_does_not_spend_saved_tender() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(5, PopPRow::new(5.0));
        pop.savings.insert(5, 5.0);
        assert!(
            pop.mass_order(
                1,
                &card_at(2.0),
                &[Some(5), None, None, None],
                &Factuals::new()
            )
            .is_none()
        );
    }

    /// Six on hand, a need of four, and tender in the purse. The extra two
    /// are sold. The one-unit desire bid does not replace that sale.
    #[test]
    fn mass_order_sells_surplus_ahead_of_the_one_unit_bid() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(6.0));
        pop.property.insert(5, PopPRow::new(10.0));
        let order = pop
            .mass_order(
                1,
                &card_at(4.0),
                &[Some(5), None, None, None],
                &Factuals::new(),
            )
            .expect("sell");
        assert!((order.target_amount - -2.0).abs() < 1e-9);
    }

    /// The meal is reserved, so nothing is for sale. The desire still bids one unit.
    #[test]
    fn mass_order_keeps_one_unit_of_a_stocked_desire() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(4.0));
        pop.property.insert(5, PopPRow::new(10.0));
        assert!(do_satisfy(&mut pop).is_none());
        let order = pop
            .mass_order(
                1,
                &card_at(2.0),
                &[Some(5), None, None, None],
                &Factuals::new(),
            )
            .expect("buy");
        assert!((order.target_amount - 1.0).abs() < 1e-9);
    }

    /// Six on hand and a need of four. The pop does not make it, so two sell at the card.
    #[test]
    fn mass_order_sells_the_surplus_above_a_need() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(6.0));
        let order = pop
            .mass_order(
                1,
                &card_at(4.0),
                &[Some(5), None, None, None],
                &Factuals::new(),
            )
            .expect("sell");
        assert!((order.target_amount - -2.0).abs() < 1e-9);
        assert!((order.accept_price - 4.0).abs() < 1e-9);
    }

    /// Six on hand, a meal of four, and two saved. Worth is under the card,
    /// so the saved two are offered and the meal stays.
    #[test]
    fn mass_order_sells_stock_valued_under_the_card() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(6.0));
        pop.savings.insert(1, 2.0);
        pop.worth.insert(1, 1.0);
        let order = pop
            .mass_order(
                1,
                &card_at(4.0),
                &[Some(5), None, None, None],
                &Factuals::new(),
            )
            .expect("sell");
        assert!((order.target_amount - -2.0).abs() < 1e-9);
    }

    /// The same pile at a worth on the card stays in savings. Nothing is offered.
    #[test]
    fn mass_order_keeps_stock_valued_at_the_card() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(6.0));
        pop.savings.insert(1, 2.0);
        pop.worth.insert(1, 4.0);
        assert!(
            pop.mass_order(
                1,
                &card_at(4.0),
                &[Some(5), None, None, None],
                &Factuals::new()
            )
            .is_none()
        );
    }

    /// The same pile, made at a profit. The card covers the cost, so two sell at the card.
    #[test]
    fn mass_order_floors_a_made_good_at_its_cost() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(6.0));
        let (job, factuals) = grain_maker();
        pop.job = job;
        let order = pop
            .mass_order(
                1,
                &card_with_time(2.0, 1.0),
                &[Some(5), None, None, None],
                &factuals,
            )
            .expect("sell");
        assert!((order.target_amount - -2.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);
    }

    /// Making grain loses at this card. Time holds at 1. The need itself is offered at the card.
    #[test]
    fn mass_order_sells_a_losing_product_through_the_need() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(6.0));
        let (job, factuals) = grain_maker();
        pop.job = job;
        let order = pop
            .mass_order(
                1,
                &card_with_time(1.0, 1.0),
                &[Some(5), None, None, None],
                &factuals,
            )
            .expect("sell");
        assert!((order.target_amount - -6.0).abs() < 1e-9);
        assert!((order.accept_price - 1.0).abs() < 1e-9);
    }

    /// Cost is 5/6 and the card is 1/2. Decay is 0, so the maker sits out.
    #[test]
    fn mass_order_refuses_a_card_under_make_cost() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(6.0));
        let (job, factuals) = grain_maker();
        pop.job = job;
        let history = card_with_time(0.5, 1.0);
        assert!(
            pop.mass_order(1, &history, &[Some(5), None, None, None], &factuals)
                .is_none()
        );
    }

    /// The pop does not make it. Surplus sells at the card.
    #[test]
    fn mass_order_sells_surplus_at_the_card() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(6.0));
        let mut history = card_at(4.0);
        history.ask_walk.insert(1, 1.0);
        let order = pop
            .mass_order(1, &history, &[Some(5), None, None, None], &Factuals::new())
            .expect("sell");
        assert!((order.target_amount - -2.0).abs() < 1e-9);
        assert!((order.accept_price - 4.0).abs() < 1e-9);
    }

    /// Water decays by half. Cost is 2 and the floor is 1, so a card of 1.5 sells.
    #[test]
    fn mass_order_sells_down_to_the_decay_floor() {
        let mut pop = make_pop();
        let (job, mut factuals) = water_maker(10.0);
        factuals.goods.get_mut(&2).expect("water").decay_rate = 0.5;
        pop.job = job;
        pop.property.insert(2, PopPRow::new(12.0));
        let history = card(&[(0, 6.0, 1.0), (2, 1.5, 1.0)]);
        let order = pop
            .mass_order(2, &history, &[Some(1), None, None, None], &factuals)
            .expect("sell");
        assert!((order.target_amount - -12.0).abs() < 1e-9);
        assert!((order.accept_price - 1.5).abs() < 1e-9);
    }

    /// The same water at 0.5 is under the floor of 1. The maker sits out.
    #[test]
    fn mass_order_refuses_a_card_under_the_decay_floor() {
        let mut pop = make_pop();
        let (job, mut factuals) = water_maker(10.0);
        factuals.goods.get_mut(&2).expect("water").decay_rate = 0.5;
        pop.job = job;
        pop.property.insert(2, PopPRow::new(12.0));
        let history = card(&[(0, 6.0, 1.0), (2, 0.5, 1.0)]);
        assert!(
            pop.mass_order(2, &history, &[Some(1), None, None, None], &factuals)
                .is_none()
        );
    }

    /// Time at 30 prices one water at 10. Decay is 0, so a card of 2 is refused.
    #[test]
    fn mass_order_refuses_a_card_under_an_undecayed_cost() {
        let mut pop = make_pop();
        let (job, factuals) = water_maker(10.0);
        pop.job = job;
        pop.property.insert(2, PopPRow::new(30.0));
        let history = card(&[(0, 30.0, 1.0), (2, 2.0, 1.0)]);
        assert!(
            pop.mass_order(2, &history, &[Some(1), None, None, None], &factuals)
                .is_none()
        );
    }

    /// No desire, so nothing is bought. Surplus of that good is still offered.
    #[test]
    fn mass_order_sells_surplus_that_feeds_no_desire() {
        let mut pop = make_pop();
        pop.property.insert(5, PopPRow::new(10.0));
        pop.property.insert(7, PopPRow::new(5.0));
        let mut history = MarketHistory::new();
        history.prices.insert(7, 1.0);
        history.prices.insert(5, 1.0);
        assert!(
            pop.mass_order(1, &history, &[Some(5), None, None, None], &Factuals::new())
                .is_none()
        );
        let sell = pop
            .mass_order(7, &history, &[Some(5), None, None, None], &Factuals::new())
            .expect("sell");
        assert!((sell.target_amount - -5.0).abs() < 1e-9);
        assert!((sell.accept_price - 1.0).abs() < 1e-9);
    }

    /// An untradeable desire is not quoted.
    #[test]
    fn mass_order_skips_an_untradeable_good() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.property.insert(5, PopPRow::new(10.0));
        let mut skill = make_good(1, "skill", 0.0);
        skill.tags.insert(GoodTag::Untradeable);
        let factuals = Factuals::new().with_good(skill);
        assert!(
            pop.mass_order(1, &card_at(1.0), &[Some(5), None, None, None], &factuals)
                .is_none()
        );
    }

    /// Grain is only the next run's input. Four are bought at the card.
    #[test]
    fn mass_order_buys_a_job_input_it_does_not_desire() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, Some(4.0), vec![])]);
        pop.property.insert(5, PopPRow::new(10.0));
        let factuals = Factuals::new()
            .with_good(make_good(1, "grain", 0.0))
            .with_process(bake());
        pop.settle_shopping(&factuals);
        let order = pop
            .mass_order(1, &card_at(2.0), &[Some(5), None, None, None], &factuals)
            .expect("buy");
        assert!((order.target_amount - 4.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);
    }

    /// The open meal is four and the job shops two more. The buy is the sum.
    #[test]
    fn mass_order_adds_the_job_shortfall_onto_the_desire() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 4.0);
        pop.job = Job::new(1, vec![JobLine::new(7, Some(2.0), vec![])]);
        pop.property.insert(5, PopPRow::new(20.0));
        let factuals = Factuals::new()
            .with_good(make_good(1, "grain", 0.0))
            .with_process(bake());
        pop.settle_shopping(&factuals);
        let order = pop
            .mass_order(1, &card_at(2.0), &[Some(5), None, None, None], &factuals)
            .expect("buy");
        assert!((order.target_amount - 6.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);
    }

    /// Five on hand and eight needed. No tender, so the five stay off the sell book.
    #[test]
    fn mass_order_keeps_a_job_input_when_the_buy_cannot_start() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, Some(8.0), vec![])]);
        pop.property.insert(1, PopPRow::new(5.0));
        let factuals = Factuals::new()
            .with_good(make_good(1, "grain", 0.0))
            .with_process(bake());
        pop.settle_shopping(&factuals);
        assert!(
            pop.mass_order(1, &card_at(2.0), &[Some(5), None, None, None], &factuals)
                .is_none()
        );
    }

    /// A card of -4 with no make cost sells at that card.
    #[test]
    fn mass_order_sells_under_a_negative_card() {
        let mut pop = make_pop();
        pop.property.insert(7, PopPRow::new(5.0));
        let mut history = MarketHistory::new();
        history.prices.insert(7, -4.0);
        history.prices.insert(5, 1.0);
        let sell = pop
            .mass_order(7, &history, &[Some(5), None, None, None], &Factuals::new())
            .expect("sell");
        assert!((sell.target_amount - -5.0).abs() < 1e-9);
        assert!((sell.accept_price - -4.0).abs() < 1e-9);
    }

    /// The grain maker's cost is positive. A card of -2 is under the floor.
    #[test]
    fn mass_order_refuses_a_negative_card_on_a_made_good() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(6.0));
        let (job, factuals) = grain_maker();
        pop.job = job;
        let mut history = card_with_time(-2.0, 1.0);
        history.prices.insert(5, 1.0);
        assert!(
            pop.mass_order(1, &history, &[Some(5), None, None, None], &factuals)
                .is_none()
        );
    }

    /// Fully missing at a card of -2. The buyer holds no tender and still bids the card.
    #[test]
    fn mass_order_buys_a_negative_card_without_spending_tender() {
        let mut pop = make_pop();
        grain_meal(&mut pop, 2.0);
        let history = card_at(-2.0);
        let order = pop
            .mass_order(1, &history, &[Some(5), None, None, None], &Factuals::new())
            .expect("buy");
        assert!((order.target_amount - 2.0).abs() < 1e-9);
        assert!((order.accept_price - -2.0).abs() < 1e-9);
    }

    /// Saved grain is not purse, so it does not bid for gold.
    #[test]
    fn mass_conversion_buy_skips_saved_grain() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(20.0));
        pop.savings.insert(1, 20.0);
        let mut history = MarketHistory::new();
        history.prices.insert(1, 1.0);
        history.prices.insert(4, 1.0);
        assert!(
            pop.mass_conversion_buy(4, &history, &[1], &Factuals::new())
                .is_none()
        );
    }

    /// Twenty free grain buys twenty gold at a card of 1.
    #[test]
    fn mass_conversion_buy_spends_liquid_grain_on_gold() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(20.0));
        let mut history = MarketHistory::new();
        history.prices.insert(1, 1.0);
        history.prices.insert(4, 1.0);
        let order = pop
            .mass_conversion_buy(4, &history, &[1], &Factuals::new())
            .expect("buy");
        assert!((order.target_amount - 20.0).abs() < 1e-9);
        assert!((order.accept_price - 1.0).abs() < 1e-9);
    }

    /// Twenty left after savings. A tender offers two and keeps the other eighteen.
    #[test]
    fn mass_order_offers_a_tenth_of_spare_tender() {
        let mut pop = make_pop();
        pop.property.insert(5, PopPRow::new(24.0));
        pop.savings.insert(5, 4.0);
        let mut history = MarketHistory::new();
        history.prices.insert(5, 1.0);
        history.prices.insert(7, 1.0);
        let sell = pop
            .mass_order(
                5,
                &history,
                &[Some(5), Some(7), None, None],
                &Factuals::new(),
            )
            .expect("sell");
        assert!((sell.target_amount - -2.0).abs() < 1e-9);
    }

    /// Twenty spare units of the second tender offer two.
    #[test]
    fn mass_order_offers_a_tenth_of_spare_second_tender() {
        let mut pop = make_pop();
        pop.property.insert(7, PopPRow::new(24.0));
        pop.savings.insert(7, 4.0);
        let mut history = MarketHistory::new();
        history.prices.insert(5, 1.0);
        history.prices.insert(7, 1.0);
        let sell = pop
            .mass_order(
                7,
                &history,
                &[Some(5), Some(7), None, None],
                &Factuals::new(),
            )
            .expect("sell");
        assert!((sell.target_amount - -2.0).abs() < 1e-9);
    }

    /// Nine free tender units floor to an empty offer.
    #[test]
    fn mass_order_keeps_a_tender_when_a_tenth_is_under_one() {
        let mut pop = make_pop();
        pop.property.insert(5, PopPRow::new(9.0));
        let mut history = MarketHistory::new();
        history.prices.insert(5, 1.0);
        history.prices.insert(7, 1.0);
        assert!(
            pop.mass_order(
                5,
                &history,
                &[Some(5), Some(7), None, None],
                &Factuals::new()
            )
            .is_none()
        );
    }

    /// The run loses, and decay of 0.8 keeps the card above the floor.
    /// The desire pile stays. Twenty spare units offer two.
    #[test]
    fn mass_order_keeps_a_losing_tenders_desire_stock() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(5, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(5, PopPRow::new(24.0));
        pop.job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        let process = Process::new(1, "mint", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(5, 1.0, true));
        let factuals = Factuals::new()
            .with_process(process)
            .with_good(make_good(5, "coin", 0.8));
        let mut history = MarketHistory::new();
        history.prices.insert(0, 1.0);
        history.prices.insert(5, 1.0);
        history.prices.insert(7, 1.0);
        let sell = pop
            .mass_order(5, &history, &[Some(5), Some(7), None, None], &factuals)
            .expect("sell");
        assert!((sell.target_amount - -2.0).abs() < 1e-9);
    }

    /// The second tender loses the same way. Decay of 0.8 keeps its card above the floor.
    /// The desire pile stays, and twenty spare units offer two.
    #[test]
    fn mass_order_keeps_a_losing_second_tenders_desire_stock() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(7, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(7, PopPRow::new(24.0));
        pop.job = Job::new(1, vec![JobLine::new(1, Some(1.0), vec![])]);
        let process = Process::new(1, "mint", 0)
            .with_input(ProcessInput::new(0, 5.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(7, 1.0, true));
        let factuals = Factuals::new()
            .with_process(process)
            .with_good(make_good(7, "coin", 0.8));
        let mut history = MarketHistory::new();
        history.prices.insert(0, 1.0);
        history.prices.insert(5, 1.0);
        history.prices.insert(7, 1.0);
        let sell = pop
            .mass_order(7, &history, &[Some(5), Some(7), None, None], &factuals)
            .expect("sell");
        assert!((sell.target_amount - -2.0).abs() < 1e-9);
    }

    /// The only tender is not quoted, even when the pop holds a surplus of it.
    #[test]
    fn mass_order_skips_the_only_tender() {
        let mut pop = make_pop();
        pop.property.insert(5, PopPRow::new(10.0));
        assert!(
            pop.mass_order(
                5,
                &card_at(1.0),
                &[Some(5), None, None, None],
                &Factuals::new()
            )
            .is_none()
        );
    }

    /// Good 5 is a tender, and good 7 pays for it. Two are bought with that other pile.
    #[test]
    fn mass_order_buys_one_tender_with_the_other() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(5, DesireTargetType::Consume, 1.0)],
            2.0,
        ));
        pop.property.insert(7, PopPRow::new(10.0));
        let mut history = MarketHistory::new();
        history.prices.insert(5, 2.0);
        history.prices.insert(7, 1.0);
        let order = pop
            .mass_order(
                5,
                &history,
                &[Some(5), Some(7), None, None],
                &Factuals::new(),
            )
            .expect("buy");
        assert!((order.target_amount - 2.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);
    }

    /// Five tender buys two of the basic good. The later good is left unfunded.
    #[test]
    fn prepare_mass_buys_funds_the_earlier_desire_first() {
        let mut pop = make_pop();
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[0].push(desire(
            2,
            vec![DesireTarget::new(7, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(5, PopPRow::new(5.0));
        let mut history = card_at(2.0);
        history.prices.insert(7, 2.0);
        let currencies = [Some(5), None, None, None];
        pop.prepare_mass_buys(&history, &currencies, &Factuals::new());
        let basic = pop
            .mass_order(7, &history, &currencies, &Factuals::new())
            .expect("basic buy");
        assert!((basic.target_amount - 2.0).abs() < 1e-9);
        assert!((basic.accept_price - 2.0).abs() < 1e-9);
        assert!(
            pop.mass_order(1, &history, &currencies, &Factuals::new())
                .is_none()
        );
    }

    /// An empty bin funds the cover at the card. Grain already in the bin
    /// is not bought again: half on hand buys the other half, and a full bin
    /// keeps only the one-unit floor.
    #[test]
    fn prepare_mass_buys_funds_the_cover_still_missing() {
        let currencies = [Some(5), None, None, None];
        let history = card_at(2.0);
        let factuals = Factuals::new();

        let mut empty = make_pop();
        grain_meal(&mut empty, 4.0);
        empty.desires[0][0].satisfaction = 4.0;
        empty.property.insert(5, PopPRow::new(10.0));
        empty.cheaper_cover.insert(1, 4.0);
        empty.prepare_mass_buys(&history, &currencies, &factuals);
        let order = empty
            .mass_order(1, &history, &currencies, &factuals)
            .expect("buy");
        assert!((order.target_amount - 4.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);

        let mut half = make_pop();
        grain_meal(&mut half, 4.0);
        half.property.insert(1, PopPRow::new(2.0));
        half.property.insert(5, PopPRow::new(10.0));
        half.cheaper_cover.insert(1, 4.0);
        half.prepare_mass_buys(&history, &currencies, &factuals);
        let order = half
            .mass_order(1, &history, &currencies, &factuals)
            .expect("buy");
        assert!((order.target_amount - 2.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);

        let mut full = make_pop();
        grain_meal(&mut full, 4.0);
        full.property.insert(1, PopPRow::new(4.0));
        full.property.insert(5, PopPRow::new(10.0));
        full.cheaper_cover.insert(1, 4.0);
        full.prepare_mass_buys(&history, &currencies, &factuals);
        let order = full
            .mass_order(1, &history, &currencies, &factuals)
            .expect("floor");
        assert!((order.target_amount - 1.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);
    }

    /// Bread satisfies more per unit and costs less, so the purse buys it
    /// before grain even though grain's id is lower.
    #[test]
    fn prepare_mass_buys_funds_the_better_satisfaction_first() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[0].push(desire(
            2,
            vec![DesireTarget::new(3, DesireTargetType::Consume, 2.0)],
            4.0,
        ));
        pop.property.insert(5, PopPRow::new(4.0));
        let mut history = MarketHistory::new();
        history.prices.insert(1, 4.0);
        history.prices.insert(3, 1.0);
        history.prices.insert(5, 1.0);
        let currencies = [Some(5), None, None, None];
        pop.prepare_mass_buys(&history, &currencies, &Factuals::new());
        let bread = pop
            .mass_order(3, &history, &currencies, &Factuals::new())
            .expect("bread");
        assert!((bread.target_amount - 2.0).abs() < 1e-9);
        assert!(
            pop.mass_order(1, &history, &currencies, &Factuals::new())
                .is_none()
        );
    }

    /// Bread satisfies twice as much and costs four times the card. The level
    /// goes to grain.
    #[test]
    fn portfolio_prefers_satisfaction_per_quote() {
        let mut pop = make_pop();
        let meal = desire(
            1,
            vec![
                DesireTarget::new(2, DesireTargetType::Consume, 2.0),
                DesireTarget::new(1, DesireTargetType::Consume, 1.0),
            ],
            6.0,
        );
        pop.desires[0].push(meal.clone());
        let mut history = MarketHistory::new();
        history.prices.insert(2, 4.0);
        history.prices.insert(1, 1.0);
        let split = pop.portfolio(&meal, 0, &history);
        let grain = split
            .iter()
            .find(|(good, _)| *good == 1)
            .map(|(_, units)| *units)
            .unwrap_or(0.0);
        let bread = split
            .iter()
            .find(|(good, _)| *good == 2)
            .map(|(_, units)| *units)
            .unwrap_or(0.0);
        assert!((grain - 4.0).abs() < 1e-9);
        assert!((bread - 1.0).abs() < 1e-9);
    }

    /// Both goods can be made. Bread is the richer meal and the dearer card,
    /// so the short desire is grain.
    #[test]
    fn night_needs_makes_the_cheaper_satisfaction() {
        let mut pop = make_pop();
        pop.job = Job::new(
            1,
            vec![
                JobLine::new(7, Some(0.0), vec![]),
                JobLine::new(9, Some(0.0), vec![]),
            ],
        );
        pop.desires[0].push(desire(
            1,
            vec![
                DesireTarget::new(2, DesireTargetType::Consume, 2.0),
                DesireTarget::new(1, DesireTargetType::Consume, 1.0),
            ],
            4.0,
        ));
        let factuals = Factuals::new()
            .with_process(Process::new(7, "bake", 0).with_output(ProcessOutput::new(2, 1.0, true)))
            .with_process(
                Process::new(9, "thresh", 0).with_output(ProcessOutput::new(1, 1.0, true)),
            );
        let mut history = MarketHistory::new();
        history.prices.insert(2, 4.0);
        history.prices.insert(1, 1.0);

        let needs = pop.night_needs(&factuals, &history);

        assert!((needs.meal.get(&1).copied().unwrap_or(0.0) - 4.0).abs() < 1e-9);
        assert!(needs.meal.get(&2).is_none());
    }

    /// Full fear still buys four at the card.
    #[test]
    fn mass_order_buys_the_card_when_the_pop_is_afraid() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::from_parts(0.0, 0.0, 0.0, 1.0, 0.0);
        grain_meal(&mut pop, 4.0);
        pop.property.insert(5, PopPRow::new(10.0));
        let order = pop
            .mass_order(
                1,
                &card_at(2.0),
                &[Some(5), None, None, None],
                &Factuals::new(),
            )
            .expect("buy");
        assert!((order.target_amount - 4.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);
    }

    /// Full fear still sells the surplus at the card.
    #[test]
    fn mass_order_sells_the_card_when_the_pop_is_afraid() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::from_parts(0.0, 0.0, 0.0, 1.0, 0.0);
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(6.0));
        let order = pop
            .mass_order(
                1,
                &card_at(4.0),
                &[Some(5), None, None, None],
                &Factuals::new(),
            )
            .expect("sell");
        assert!((order.target_amount - -2.0).abs() < 1e-9);
        assert!((order.accept_price - 4.0).abs() < 1e-9);
    }

    /// Full fear still sells a made good at the card when the card covers its cost.
    #[test]
    fn mass_order_sells_a_made_good_at_the_card_when_the_pop_is_afraid() {
        let mut pop = make_pop();
        pop.sentiment = Sentiment::from_parts(0.0, 0.0, 0.0, 1.0, 0.0);
        grain_meal(&mut pop, 4.0);
        pop.property.insert(1, PopPRow::new(6.0));
        let (job, factuals) = grain_maker();
        pop.job = job;
        let order = pop
            .mass_order(
                1,
                &card_with_time(2.0, 1.0),
                &[Some(5), None, None, None],
                &factuals,
            )
            .expect("sell");
        assert!((order.target_amount - -2.0).abs() < 1e-9);
        assert!((order.accept_price - 2.0).abs() < 1e-9);
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

        // Four gold feed the jewelry desire. The other six are for sale.
        assert!((pop.surplus_for_sale(4) - 6.0).abs() < 1e-9);
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

        // Two cabins take six wood. The other four are for sale.
        assert!((pop.surplus_for_sale(7) - 4.0).abs() < 1e-9);
    }

    /// # Water Seller
    ///
    /// A water maker with thirty on hand, sixty made, and a target of ten.
    ///
    /// Returns that pop and the water process.
    fn water_seller() -> (Pop, Factuals) {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(2, Some(10.0), vec![])]);
        let mut water = PopPRow::new(30.0);
        water.produced = 60.0;
        pop.property.insert(2, water);
        pop.property.insert(TIME, PopPRow::new(10_000.0));
        let factuals = Factuals::new().with_process(
            Process::new(2, "make water", 0)
                .with_input(ProcessInput::new(TIME, 2.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(2, 6.0, true)),
        );
        (pop, factuals)
    }

    /// # Water Currency
    ///
    /// Water is the tender, and its card is above a free cost.
    ///
    /// Returns that board.
    fn water_currency() -> MarketHistory {
        let mut history = MarketHistory::new();
        history.prices.insert(TIME, 0.0);
        history.prices.insert(2, 2.0);
        history.salability.insert(2, 1.0);
        history.currencies = vec![2];
        history
    }

    /// The listed unit sold and the pile did not. The sale leaves the march.
    #[test]
    fn plan_leaves_the_march_when_the_posted_tender_sells_out() {
        let (mut pop, factuals) = water_seller();
        pop.note_currency_sale_room(&[Some(2)]);
        pop.spend_currency_sale_room(2, 1.0);
        pop.day_gave.insert(2, 1.0);

        pop.plan(&factuals, &water_currency());

        assert!((pop.job.lines[0].target.unwrap_or(0.0) - 10.0).abs() < 1e-9);
    }

    /// Both posts stay listed. One sale of those two units aims at that unit.
    #[test]
    fn plan_counts_both_tender_posts_toward_what_sold() {
        let (mut pop, factuals) = water_seller();
        pop.note_currency_sale_room(&[Some(2)]);
        pop.spend_currency_sale_room(2, 1.0);
        pop.note_currency_sale_room(&[Some(2)]);
        pop.spend_currency_sale_room(2, 1.0);
        pop.day_gave.insert(2, 1.0);

        pop.plan(&factuals, &water_currency());

        assert!((pop.job.lines[0].target.unwrap_or(0.0) - 1.0 / 6.0).abs() < 1e-9);
    }

    /// Nothing was posted. The pile remains and the card covers cost,
    /// so the sale leaves the march.
    #[test]
    fn plan_leaves_the_march_when_the_tender_was_not_posted() {
        let (mut pop, factuals) = water_seller();

        pop.plan(&factuals, &water_currency());

        assert!((pop.job.lines[0].target.unwrap_or(0.0) - 10.0).abs() < 1e-9);
    }

    /// No water, and the six wood sit inside a later desire, so none of it is surplus.
    /// The buyer still pays with that wood, because water is the earlier desire.
    /// Four water are still wanted: amount 8 at efficiency 2. Those four units
    /// place 8 satisfaction. Four wood place 4. The gain is larger, so the
    /// later good pays for the earlier one.
    #[test]
    fn plan_covers_one_full_tier_of_the_best_target() {
        let mut pop = make_pop();
        pop.job = Job::new(
            1,
            vec![JobLine::new(7, None, vec![]), JobLine::new(9, None, vec![])],
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
        let factuals = Factuals::new().with_process(bake()).with_process(
            Process::new(9, "eat grain", 0).with_output(ProcessOutput::new(1, 1.0, true)),
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
        let factuals = Factuals::new().with_process(bake()).with_process(
            Process::new(9, "weave", 0).with_output(ProcessOutput::new(4, 1.0, true)),
        );
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
        let factuals = Factuals::new().with_process(
            Process::new(9, "thresh", 0).with_output(ProcessOutput::new(1, 1.0, true)),
        );
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
        let factuals = Factuals::new().with_process(bake()).with_process(
            Process::new(9, "cut gem", 0).with_output(ProcessOutput::new(4, 1.0, true)),
        );
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
                .with_input(ProcessInput::new(
                    TIME,
                    0.5,
                    true,
                    InputType::Destroyed,
                    false,
                ))
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
        let factuals =
            Factuals::new().with_process(bake().with_effect(ProcessEffect::Culture(2.0)));
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
    /// the job on the line it already had. The process is missing, so the
    /// morning plan writes a target of zero.
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
        assert_eq!(pop.job.lines[0].target, Some(0.0));
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
        assert!(
            pop.job.lines[1..]
                .iter()
                .all(|line| line.target == Some(0.0))
        );
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

    /// A cap of half a level is reserved from the good on hand. The level
    /// stays open at that half.
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

    /// A sale books the deal's good apart from the coin. A negative payment
    /// is the seller paying the buyer to take the good.
    #[test]
    fn finalize_books_the_deal_good_apart_from_the_payment() {
        let factuals = Factuals::new();
        let mut buyer = make_pop();
        buyer.property.insert(9, PopPRow::new(10.0));
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(2, PopPRow::new(4.0));
        let deal = proposal(2, HashMap::from([(2, 4.0), (9, -9.0)]));

        buyer.finalize(&deal, &factuals);
        seller.finalize(&deal, &factuals);

        assert!((buyer.day_got[&2] - 4.0).abs() < 1e-12);
        assert!((buyer.day_paid[&9] - 9.0).abs() < 1e-12);
        assert!(buyer.day_gave.is_empty());
        assert!(buyer.day_received.is_empty());
        assert!((seller.day_gave[&2] - 4.0).abs() < 1e-12);
        assert!((seller.day_received[&9] - 9.0).abs() < 1e-12);
        assert!(seller.day_got.is_empty());
        assert!(seller.day_paid.is_empty());

        let mut buyer = make_pop();
        let mut seller = make_pop();
        seller.id = 2;
        seller.property.insert(9, PopPRow::new(3.0));
        let dumped = proposal(2, HashMap::from([(2, 4.0), (9, 3.0)]));
        buyer.finalize(&dumped, &factuals);
        seller.finalize(&dumped, &factuals);

        assert!((buyer.day_got[&2] - 4.0).abs() < 1e-12);
        assert!((buyer.day_received[&9] - 3.0).abs() < 1e-12);
        assert!(buyer.day_paid.is_empty());
        assert!((seller.day_gave[&2] - 4.0).abs() < 1e-12);
        assert!((seller.day_paid[&9] - 3.0).abs() < 1e-12);
        assert!(seller.day_received.is_empty());
    }

    /// Production, payment, the deal's good, freight, and rot are priced on
    /// the card. The meal is its own line. Capital and a `consumed` balance
    /// that was not eaten for a desire are not. The morning reset clears them.
    #[test]
    fn day_books_prices_production_and_trade() {
        let mut pop = make_pop();
        let mut grain = PopPRow::new(4.0);
        grain.produced = 4.0;
        grain.used = 9.0;
        grain.lost = 9.0;
        grain.consumed = 9.0;
        pop.property.insert(1, grain);
        let mut time = PopPRow::new(0.0);
        time.spent = 50.0;
        pop.property.insert(0, time);
        pop.day_received.insert(5, 3.0);
        pop.day_got.insert(2, 2.0);
        pop.day_gave.insert(7, 1.0);
        pop.day_paid.insert(5, 1.0);
        pop.day_haul.insert(0, 2.0);
        pop.day_eaten.insert(2, 2.0);
        let history = card(&[
            (0, 0.1, 1.0),
            (1, 2.0, 1.0),
            (2, 4.0, 1.0),
            (5, 1.0, 1.0),
            (7, 3.0, 0.5),
        ]);

        let (income, expenses, meal) = pop.day_books(&history);

        // Grain 8, gold taken 3, bread received 8. Time spent 5, grain rot 18,
        // wood at half salability 1.5, gold given 1, freight 0.2. Meal is the
        // bread eaten, 8.
        assert!((income - 19.0).abs() < 1e-9);
        assert!((expenses - 25.7).abs() < 1e-9);
        assert!((meal - 8.0).abs() < 1e-9);

        pop.reset_day();
        let (income, expenses, meal) = pop.day_books(&history);
        assert_eq!(income, 0.0);
        assert_eq!(expenses, 0.0);
        assert_eq!(meal, 0.0);
        assert_eq!(pop.property[&1].produced, 0.0);
        assert_eq!(pop.property[&1].lost, 0.0);
        assert_eq!(pop.property[&0].spent, 0.0);
        assert!(pop.day_haul.is_empty());
        assert!(pop.day_eaten.is_empty());
    }

    /// A desire that eats a good is the meal. A desire that only uses a good
    /// leaves that good out. Freight is not a meal.
    #[test]
    fn day_books_prices_the_meal_and_leaves_a_used_good() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(4.0));
        pop.property.insert(3, PopPRow::new(2.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.desires[1].push(desire(
            2,
            vec![DesireTarget::new(3, DesireTargetType::Use, 1.0)],
            2.0,
        ));

        pop.consume();

        assert!((pop.day_eaten[&1] - 4.0).abs() < 1e-9);
        assert!(pop.day_eaten.get(&3).is_none());
        assert!((pop.property[&3].used - 2.0).abs() < 1e-9);
        let history = card(&[(1, 2.0, 1.0), (3, 5.0, 1.0)]);
        let (income, expenses, meal) = pop.day_books(&history);
        assert_eq!(income, 0.0);
        assert_eq!(expenses, 0.0);
        assert!((meal - 8.0).abs() < 1e-9);
    }

    /// The morning grant is income. Time spent, and time later lost to rot,
    /// are expenses of that same grant, so a day that uses it up nets nothing.
    #[test]
    fn day_books_counts_the_morning_grant_against_later_time() {
        let mut pop = make_pop();
        pop.demographics.household = Household::with_members(1.0, 0.0, 0.0);
        let added = pop.start_day(&[(TIME, ScalingFactor::Labor(TIME_PER_LABOR))]);
        assert!((added[0].1 - TIME_PER_LABOR).abs() < 1e-12);

        // The market day clears yesterday after the grant. The grant stays.
        pop.reset_day();

        let history = card(&[(TIME, 0.1, 1.0)]);
        pop.property.get_mut(&TIME).unwrap().spent = 40.0;
        let (income, expenses, meal) = pop.day_books(&history);
        // 64 created at 0.1, and 40 of it spent. The 24 still on hand stay income.
        assert!((income - 6.4).abs() < 1e-9);
        assert!((expenses - 4.0).abs() < 1e-9);
        assert_eq!(meal, 0.0);

        pop.property.get_mut(&TIME).unwrap().lost = 24.0;
        let (income, expenses, meal) = pop.day_books(&history);
        assert!((income - 6.4).abs() < 1e-9);
        assert!((expenses - 6.4).abs() < 1e-9);
        assert_eq!(meal, 0.0);

        // A second morning replaces the grant. It does not add to the first.
        pop.start_day(&[(TIME, ScalingFactor::Labor(TIME_PER_LABOR))]);
        let (income, expenses, _) = pop.day_books(&history);
        assert!((income - 6.4).abs() < 1e-9);
        assert!((expenses - 6.4).abs() < 1e-9);
    }

    /// Freight units are an expense of the move, not an input a process destroyed.
    #[test]
    fn pay_freight_books_the_transport_it_takes() {
        let mut time_good = make_good(0, "time", 0.5);
        time_good.set_transport_efficiency(1.0);
        let factuals = Factuals::new().with_good(time_good);
        let mut buyer = make_pop();
        buyer.property.insert(0, PopPRow::new(10.0));

        buyer.pay_freight(4.0, &factuals);

        assert!((buyer.day_haul[&0] - 4.0).abs() < 1e-12);
        assert!((buyer.property[&0].consumed - 4.0).abs() < 1e-12);
        assert_eq!(buyer.property[&0].spent, 0.0);
        assert!(buyer.day_eaten.is_empty());
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
        common.effect.push(DesireEffect::Satisfaction(2.0, false));
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
        pop.stored_effects.push(PopEffect::Satisfaction {
            tier: 1,
            amount: 1.5,
        });
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
        basic.effect.push(DesireEffect::SentimentFlat(
            SentimentKind::Happiness,
            0.2,
            true,
        ));
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
        pop.stored_effects.push(PopEffect::SentimentFlat {
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
        common.effect.push(DesireEffect::Satisfaction(0.5, true));
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
        pop.stored_effects.push(PopEffect::Satisfaction {
            tier: 1,
            amount: 1.5,
        });
        pop.stored_effects.push(PopEffect::Birthrate(0.01));

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.contentment() - 1.03 / 1.03).abs() < 1e-9);
        assert_eq!(
            pop.stored_effects,
            vec![
                PopEffect::Satisfaction {
                    tier: 1,
                    amount: 1.5
                },
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
        assert!(
            pop.sol_samples
                .iter()
                .all(|sample| (sample - 1.0).abs() < 1e-9)
        );
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
        pop.stored_effects.push(PopEffect::Satisfaction {
            tier: 0,
            amount: 0.5,
        });

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.anger() - 0.02 / 1.04).abs() < 1e-9);
        assert!((pop.sentiment.fear() - 0.02 / 1.04).abs() < 1e-9);
        assert!((pop.sentiment.contentment() - 1.0 / 1.04).abs() < 1e-9);
        assert_eq!(
            pop.stored_effects,
            vec![PopEffect::Satisfaction {
                tier: 0,
                amount: 0.5
            }]
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
        pop.stored_effects.push(PopEffect::Satisfaction {
            tier: 0,
            amount: 1.5,
        });

        pop.update_sentiments(&MarketHistory::new());

        assert_eq!(pop.sentiment.contentment(), 1.0);
        assert_eq!(pop.sentiment.anger(), 0.0);
        assert_eq!(pop.sentiment.fear(), 0.0);
        assert_eq!(
            pop.stored_effects,
            vec![PopEffect::Satisfaction {
                tier: 0,
                amount: 1.5
            }]
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
        basic.effect.push(DesireEffect::Satisfaction(0.5, false));
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
        basic.effect.push(DesireEffect::Satisfaction(0.5, true));
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
        common.effect.push(DesireEffect::Satisfaction(0.5, true));
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
        luxury.effect.push(DesireEffect::Satisfaction(0.5, true));
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
        common.effect.push(DesireEffect::Satisfaction(0.5, false));
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
        pop.stored_effects.push(PopEffect::Satisfaction {
            tier: 1,
            amount: 1.5,
        });

        pop.update_sentiments(&MarketHistory::new());

        assert!((pop.sentiment.contentment() - 0.35 / 1.03).abs() < 1e-9);
        assert!((pop.sentiment.happiness() - 0.17 / 1.03).abs() < 1e-9);
        assert!((pop.sentiment.hope() - 0.17 / 1.03).abs() < 1e-9);
        assert!(pop.sol_samples.is_empty());
        assert_eq!(
            pop.stored_effects,
            vec![PopEffect::Satisfaction {
                tier: 1,
                amount: 1.5
            }]
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
    /// The currency is not on the card, so the AMV stays at 2.
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

        market.market_day(&mut actors, &factuals, &mut rng);

        let pop = actors.pop(1);
        assert_eq!(pop.desires[0][0].satisfaction, 0.0);
        assert_eq!(pop.property[&1].reserved, 0.0);
        assert!((pop.property[&1].quantity - 8.0).abs() < 1e-9);
        assert_eq!(market.goods[&1].decayed, 0.0);
        assert!((market.goods[&1].salability - 0.5).abs() < 1e-9);
        assert!((market.goods[&1].amv - 2.0).abs() < 1e-9);
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

    /// The buyer wants 4 of good 2 and pays with good 9, the tender. The
    /// seller holds 10 of good 2 and wants nothing. Neither good rots.
    ///
    /// Day 1 buys four bread and eats them. Day 2 buys four more and eats
    /// them. The seller keeps the two that were not bought.
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
        market.currencies = [Some(9), None, None, None];
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let factuals = Factuals::new()
            .with_good(make_good(2, "bread", 0.0))
            .with_good(make_good(9, "gold", 0.0));
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);
        assert!(
            market
                .mass_closes()
                .iter()
                .any(|close| close.good == 2 && close.exchanged >= 1.0)
        );
        assert!(actors.pop(1).desires[0][0].satisfaction > 0.0);

        market.market_day(&mut actors, &factuals, &mut rng);
        assert!(
            market
                .mass_closes()
                .iter()
                .any(|close| close.good == 2 && close.exchanged >= 1.0)
        );
        let buyer_bread = actors
            .pop(1)
            .property
            .get(&2)
            .map(|row| row.quantity)
            .unwrap_or(0.0);
        assert!(buyer_bread.abs() < 1e-9);
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

    /// The leaving share of today's books stays off the destination.
    #[test]
    fn take_migrants_shrinks_the_day_books() {
        let mut origin = make_pop();
        origin.demographics.household.count = 10.0;
        let mut grain = PopPRow::new(10.0);
        grain.produced = 10.0;
        grain.spent = 5.0;
        origin.property.insert(1, grain);
        origin.day_paid.insert(5, 10.0);
        origin.day_got.insert(2, 2.5);
        origin.day_eaten.insert(1, 10.0);
        origin.day_listed.insert(4, 10.0);
        origin.day_granted.insert(0, 10.0);
        let mut dest = make_pop();
        dest.id = 2;
        dest.demographics.household.count = 10.0;

        let group = origin.take_migrants(4.0);
        dest.receive_migrants(&group);

        assert!((origin.property[&1].produced - 8.0).abs() < 1e-9);
        assert!((origin.property[&1].spent - 4.0).abs() < 1e-9);
        assert!((origin.day_paid[&5] - 8.0).abs() < 1e-9);
        assert!((origin.day_got[&2] - 2.0).abs() < 1e-9);
        assert!((origin.day_eaten[&1] - 8.0).abs() < 1e-9);
        assert!((origin.day_listed[&4] - 8.0).abs() < 1e-9);
        assert!((origin.day_granted[&0] - 8.0).abs() < 1e-9);
        assert!(dest.day_paid.is_empty());
        assert!(dest.day_got.is_empty());
        assert!(dest.day_eaten.is_empty());
        assert!(dest.day_granted.is_empty());
        let arrived = dest.property.get(&1).expect("quantity traveled");
        assert!((arrived.quantity - 2.0).abs() < 1e-9);
        assert_eq!(arrived.produced, 0.0);
        assert_eq!(arrived.spent, 0.0);
    }

    /// Posts and trades stay on the day tape when households leave.
    ///
    /// Saved stock that does not travel is noted. The destination notes
    /// the property that arrives.
    #[test]
    fn day_tape_keeps_a_post_when_households_leave() {
        let mut origin = make_pop();
        origin.demographics.household.count = 10.0;
        origin.property.insert(1, PopPRow::new(10.0));
        origin.saved.insert(4, 10.0);
        origin.note_sell_offer(1, 5.0);
        origin.note_buy_offer(2, 3.0);
        origin.note_flow(super::DayFlow::Gave, 1, 4.0);
        origin.note_flow(super::DayFlow::Got, 2, 1.0);
        origin.stash_working(1, 2.0, super::SaveHold::Meal);
        origin.job.unlisted.insert(1, 3.0);

        let group = origin.take_migrants(4.0);
        let mut dest = make_pop();
        dest.id = 2;
        dest.demographics.household.count = 10.0;
        dest.receive_migrants(&group);

        let origin_tape = origin.day_tape();
        assert!((origin_tape.sell_offers[&1] - 5.0).abs() < 1e-9);
        assert!((origin_tape.buy_offers[&2] - 3.0).abs() < 1e-9);
        assert!((origin_tape.sold[&1] - 4.0).abs() < 1e-9);
        assert!((origin_tape.bought[&2] - 1.0).abs() < 1e-9);
        assert!((origin.day_gave[&1] - 3.2).abs() < 1e-9);
        assert!((origin_tape.meal[&1] - 2.0).abs() < 1e-9);
        assert!((origin_tape.unlisted[&1] - 3.0).abs() < 1e-9);
        assert!((origin_tape.left[&1] - 1.6).abs() < 1e-9);
        assert!((origin_tape.dropped[&4] - 2.0).abs() < 1e-9);
        assert!((origin_tape.dropped[&1] - 0.4).abs() < 1e-9);
        let dest_tape = dest.day_tape();
        assert!((dest_tape.arrived[&1] - 1.6).abs() < 1e-9);
        assert!(dest_tape.left.is_empty());
    }

    #[test]
    fn market_day_matches_desires_to_the_fitted_population() {
        let demo = DemoDesire::new(1)
            .with_tier(0)
            .with_scalar(ScalingFactor::Household(1.0))
            .with_amount(2.0);
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_desire(demo));
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
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_desire(demo));
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

    /// A basic day stays on bread. Grain is held and does not rot, and it is
    /// not a meal of this desire. Bread rots, and this pop does not make it,
    /// so the day stays bread.
    #[test]
    fn savings_keeps_a_rotting_day_the_pop_does_not_make() {
        let mut pop = make_pop();
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.property.insert(1, PopPRow::new(5.0));
        pop.property.insert(2, PopPRow::new(8.0));
        let factuals = Factuals::new()
            .with_good(make_good(2, "bread", 0.5))
            .with_good(make_good(1, "grain", 0.0));
        let history = card(&[(1, 1.0, 1.0), (2, 1.0, 1.0)]);

        pop.plan(&factuals, &history);

        assert_eq!(pop.savings, HashMap::from([(2, 8.0)]));
    }

    /// Bread rots and this pop bakes it. No tender is on hand, so the day
    /// is left for the morning make.
    #[test]
    fn savings_leaves_a_rotting_day_the_pop_makes() {
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

        assert!(pop.savings.is_empty());
    }

    /// Bread rots and this pop bakes it. Gold is the tender and does not
    /// rot. Nothing is piled until five gold, one outside day, is on hand.
    #[test]
    fn savings_holds_a_rotting_day_in_durable_money() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(7, Some(1.0), vec![])]);
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(2, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let factuals = Factuals::new()
            .with_good(make_good(2, "bread", 0.5))
            .with_good(make_good(9, "gold", 0.0))
            .with_process(bake());
        let mut history = card(&[(2, 1.0, 1.0), (9, 1.0, 1.0)]);
        history.currencies = vec![9];

        pop.plan(&factuals, &history);
        assert!(pop.savings.is_empty());

        pop.property.insert(9, PopPRow::new(5.0));
        pop.plan(&factuals, &history);
        assert_eq!(pop.savings, HashMap::from([(9, 5.0)]));
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

    /// Four households. Grain is the outside day of five. The first night
    /// saves two. The next night saves one more.
    #[test]
    fn savings_walks_one_household_toward_the_higher_rated_good() {
        let mut pop = make_pop();
        pop.demographics.household.count = 4.0;
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
        assert_eq!(pop.savings, HashMap::from([(1, 2.0)]));

        pop.plan(&factuals, &history);
        assert_eq!(pop.savings, HashMap::from([(1, 3.0)]));
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

    /// Luxury may leave the bucket. Gold that did not arrive rates 0, and
    /// bread rots and is made here, so nothing is piled. The same buy,
    /// filled, outrates bread.
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
        assert!(pop.savings.is_empty());

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

    /// Bread decays by half and this pop makes it, so the day is not piled.
    /// Three on hand is short of one day, and cloth stays closed. Four is
    /// that day, so cloth's hold of 2 is saved.
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
        assert!(pop.savings.is_empty());

        pop.property.insert(2, PopPRow::new(4.0));
        pop.plan(&factuals, &history);
        assert_eq!(pop.savings, HashMap::from([(4, 2.0)]));
    }

    #[test]
    fn revise_worth_follows_success_and_rot() {
        let mut history = MarketHistory::new();
        history.prices.insert(1, 4.0);

        let mut bought = make_pop();
        bought.property.insert(1, PopPRow::new(4.0));
        bought.buy_received.insert(1, 4.0);
        bought.record_keeping(&Factuals::new(), &history);
        assert!((bought.worth[&1] - 8.0).abs() < 1e-9);
        assert!((bought.buy_received[&1] - 4.0).abs() < 1e-9);
        bought.buy_received.insert(1, 100.0);
        bought.record_keeping(&Factuals::new(), &history);
        assert!((bought.worth[&1] - 8.0).abs() < 1e-9);

        let mut rotted = make_pop();
        let mut row = PopPRow::new(0.0);
        row.lost = 5.0;
        rotted.property.insert(1, row);
        rotted.record_keeping(&Factuals::new(), &history);
        assert!((rotted.worth[&1] - super::AMV_EPSILON).abs() < 1e-15);

        let mut made = make_pop();
        let mut pile = PopPRow::new(10.0);
        pile.produced = 2.0;
        made.property.insert(1, pile);
        made.record_keeping(&Factuals::new(), &history);
        assert!((made.worth[&1] - 4.8).abs() < 1e-9);

        let mut bare = make_pop();
        bare.property.insert(1, PopPRow::new(4.0));
        bare.buy_received.insert(1, 4.0);
        bare.record_keeping(&Factuals::new(), &MarketHistory::new());
        assert!(bare.worth.get(&1).is_none());
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
    fn consume_eats_further_luxury_when_an_earlier_day_was_short() {
        let mut pop = make_pop();
        pop.property.insert(8, PopPRow::new(12.0));
        pop.desires[2].push(desire(
            1,
            vec![DesireTarget::new(8, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        pop.savings.insert(8, 4.0);
        pop.luxury_savings.insert(8, 4.0);
        pop.luxury_savings_open = false;

        pop.consume();

        // Two levels, and the saved day stays. The earlier tier being short
        // does not bench the second level.
        assert!((pop.desires[2][0].satisfaction - 8.0).abs() < 1e-9);
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
                    .with_input(ProcessInput::new(
                        TIME,
                        10.0,
                        true,
                        InputType::Destroyed,
                        false,
                    ))
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
    /// not on a job that runs it. The meal stays posted once that grain is
    /// on hand. The farm keeps making the gap until then, then steps down.
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
                    .with_input(ProcessInput::new(
                        TIME,
                        10.0,
                        true,
                        InputType::Destroyed,
                        false,
                    ))
                    .with_output(ProcessOutput::new(1, 2.0, true)),
            )
            .with_process(
                Process::new(1, "make grain", 0)
                    .with_input(ProcessInput::new(
                        TIME,
                        0.5,
                        true,
                        InputType::Destroyed,
                        false,
                    ))
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

    /// Make grain is known, and the mass book offered grain. The farm is still
    /// short, and it walks a quarter off the meal. The buy stays at that meal.
    /// The best yield keeps its meal when the same book is offering.
    #[test]
    fn plan_walks_a_worse_meal_off_when_the_book_offers_it() {
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
                    .with_input(ProcessInput::new(
                        TIME,
                        10.0,
                        true,
                        InputType::Destroyed,
                        false,
                    ))
                    .with_output(ProcessOutput::new(1, 2.0, true)),
            )
            .with_process(
                Process::new(1, "make grain", 0)
                    .with_input(ProcessInput::new(
                        TIME,
                        0.5,
                        true,
                        InputType::Destroyed,
                        false,
                    ))
                    .with_output(ProcessOutput::new(1, 6.0, true)),
            );
        let mut history = card(&[(1, 1.0, 1.0)]);

        pop.plan(&factuals, &history);

        assert_eq!(pop.cheaper_cover, HashMap::from([(1, 4.0)]));
        let farm = pop
            .job
            .lines
            .iter()
            .find(|line| line.process == 29)
            .expect("farm line");
        assert!((farm.target.expect("farm target") - 2.5).abs() < 1e-9);

        history.mass_offered.insert(1, 1.0);
        pop.plan(&factuals, &history);

        assert_eq!(pop.cheaper_cover, HashMap::from([(1, 4.0)]));
        let farm = pop
            .job
            .lines
            .iter()
            .find(|line| line.process == 29)
            .expect("farm line");
        assert!((farm.target.expect("farm target") - 1.875).abs() < 1e-9);
    }

    /// Half a time makes six grain, and that costs more than the card. The
    /// book is offering grain. This line is the best yield, so the meal stays.
    #[test]
    fn plan_keeps_the_best_meal_when_the_book_offers_it() {
        let mut pop = make_pop();
        pop.job = Job::new(1, vec![JobLine::new(1, Some(0.0), vec![])]);
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            4.0,
        ));
        let factuals = Factuals::new()
            .with_good(make_good(TIME, "time", 1.0))
            .with_good(make_good(1, "grain", 0.0))
            .with_process(
                Process::new(1, "make grain", 0)
                    .with_input(ProcessInput::new(
                        TIME,
                        0.5,
                        true,
                        InputType::Destroyed,
                        false,
                    ))
                    .with_output(ProcessOutput::new(1, 6.0, true)),
            );
        let mut history = card(&[(1, 1.0, 1.0), (TIME, 100.0, 1.0)]);
        history.mass_offered.insert(1, 4.0);

        pop.plan(&factuals, &history);

        assert_eq!(pop.cheaper_cover, HashMap::from([(1, 4.0)]));
        let specialty = pop
            .job
            .lines
            .iter()
            .find(|line| line.process == 1)
            .expect("specialty line");
        assert!((specialty.target.expect("specialty target") - 5.0 / 6.0).abs() < 1e-9);
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
                    .with_input(ProcessInput::new(
                        TIME,
                        10.0,
                        true,
                        InputType::Destroyed,
                        false,
                    ))
                    .with_output(ProcessOutput::new(1, 2.0, true)),
            )
            .with_process(
                Process::new(1, "make grain", 0)
                    .with_input(ProcessInput::new(
                        TIME,
                        0.5,
                        true,
                        InputType::Destroyed,
                        false,
                    ))
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
                    .with_input(ProcessInput::new(
                        TIME,
                        10.0,
                        true,
                        InputType::Destroyed,
                        false,
                    ))
                    .with_output(ProcessOutput::new(1, 2.0, true)),
            )
            .with_process(
                Process::new(1, "make grain", 0)
                    .with_input(ProcessInput::new(
                        TIME,
                        0.5,
                        true,
                        InputType::Destroyed,
                        false,
                    ))
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

    /// Two households eat two leftover units. Each household gains the sink's
    /// efficiency once, so living rises by 0.01. One household eating one unit
    /// is the same rate.
    #[test]
    fn rest_adds_one_efficiency_per_unit_per_household() {
        let mut pop = make_pop();
        pop.demographics.household.count = 2.0;
        pop.property.insert(0, PopPRow::new(2.0));
        pop.desires[0].push(desire(1, vec![], 1.0));
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_sink(0, 0.01));

        pop.rest(&factuals);

        assert!(pop.property[&0].quantity.abs() < 1e-12);
        assert!((pop.property[&0].consumed - 2.0).abs() < 1e-12);
        assert!((pop.rest_taken[&0] - 2.0).abs() < 1e-12);
        assert!((pop.standard_of_living() - 0.01).abs() < 1e-12);
    }

    /// One household, one free unit above a reserve and a saved unit, adds 0.01.
    /// The reserve and the saved unit stay.
    #[test]
    fn rest_leaves_reserved_stock_and_the_saved_pile() {
        let mut pop = make_pop();
        pop.property.insert(0, PopPRow::new(3.0).with_reserve(1.0));
        pop.savings.insert(0, 1.0);
        pop.desires[0].push(desire(1, vec![], 1.0));
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_sink(0, 0.01));

        pop.rest(&factuals);

        assert!((pop.property[&0].quantity - 2.0).abs() < 1e-12);
        assert!((pop.property[&0].reserved - 1.0).abs() < 1e-12);
        assert!((pop.rest_taken[&0] - 1.0).abs() < 1e-12);
        assert!((pop.standard_of_living() - 0.01).abs() < 1e-12);
    }

    /// The desire ate the only unit. Rest does not take it, and living stays
    /// that one level.
    #[test]
    fn rest_does_not_eat_a_unit_the_desire_took() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(1.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Consume, 1.0)],
            1.0,
        ));
        let factuals = Factuals::new().with_species(Species::new(0, "human").with_sink(1, 0.05));

        pop.consume();
        pop.rest(&factuals);

        assert!(pop.property[&1].quantity.abs() < 1e-12);
        assert!(pop.rest_taken.get(&1).copied().unwrap_or(0.0).abs() < 1e-12);
        assert!((pop.standard_of_living() - 1.0).abs() < 1e-12);
    }

    /// A species that was not loaded leaves the stock where it is.
    #[test]
    fn rest_with_no_species_eats_nothing() {
        let mut pop = make_pop();
        pop.property.insert(0, PopPRow::new(1.0));
        pop.desires[0].push(desire(1, vec![], 1.0));

        pop.rest(&Factuals::new());

        assert!((pop.property[&0].quantity - 1.0).abs() < 1e-12);
        assert_eq!(pop.rest_sol, 0.0);
    }

    /// A used good is recorded apart from the meal.
    #[test]
    fn consume_records_a_used_good_apart_from_the_meal() {
        let mut pop = make_pop();
        pop.property.insert(1, PopPRow::new(1.0));
        pop.desires[0].push(desire(
            1,
            vec![DesireTarget::new(1, DesireTargetType::Use, 1.0)],
            1.0,
        ));

        pop.consume();

        assert!((pop.day_held[&1] - 1.0).abs() < 1e-9);
        assert!(pop.day_eaten.get(&1).copied().unwrap_or(0.0).abs() < 1e-12);
    }
}
