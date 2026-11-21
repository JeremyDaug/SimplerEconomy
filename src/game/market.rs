#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use rand::RngCore;

use crate::game::firm::Firm;
use crate::game::good::TIME;
use crate::game::institution::Institution;
use crate::game::map::Map;
use crate::game::players::Players;
use crate::game::pop::{buy_from_offers, Pop, SellOffer, ShopBook};
use crate::game::process::InputType;
use crate::game::config::{IDLE_COST_MARGIN, IDLE_MOVE_SHARE, MIN_PRICE, MarketConfig};
use crate::game::factuals::Factuals;

mod founding;

pub use founding::{FoundingDay, FoundingEvent, FoundingOutlook, FoundingProspect};

/// # Posted Offers
///
/// Units of each good in `offers`.
///
/// Amounts of the same good add. A non-positive amount is left out.
/// Returns those totals.
fn posted_offers(offers: &[SellOffer]) -> HashMap<usize, f64> {
    let mut totals = HashMap::new();
    for offer in offers {
        if offer.amount > 0.0 {
            *totals.entry(offer.good).or_insert(0.0) += offer.amount;
        }
    }
    totals
}

/// # Member Homes
///
/// Maps each member id to the lowest market id that lists it.
///
/// `markets` is every market, keyed by id. `members` reads one market's
/// member set, such as [`Market::pops`] or [`Market::firms`]. A member listed
/// on several markets maps to the lowest id. A member listed on none is
/// absent. Returns that map.
pub fn member_homes(
    markets: &HashMap<usize, Market>,
    members: impl Fn(&Market) -> &HashSet<usize>,
) -> HashMap<usize, usize> {
    let mut homes = HashMap::new();
    let mut ids: Vec<usize> = markets.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        for &member in members(&markets[&id]) {
            homes.entry(member).or_insert(id);
        }
    }
    homes
}

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
    /// Market days left before another pop may be created.
    ///
    /// A join does not start this wait. [`Self::found_pop`] counts it down.
    founding_rest: u32,
    /// Open market days since the last pop was created.
    ///
    /// A new pop sets this back to 0. [`founding::attempt_chance`] reads it.
    founding_open: u32,
    /// What [`Self::found_pop`] did the last time it ran.
    founding_day: Option<FoundingDay>,
    /// Pops removed today. [`Self::reset_day`] clears them.
    pop_removals: Vec<PopRemoval>,
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
            founding_day: None,
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

    /// # Stored Rate
    ///
    /// Goods of `good` one unit of `currency` buys, when a rate is stored.
    ///
    /// A stored rate must be finite and above 0. Returns that rate, or
    /// `None` when `currency` stores no rate for `good`.
    pub fn stored_rate(&self, currency: usize, good: usize) -> Option<f64> {
        let rate = self
            .exchange_rates
            .get(&currency)
            .and_then(|row| row.get(&good))
            .copied()?;
        debug_assert!(rate.is_finite() && rate > 0.0, "exchange rate must be > 0");
        Some(rate)
    }

    /// # Price In
    ///
    /// What one unit of `good` costs in `currency`.
    ///
    /// The currency itself costs 1. Any other good costs one over its
    /// [`Self::stored_rate`]. A missing rate costs 1. Returns that cost.
    pub fn price_in(&self, currency: usize, good: usize) -> f64 {
        if good == currency {
            return 1.0;
        }
        self.stored_rate(currency, good).map_or(1.0, |rate| 1.0 / rate)
    }

    /// # Price
    ///
    /// What one unit of `good` costs in this market's first tender.
    ///
    /// The cost is [`Self::price_in`] that tender. A market with no tender
    /// prices every good at 1. Returns that cost.
    pub fn price(&self, good: usize) -> f64 {
        match self.currencies.first() {
            Some(&currency) => self.price_in(currency, good),
            None => 1.0,
        }
    }

    /// # Market Day
    ///
    /// Runs this market's day.
    ///
    /// Pops clear yesterday's sales, run subsistence, then sort property
    /// into reserves and sell offers. Then early player, institutions,
    /// firms, pops, and late player trade. Trade consolidation follows.
    /// Prices move in record keeping, after the rest of the day.
    ///
    /// `factuals` supplies the world. `pops` and `firms` are the members
    /// handed to this market. Firms walk in the order given. Pops shop
    /// through [`buy_from_offers`], wealthiest household first.
    /// `institutions` is every institution. This market calls the ones in
    /// `institution_ids`, in id order. `players` buy first and again last. `rng` picks the
    /// sellers. Offers, bids, and completed exchanges are written after
    /// the pops shop.
    pub fn market_day(
        &mut self,
        factuals: &Factuals,
        pops: &mut [Pop],
        firms: &mut [Firm],
        institutions: &HashMap<usize, Institution>,
        players: &Players,
        rng: &mut dyn RngCore,
    ) {
        // Yesterday's sales have already been planned. Clear them before the new day.
        for pop in pops.iter_mut() {
            pop.clear_sold();
            pop.run_subsistence(factuals);
        }
        // Each pop sorts its property into reserves and sell offers.
        let mut offers = Vec::new();
        for pop in pops.iter_mut() {
            let pop_offers = pop.split_goods_sale_reserve(factuals);
            offers.extend(pop_offers);
        }
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
        // Pops shop the offers. A buy takes another pop's goods.
        // The book is the offers as posted. Later takes reduce what is left.
        let offered = posted_offers(&offers);
        let book = buy_from_offers(pops, self, factuals, &mut offers, rng);
        // Players who buy after the other actors.
        players.late_market_day(self, factuals, &offers);
        // Write the offers, the bids, and the completed exchanges.
        self.consolidate(&offered, &book);
    }

    /// # Consolidate
    ///
    /// Writes today's offers, bids, and exchanges onto this market.
    ///
    /// `offered` is the units posted for sale, by good. `book` is the units
    /// bid, the units exchanged, and the tender spent on each good.
    /// Yesterday's offers, bids, volume, and currency counts are replaced.
    /// A good with posted supply replaces its salability with the average of
    /// that share and today's volume over the posted supply. A good with no
    /// posted supply keeps its salability.
    fn consolidate(&mut self, offered: &HashMap<usize, f64>, book: &ShopBook) {
        for row in self.goods.values_mut() {
            row.offered = 0.0;
            row.sought = 0.0;
            row.volume = 0.0;
            row.currency_counts.clear();
        }
        for (&good, &amount) in offered {
            if amount > 0.0 {
                self.goods.entry(good).or_default().offered = amount;
            }
        }
        for (&good, &amount) in &book.sought {
            if amount > 0.0 {
                self.goods.entry(good).or_default().sought = amount;
            }
        }
        for (&good, &amount) in &book.volume {
            if amount > 0.0 {
                self.goods.entry(good).or_default().volume = amount;
            }
        }
        for (&good, tenders) in &book.paid {
            let row = self.goods.entry(good).or_default();
            for (&tender, &amount) in tenders {
                if amount > 0.0 {
                    row.currency_counts.insert(tender, amount);
                }
            }
        }
        // Posted supply averages the stored share with today's sell-through.
        for row in self.goods.values_mut() {
            if row.offered > 0.0 {
                row.salability = (row.salability + row.volume / row.offered) / 2.0;
                debug_assert!(row.salability.is_finite());
            }
        }
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
    /// [`traded_price`]. A good with volume 0 and a stored rate keeps its
    /// morning price unless [`raised_idle_price`] lifts it. A good with
    /// posted supply or demand and no rate opens at [`opening_price`]. A
    /// price this pass writes is at least [`MIN_PRICE`]. The stored rate is
    /// the inverse of that price. Every price is read before any rate is
    /// written, so one good's new price does not feed the next. Counts are
    /// left in place.
    fn move_prices(&mut self, factuals: &Factuals) {
        let gain = factuals.config.market.price_gain;
        let cap = factuals.config.market.price_cap;
        debug_assert!(gain.is_finite() && gain > 0.0);
        debug_assert!(cap.is_finite() && cap > 0.0 && cap <= 1.0);
        let mut priced = Vec::new();
        for good in self.goods_to_price() {
            let Some(row) = self.goods.get(&good) else {
                continue;
            };
            debug_assert!(row.sought.is_finite() && row.sought >= 0.0);
            debug_assert!(row.offered.is_finite() && row.offered >= 0.0);
            debug_assert!(row.volume.is_finite() && row.volume >= 0.0);
            let currencies = self.currencies_for_good(good, row);
            if row.volume == 0.0 {
                for &currency in &currencies {
                    let cost = self.dearest_unit_cost(factuals, currency, good);
                    // A stored price only rises. Supply or demand with no rate opens one.
                    let price = match self.stored_rate(currency, good) {
                        Some(rate) => raised_idle_price(1.0 / rate, cost),
                        None if row.is_active() => Some(opening_price(cost)),
                        None => None,
                    };
                    if let Some(price) = price {
                        priced.push((currency, good, price));
                    }
                }
            } else {
                let mut quotes = Vec::with_capacity(currencies.len());
                let mut total = 0.0;
                for &currency in &currencies {
                    let count = row.currency_counts.get(&currency).copied().unwrap_or(0.0);
                    debug_assert!(count.is_finite() && count >= 0.0);
                    let rate = self.stored_rate(currency, good).unwrap_or(1.0);
                    let weight = count * rate;
                    total += weight;
                    quotes.push((currency, 1.0 / rate, weight));
                }
                let n = quotes.len();
                for (currency, price, weight) in quotes {
                    let share = currency_share(n, total, weight);
                    priced.push((
                        currency,
                        good,
                        traded_price(price, row.sought, row.offered, share, gain, cap),
                    ));
                }
            }
        }
        for (currency, good, price) in priced {
            let price = price.max(MIN_PRICE);
            debug_assert!(price.is_finite() && price >= MIN_PRICE);
            self.exchange_rates
                .entry(currency)
                .or_default()
                .insert(good, 1.0 / price);
        }
    }

    /// # Goods To Price
    ///
    /// Goods [`Self::move_prices`] reprices.
    ///
    /// A good already rated in some currency comes first. A good with a
    /// currency count, or that [`MarketGood::is_active`], follows. Time is
    /// left out. A good is listed once. Returns that list.
    fn goods_to_price(&self) -> Vec<usize> {
        let mut goods = Vec::new();
        let mut push = |id: usize| {
            if id != TIME && !goods.contains(&id) {
                goods.push(id);
            }
        };
        for rates in self.exchange_rates.values() {
            for &good in rates.keys() {
                push(good);
            }
        }
        for (&good, row) in &self.goods {
            if !row.currency_counts.is_empty() || row.is_active() {
                push(good);
            }
        }
        goods
    }

    /// # Currencies For Good
    ///
    /// Tender ids that take a price for `good`.
    ///
    /// `row` is that good's record. When it [`MarketGood::is_active`], the
    /// market's currency list comes first. A currency that already rates the
    /// good, or that has a count on `row`, follows. The good's own id is left
    /// out. A currency is listed once. Returns that list.
    fn currencies_for_good(&self, good: usize, row: &MarketGood) -> Vec<usize> {
        let mut currencies = Vec::new();
        let mut push = |id: usize| {
            if id != good && !currencies.contains(&id) {
                currencies.push(id);
            }
        };
        if row.is_active() {
            for &id in &self.currencies {
                push(id);
            }
        }
        for (&currency, rates) in &self.exchange_rates {
            if rates.contains_key(&good) {
                push(currency);
            }
        }
        for &id in row.currency_counts.keys() {
            push(id);
        }
        currencies
    }

    /// # Dearest Unit Cost
    ///
    /// The highest input cost of one unit of `good` in `currency`.
    ///
    /// Each process that outputs `good` costs the inputs that are not
    /// optional and not factors, at [`Self::price_in`], divided by how much
    /// of `good` one iteration makes. Other outputs are not credited. A
    /// method with no such cost is left out. Returns that cost, or `None`
    /// when every method is free or none make the good.
    fn dearest_unit_cost(&self, factuals: &Factuals, currency: usize, good: usize) -> Option<f64> {
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
                cost += input.amount * self.price_in(currency, input.good);
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
    /// first, with the arrived and left household counts. Local migration
    /// runs, then regional. Regional is between hexes and is not written yet.
    pub fn internal_migration(
        &mut self, pops: &mut HashMap<usize, Pop>, map: &Map, factuals: &Factuals,
    ) {
        for &id in &self.pops {
            // An earlier market may already have dropped this pop.
            let Some(pop) = pops.get_mut(&id) else {
                continue;
            };
            pop.migration = 0.0;
            pop.records.migration_in = 0.0;
            pop.records.migration_out = 0.0;
        }
        self.local_migration(pops, map, factuals);
        // Between hexes of this market. Not written in this version.
        self.regional_migration(pops, map, factuals);
    }

    /// # Local Migration
    ///
    /// Moves households between the pops of this market.
    ///
    /// `pops`, `map`, and `factuals` are the same inputs as
    /// [`Self::internal_migration`]. Each member's offer is
    /// [`Pop::migration_households`]. A different species is matched on
    /// its own, so it does not move across species. Pops are not on a hex
    /// yet, so [`Map::region_hexes`] is not a split. Every member is one pool.
    fn local_migration(
        &mut self, pops: &mut HashMap<usize, Pop>, map: &Map, factuals: &Factuals,
    ) {
        // The partnered hexes stay on the map. A pop has no hex, so they
        // do not group this pool.
        let _hexes = map.region_hexes(self.id);
        // Households weight the mean. A large pop is more of the market's living.
        let mean = weighted_migration_sol(pops, &self.pops);
        let anger = factuals.config.market.migration_anger_weight;
        let pace = factuals.config.market.migration_pace;
        let mut ids: Vec<usize> = self.pops.iter().copied().collect();
        ids.sort_unstable();
        let mut pushers: HashMap<usize, Vec<MigrationSide>> = HashMap::new();
        let mut pullers: HashMap<usize, Vec<MigrationSide>> = HashMap::new();
        let mut counts = HashMap::new();
        let mut rooms = HashMap::new();
        for id in ids {
            // Already dropped.
            let Some(pop) = pops.get(&id) else {
                continue;
            };
            let count = pop.demographics.household.count;
            if count < 1.0 {
                continue;
            }
            counts.insert(id, count);
            let offer = pop.migration_households(mean, anger, pace);
            let side = MigrationSide {
                id,
                culture: pop.demographics.culture,
                households: offer.abs(),
            };
            // Negative offers leave. Positive offers have room. Zero stays.
            if offer < 0.0 {
                pushers.entry(pop.demographics.species).or_default().push(side);
            } else if offer > 0.0 {
                rooms.insert(id, offer);
                pullers.entry(pop.demographics.species).or_default().push(side);
            }
        }
        let mut species: Vec<usize> = pushers
            .keys()
            .copied()
            .filter(|id| pullers.contains_key(id))
            .collect();
        species.sort_unstable();
        let mut flows = Vec::new();
        for species_id in species {
            flows.extend(plan_species_flows(
                &pushers[&species_id],
                &pullers[&species_id],
                factuals,
            ));
        }
        let evacuate = keep_last_household(&counts, &rooms, &mut flows);
        self.apply_migrant_flows(pops, &flows, &evacuate);
    }

    /// # Apply Migrant Flows
    ///
    /// Moves the planned households and the goods that go with them.
    ///
    /// `flows` are the households leaving each origin. `evacuate` is the
    /// origins whose whole pop leaves. A partial move carries half of that
    /// destination's share of property and savings. The other half stays
    /// with the origin, so the move does not destroy goods. A whole pop
    /// has nobody left to keep that half, so its goods are split by the
    /// households each destination receives. The destination's age mix
    /// absorbs the arrivals. Those households are recorded as arrivals on
    /// the destination and departures on the origin. An emptied pop is
    /// unlisted and dropped.
    fn apply_migrant_flows(
        &mut self,
        pops: &mut HashMap<usize, Pop>,
        flows: &[MigrantFlow],
        evacuate: &HashSet<usize>,
    ) {
        let mut origins: Vec<usize> = flows.iter().map(|flow| flow.from).collect();
        origins.sort_unstable();
        origins.dedup();
        for from in origins {
            let leaving: Vec<MigrantFlow> = flows
                .iter()
                .copied()
                .filter(|flow| flow.from == from && flow.households > 1e-12)
                .collect();
            if leaving.is_empty() {
                continue;
            }
            let origin_count = pops
                .get(&from)
                .unwrap_or_else(|| panic!("pop {from} is not in the migration set"))
                .demographics
                .household
                .count;
            if origin_count <= 0.0 {
                continue;
            }
            let snapshot = pops[&from].demographics.household;
            let (stock, saved) = pops[&from].belonging_snapshot();
            let sent: f64 = leaving.iter().map(|flow| flow.households).sum();
            let empty = evacuate.contains(&from);
            let mut origin = pops
                .remove(&from)
                .unwrap_or_else(|| panic!("pop {from} is not in the migration set"));
            let mut went = Vec::new();
            for flow in &leaving {
                // A full departure splits every good. A partial one sends
                // half of this destination's share and leaves the rest.
                let fraction = if empty {
                    flow.households / sent
                } else {
                    0.5 * flow.households / origin_count
                };
                let dest = pops.get_mut(&flow.to).unwrap_or_else(|| {
                    panic!("pop {} is not in the migration set", flow.to)
                });
                origin.shift_belongings_to(dest, fraction, &stock, &saved);
                dest.demographics.household.absorb(&snapshot, flow.households);
                dest.migration += flow.households;
                dest.records.migration_in += flow.households;
                origin.demographics.household.count -= flow.households;
                origin.migration -= flow.households;
                origin.records.migration_out += flow.households;
                went.push(flow.to);
            }
            if empty {
                went.sort_unstable();
                went.dedup();
                self.pops.remove(&from);
                self.pop_removals.push(PopRemoval {
                    id: from,
                    why: PopExit::UnderOneMove { to: went },
                });
            } else {
                // The sweep keeps one household. Dust under that line would
                // be pruned later and the people would vanish.
                if origin.demographics.household.count < 1.0 {
                    debug_assert!(
                        origin.demographics.household.count > 1.0 - 1e-6,
                        "migration left pop {from} under one household"
                    );
                    origin.demographics.household.count = 1.0;
                }
                pops.insert(from, origin);
            }
        }
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

/// One side of a migration offer. `households` is a positive count.
#[derive(Clone, Copy)]
struct MigrationSide {
    id: usize,
    culture: usize,
    households: f64,
}

/// Households leaving one pop for another.
#[derive(Clone, Copy)]
struct MigrantFlow {
    from: usize,
    to: usize,
    households: f64,
}

/// # Weighted Migration Sol
///
/// The household-weighted standard of living of `members`.
///
/// `pops` are the world's pops. A pop under one household is skipped.
/// A pop with no finite living is skipped, so an infinite reading cannot
/// pull the mean. No remaining weight returns `None`.
fn weighted_migration_sol(
    pops: &HashMap<usize, Pop>,
    members: &HashSet<usize>,
) -> Option<f64> {
    let mut weight = 0.0;
    let mut sum = 0.0;
    for &id in members {
        // Already dropped.
        let Some(pop) = pops.get(&id) else {
            continue;
        };
        let households = pop.demographics.household.count;
        if households < 1.0 {
            continue;
        }
        let Some(living) = pop.migration_living() else {
            continue;
        };
        sum += living * households;
        weight += households;
    }
    if weight > 0.0 {
        Some(sum / weight)
    } else {
        None
    }
}

/// # Culture Entry Cap
///
/// The share of an origin's departure that may enter a destination.
///
/// The same culture is fully open. Assimilation is the ease of leaving a
/// culture and of others entering it, and this move does not leave it.
/// Two cultures use the average of their assimilation. Each stored value
/// is clamped to `0..=1` here.
fn culture_entry_cap(factuals: &Factuals, from: usize, to: usize) -> f64 {
    if from == to {
        return 1.0;
    }
    let leave = factuals.culture_assimilation(from).clamp(0.0, 1.0);
    let enter = factuals.culture_assimilation(to).clamp(0.0, 1.0);
    if !leave.is_finite() || !enter.is_finite() {
        return 0.0;
    }
    (leave + enter) * 0.5
}

/// # Plan Species Flows
///
/// Households moving from `pushers` to `pullers` of one species.
///
/// A destination's weight is the origin's send times [`culture_entry_cap`].
/// It accepts no more than its room, split by those weights. An origin
/// whose destinations would take more than it offered is scaled back.
/// Households the cap does not place stay home.
fn plan_species_flows(
    pushers: &[MigrationSide],
    pullers: &[MigrationSide],
    factuals: &Factuals,
) -> Vec<MigrantFlow> {
    let mut pushers = pushers.to_vec();
    let mut pullers = pullers.to_vec();
    pushers.sort_by_key(|side| side.id);
    pullers.sort_by_key(|side| side.id);
    let mut flow = vec![vec![0.0; pullers.len()]; pushers.len()];
    for (j, pull) in pullers.iter().enumerate() {
        let mut weights = Vec::with_capacity(pushers.len());
        let mut sum_w = 0.0;
        for push in &pushers {
            let cap = culture_entry_cap(factuals, push.culture, pull.culture);
            let weight = push.households * cap;
            weights.push(weight);
            sum_w += weight;
        }
        if sum_w <= 0.0 {
            continue;
        }
        // The room stops a destination from taking more than it offered.
        let accepted = pull.households.min(sum_w);
        for (i, weight) in weights.iter().enumerate() {
            flow[i][j] = accepted * weight / sum_w;
        }
    }
    let mut planned = Vec::new();
    for (i, push) in pushers.iter().enumerate() {
        let total: f64 = flow[i].iter().sum();
        if total > push.households && total > 0.0 {
            let scale = push.households / total;
            for cell in &mut flow[i] {
                *cell *= scale;
            }
        }
        for (j, &households) in flow[i].iter().enumerate() {
            if households > 1e-12 {
                planned.push(MigrantFlow {
                    from: push.id,
                    to: pullers[j].id,
                    households,
                });
            }
        }
    }
    planned
}

/// # Keep Last Household
///
/// Stops a departure from leaving a pop under one household.
///
/// `counts` are the household counts before the move. `rooms` are the
/// positive offers. `flows` is updated. A departure that would leave fewer
/// than one household tries to send the rest to the destinations already
/// taking this pop, up to the room they have left. If that rest fits, the
/// pop is emptied and its id is returned. If it does not, the departure
/// shrinks until one household stays.
fn keep_last_household(
    counts: &HashMap<usize, f64>,
    rooms: &HashMap<usize, f64>,
    flows: &mut [MigrantFlow],
) -> HashSet<usize> {
    let mut evacuate = HashSet::new();
    let mut origins: Vec<usize> = flows.iter().map(|flow| flow.from).collect();
    origins.sort_unstable();
    origins.dedup();
    for from in origins {
        let sent: f64 = flows
            .iter()
            .filter(|flow| flow.from == from)
            .map(|flow| flow.households)
            .sum();
        if sent <= 0.0 {
            continue;
        }
        let count = counts.get(&from).copied().unwrap_or(0.0);
        let remaining = count - sent;
        if remaining >= 1.0 {
            continue;
        }
        // The planned move already takes everyone.
        if remaining <= 1e-9 {
            evacuate.insert(from);
            continue;
        }
        let idxs: Vec<usize> = flows
            .iter()
            .enumerate()
            .filter(|(_, flow)| flow.from == from)
            .map(|(index, _)| index)
            .collect();
        let mut adds = vec![0.0; idxs.len()];
        let mut left = remaining;
        for (n, &index) in idxs.iter().enumerate() {
            let to = flows[index].to;
            let used: f64 = flows
                .iter()
                .filter(|flow| flow.to == to)
                .map(|flow| flow.households)
                .sum();
            let room = (rooms.get(&to).copied().unwrap_or(0.0) - used).max(0.0);
            let want = remaining * flows[index].households / sent;
            let take = want.min(room);
            adds[n] = take;
            left -= take;
        }
        if left <= 1e-8 {
            for (n, &index) in idxs.iter().enumerate() {
                flows[index].households += adds[n];
            }
            evacuate.insert(from);
        } else {
            // No room for the last households. Shrink until one remains.
            let keep = (count - 1.0).max(0.0);
            let scale = keep / sent;
            for &index in &idxs {
                flows[index].households *= scale;
            }
        }
    }
    evacuate
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

/// # Opening Price
///
/// The first price of a good that has supply or demand and no rate.
///
/// `cost` is [`Market::dearest_unit_cost`], or `None` when no method charges for the
/// good. A method opens at that cost times `1 + IDLE_COST_MARGIN`, and not
/// under [`MIN_PRICE`]. No method opens at 1. Returns that price.
fn opening_price(cost: Option<f64>) -> f64 {
    match cost {
        Some(cost) => {
            debug_assert!(cost.is_finite() && cost > 0.0);
            (cost * (1.0 + IDLE_COST_MARGIN)).max(MIN_PRICE)
        }
        None => 1.0,
    }
}

/// # Raised Idle Price
///
/// The no-trade price when it is above `morning`.
///
/// `morning` is currency units per good before this pass. `cost` is
/// [`Market::dearest_unit_cost`], or `None` when no method charges for the good.
/// The dearest cost times `1 + IDLE_COST_MARGIN` replaces `morning` only
/// when it is higher. A `morning` under [`MIN_PRICE`] rises to that floor.
/// Returns the raised price, or `None` when `morning` stays.
fn raised_idle_price(morning: f64, cost: Option<f64>) -> Option<f64> {
    debug_assert!(morning.is_finite() && morning > 0.0);
    let mut next = morning;
    if let Some(cost) = cost {
        debug_assert!(cost.is_finite() && cost > 0.0);
        let raised = cost * (1.0 + IDLE_COST_MARGIN);
        if raised > next {
            next = raised;
        }
    }
    if next < MIN_PRICE {
        next = MIN_PRICE;
    }
    if next > morning {
        Some(next)
    } else {
        None
    }
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
    /// Stores `salability` on this good. Returns the good.
    pub fn with_salability(mut self, salability: f64) -> Self {
        self.salability = salability;
        self
    }

    /// # Is Active
    ///
    /// Whether this good traded, or had posted supply or demand, today.
    ///
    /// Returns whether volume, units offered, or units sought is above 0.
    pub fn is_active(&self) -> bool {
        self.volume > 0.0 || self.offered > 0.0 || self.sought > 0.0
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
    use crate::game::culture::Culture;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::Desire;
    use crate::game::factuals::Factuals;
    use crate::game::household::Household;
    use crate::game::map::Map;
    use crate::game::pop::Pop;
    use crate::game::pop_property::PopPRow;
    use crate::game::scalingfactor::ScalingFactor;
    use crate::game::sentiment::Sentiment;

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
        pop.records.migration_in = 2.0;
        pop.records.migration_out = 1.0;
        let mut pops = HashMap::from([(7, pop)]);
        market.internal_migration(&mut pops, &Map::new(1, 1), &Factuals::new());
        assert_eq!(pops[&7].migration, 0.0);
        assert_eq!(pops[&7].records.migration_in, 0.0);
        assert_eq!(pops[&7].records.migration_out, 0.0);
    }

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    fn living(sol: f64) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![],
            amount: 1.0,
            satisfaction: sol,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    fn settled(id: usize, count: f64, sol: f64) -> Pop {
        let mut pop = member(id, count, None);
        pop.desires[0].push(living(sol));
        pop
    }

    fn run(market: &mut Market, pops: &mut HashMap<usize, Pop>, factuals: &Factuals) {
        market.internal_migration(pops, &Map::new(1, 1), factuals);
    }

    #[test]
    fn half_the_moving_share_stays_and_the_age_mix_blends() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        let mut poor = settled(1, 10.0, 1.0);
        let mut rich = settled(2, 10.0, 4.0);
        poor.demographics.household.adult = 4.0;
        rich.demographics.household.adult = 2.0;
        poor.property.insert(1, PopPRow::new(100.0));
        poor.property.get_mut(&1).unwrap().fresh = 20.0;
        rich.property.insert(1, PopPRow::new(50.0));
        let mut pops = HashMap::from([(1, poor), (2, rich)]);
        run(&mut market, &mut pops, &Factuals::new());

        let moved = -pops[&1].migration;
        let full = 10.0 * 0.1 * 50.0_f64.log2();
        near(moved, full);
        near(pops[&2].migration, full);
        near(pops[&1].records.migration_out, full);
        assert_eq!(pops[&1].records.migration_in, 0.0);
        near(pops[&2].records.migration_in, full);
        assert_eq!(pops[&2].records.migration_out, 0.0);
        near(pops[&1].demographics.household.count, 10.0 - full);
        near(pops[&2].demographics.household.count, 10.0 + full);
        // Half of the moving share travels. The rest, including the stayers' goods, stays.
        let travel = 0.5 * full / 10.0;
        near(pops[&2].property[&1].quantity, 50.0 + 100.0 * travel);
        near(pops[&2].property[&1].fresh, 20.0 * travel);
        near(pops[&1].property[&1].quantity, 100.0 * (1.0 - travel));
        near(pops[&1].property[&1].fresh, 20.0 * (1.0 - travel));
        near(pops[&1].property[&1].quantity + pops[&2].property[&1].quantity, 150.0);
        near(pops[&2].demographics.household.adult, (20.0 + 4.0 * full) / (10.0 + full));
        assert!((pops[&1].demographics.household.adult - 4.0).abs() < 1e-12);
        assert!(market.pop_removals().is_empty());
    }

    #[test]
    fn a_different_species_does_not_move() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        let mut poor = settled(1, 10.0, 1.0);
        let mut rich = settled(2, 10.0, 4.0);
        poor.demographics.species = 1;
        rich.demographics.species = 2;
        poor.property.insert(1, PopPRow::new(100.0));
        let mut pops = HashMap::from([(1, poor), (2, rich)]);
        run(&mut market, &mut pops, &Factuals::new());
        near(pops[&1].demographics.household.count, 10.0);
        near(pops[&2].demographics.household.count, 10.0);
        assert_eq!(pops[&1].migration, 0.0);
        near(pops[&1].property[&1].quantity, 100.0);
    }

    #[test]
    fn culture_assimilation_caps_the_share_that_may_enter() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        let mut poor = settled(1, 10.0, 1.0);
        let mut rich = settled(2, 10.0, 4.0);
        poor.demographics.culture = 1;
        rich.demographics.culture = 2;
        let factuals = Factuals::new()
            .with_culture(Culture::new(1, "closed").with_assimilation(0.0))
            .with_culture(Culture::new(2, "open").with_assimilation(1.0));
        let mut pops = HashMap::from([(1, poor), (2, rich)]);
        run(&mut market, &mut pops, &factuals);
        let full = 10.0 * 0.1 * 50.0_f64.log2();
        near(-pops[&1].migration, full * 0.5);
    }

    #[test]
    fn the_same_culture_stays_open_when_it_is_insular() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        let mut poor = settled(1, 10.0, 1.0);
        let mut rich = settled(2, 10.0, 4.0);
        poor.demographics.culture = 7;
        rich.demographics.culture = 7;
        let factuals = Factuals::new()
            .with_culture(Culture::new(7, "closed").with_assimilation(0.0));
        let mut pops = HashMap::from([(1, poor), (2, rich)]);
        run(&mut market, &mut pops, &factuals);
        let full = 10.0 * 0.1 * 50.0_f64.log2();
        near(-pops[&1].migration, full);
    }

    #[test]
    fn a_small_pop_does_not_take_the_whole_push() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        let large = settled(1, 40.0, 1.0);
        let small = settled(2, 4.0, 8.0);
        let mut pops = HashMap::from([(1, large), (2, small)]);
        run(&mut market, &mut pops, &Factuals::new());
        let gained = pops[&2].migration;
        assert!(gained > 1.0, "{gained}");
        assert!(gained < 4.0, "{gained}");
        near(pops[&1].migration, -gained);
        near(pops[&2].demographics.household.count, 4.0 + gained);
    }

    #[test]
    fn fear_without_a_pull_stays_home() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        let mut afraid = settled(1, 10.0, 4.0);
        afraid.sentiment = Sentiment::from_parts(0.0, 0.0, 0.0, 1.0, 0.0);
        let calm = settled(2, 10.0, 4.0);
        let mut pops = HashMap::from([(1, afraid), (2, calm)]);
        run(&mut market, &mut pops, &Factuals::new());
        assert_eq!(pops[&1].migration, 0.0);
        assert_eq!(pops[&2].migration, 0.0);
    }

    #[test]
    fn a_gap_inside_the_band_does_not_move_the_market() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        let mut pops = HashMap::from([(1, settled(1, 10.0, 2.5)), (2, settled(2, 10.0, 2.6))]);
        run(&mut market, &mut pops, &Factuals::new());
        assert_eq!(pops[&1].migration, 0.0);
        near(pops[&1].demographics.household.count, 10.0);
        near(pops[&2].demographics.household.count, 10.0);
    }

    #[test]
    fn a_departure_that_would_leave_under_one_takes_everyone_and_the_goods() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        let mut poor = settled(1, 300.0, 1.0);
        let rich = settled(2, 300.0, 4.0);
        poor.property.insert(1, PopPRow::new(80.0));
        let mut pops = HashMap::from([(1, poor), (2, rich)]);
        run(&mut market, &mut pops, &Factuals::new());
        assert!(!pops.contains_key(&1));
        assert!(!market.pops.contains(&1));
        near(pops[&2].demographics.household.count, 600.0);
        near(pops[&2].migration, 300.0);
        near(pops[&2].records.migration_in, 300.0);
        assert_eq!(pops[&2].records.migration_out, 0.0);
        near(pops[&2].property[&1].quantity, 80.0);
        assert_eq!(market.pop_removals().len(), 1);
        assert_eq!(market.pop_removals()[0].id, 1);
        assert_eq!(market.pop_removals()[0].why, PopExit::UnderOneMove { to: vec![2] });
    }
}

#[cfg(test)]
mod orders_should {
    use std::collections::{HashMap, HashSet};

    use rand::rngs::StdRng;
    use rand::SeedableRng;

    use super::{Market, MarketGood};
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::firm::Firm;
    use crate::game::good::Good;
    use crate::game::institution::Institution;
    use crate::game::players::Players;
    use crate::game::pop::Pop;
    use crate::game::pop_property::PopPRow;
    use crate::game::scalingfactor::ScalingFactor;

    const GRAIN: usize = 1;
    const BREAD: usize = 3;

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    fn plain(id: usize) -> Good {
        Good {
            id,
            name: format!("g{id}"),
            class: None,
            decay_rate: 0.0,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: vec![],
        }
    }

    fn bread_desire(amount: f64) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![DesireTarget::new(BREAD, DesireTargetType::Consume, 1.0)],
            amount,
            satisfaction: 0.0,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 1.0,
        }
    }

    #[test]
    fn bids_offers_and_trades_replace_yesterday() {
        // Four satisfaction rounds up by 2%, so the bid is five loaves.
        // Six were posted. Yesterday's book, including its volume, is replaced
        // by today's bid, the posted offer, and the five loaves paid in grain.
        let mut factuals = Factuals::new();
        factuals.goods.insert(GRAIN, plain(GRAIN));
        factuals.goods.insert(BREAD, plain(BREAD));
        let mut market = Market::new(1);
        market.currencies.push(GRAIN);
        market.goods.insert(
            BREAD,
            MarketGood {
                volume: 2.0,
                sought: 9.0,
                offered: 9.0,
                salability: 0.25,
                ..MarketGood::new()
            },
        );
        market.goods.insert(8, MarketGood::new().with_salability(0.7));
        let mut buyer = Pop::new(1);
        buyer.property.insert(GRAIN, PopPRow::new(10.0));
        buyer.desires[0].push(bread_desire(4.0));
        let mut seller = Pop::new(2);
        seller.property.insert(BREAD, PopPRow::new(6.0));
        let mut pops = [buyer, seller];
        let mut firms: Vec<Firm> = Vec::new();
        let players = Players { players: HashMap::new() };
        let institutions: HashMap<usize, Institution> = HashMap::new();
        let mut rng = StdRng::seed_from_u64(1);
        market.market_day(
            &factuals,
            &mut pops,
            &mut firms,
            &institutions,
            &players,
            &mut rng,
        );
        near(market.goods[&BREAD].sought, 5.0);
        near(market.goods[&BREAD].offered, 6.0);
        near(market.goods[&BREAD].volume, 5.0);
        near(market.goods[&BREAD].currency_counts[&GRAIN], 5.0);
        // Six posted and five sold. The stored 0.25 averages with that sell-through.
        near(market.goods[&BREAD].salability, (0.25 + 5.0 / 6.0) / 2.0);
        near(market.goods[&GRAIN].offered, 10.0);
        near(market.goods[&GRAIN].sought, 0.0);
        near(market.goods[&GRAIN].volume, 0.0);
        // Grain was posted and none sold. A new row starts at 1, then averages with 0.
        near(market.goods[&GRAIN].salability, 0.5);
        // Nobody posted good 8, so its salability stays.
        near(market.goods[&8].offered, 0.0);
        near(market.goods[&8].salability, 0.7);
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
    fn no_trade_raises_a_cheap_price_to_the_dearest_cost() {
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

    #[test]
    fn no_trade_leaves_a_price_above_the_dearest_cost() {
        let mut market = Market::new(1);
        market.currencies = vec![9];
        // Good 2 is 10 per unit. Good 4 is 0.4. Both methods cost 2.2 after the margin.
        market.exchange_rates.insert(9, HashMap::from([(1, 2.0), (2, 0.1), (4, 2.5)]));
        market.goods.insert(
            2,
            super::MarketGood {
                currency_counts: HashMap::from([(9, 0.0)]),
                ..super::MarketGood::new()
            },
        );
        market.goods.insert(
            4,
            super::MarketGood {
                currency_counts: HashMap::from([(9, 0.0)]),
                ..super::MarketGood::new()
            },
        );
        let mut factuals = Factuals::new();
        factuals.processes.insert(1, method(1, vec![input(1, 4.0, false)], 2, 1.0));
        factuals.processes.insert(2, method(2, vec![input(1, 4.0, false)], 4, 1.0));
        market.move_prices(&factuals);

        assert_eq!(market.exchange_rates[&9][&2], 0.1);
        near(market.exchange_rates[&9][&4], 1.0 / 2.2);
        assert_eq!(market.exchange_rates[&9][&1], 2.0);
    }

    #[test]
    fn a_written_price_stays_at_least_a_thousandth() {
        let mut market = Market::new(1);
        market.currencies = vec![9];
        // Good 2 sits under both its method and the floor. Good 5 has no method.
        // Good 3 traded and the surplus step would cross under the floor.
        market.exchange_rates.insert(
            9,
            HashMap::from([(2, 10_000.0), (3, 1_000.0), (5, 2_500.0)]),
        );
        market.goods.insert(
            2,
            super::MarketGood {
                currency_counts: HashMap::from([(9, 0.0)]),
                ..super::MarketGood::new()
            },
        );
        market.goods.insert(
            3,
            super::MarketGood {
                sought: 0.0,
                offered: 10.0,
                volume: 1.0,
                currency_counts: HashMap::from([(9, 1.0)]),
                ..super::MarketGood::new()
            },
        );
        market.goods.insert(
            5,
            super::MarketGood {
                currency_counts: HashMap::from([(9, 0.0)]),
                ..super::MarketGood::new()
            },
        );
        let mut factuals = Factuals::new();
        // 0.0002 of the currency makes one unit. With the margin that is 0.00022.
        factuals.processes.insert(1, method(1, vec![input(9, 0.0002, false)], 2, 1.0));
        market.move_prices(&factuals);

        near(1.0 / market.exchange_rates[&9][&2], 0.001);
        near(1.0 / market.exchange_rates[&9][&3], 0.001);
        near(1.0 / market.exchange_rates[&9][&5], 0.001);
    }

    #[test]
    fn supply_or_demand_opens_a_price() {
        let mut market = Market::new(1);
        market.currencies = vec![9];
        // Good 8 already costs 10. Good 2 is demanded with no rate. Good 6 is
        // offered and costs less than 1. Good 7 is demanded and no method
        // charges. Good 4 has neither. Time is demanded and stays unpriced.
        market.exchange_rates.insert(9, HashMap::from([(8, 0.1)]));
        market.goods.insert(
            2,
            super::MarketGood {
                sought: 3.0,
                ..super::MarketGood::new()
            },
        );
        market.goods.insert(
            6,
            super::MarketGood {
                offered: 2.0,
                ..super::MarketGood::new()
            },
        );
        market.goods.insert(
            7,
            super::MarketGood {
                sought: 1.0,
                ..super::MarketGood::new()
            },
        );
        market.goods.insert(4, super::MarketGood::new());
        market.goods.insert(
            0,
            super::MarketGood {
                sought: 5.0,
                ..super::MarketGood::new()
            },
        );
        market.goods.insert(
            8,
            super::MarketGood {
                sought: 4.0,
                ..super::MarketGood::new()
            },
        );
        let mut factuals = Factuals::new();
        // 4 of good 1 at the missing rate of 1 is a unit cost of 4.
        factuals.processes.insert(1, method(1, vec![input(1, 4.0, false)], 2, 1.0));
        // Half a unit of the currency is a unit cost of 0.5.
        factuals.processes.insert(2, method(2, vec![input(9, 0.5, false)], 6, 1.0));
        factuals.processes.insert(3, method(3, vec![input(1, 2.0, false)], 8, 1.0));
        market.move_prices(&factuals);

        near(market.exchange_rates[&9][&2], 1.0 / 4.4);
        near(market.exchange_rates[&9][&6], 1.0 / 0.55);
        near(market.exchange_rates[&9][&7], 1.0);
        assert_eq!(market.exchange_rates[&9][&8], 0.1);
        assert!(market.exchange_rates[&9].get(&4).is_none());
        assert!(market.exchange_rates[&9].get(&0).is_none());

        // The opened price stays once it is the dearest cost plus the margin.
        market.move_prices(&factuals);
        near(market.exchange_rates[&9][&6], 1.0 / 0.55);
    }
}
