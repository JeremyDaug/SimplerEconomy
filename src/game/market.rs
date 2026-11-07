#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::config::MarketConfig;

/// Why a pop was removed today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopExit {
    /// Household count was under 1 after growth and the population cap.
    UnderOneHousehold,
    /// No running line and no whole coin. `to` received the households.
    NoWorkNoCoin { to: Vec<usize> },
    /// The move would have left fewer than one household, so everyone went.
    /// `to` received the households.
    UnderOneMove { to: Vec<usize> },
    /// The same demographic and the same process set. `to` is the earlier pop.
    SameJob { to: Vec<usize> },
}

/// A pop removed today.
///
/// `id` is the pop. `why` is the rule that removed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopRemoval {
    pub id: usize,
    pub why: PopExit,
}

/// A local market. Member ids point at actors. Goods hold the stored price.
#[derive(Debug, Clone)]
pub struct Market {
    /// Unique id. Matches the region this market represents, when it has one.
    pub id: usize,
    /// Pop ids present here.
    pub pops: HashSet<usize>,
    /// Firm ids present here.
    pub firms: HashSet<usize>,
    /// Institution ids present here. An institution may sit in several markets.
    pub institution_ids: HashSet<usize>,
    /// Per-good price and quantity record. Keyed by good id.
    pub goods: HashMap<usize, MarketGood>,
    /// The exchange rates from each market currency to all goods in the market.
    /// 
    /// Measured in 1 unit of good per unit of currency.
    pub exchange_rates: HashMap<usize, HashMap<usize, f64>>,
    /// Distance / size multiplier on deal bulk. 0 on a one-hex market.
    pub friction: f64,
    /// Tender goods. Can be any number of goods up to the number of goods marked as 'currency'.
    pub currencies: Vec<usize>,
    /// Market days left before another household may leave to found.
    founding_rest: u32,
    /// Open market days since that wait last closed, with nobody leaving.
    ///
    /// The first day the wait is open is 0. [`Self::found_pop`] adds one
    /// when that day ends and nobody left. A founding sets it back to 0.
    founding_open: u32,
    /// Pops removed today. [`Self::reset_day`] clears them.
    pop_removals: Vec<PopRemoval>,
}

/// A saved price snapshot for one market.
#[derive(Debug, Clone)]
pub struct MarketHistory {
    /// Last known AMV per good.
    pub prices: HashMap<usize, f64>,
    /// Last known salability per good.
    pub salability: HashMap<usize, f64>,
    /// Units the night plan still treats as wanted.
    ///
    /// [`Market::history`] fills this. Empty on a snapshot taken before exchange.
    pub sought: HashMap<usize, f64>,
    /// Units produced today, per good.
    ///
    /// Copied from [`MarketGood::production`]. Empty when nothing was made.
    pub produced: HashMap<usize, f64>,
    /// Whole units offered on today's mass book.
    ///
    /// [`Market::history`] copies [`MassClose::offered`]. Empty when the book
    /// quoted no sell. [`Self::is_mass_supplied`] is what the night plan reads.
    pub mass_offered: HashMap<usize, f64>,
    /// Whole units exchanged on today's mass book.
    ///
    /// [`Market::history`] copies [`MassClose::exchanged`].
    pub mass_exchanged: HashMap<usize, f64>,
    /// Copied from [`Market::friction`].
    pub friction: f64,
    /// Used when a good has no recorded salability.
    pub default_salability: f64,
    /// Refused-ask fraction per good, copied from the market.
    pub ask_walk: HashMap<usize, f64>,
    /// Tender good ids, filled slots only, in slot order.
    ///
    /// [`Market::history`] copies them. Empty when the snapshot has no tender.
    pub currencies: Vec<usize>,
    /// Goods a buyer asked that nobody could cover for one unit.
    ///
    /// [`Market::history`] copies the market's set. Empty when every ask
    /// could buy, or when the snapshot was taken after the set was cleared.
    pub unaffordable: HashSet<usize>,
}

/// Per-market price snapshots plus pop-id to market-id.
#[derive(Debug, Clone, Default)]
pub struct MarketLookups {
    pub histories: HashMap<usize, MarketHistory>,
    pub pop_to_market: HashMap<usize, usize>,
}

/// Stored price and quantity for one good in a market.
#[derive(Debug, Clone)]
pub struct MarketGood {
    /// Percent share of the good that successfully sold on the market.
    pub salability: f64,
    /// Percent share of goods that were purchased on the market.
    pub availability: f64,

    // Market Records
    /// Units offered in trade today.
    pub offered: f64,
    /// Units sought today.
    pub sought: f64,
    /// Units made today.
    pub production: f64,
    /// Units consumed today, both processes and desires.
    pub consumption: f64,
    /// Units that survived from yesterday.
    pub stock: f64,
    /// Units of this good exchanged today.
    pub volume: f64,
    /// Units lost to rot today.
    pub decayed: f64,
}

/// One founding. `Join` sends the household to a pop that already has the set.
enum FoundingChoice {
    Create(Vec<usize>),
    Join { dest: usize, processes: Vec<usize> },
}

/// One change applied to the founder's lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FoundingEdit {
    /// Add this process.
    Add(usize),
    /// Remove this process.
    Drop(usize),
}

/// The four demographic ids and the process set on one pop.
///
/// Process ids are sorted and are not repeated. The household shape is not
/// part of the key. [`job_key`] builds it.
struct JobKey {
    species: usize,
    culture: usize,
    stratum: usize,
    religion: usize,
    processes: Vec<usize>,
}

impl Market {
    /// # New
    ///
    /// Empty market with this id.
    ///
    /// `id` is the market id. The tender list is [`CURRENCY_SLOTS`] long.
    /// The first slot holds [`OPENING_CURRENCY`]. The other three are empty.
    /// Returns that market.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            pops: HashSet::new(),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods: HashMap::new(),
            exchange_rates: HashMap::new(),
            friction: 0.0,
            currencies: Vec::new(),
            founding_rest: 0,
            founding_open: 0,
            pop_removals: Vec::new(),
        }
    }

    /// # With Friction
    ///
    /// Sets the market friction factor. Must be `>= 0.0`.
    pub fn with_friction(mut self, friction: f64) -> Self {
        debug_assert!(friction >= 0.0, "friction must be >= 0.0");
        self.friction = friction;
        self
    }

    /// # Pop Removals
    ///
    /// Pops removed today, in the order they were recorded.
    pub fn pop_removals(&self) -> &[PopRemoval] {
        &self.pop_removals
    }
}

impl MarketHistory {
    /// # New
    ///
    /// An empty price snapshot using [`MarketConfig::default`].
    pub fn new() -> Self {
        Self::from_config(&MarketConfig::default())
    }

    /// # From Config
    ///
    /// An empty price snapshot.
    ///
    /// Salability falls back to `market.salability_default`. The quantity
    /// maps start empty.
    pub fn from_config(market: &MarketConfig) -> Self {
        Self {
            prices: HashMap::new(),
            salability: HashMap::new(),
            sought: HashMap::new(),
            produced: HashMap::new(),
            mass_offered: HashMap::new(),
            mass_exchanged: HashMap::new(),
            friction: 0.0,
            default_salability: market.salability_default,
            ask_walk: HashMap::new(),
            currencies: Vec::new(),
            unaffordable: HashSet::new(),
        }
    }

    /// # Is Unaffordable
    ///
    /// True when a buyer asked for `good_id` and nobody could cover one unit.
    ///
    /// A good missing from the set is false.
    pub fn is_unaffordable(&self, good_id: usize) -> bool {
        self.unaffordable.contains(&good_id)
    }

    /// # Currencies
    ///
    /// Tender good ids on this snapshot, in slot order.
    pub fn currencies(&self) -> &[usize] {
        &self.currencies
    }

    /// # Ask Walk
    ///
    /// How far `good_id`'s refused sell floor has walked toward the card.
    ///
    /// A good with no stored fraction returns 0.
    pub fn ask_walk(&self, good_id: usize) -> f64 {
        self.ask_walk.get(&good_id).copied().unwrap_or(0.0)
    }

    /// # Sought
    ///
    /// Units of `good_id` still sought on this snapshot, or 0.
    pub fn sought(&self, good_id: usize) -> f64 {
        self.sought.get(&good_id).copied().unwrap_or(0.0)
    }

    /// # Produced
    ///
    /// Units of `good_id` produced on this snapshot, or 0.
    pub fn produced(&self, good_id: usize) -> f64 {
        self.produced.get(&good_id).copied().unwrap_or(0.0)
    }

    /// # Mass Offered
    ///
    /// Whole units of `good_id` offered on this snapshot's mass book, or 0.
    pub fn mass_offered(&self, good_id: usize) -> f64 {
        self.mass_offered.get(&good_id).copied().unwrap_or(0.0)
    }

    /// # Mass Exchanged
    ///
    /// Whole units of `good_id` exchanged on this snapshot's mass book, or 0.
    pub fn mass_exchanged(&self, good_id: usize) -> f64 {
        self.mass_exchanged.get(&good_id).copied().unwrap_or(0.0)
    }

    /// # Is Mass Supplied
    ///
    /// True when the mass book offered or exchanged at least one unit of `good_id`.
    ///
    /// A good missing from both maps is false. A fraction under one is false.
    pub fn is_mass_supplied(&self, good_id: usize) -> bool {
        self.mass_offered.get(&good_id).copied().unwrap_or(0.0) >= 1.0
            || self.mass_exchanged.get(&good_id).copied().unwrap_or(0.0) >= 1.0
    }

    /// # Price
    ///
    /// AMV for `good_id`, or 1.0 if this snapshot has none.
    pub fn price(&self, good_id: usize) -> f64 {
        self.prices.get(&good_id).copied().unwrap_or(1.0)
    }

    /// # Salability
    ///
    /// Salability for `good_id`, or this snapshot's default if missing.
    pub fn salability(&self, good_id: usize) -> f64 {
        self.salability
            .get(&good_id)
            .copied()
            .unwrap_or(self.default_salability)
    }
}

impl MarketLookups {
    /// # New
    ///
    /// Empty histories and an empty pop-to-market map.
    pub fn new() -> Self {
        Self::default()
    }

    /// # History For Pop
    ///
    /// History for `pop_id`'s market, or `empty` if the pop is in none.
    pub fn history_for_pop<'a>(
        &'a self,
        pop_id: usize,
        empty: &'a MarketHistory,
    ) -> &'a MarketHistory {
        self.pop_to_market
            .get(&pop_id)
            .and_then(|mid| self.histories.get(mid))
            .unwrap_or(empty)
    }
}

/// # Bounce Away From Zero
///
/// If `new` is inside `(-epsilon, epsilon)`, return `epsilon` with the
/// opposite sign from `old`. Otherwise return `new`.
///
/// `old` is the previous AMV. `new` is the candidate. `epsilon` is
/// [`MarketConfig::amv_epsilon`].
fn bounce_away_from_zero(old: f64, new: f64, epsilon: f64) -> f64 {
    debug_assert!(new.is_finite(), "new AMV must be finite");
    debug_assert!(epsilon > 0.0 && epsilon.is_finite(), "epsilon must be > 0");
    if new.abs() >= epsilon {
        new
    } else if old >= 0.0 {
        -epsilon
    } else {
        epsilon
    }
}

impl Default for MarketGood {
    /// # Default
    ///
    /// Salability starts at the market config default. The quantity ledgers
    /// start at 0.
    fn default() -> Self {
        Self {
            salability: MarketConfig::default().salability_default,
            production: 0.0,
            consumption: 0.0,
            stock: 0.0,
            decayed: 0.0,
            volume: 0.0,
            availability: 0.0,
            offered: 0.0,
            sought: 0.0,
        }
    }
}

impl MarketGood {
    /// # New
    ///
    /// Same value as [`Self::default`].
    pub fn new() -> Self {
        Self::default()
    }
    
    /// # With Salability
    ///
    /// [`Self::set_salability`] as a builder.
    pub fn with_salability(mut self, salability: f64) -> Self {
        self.salability = salability;
        self
    }

    /// # Set Production
    ///
    /// Stores `production` on this good.
    ///
    /// `production` must be `>= 0`.
    pub fn set_production(&mut self, production: f64) {
        debug_assert!(production >= 0.0, "production must be >= 0");
        self.production = production;
    }

    /// # Set Consumption
    ///
    /// Stores `consumption` on this good.
    ///
    /// `consumption` must be `>= 0`.
    pub fn set_consumption(&mut self, consumption: f64) {
        debug_assert!(consumption >= 0.0, "consumption must be >= 0");
        self.consumption = consumption;
    }

    /// # Set Stock
    ///
    /// Stores `stock` on this good.
    ///
    /// `stock` must be `>= 0`.
    pub fn set_stock(&mut self, stock: f64) {
        debug_assert!(stock >= 0.0, "stock must be >= 0");
        self.stock = stock;
    }
}
