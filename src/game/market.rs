#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use rand::RngCore;

use crate::game::firm::Firm;
use crate::game::institution::Institution;
use crate::game::map::Map;
use crate::game::players::Players;
use crate::game::pop::{Pop, SellOffer};
use crate::game::process::InputType;
use crate::game::{config::MarketConfig, factuals::Factuals};

/// How far a no-trade price sits above the dearest method's unit cost.
const IDLE_COST_MARGIN: f64 = 0.10;

/// Share of a traded move held by the currencies that did not trade, together.
///
/// A sole trader keeps `1 - IDLE_MOVE_SHARE`. With more currencies, each idle
/// currency's floor is this divided by one less than the count.
const IDLE_MOVE_SHARE: f64 = 0.05;

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
    /// Is it (Currency, Good) access.
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
    /// Saved exchange rates. Each entry is one currency and one good.
    pub exchange_rates: Vec<ExchangeRate>,
    /// Goods a buyer asked that nobody could cover for one unit.
    ///
    /// [`Market::history`] copies the market's set. Empty when every ask
    /// could buy, or when the snapshot was taken after the set was cleared.
    pub unaffordable: HashSet<usize>,
}

/// One saved exchange rate.
///
/// `rate` is the units of `good` one unit of `currency` buys.
#[derive(Debug, Clone, Copy)]
pub struct ExchangeRate {
    /// Tender good this rate is priced in.
    pub currency: usize,
    /// Good this rate buys.
    pub good: usize,
    /// Units of `good` bought by one unit of `currency`.
    pub rate: f64,
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
    /// How much of each currency was used for this good today.
    ///
    /// Keyed by tender id. Empty when no currency was counted. The price
    /// step turns a count into weight with the morning rate.
    pub currency_counts: HashMap<usize, f64>,
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

    /// # Currencies
    ///
    /// Tender good ids on this market, in list order.
    pub fn currencies(&self) -> &[usize] {
        &self.currencies
    }

    /// # Market Day
    ///
    /// Runs this market's day.
    ///
    /// Pops run subsistence, then sort property into reserves and sell
    /// offers. Then early player, institutions, firms, pops, and late
    /// player trade. Trade consolidation follows. Prices move in
    /// record keeping, after the rest of the day.
    ///
    /// `factuals` supplies the world. `history` is the snapshot the offer
    /// split reads. `pops` and `firms` are the members handed to this
    /// market, walked in the order given. `institutions` is every
    /// institution. This market calls the ones in `institution_ids`, in id
    /// order. `players` buy first and again last. `rng` is this market's
    /// draws. The offers, buys, and trade records are not written yet.
    pub fn market_day(
        &mut self,
        factuals: &Factuals,
        history: &MarketHistory,
        pops: &mut [Pop],
        firms: &mut [Firm],
        institutions: &HashMap<usize, Institution>,
        players: &Players,
        rng: &mut dyn RngCore,
    ) {
        // Each pop runs subsistence before it sorts reserves and offers.
        for pop in pops.iter_mut() {
            pop.run_subsistence(factuals);
        }
        // Each pop sorts its property into reserves and sell offers.
        let mut offers = Vec::new();
        for pop in pops.iter_mut() {
            let pop_offers = pop.split_goods_sale_reserve(history, factuals);
            offers.extend(pop_offers);
        }
        // Buys run after every offer is in. Draws sit unused until a buy needs one.
        let _ = rng;
        players.early_market_day(self, factuals, &offers);
        // Institutions listed on this market, lowest id first.
        let mut institution_ids: Vec<usize> = self.institution_ids.iter().copied().collect();
        institution_ids.sort_unstable();
        for id in institution_ids {
            let institution = institutions
                .get(&id)
                .unwrap_or_else(|| panic!("institution {id} is not in Actors"));
            institution.market_day(self, factuals, &offers);
        }
        // Firms, in the order they were handed in.
        for firm in firms.iter_mut() {
            firm.market_day(self, factuals, &offers);
        }
        // Pops, in the order they were handed in.
        for pop in pops.iter_mut() {
            pop.market_day(self, factuals, &offers);
        }
        // Players who buy after the other actors.
        players.late_market_day(self, factuals, &offers);
        // Record the trades and consolidate property.
        self.consolidate(factuals, pops, firms, &offers);
    }

    /// # Consolidate
    ///
    /// Records the day's trades and consolidates property.
    ///
    /// `factuals` supplies the world. `pops` and `firms` are the members
    /// that traded. `offers` are the sell offers from the start of the day.
    /// The records are not written yet.
    fn consolidate(
        &mut self,
        factuals: &Factuals,
        pops: &mut [Pop],
        firms: &mut [Firm],
        offers: &[SellOffer],
    ) {
        let _ = (self, factuals, pops, firms, offers);
    }

    /// # Record Keeping
    ///
    /// Rolls member output, consumption, and rot into this market's goods.
    ///
    /// `pops` and `firms` are the world's actors. `pop_home` and `firm_home`
    /// map an actor to the market that counts it. A member whose home is not
    /// this market is left out. Production, consumption, and decay are
    /// replaced by today's sums. Stock, offers, and prices are left to their
    /// own writers. [`Self::move_prices`] runs after the rollup.
    pub fn record_keeping(
        &mut self,
        factuals: &Factuals,
        pops: &HashMap<usize, Pop>,
        firms: &HashMap<usize, Firm>,
        pop_home: &HashMap<usize, usize>,
        firm_home: &HashMap<usize, usize>,
    ) {
        for row in self.goods.values_mut() {
            row.production = 0.0;
            row.consumption = 0.0;
            row.decayed = 0.0;
        }
        for &id in &self.pops {
            if pop_home.get(&id).copied() != Some(self.id) {
                continue;
            }
            let pop = pops
                .get(&id)
                .unwrap_or_else(|| panic!("pop {id} is not in the record set"));
            for (&good_id, row) in &pop.property {
                debug_assert!(row.produced.is_finite() && row.produced >= 0.0);
                debug_assert!(row.consumed.is_finite() && row.consumed >= 0.0);
                debug_assert!(row.spent.is_finite() && row.spent >= 0.0);
                debug_assert!(row.lost.is_finite() && row.lost >= 0.0);
                let market_row = self.goods.entry(good_id).or_default();
                market_row.production += row.produced;
                market_row.consumption += row.consumed + row.spent;
                market_row.decayed += row.lost;
            }
        }
        for &id in &self.firms {
            if firm_home.get(&id).copied() != Some(self.id) {
                continue;
            }
            let firm = firms
                .get(&id)
                .unwrap_or_else(|| panic!("firm {id} is not in the record set"));
            for (&good_id, row) in &firm.property {
                debug_assert!(row.produced.is_finite() && row.produced >= 0.0);
                debug_assert!(row.consumed.is_finite() && row.consumed >= 0.0);
                let market_row = self.goods.entry(good_id).or_default();
                market_row.production += row.produced;
                market_row.consumption += row.consumed;
            }
        }
        self.move_prices(factuals);
    }

    /// # Move Prices
    ///
    /// Moves rates one good at a time.
    ///
    /// `factuals` supplies the processes and `market.price_gain` and
    /// `market.price_cap`. A good with volume above 0 is priced by
    /// [`traded_price`]. A good with volume 0 takes [`dearest_unit_cost`]
    /// times `1 + IDLE_COST_MARGIN`. The stored rate is the inverse of that
    /// price. Rates are read before any of them are written. Counts are left
    /// in place.
    fn move_prices(&mut self, factuals: &Factuals) {
        let gain = factuals.config.market.price_gain;
        let cap = factuals.config.market.price_cap;
        debug_assert!(gain.is_finite() && gain > 0.0);
        debug_assert!(cap.is_finite() && cap > 0.0 && cap <= 1.0);
        // Input costs use these rates so one good's new price does not feed the next.
        let rates = self.exchange_rates.clone();
        let goods = goods_to_price(self);
        for &good in &goods {
            let Some((sought, offered, volume, counts)) = self.good_ledger(good) else {
                continue;
            };
            debug_assert!(sought.is_finite() && sought >= 0.0);
            debug_assert!(offered.is_finite() && offered >= 0.0);
            debug_assert!(volume.is_finite() && volume >= 0.0);
            let currencies = currencies_for_good(&self.currencies, &rates, &counts, good, volume);
            let mut priced = Vec::new();
            if volume == 0.0 {
                for &currency in &currencies {
                    let Some(cost) = dearest_unit_cost(factuals, &rates, currency, good) else {
                        continue;
                    };
                    priced.push((currency, cost * (1.0 + IDLE_COST_MARGIN)));
                }
            } else {
                let mut quotes = Vec::with_capacity(currencies.len());
                let mut total = 0.0;
                for &currency in &currencies {
                    let count = counts.get(&currency).copied().unwrap_or(0.0);
                    debug_assert!(count.is_finite() && count >= 0.0);
                    let rate = morning_rate(&rates, currency, good);
                    let weight = count * rate;
                    total += weight;
                    quotes.push((currency, 1.0 / rate, weight));
                }
                let n = quotes.len();
                for (currency, price, weight) in quotes {
                    let share = currency_share(n, total, weight);
                    priced.push((
                        currency,
                        traded_price(price, sought, offered, share, gain, cap),
                    ));
                }
            }
            for (currency, price) in priced {
                debug_assert!(price.is_finite() && price > 0.0);
                self.exchange_rates
                    .entry(currency)
                    .or_default()
                    .insert(good, 1.0 / price);
            }
        }
    }

    /// # Good Ledger
    ///
    /// Sought, offered, volume, and currency counts for `good`.
    ///
    /// Returns `None` when the market has no row for that good. Counts are
    /// copied out so the rate write can follow.
    fn good_ledger(&self, good: usize) -> Option<(f64, f64, f64, HashMap<usize, f64>)> {
        let row = self.goods.get(&good)?;
        Some((
            row.sought,
            row.offered,
            row.volume,
            row.currency_counts.clone(),
        ))
    }

    /// # Rebalance Capped
    ///
    /// Scales capped pops so their people sum to the market ceiling.
    ///
    /// `pops` are the world's pops. A pop with no [`Pop::population_cap`]
    /// is left out. The ceiling is the sum of the caps that remain, in
    /// people. When the people in those pops are above that sum, every
    /// capped household count is multiplied by the same factor. A slower
    /// pop then holds a smaller share than a faster one. A total at or
    /// under the ceiling is left as it is.
    pub fn rebalance_capped(&self, pops: &mut HashMap<usize, Pop>) {
        let mut ceiling = 0.0;
        let mut people = 0.0;
        let mut capped = Vec::new();
        for &id in &self.pops {
            let pop = pops
                .get(&id)
                .unwrap_or_else(|| panic!("pop {id} is not in the rebalance set"));
            let Some(cap) = pop.population_cap else {
                continue;
            };
            debug_assert!(
                cap.is_finite() && cap >= 0.0,
                "population cap must be finite and >= 0, got {cap}"
            );
            let headcount = pop.demographics.total_population();
            debug_assert!(headcount.is_finite() && headcount >= 0.0);
            ceiling += cap;
            people += headcount;
            capped.push(id);
        }
        if people <= ceiling {
            return;
        }
        let factor = ceiling / people;
        for id in capped {
            let pop = pops
                .get_mut(&id)
                .unwrap_or_else(|| panic!("pop {id} is not in the rebalance set"));
            pop.demographics.household.count *= factor;
        }
    }

    /// # Prune Under One
    ///
    /// Unlists member pops whose household count is under 1.
    ///
    /// `pops` are the world's pops. Each removal is
    /// [`PopExit::UnderOneHousehold`]. The pop stays in `pops` so a later
    /// market can record the same id. The caller drops the actor after
    /// every market has recorded.
    pub fn prune_under_one(&mut self, pops: &HashMap<usize, Pop>) {
        let gone: Vec<usize> = self
            .pops
            .iter()
            .copied()
            .filter(|&id| {
                pops.get(&id)
                    .is_some_and(|pop| pop.demographics.household.count < 1.0)
            })
            .collect();
        for id in gone {
            self.pops.remove(&id);
            self.pop_removals.push(PopRemoval {
                id,
                why: PopExit::UnderOneHousehold,
            });
        }
    }

    /// # Internal Migration
    ///
    /// Moves households inside this market.
    ///
    /// `pops` are the world's pops. `map` is the hex grid. `factuals`
    /// supplies the world. Member [`Pop::migration`] values are cleared
    /// first. Local migration is the same hex. Regional migration is
    /// between hexes and runs after local.
    pub fn internal_migration(
        &mut self, pops: &mut HashMap<usize, Pop>, map: &Map, factuals: &Factuals,
    ) {
        for &id in &self.pops {
            let pop = pops
                .get_mut(&id)
                .unwrap_or_else(|| panic!("pop {id} is not in the migration set"));
            pop.migration = 0.0;
        }
        self.local_migration(pops, map, factuals);
        // Between hexes of this market. Not written in this version.
        self.regional_migration(pops, map, factuals);
    }

    /// # Local Migration
    ///
    /// Moves households between pops on the same hex.
    ///
    /// `pops`, `map`, and `factuals` are the same inputs as
    /// [`Self::internal_migration`]. The hexes are
    /// [`Map::region_hexes`] for this market's id. The same-hex offer
    /// is not written yet.
    fn local_migration(
        &mut self, pops: &mut HashMap<usize, Pop>, map: &Map, factuals: &Factuals,
    ) {
        // Same-hex offers use the partnered region's hexes.
        let _hexes = map.region_hexes(self.id);
        let _ = (self, pops, factuals, _hexes);
    }

    /// # Regional Migration
    ///
    /// Moves households between hexes in this market.
    ///
    /// `pops`, `map`, and `factuals` are the same inputs as
    /// [`Self::internal_migration`]. Called after [`Self::local_migration`].
    /// Not written in this version.
    fn regional_migration(
        &mut self, pops: &mut HashMap<usize, Pop>, map: &Map, factuals: &Factuals,
    ) {
        let _ = (self, pops, map, factuals);
    }
    
    /// # Record Supply
    ///
    /// Adds `amount` to the market's supply of `good_id`.
    pub(crate) fn record_supply(&mut self, good_id: usize, amount: f64) {
        let row = self
            .goods
            .entry(good_id)
            .or_insert(MarketGood::new());
        row.stock += amount;
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
            exchange_rates: Vec::new(),
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

/// # Goods To Price
///
/// Goods this pass reprices.
///
/// A good already rated in some currency comes first. A good with a currency
/// count, or with volume above 0, follows. A good is listed once. Returns
/// that list.
fn goods_to_price(market: &Market) -> Vec<usize> {
    let mut goods = Vec::new();
    let mut push = |id: usize| {
        if !goods.contains(&id) {
            goods.push(id);
        }
    };
    for rates in market.exchange_rates.values() {
        for &good in rates.keys() {
            push(good);
        }
    }
    for (&good, row) in &market.goods {
        if !row.currency_counts.is_empty() || row.volume > 0.0 {
            push(good);
        }
    }
    goods
}

/// # Currencies For Good
///
/// Tender ids that take a price for `good`.
///
/// On a day that traded, the market's currency list comes first. A currency
/// that already rates the good, or that has a count, follows. The good's own
/// id is left out. A currency is listed once. Returns that list.
fn currencies_for_good(
    market_currencies: &[usize],
    rates: &HashMap<usize, HashMap<usize, f64>>,
    counts: &HashMap<usize, f64>,
    good: usize, volume: f64,
) -> Vec<usize> {
    let mut currencies = Vec::new();
    let mut push = |id: usize| {
        if id != good && !currencies.contains(&id) {
            currencies.push(id);
        }
    };
    if volume > 0.0 {
        for &id in market_currencies {
            push(id);
        }
    }
    for (&currency, row) in rates {
        if row.contains_key(&good) {
            push(currency);
        }
    }
    for &id in counts.keys() {
        push(id);
    }
    currencies
}

/// # Currency Share
///
/// The fraction of this good's price move that one currency absorbs.
///
/// `n` is how many currencies are priced for the good. `total` is the sum of
/// their weights, and `mine` is this currency's weight. One currency takes
/// the whole move. With more, the share is a floor of [`IDLE_MOVE_SHARE`]
/// spread across the others, plus the rest by weight. Equal weights stay
/// equal. A sole trader keeps `1 - IDLE_MOVE_SHARE`. A zero total splits the
/// move evenly. Returns that fraction.
fn currency_share(n: usize, total: f64, mine: f64) -> f64 {
    debug_assert!(n > 0);
    debug_assert!(total.is_finite() && total >= 0.0);
    debug_assert!(mine.is_finite() && mine >= 0.0);
    if n == 1 {
        return 1.0;
    }
    let strict = if total == 0.0 { 1.0 / n as f64 } else { mine / total };
    let floor = IDLE_MOVE_SHARE / (n - 1) as f64;
    let share = floor + (1.0 - n as f64 * floor) * strict;
    debug_assert!(share.is_finite() && (0.0..=1.0).contains(&share));
    share
}

/// # Morning Rate
///
/// Goods of `good` one unit of `currency` bought before this price pass.
///
/// `rates` are that snapshot. A missing rate is one good per currency unit.
/// Returns that rate.
fn morning_rate(
    rates: &HashMap<usize, HashMap<usize, f64>>,
    currency: usize, good: usize,
) -> f64 {
    match rates.get(&currency).and_then(|row| row.get(&good)).copied() {
        Some(rate) => {
            debug_assert!(rate.is_finite() && rate > 0.0, "exchange rate must be > 0");
            rate
        }
        None => 1.0,
    }
}

/// # Currency Price
///
/// What one unit of `good` costs in `currency`.
///
/// `rates` are goods per currency unit, from before this price pass. The
/// currency itself costs 1. Any other good costs one over its [`morning_rate`].
/// Returns that cost.
fn currency_price(
    rates: &HashMap<usize, HashMap<usize, f64>>,
    currency: usize,
    good: usize,
) -> f64 {
    if good == currency {
        return 1.0;
    }
    1.0 / morning_rate(rates, currency, good)
}

/// # Dearest Unit Cost
///
/// The highest input cost of one unit of `good` in `currency`.
///
/// Each process that outputs `good` costs the inputs that are not optional
/// and not factors, at [`currency_price`], divided by how much of `good` one
/// iteration makes. Other outputs are not credited. A method with no such
/// cost is left out. Returns that cost, or `None` when every method is free
/// or none make the good.
fn dearest_unit_cost(
    factuals: &Factuals,
    rates: &HashMap<usize, HashMap<usize, f64>>,
    currency: usize,
    good: usize,
) -> Option<f64> {
    let mut dearest = None;
    for process in factuals.each_process() {
        let made: f64 = process
            .outputs
            .iter()
            .filter(|row| row.good == good)
            .map(|row| row.amount)
            .sum();
        if made <= 0.0 {
            continue;
        }
        let mut cost = 0.0;
        for input in &process.inputs {
            if input.is_optional() || matches!(input.input_type, InputType::Factor) {
                continue;
            }
            debug_assert!(input.amount.is_finite() && input.amount > 0.0);
            cost += input.amount * currency_price(rates, currency, input.good);
        }
        if cost <= 0.0 {
            continue;
        }
        let unit = cost / made;
        debug_assert!(unit.is_finite() && unit > 0.0);
        dearest = Some(dearest.map_or(unit, |best: f64| best.max(unit)));
    }
    dearest
}

/// # Traded Price
///
/// The next price of a good that traded.
///
/// `price` is currency units per good before this step. `sought` and
/// `offered` are the good's own ledger. `share` is [`currency_share`]. The
/// price is multiplied by
/// `1 + share * gain * clamp((sought - offered) / max(sought, offered), cap, -cap)`.
/// A ledger with both sides at 0 leaves the price where it is. Returns
/// currency units per good.
fn traded_price(
    price: f64, sought: f64, offered: f64, share: f64, gain: f64, cap: f64,
) -> f64 {
    debug_assert!(price.is_finite() && price > 0.0);
    debug_assert!(share.is_finite() && (0.0..=1.0).contains(&share));
    debug_assert!(sought.is_finite() && sought >= 0.0);
    debug_assert!(offered.is_finite() && offered >= 0.0);
    let scale = sought.max(offered);
    let step = if scale == 0.0 {
        0.0
    } else {
        ((sought - offered) / scale).clamp(-cap, cap)
    };
    let next = price * (1.0 + share * gain * step);
    debug_assert!(next.is_finite() && next > 0.0);
    next
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
    /// start at 0. Currency counts start empty.
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
            currency_counts: HashMap::new(),
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

#[cfg(test)]
mod growth_should {
    use std::collections::HashMap;

    use super::{Market, PopExit};
    use crate::game::factuals::Factuals;
    use crate::game::household::Household;
    use crate::game::map::Map;
    use crate::game::pop::Pop;

    fn member(id: usize, count: f64, cap: Option<f64>) -> Pop {
        let mut pop = Pop::new(id);
        pop.demographics.household = Household::with_count(count);
        pop.population_cap = cap;
        pop
    }

    #[test]
    fn over_the_ceiling_scales_capped_pops_by_one_factor() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.pops.insert(3);
        let mut pops = HashMap::from([
            (1, member(1, 12.0, Some(30.0))),
            (2, member(2, 8.0, Some(50.0))),
            (3, member(3, 4.0, None)),
        ]);
        market.rebalance_capped(&mut pops);
        assert!((pops[&1].demographics.household.count - 9.6).abs() < 1e-12);
        assert!((pops[&2].demographics.household.count - 6.4).abs() < 1e-12);
        assert!((pops[&3].demographics.household.count - 4.0).abs() < 1e-12);
        let people = pops[&1].demographics.total_population() + pops[&2].demographics.total_population();
        assert!((people - 80.0).abs() < 1e-9);
    }

    #[test]
    fn under_the_ceiling_leaves_counts_alone() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        let mut pops = HashMap::from([(1, member(1, 2.0, Some(40.0)))]);
        market.rebalance_capped(&mut pops);
        assert_eq!(pops[&1].demographics.household.count, 2.0);
    }

    #[test]
    fn under_one_household_is_recorded_and_unlisted() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        let pops = HashMap::from([
            (1, member(1, 0.4, None)),
            (2, member(2, 1.0, None)),
        ]);
        market.prune_under_one(&pops);
        assert!(!market.pops.contains(&1));
        assert!(market.pops.contains(&2));
        assert_eq!(market.pop_removals().len(), 1);
        assert_eq!(market.pop_removals()[0].id, 1);
        assert_eq!(market.pop_removals()[0].why, PopExit::UnderOneHousehold);
        assert!(pops.contains_key(&1));
    }

    #[test]
    fn a_shared_pop_is_recorded_on_each_market() {
        let mut first = Market::new(1);
        let mut second = Market::new(2);
        first.pops.insert(1);
        second.pops.insert(1);
        let pops = HashMap::from([(1, member(1, 0.4, None))]);
        first.prune_under_one(&pops);
        second.prune_under_one(&pops);
        assert!(first.pops.is_empty());
        assert!(second.pops.is_empty());
        assert_eq!(first.pop_removals()[0].why, PopExit::UnderOneHousehold);
        assert_eq!(second.pop_removals()[0].why, PopExit::UnderOneHousehold);
    }

    #[test]
    fn internal_migration_clears_household_movement() {
        let mut market = Market::new(1);
        market.pops.insert(7);
        let mut pop = Pop::new(7);
        pop.migration = 3.0;
        let mut pops = HashMap::from([(7, pop)]);
        market.internal_migration(&mut pops, &Map::new(1, 1), &Factuals::new());
        assert_eq!(pops[&7].migration, 0.0);
    }
}

#[cfg(test)]
mod record_keeping_should {
    use std::collections::HashMap;

    use hexx::Hex;

    use super::Market;
    use crate::game::factuals::Factuals;
    use crate::game::firm::{Firm, FirmPRow};
    use crate::game::pop::Pop;
    use crate::game::pop_property::PopPRow;

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn member_flows_replace_the_day_and_skip_another_markets_actors() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.firms.insert(3);
        market.firms.insert(4);
        market.goods.insert(
            1,
            super::MarketGood {
                salability: 0.25,
                availability: 0.0,
                offered: 6.0,
                sought: 0.0,
                production: 99.0,
                consumption: 99.0,
                stock: 4.0,
                volume: 2.0,
                decayed: 99.0,
                currency_counts: HashMap::new(),
            },
        );
        market.goods.insert(9, super::MarketGood {
            production: 3.0,
            stock: 2.0,
            ..super::MarketGood::new()
        });
        market.exchange_rates.insert(1, HashMap::from([(1, 2.0)]));

        let mut pop = Pop::new(1);
        pop.property.insert(
            1,
            PopPRow::new(7.0)
                .with_produced(4.0)
                .with_consumed(1.0)
                .with_spent(2.0)
                .with_lost(0.5),
        );
        pop.property.insert(2, PopPRow::new(5.0).with_produced(5.0));
        let mut other = Pop::new(2);
        other.property.insert(1, PopPRow::new(1.0).with_produced(9.0));
        let mut firm = Firm::new(3, "shop".into(), 1, Hex::new(0, 0));
        firm.property.insert(
            1,
            FirmPRow {
                produced: 8.0,
                consumed: 1.5,
                quantity: 3.0,
                ..FirmPRow::default()
            },
        );
        let mut elsewhere = Firm::new(4, "other".into(), 5, Hex::new(1, 0));
        elsewhere.property.insert(
            1,
            FirmPRow {
                produced: 100.0,
                consumed: 100.0,
                ..FirmPRow::default()
            },
        );
        let pops = HashMap::from([(1, pop), (2, other)]);
        let firms = HashMap::from([(3, firm), (4, elsewhere)]);
        let pop_home = HashMap::from([(1, 1), (2, 2)]);
        let firm_home = HashMap::from([(3, 1), (4, 5)]);
        market.record_keeping(&Factuals::new(), &pops, &firms, &pop_home, &firm_home);

        let good = &market.goods[&1];
        near(good.production, 12.0);
        near(good.consumption, 4.5);
        near(good.decayed, 0.5);
        near(good.stock, 4.0);
        near(good.offered, 6.0);
        near(good.volume, 2.0);
        near(good.salability, 0.25);
        near(market.goods[&2].production, 5.0);
        near(market.goods[&2].consumption, 0.0);
        near(market.goods[&9].production, 0.0);
        near(market.goods[&9].stock, 2.0);
        assert_eq!(market.exchange_rates[&1][&1], 2.0);
    }
}

#[cfg(test)]
mod move_prices_should {
    use std::collections::HashMap;

    use super::Market;
    use crate::game::factuals::Factuals;
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    fn method(id: usize, inputs: Vec<ProcessInput>, output: usize, amount: f64) -> Process {
        let mut process = Process::new(id, format!("p{id}"), 0);
        for input in inputs {
            process = process.with_input(input);
        }
        process.with_output(ProcessOutput::new(output, amount, false))
    }

    fn input(good: usize, amount: f64, optional: bool) -> ProcessInput {
        ProcessInput::new(good, amount, false, InputType::Destroyed, optional)
    }

    #[test]
    fn a_shortage_is_split_by_currency_weight() {
        let mut market = Market::new(1);
        market.currencies = vec![1, 2];
        market.exchange_rates.insert(1, HashMap::from([(5, 2.0)]));
        market.exchange_rates.insert(2, HashMap::from([(5, 2.0)]));
        market.goods.insert(
            5,
            super::MarketGood {
                stock: 3.0,
                sought: 8.0,
                offered: 4.0,
                volume: 1.0,
                currency_counts: HashMap::from([(1, 10.0), (2, 10.0)]),
                ..super::MarketGood::new()
            },
        );
        market.move_prices(&Factuals::new());

        // Equal weights take half of the clamped step.
        near(market.exchange_rates[&1][&5], 1.0 / 0.505);
        near(market.exchange_rates[&2][&5], 1.0 / 0.505);

        let mut fresh = Market::new(2);
        fresh.goods.insert(
            6,
            super::MarketGood {
                sought: 8.0,
                offered: 4.0,
                volume: 1.0,
                currency_counts: HashMap::from([(4, 3.0)]),
                ..super::MarketGood::new()
            },
        );
        fresh.move_prices(&Factuals::new());
        // A missing rate starts at 1, and the only currency takes the whole step.
        near(fresh.exchange_rates[&4][&6], 1.0 / 1.02);
        near(market.goods[&5].stock, 3.0);
        near(market.goods[&5].offered, 4.0);
        assert_eq!(market.goods[&5].currency_counts[&1], 10.0);
    }

    #[test]
    fn an_idle_currency_absorbs_five_percent() {
        let mut market = Market::new(1);
        market.currencies = vec![1, 2];
        market.exchange_rates.insert(1, HashMap::from([(5, 2.0)]));
        market.exchange_rates.insert(2, HashMap::from([(5, 2.0)]));
        market.goods.insert(
            5,
            super::MarketGood {
                sought: 8.0,
                offered: 0.0,
                volume: 1.0,
                currency_counts: HashMap::from([(1, 10.0)]),
                ..super::MarketGood::new()
            },
        );
        market.move_prices(&Factuals::new());
        near(market.exchange_rates[&1][&5], 1.0 / 0.5095);
        near(market.exchange_rates[&2][&5], 1.0 / 0.5005);
    }

    #[test]
    fn a_small_gap_uses_the_configured_gain() {
        let mut market = Market::new(1);
        market.currencies = vec![1];
        market.exchange_rates.insert(1, HashMap::from([(5, 2.0)]));
        market.goods.insert(
            5,
            super::MarketGood {
                sought: 10.0,
                offered: 9.0,
                volume: 1.0,
                currency_counts: HashMap::from([(1, 4.0)]),
                ..super::MarketGood::new()
            },
        );
        let mut factuals = Factuals::new();
        factuals.config.market.price_gain = 0.05;
        market.move_prices(&factuals);
        near(market.exchange_rates[&1][&5], 1.0 / 0.5025);
    }

    #[test]
    fn no_trade_takes_the_dearest_cost_plus_ten_percent() {
        let mut market = Market::new(1);
        market.currencies = vec![9];
        market.exchange_rates.insert(9, HashMap::from([(1, 2.0), (8, 3.0), (9, 4.0)]));
        market.goods.insert(
            2,
            super::MarketGood {
                sought: 5.0,
                offered: 1.0,
                currency_counts: HashMap::from([(9, 0.0)]),
                ..super::MarketGood::new()
            },
        );
        market.goods.insert(7, super::MarketGood::new());
        market.goods.insert(8, super::MarketGood::new());
        market.goods.insert(9, super::MarketGood::new());
        let mut factuals = Factuals::new();
        // 4 inputs at 0.5 is a unit cost of 2. The optional pile is not in the cost.
        factuals.processes.insert(1, method(1, vec![input(1, 4.0, false)], 2, 1.0));
        factuals.processes.insert(
            2,
            method(2, vec![input(1, 1.0, false), input(3, 100.0, true)], 2, 1.0),
        );
        factuals.processes.insert(3, method(3, vec![], 7, 1.0));
        factuals.processes.insert(4, method(4, vec![input(1, 1.0, false)], 6, 1.0));
        factuals.processes.insert(5, method(5, vec![input(1, 1.0, false)], 9, 1.0));
        market.move_prices(&factuals);

        near(market.exchange_rates[&9][&2], 1.0 / 2.2);
        near(market.exchange_rates[&9][&1], 2.0);
        near(market.exchange_rates[&9][&8], 3.0);
        near(market.exchange_rates[&9][&9], 4.0);
        assert!(market.exchange_rates[&9].get(&6).is_none());
        assert!(market.exchange_rates[&9].get(&7).is_none());
        assert_eq!(market.goods[&2].currency_counts[&9], 0.0);
    }
}
