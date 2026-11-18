#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use rand::RngCore;

use crate::game::firm::Firm;
use crate::game::good::TIME;
use crate::game::institution::Institution;
use crate::game::job::{Job, JobLine};
use crate::game::map::Map;
use crate::game::players::Players;
use crate::game::pop::{buy_from_offers, line_value, Pop, SellOffer, ShopBook};
use crate::game::process::{InputType, Process};
use crate::game::config::{IDLE_COST_MARGIN, IDLE_MOVE_SHARE, MIN_PRICE, MarketConfig};
use crate::game::factuals::Factuals;

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
    /// A new pop sets this back to 0. [`attempt_chance`] reads it.
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

/// What one founding attempt did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundingEvent {
    /// The market was waiting. No roll.
    Rest,
    /// The attempt missed.
    Miss,
    /// The attempt hit and nobody could leave.
    Nobody,
    /// Households left `parent` to form pop `id`, running `process`.
    New { parent: usize, process: usize, id: usize },
    /// Households left `parent` and joined `dest`, to run `process`.
    Join { parent: usize, process: usize, dest: usize },
}

/// The founding attempt that just ran, and the chance it used.
///
/// A wait stores a chance of 0. [`Market::founding_day`] returns this.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoundingDay {
    /// The chance [`Market::found_pop`] compared with its draw.
    pub chance: f64,
    /// How that attempt came out.
    pub event: FoundingEvent,
}

/// One parent and process a hit might draw, and where those households go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoundingProspect {
    /// The pop who would lose the households.
    pub parent: usize,
    /// The process those households would run.
    pub process: usize,
    /// This pair's share of a hit.
    pub chance: f64,
    /// The member who already does this work. None means a new pop.
    pub joins: Option<usize>,
}

/// The next founding attempt, and every split a hit could draw.
///
/// [`Market::founding_outlook`] builds this.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundingOutlook {
    /// Chance of the next attempt, from [`attempt_chance`].
    pub chance: f64,
    /// Days left before another pop may be created.
    pub rest: u32,
    /// Parent and process pairs, highest [`FoundingProspect::chance`] first.
    pub prospects: Vec<FoundingProspect>,
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
    /// the inverse of that price. Rates are read before any of them are
    /// written. Counts are left in place.
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
            let currencies = currencies_for_good(
                &self.currencies, &rates, &counts, good, offered, sought, volume,
            );
            let mut priced = Vec::new();
            if volume == 0.0 {
                for &currency in &currencies {
                    let cost = dearest_unit_cost(factuals, &rates, currency, good);
                    // A stored price only rises. Supply or demand with no rate opens one.
                    let price = match stored_price(&rates, currency, good) {
                        Some(morning) => raised_idle_price(morning, cost),
                        None if offered > 0.0 || sought > 0.0 => Some(opening_price(cost)),
                        None => None,
                    };
                    if let Some(price) = price {
                        priced.push((currency, price));
                    }
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
                let price = price.max(MIN_PRICE);
                debug_assert!(price.is_finite() && price >= MIN_PRICE);
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

    /// # Found Pop
    ///
    /// May split one member into a new pop, or send those households to one
    /// that already does that work.
    ///
    /// `pops` are the world's pops. `factuals` supplies processes, goods, and
    /// the founding knobs. `rng` draws the attempt and the choice. `next_id`
    /// is the id of a pop this call creates. Savings are still set aside.
    ///
    /// A market with [`Self::founding_rest`] days left waits, and does not
    /// roll. Otherwise it attempts at [`attempt_chance`].
    /// A miss leaves the chance climbing. A hit picks one member with at least
    /// [`MarketConfig::founding_min_households`] households whose double
    /// headcount share can fund the process. [`MarketConfig::founding_households`]
    /// households leave. They take twice that headcount's share of property
    /// and of savings, and not more than the parent has. That share has to
    /// cover [`FOUNDING_INPUT_RUNS`] of the job's required inputs and
    /// [`FOUNDING_DESIRE_DAYS`] of the new pop's basic desires. Stock and
    /// savings count together. Time is not an input the pile has to hold.
    ///
    /// The process is one loaded process with a non-time output.
    /// [`founding_weights`] gives profitable crafts most of the draw, and a
    /// process that makes a desired good nobody supplies weighs more. The
    /// parent keeps its job. The same species, culture, craft, and labor on
    /// another member is a join: the households and the share move there, and
    /// the chance keeps climbing. A process that would only copy the parent
    /// is not drawn. A new pop resets the wait to [`FOUNDING_REST_DAYS`] and
    /// the chance to 0. Its line starts at target 0. Returns the new pop's
    /// id, or nothing when no pop is created.
    pub fn found_pop(
        &mut self,
        pops: &mut HashMap<usize, Pop>,
        factuals: &Factuals,
        rng: &mut dyn RngCore,
        next_id: usize,
    ) -> Option<usize> {
        if self.founding_rest > 0 {
            self.founding_rest -= 1;
            self.founding_day = Some(FoundingDay { chance: 0.0, event: FoundingEvent::Rest });
            return None;
        }
        let chance = attempt_chance(self.founding_open, factuals.config.market.founding_drought_step);
        let hit = chance >= 1.0 || (chance > 0.0 && unit_draw(rng) < chance);
        if !hit {
            self.founding_open = self.founding_open.saturating_add(1);
            self.founding_day = Some(FoundingDay { chance, event: FoundingEvent::Miss });
            return None;
        }
        let Some(choice) = choose_founding(self, pops, factuals, rng) else {
            self.founding_open = self.founding_open.saturating_add(1);
            self.founding_day = Some(FoundingDay { chance, event: FoundingEvent::Nobody });
            return None;
        };
        let parent = choice.parent;
        let process_id = choice.process;
        let created = match choice.dest {
            Some(dest) => {
                move_founders(pops, parent, FoundingDest::Join(dest), &choice);
                self.founding_day = Some(FoundingDay {
                    chance,
                    event: FoundingEvent::Join { parent, process: process_id, dest },
                });
                None
            }
            None => {
                let id = fresh_id(pops, next_id);
                let process = factuals
                    .get_process(process_id)
                    .expect("chosen process is loaded");
                move_founders(pops, parent, FoundingDest::Create { id, process }, &choice);
                self.pops.insert(id);
                self.founding_day = Some(FoundingDay {
                    chance,
                    event: FoundingEvent::New { parent, process: process_id, id },
                });
                Some(id)
            }
        };
        if created.is_some() {
            self.founding_rest = FOUNDING_REST_DAYS;
            self.founding_open = 0;
        } else {
            self.founding_open = self.founding_open.saturating_add(1);
        }
        created
    }

    /// # Founding Day
    ///
    /// The last attempt this market ran.
    ///
    /// Returns that day, or nothing before the first attempt.
    pub fn founding_day(&self) -> Option<FoundingDay> {
        self.founding_day
    }

    /// # Founding Outlook
    ///
    /// The next attempt, and every split a hit could draw.
    ///
    /// `pops` are the world's pops. `factuals` supplies processes and the
    /// knobs. The chance is [`attempt_chance`] for the open days so far,
    /// including while [`Self::founding_rest`] is counting down. Each
    /// prospect is one parent and process from [`ready_founders`]. Its
    /// chance is the share [`choose_founding`] would give that pair.
    /// Prospects are highest chance first, then
    /// parent id, then process id. Returns that outlook.
    pub fn founding_outlook(&self, pops: &HashMap<usize, Pop>, factuals: &Factuals) -> FoundingOutlook {
        let chance = attempt_chance(self.founding_open, factuals.config.market.founding_drought_step);
        let ready = ready_founders(self, pops, factuals);
        let mut prospects = Vec::new();
        if !ready.is_empty() {
            let parent_share = 1.0 / ready.len() as f64;
            for founder in &ready {
                let total: f64 = founder.options.iter().map(|(_, weight, _)| *weight).sum();
                if total <= 0.0 {
                    continue;
                }
                for &(process, weight, dest) in &founder.options {
                    prospects.push(FoundingProspect {
                        parent: founder.parent,
                        process,
                        chance: parent_share * weight / total,
                        joins: dest,
                    });
                }
            }
            prospects.sort_by(|left, right| {
                right.chance.total_cmp(&left.chance)
                    .then(left.parent.cmp(&right.parent))
                    .then(left.process.cmp(&right.process))
            });
        }
        FoundingOutlook { chance, rest: self.founding_rest, prospects }
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

/// # Goods To Price
///
/// Goods this pass reprices.
///
/// A good already rated in some currency comes first. A good with a currency
/// count, with volume above 0, or with posted supply or demand, follows.
/// Time is left out. A good is listed once. Returns that list.
fn goods_to_price(market: &Market) -> Vec<usize> {
    let mut goods = Vec::new();
    let mut push = |id: usize| {
        if id != TIME && !goods.contains(&id) {
            goods.push(id);
        }
    };
    for rates in market.exchange_rates.values() {
        for &good in rates.keys() {
            push(good);
        }
    }
    for (&good, row) in &market.goods {
        if !row.currency_counts.is_empty()
            || row.volume > 0.0
            || row.offered > 0.0
            || row.sought > 0.0
        {
            push(good);
        }
    }
    goods
}

/// # Currencies For Good
///
/// Tender ids that take a price for `good`.
///
/// On a day with trade, posted supply, or demand, the market's currency
/// list comes first. A currency that already rates the good, or that has a
/// count, follows. The good's own id is left out. A currency is listed
/// once. Returns that list.
fn currencies_for_good(
    market_currencies: &[usize],
    rates: &HashMap<usize, HashMap<usize, f64>>,
    counts: &HashMap<usize, f64>,
    good: usize, offered: f64, sought: f64, volume: f64,
) -> Vec<usize> {
    let mut currencies = Vec::new();
    let mut push = |id: usize| {
        if id != good && !currencies.contains(&id) {
            currencies.push(id);
        }
    };
    if volume > 0.0 || offered > 0.0 || sought > 0.0 {
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

/// # Stored Price
///
/// Currency units one unit of `good` costs in `currency`, when a rate is stored.
///
/// `rates` are goods per currency unit from before this price pass. A missing
/// rate is `None`. Returns that price.
fn stored_price(
    rates: &HashMap<usize, HashMap<usize, f64>>,
    currency: usize,
    good: usize,
) -> Option<f64> {
    let rate = rates.get(&currency).and_then(|row| row.get(&good)).copied()?;
    debug_assert!(rate.is_finite() && rate > 0.0, "exchange rate must be > 0");
    Some(1.0 / rate)
}

/// # Opening Price
///
/// The first price of a good that has supply or demand and no rate.
///
/// `cost` is [`dearest_unit_cost`], or `None` when no method charges for the
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
/// [`dearest_unit_cost`], or `None` when no method charges for the good.
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

/// Days a market waits after it creates a pop.
const FOUNDING_REST_DAYS: u32 = 5;
/// Required input runs the leaving share has to cover.
const FOUNDING_INPUT_RUNS: f64 = 3.0;
/// Days of the new pop's basic desires the leaving share has to cover.
const FOUNDING_DESIRE_DAYS: f64 = 1.5;
/// Share of the draw held by profitable crafts, before the unmet boost.
const FOUNDING_CRAFT_SHARE: f64 = 0.95;
/// Extra weight on a process that makes a desired good nobody supplies.
const FOUNDING_UNMET_WEIGHT: f64 = 20.0;

/// One process a parent can leave to do.
struct FoundingChoice {
    parent: usize,
    process: usize,
    dest: Option<usize>,
    fraction: f64,
    leaving: f64,
}

/// Where the leaving households go.
enum FoundingDest<'a> {
    /// A new pop with this id and process.
    Create { id: usize, process: &'a Process },
    /// An existing member.
    Join(usize),
}

/// Profit and gap flags for one process.
struct FoundingScore {
    profitable_craft: bool,
    per_time: f64,
    unmet: bool,
}

/// # Unit Draw
///
/// One draw in `0..1` from `rng`.
///
/// The draw is `next_u64` over `u64::MAX`. Returns that number.
fn unit_draw(rng: &mut dyn RngCore) -> f64 {
    (rng.next_u64() as f64) / (u64::MAX as f64)
}

/// # Fresh Id
///
/// The first id at or after `next_id` that `pops` does not use.
///
/// Returns that id.
fn fresh_id(pops: &HashMap<usize, Pop>, next_id: usize) -> usize {
    let mut id = next_id;
    while pops.contains_key(&id) {
        id = id.saturating_add(1);
    }
    id
}

/// # Attempt Chance
///
/// The chance a founding is attempted after `open` days that did not create a pop.
///
/// `step` is [`MarketConfig::founding_drought_step`]. The first open day is
/// one step. Each recorded open day adds another. The chance stops at 1.
/// Returns that chance.
fn attempt_chance(open: u32, step: f64) -> f64 {
    debug_assert!(step.is_finite() && (0.0..=1.0).contains(&step));
    ((open as f64 + 1.0) * step).clamp(0.0, 1.0)
}

/// One parent who can fund at least one process.
struct ReadyParent {
    parent: usize,
    fraction: f64,
    options: Vec<(usize, f64, Option<usize>)>,
}

/// # Choose Founding
///
/// The parent, process, and destination for one successful attempt.
///
/// `market` is the market. `pops` are the world's pops. `factuals` supplies
/// processes and the household knobs. `rng` picks the parent, then the
/// process. The candidates are [`ready_founders`]. Parents are equally
/// likely. Processes use their weights. Returns that choice, or nothing
/// when nobody can leave.
fn choose_founding(
    market: &Market,
    pops: &HashMap<usize, Pop>,
    factuals: &Factuals,
    rng: &mut dyn RngCore,
) -> Option<FoundingChoice> {
    let leaving = factuals.config.market.founding_households;
    let ready = ready_founders(market, pops, factuals);
    let index = weighted_index(rng, &vec![1.0; ready.len()])?;
    let founder = &ready[index];
    let picked = weighted_index(
        rng,
        &founder.options.iter().map(|(_, weight, _)| *weight).collect::<Vec<_>>(),
    )?;
    let (process, _, dest) = founder.options[picked];
    Some(FoundingChoice {
        parent: founder.parent,
        process,
        dest,
        fraction: founder.fraction,
        leaving,
    })
}

/// # Ready Founders
///
/// The parents who can leave, and the processes each one can fund.
///
/// `market` is the market. `pops` are the world's pops. `factuals` supplies
/// processes and the household knobs. A parent under the minimum size is
/// left out, and so is one that would keep fewer than one household. A
/// process the double share cannot fund is left out. A process that would
/// only copy the parent is left out. An option is the process id, its
/// [`founding_weights`] weight, and the member who already does that work.
/// Returns those parents in id order.
fn ready_founders(market: &Market, pops: &HashMap<usize, Pop>, factuals: &Factuals) -> Vec<ReadyParent> {
    let leaving = factuals.config.market.founding_households;
    let minimum = factuals.config.market.founding_min_households;
    debug_assert!(leaving.is_finite() && leaving > 0.0);
    debug_assert!(minimum.is_finite() && minimum > 0.0);
    let unmet = unmet_goods(market, pops, factuals);
    let mut processes: Vec<&Process> = factuals.processes.values().collect();
    processes.sort_by_key(|process| process.id);
    processes.retain(|process| has_product(process));
    let scores: Vec<FoundingScore> = processes
        .iter()
        .map(|process| founding_score(process, market, &unmet))
        .collect();
    let weights = founding_weights(&scores);
    let mut parents: Vec<usize> = market.pops.iter().copied().collect();
    parents.sort_unstable();
    let mut ready = Vec::new();
    for parent in parents {
        let Some(pop) = pops.get(&parent) else {
            continue;
        };
        let count = pop.demographics.household.count;
        if count + 1e-9 < minimum || count + 1e-9 < leaving + 1.0 {
            continue;
        }
        let fraction = (2.0 * leaving / count).min(1.0);
        let parent_key = work_key(pop, factuals);
        let mut options = Vec::new();
        for (process, weight) in processes.iter().zip(&weights) {
            if *weight <= 0.0 || !share_funds(pop, market, process, fraction, leaving, count) {
                continue;
            }
            let key = key_for(process);
            let dest = same_work(market, pops, factuals, pop, key, parent);
            if dest.is_none() && parent_key == key {
                continue;
            }
            options.push((process.id, *weight, dest));
        }
        if !options.is_empty() {
            ready.push(ReadyParent { parent, fraction, options });
        }
    }
    ready
}

/// # Share Funds
///
/// Whether `fraction` of `pop` covers the new job and the desire days.
///
/// `process` is the job. `leaving` households out of `count` are the new
/// pop, so its basic-desire day is that share of [`Pop::day_savings`].
/// Required destroyed or consumed inputs, other than time, have to cover
/// [`FOUNDING_INPUT_RUNS`]. The priced pile has to cover
/// [`FOUNDING_DESIRE_DAYS`] of that day. Property and savings both count.
/// Returns whether both hold.
fn share_funds(
    pop: &Pop, market: &Market, process: &Process, fraction: f64, leaving: f64, count: f64,
) -> bool {
    if count <= 0.0 || fraction <= 0.0 {
        return false;
    }
    for input in &process.inputs {
        if input.good == TIME || input.is_optional() {
            continue;
        }
        if !matches!(input.input_type, InputType::Destroyed | InputType::Consumed) {
            continue;
        }
        let have = held(pop, input.good) * fraction;
        if have + 1e-9 < input.amount * FOUNDING_INPUT_RUNS {
            return false;
        }
    }
    let child_day = pop.day_savings(market) * (leaving / count);
    let need = FOUNDING_DESIRE_DAYS * child_day;
    let have = pop.wealth_per_household(Some(market)) * count * fraction;
    have + 1e-9 >= need
}

/// # Held
///
/// Units of `good` in property and in savings.
///
/// Returns that sum.
fn held(pop: &Pop, good: usize) -> f64 {
    let stock = pop.property.get(&good).map(|row| row.quantity).unwrap_or(0.0);
    let saved = pop.savings().get(&good).copied().unwrap_or(0.0);
    stock + saved
}

/// # Move Founders
///
/// Moves the leaving households and their double share off the parent.
///
/// `choice` is the split. `dest` is the new pop or the member they join.
/// Desire amounts and satisfaction scale with the household count, so the
/// met share stays. The parent's job is left as it is.
fn move_founders(
    pops: &mut HashMap<usize, Pop>, parent_id: usize, dest: FoundingDest<'_>, choice: &FoundingChoice,
) {
    let parent = pops
        .get(&parent_id)
        .unwrap_or_else(|| panic!("pop {parent_id} is not in the founding set"));
    let count = parent.demographics.household.count;
    let snapshot = parent.demographics.household;
    let (stock, saved) = parent.belonging_snapshot();
    let kept = (count - choice.leaving) / count;
    match dest {
        FoundingDest::Create { id, process } => {
            let mut child = new_pop_from(parent, id, choice.leaving, process);
            let mut parent = pops
                .remove(&parent_id)
                .unwrap_or_else(|| panic!("pop {parent_id} is not in the founding set"));
            scale_desire_size(&mut parent, kept);
            parent.demographics.household.count -= choice.leaving;
            parent.shift_belongings_to(&mut child, choice.fraction, &stock, &saved);
            pops.insert(parent_id, parent);
            pops.insert(id, child);
        }
        FoundingDest::Join(dest_id) => {
            let mut parent = pops
                .remove(&parent_id)
                .unwrap_or_else(|| panic!("pop {parent_id} is not in the founding set"));
            scale_desire_size(&mut parent, kept);
            parent.demographics.household.count -= choice.leaving;
            let dest = pops
                .get_mut(&dest_id)
                .unwrap_or_else(|| panic!("pop {dest_id} is not in the founding set"));
            let dest_count = dest.demographics.household.count;
            scale_desire_size(dest, (dest_count + choice.leaving) / dest_count);
            dest.demographics.household.absorb(&snapshot, choice.leaving);
            parent.shift_belongings_to(dest, choice.fraction, &stock, &saved);
            pops.insert(parent_id, parent);
        }
    }
}

/// # New Pop From
///
/// A pop of `leaving` households split from `parent` to run `process`.
///
/// Species, culture, subsistence, sentiment, and desires are copied. Desire
/// amounts and satisfaction are that household share of the parent. Property
/// and savings start empty. The process sits on the craft line, or on the
/// labor line when it is labor, at target 0. The other line is empty.
/// Returns that pop.
fn new_pop_from(parent: &Pop, id: usize, leaving: f64, process: &Process) -> Pop {
    let mut child = Pop::new(id);
    child.subsistence_rate = parent.subsistence_rate;
    child.subsistence_type = parent.subsistence_type;
    child.demographics = parent.demographics;
    child.demographics.household.count = leaving;
    child.pre_growth_population = child.demographics.total_population();
    child.sentiment = parent.sentiment;
    child.desires = parent.desires.clone();
    let count = parent.demographics.household.count;
    scale_desire_size(&mut child, leaving / count);
    child.job = job_for(process);
    child
}

/// # Job For
///
/// A job whose only line is `process` at target 0.
///
/// A labor process takes the second line. Any other process takes the
/// first. The empty line is process 0 at target 0. Returns that job.
fn job_for(process: &Process) -> Job {
    let line = JobLine::new(process.id, Some(0.0), Vec::new());
    let empty = JobLine::new(0, Some(0.0), Vec::new());
    if process.is_labor() {
        Job::new([empty, line])
    } else {
        Job::new([line, empty])
    }
}

/// # Scale Desire Size
///
/// Multiplies every desire amount and its satisfaction by `factor`.
///
/// The met share stays. A non-finite factor leaves the desires alone.
fn scale_desire_size(pop: &mut Pop, factor: f64) {
    if !factor.is_finite() || factor < 0.0 {
        return;
    }
    for tier in &mut pop.desires {
        for desire in tier.iter_mut() {
            desire.amount *= factor;
            desire.satisfaction *= factor;
        }
    }
}

/// # Has Product
///
/// Whether `process` outputs something other than time.
///
/// Returns that check.
fn has_product(process: &Process) -> bool {
    process
        .outputs
        .iter()
        .any(|row| row.good != TIME && row.amount > 0.0)
}

/// # Founding Score
///
/// The craft, profit, and gap flags of `process` on `market`.
///
/// A profitable craft is not labor and its value per time is above 0.
/// Unmet is a variable output in `unmet`. Returns that score.
fn founding_score(process: &Process, market: &Market, unmet: &HashSet<usize>) -> FoundingScore {
    let value = line_value(process, Some(market));
    FoundingScore {
        profitable_craft: !process.is_labor() && value.per_time > 0.0,
        per_time: value.per_time,
        unmet: process.variable_goods().into_iter().any(|good| unmet.contains(&good)),
    }
}

/// # Founding Weights
///
/// Draw weights for `scores`, in the same order.
///
/// Profitable crafts share [`FOUNDING_CRAFT_SHARE`] in proportion to value
/// per time. Every other score shares the rest equally. With no profitable
/// craft, every score shares equally. A score that fills an unmet desire
/// then weighs [`FOUNDING_UNMET_WEIGHT`] times that share. Returns those
/// weights.
fn founding_weights(scores: &[FoundingScore]) -> Vec<f64> {
    let craft_sum: f64 = scores
        .iter()
        .filter(|score| score.profitable_craft)
        .map(|score| score.per_time)
        .sum();
    let crafts = craft_sum > 0.0;
    let others = scores.iter().filter(|score| !score.profitable_craft).count();
    let count = scores.len();
    scores
        .iter()
        .map(|score| {
            let base = if score.profitable_craft && crafts {
                FOUNDING_CRAFT_SHARE * score.per_time / craft_sum
            } else if crafts && others > 0 {
                (1.0 - FOUNDING_CRAFT_SHARE) / others as f64
            } else if count > 0 {
                1.0 / count as f64
            } else {
                0.0
            };
            if score.unmet { base * FOUNDING_UNMET_WEIGHT } else { base }
        })
        .collect()
}

/// # Unmet Goods
///
/// Desired goods nobody in `market` supplies.
///
/// A desire target on a member with a positive amount is desired. A member's
/// job output supplies its variable goods. A posted offer supplies that
/// good. Returns the desired goods that are not supplied.
fn unmet_goods(market: &Market, pops: &HashMap<usize, Pop>, factuals: &Factuals) -> HashSet<usize> {
    let mut desired = HashSet::new();
    let mut supplied = HashSet::new();
    for &id in &market.pops {
        let Some(pop) = pops.get(&id) else {
            continue;
        };
        for tier in &pop.desires {
            for desire in tier {
                if desire.amount <= 0.0 {
                    continue;
                }
                for target in &desire.target {
                    desired.insert(target.good);
                }
            }
        }
        for line in &pop.job.lines {
            let Some(process) = factuals.get_process(line.process) else {
                continue;
            };
            for good in process.variable_goods() {
                supplied.insert(good);
            }
        }
    }
    for (&good, row) in &market.goods {
        if row.offered > 0.0 {
            supplied.insert(good);
        }
    }
    desired.retain(|good| !supplied.contains(good));
    desired
}

/// # Same Work
///
/// The lowest member, other than `parent`, with this species, culture, and work.
///
/// `key` is the craft and labor. A member under one household is left out.
/// Returns that pop, or nothing when the work is free.
fn same_work(
    market: &Market,
    pops: &HashMap<usize, Pop>,
    factuals: &Factuals,
    parent: &Pop,
    key: (Option<usize>, Option<usize>),
    parent_id: usize,
) -> Option<usize> {
    let mut ids: Vec<usize> = market.pops.iter().copied().filter(|id| *id != parent_id).collect();
    ids.sort_unstable();
    for id in ids {
        let Some(pop) = pops.get(&id) else {
            continue;
        };
        if pop.demographics.household.count < 1.0 {
            continue;
        }
        if pop.demographics.species != parent.demographics.species
            || pop.demographics.culture != parent.demographics.culture
        {
            continue;
        }
        if work_key(pop, factuals) == key {
            return Some(id);
        }
    }
    None
}

/// # Work Key
///
/// The craft process and the labor process on `pop`.
///
/// A line whose process is not loaded is empty. The first labor line is the
/// labor. The first other line is the craft. Returns that pair.
fn work_key(pop: &Pop, factuals: &Factuals) -> (Option<usize>, Option<usize>) {
    let mut craft = None;
    let mut labor = None;
    for line in &pop.job.lines {
        let Some(process) = factuals.get_process(line.process) else {
            continue;
        };
        if process.is_labor() {
            if labor.is_none() {
                labor = Some(process.id);
            }
        } else if craft.is_none() {
            craft = Some(process.id);
        }
    }
    (craft, labor)
}

/// # Key For
///
/// The craft and labor pair a new pop running `process` would have.
///
/// Labor fills the labor side. Any other process fills the craft side.
/// Returns that pair.
fn key_for(process: &Process) -> (Option<usize>, Option<usize>) {
    if process.is_labor() {
        (None, Some(process.id))
    } else {
        (Some(process.id), None)
    }
}

/// # Weighted Index
///
/// An index into `weights`, drawn in proportion to the weight.
///
/// A non-positive weight is skipped. `rng` draws the roll. The last
/// positive weight catches a roll that lands on the end. Returns that
/// index, or nothing when every weight is empty.
fn weighted_index(rng: &mut dyn RngCore, weights: &[f64]) -> Option<usize> {
    let total: f64 = weights.iter().copied().filter(|weight| *weight > 0.0).sum();
    if total <= 0.0 {
        return None;
    }
    let mut cursor = unit_draw(rng) * total;
    let mut last = None;
    for (index, weight) in weights.iter().copied().enumerate() {
        if weight <= 0.0 {
            continue;
        }
        last = Some(index);
        cursor -= weight;
        if cursor <= 0.0 {
            return Some(index);
        }
    }
    last
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

#[cfg(test)]
mod founding_should {
    use std::collections::{HashMap, HashSet};

    use rand::RngCore;

    use super::{
        founding_score, founding_weights, unmet_goods, FoundingDay, FoundingEvent, FoundingScore, Market,
        MarketGood,
    };
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::good::TIME;
    use crate::game::household::Household;
    use crate::game::job::{Job, JobLine};
    use crate::game::pop::{Pop, SubsistenceType};
    use crate::game::pop_property::PopPRow;
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput, ProcessTag};
    use crate::game::scalingfactor::ScalingFactor;
    use crate::game::sentiment::Sentiment;

    const GRAIN: usize = 1;
    const BREAD: usize = 2;
    const WOOD: usize = 3;
    const WATER: usize = 4;

    fn near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "{actual} != {expected}"
        );
    }

    /// Every draw is the same `u64`.
    struct FixedRng(u64);

    impl RngCore for FixedRng {
        fn next_u32(&mut self) -> u32 {
            self.next_u64() as u32
        }

        fn next_u64(&mut self) -> u64 {
            self.0
        }

        fn fill_bytes(&mut self, dest: &mut [u8]) {
            for chunk in dest.chunks_mut(8) {
                let bytes = self.next_u64().to_le_bytes();
                chunk.copy_from_slice(&bytes[..chunk.len()]);
            }
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    /// A draw here means the founding rolled when it should not have.
    struct PanicRng;

    impl RngCore for PanicRng {
        fn next_u32(&mut self) -> u32 {
            panic!("founding drew");
        }

        fn next_u64(&mut self) -> u64 {
            panic!("founding drew");
        }

        fn fill_bytes(&mut self, _dest: &mut [u8]) {
            panic!("founding drew");
        }

        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
            self.fill_bytes(dest);
            Ok(())
        }
    }

    fn basic(amount: f64, satisfaction: f64, good: usize) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: 1,
            priority: 0,
            target: vec![DesireTarget::new(good, DesireTargetType::Consume, 1.0)],
            amount,
            satisfaction,
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    fn score(profitable_craft: bool, per_time: f64, unmet: bool) -> FoundingScore {
        FoundingScore {
            profitable_craft,
            per_time,
            unmet,
        }
    }

    /// Four bread from one wood and one time. Capital and an optional input sit on it.
    fn bake(id: usize, good: usize) -> Process {
        Process::new(id, format!("bake {id}"), 0)
            .with_input(ProcessInput::new(TIME, 1.0, false, InputType::Destroyed, false))
            .with_input(ProcessInput::new(WOOD, 1.0, false, InputType::Destroyed, false))
            .with_input(ProcessInput::new(99, 100.0, false, InputType::Capital, false))
            .with_input(ProcessInput::new(98, 50.0, false, InputType::Destroyed, true))
            .with_output(ProcessOutput::new(good, 4.0, false))
    }

    fn labor(id: usize, wood: f64) -> Process {
        let mut process = Process::new(id, "haul", 0)
            .with_input(ProcessInput::new(TIME, 1.0, false, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(WATER, 10.0, false));
        if wood > 0.0 {
            process = process.with_input(ProcessInput::new(WOOD, wood, false, InputType::Destroyed, false));
        }
        process.process_tags.push(ProcessTag::Labor);
        process
    }

    fn time_only(id: usize) -> Process {
        Process::new(id, "gather", 0)
            .with_input(ProcessInput::new(TIME, 1.0, false, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(BREAD, 4.0, false))
    }

    fn world(step: f64, processes: Vec<Process>) -> Factuals {
        let mut factuals = Factuals::new();
        factuals.config.market.founding_drought_step = step;
        for process in processes {
            factuals.processes.insert(process.id, process);
        }
        factuals
    }

    fn member(count: f64, wood: f64, grain: f64, desire: f64) -> Pop {
        let mut pop = Pop::new(1);
        pop.demographics.household = Household::with_count(count);
        pop.demographics.species = 3;
        pop.demographics.culture = 4;
        pop.subsistence_rate = 0.4;
        pop.subsistence_type = SubsistenceType::Nomadic;
        pop.sentiment = Sentiment::from_parts(1.0, 0.0, 0.0, 0.0, 0.0);
        pop.property.insert(WOOD, PopPRow::new(wood));
        pop.property.insert(GRAIN, PopPRow::new(grain));
        pop.desires[0].push(basic(desire, desire, GRAIN));
        pop.job = Job::new([
            JobLine::new(7, Some(3.0), Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        pop
    }

    fn market_of(ids: &[usize]) -> Market {
        let mut market = Market::new(1);
        market.pops.extend(ids);
        market
    }

    fn qty(pop: &Pop, good: usize) -> f64 {
        pop.property.get(&good).map(|row| row.quantity).unwrap_or(0.0)
    }

    fn found(market: &mut Market, pops: &mut HashMap<usize, Pop>, factuals: &Factuals, rng: &mut dyn RngCore) -> Option<usize> {
        market.found_pop(pops, factuals, rng, 2)
    }

    #[test]
    fn weights_follow_profit_then_the_unmet_gap() {
        // Crafts at 1 and 3 share 0.95. Two others share 0.05, and one of those is unmet.
        let weights = founding_weights(&[
            score(true, 1.0, false),
            score(true, 3.0, false),
            score(false, 0.0, true),
            score(false, 4.0, false),
        ]);
        near(weights[0], 0.2375);
        near(weights[1], 0.7125);
        near(weights[2], 0.5);
        near(weights[3], 0.025);

        // Equal crafts. The unmet one keeps its share and then weighs 20 times.
        let weights = founding_weights(&[score(true, 2.0, false), score(true, 2.0, true)]);
        near(weights[0], 0.475);
        near(weights[1], 0.475 * 20.0);

        // No profitable craft, so the base is equal and the unmet boost still applies.
        let weights = founding_weights(&[
            score(false, 1.0, false),
            score(false, 9.0, true),
            score(false, 0.0, false),
        ]);
        near(weights[0], 1.0 / 3.0);
        near(weights[1], 20.0 / 3.0);
        near(weights[2], 1.0 / 3.0);

        // Labor's value per time stays out of the craft share.
        let market = Market::new(1);
        let craft = founding_score(&bake(1, BREAD), &market, &HashSet::new());
        let haul = founding_score(&labor(9, 100.0), &market, &HashSet::new());
        assert!(craft.profitable_craft);
        near(craft.per_time, 3.0);
        assert!(!haul.profitable_craft);
        near(haul.per_time, 10.0);
        let weights = founding_weights(&[craft, haul]);
        near(weights[0], 0.95);
        near(weights[1], 0.05);
    }

    #[test]
    fn an_unmet_good_is_desired_and_not_made_or_offered() {
        let water = Process::new(4, "draw", 0)
            .with_output(ProcessOutput::new(WATER, 1.0, false))
            .with_output(ProcessOutput::new(BREAD, 1.0, true));
        let factuals = world(1.0, vec![water]);
        let mut market = market_of(&[1]);
        market.goods.insert(GRAIN, MarketGood { offered: 3.0, ..MarketGood::new() });
        market.goods.insert(BREAD, MarketGood { offered: 0.0, ..MarketGood::new() });
        let mut pop = member(25.0, 0.0, 0.0, 0.0);
        pop.desires[0].clear();
        pop.job = Job::new([
            JobLine::new(4, Some(1.0), Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        pop.property.insert(BREAD, PopPRow::new(50.0));
        pop.desires[0].push(basic(1.0, 0.0, BREAD));
        pop.desires[1].push(basic(1.0, 0.0, WATER));
        pop.desires[1].push(basic(1.0, 0.0, GRAIN));
        pop.desires[2].push(basic(0.0, 0.0, 8));
        let mut pops = HashMap::new();
        pops.insert(1, pop);
        assert_eq!(unmet_goods(&market, &pops, &factuals), HashSet::from([BREAD]));
    }

    #[test]
    fn an_unmet_craft_wins_the_draw_the_equal_craft_would_have_taken() {
        // A quarter of two equal weights lands on the first. The second weighs 20 times, so it wins.
        let factuals = world(1.0, vec![bake(1, BREAD), bake(2, WATER)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        pops.get_mut(&1).unwrap().desires[0][0] = basic(10.0, 10.0, WATER);
        let outlook = market.founding_outlook(&pops, &factuals);
        assert_eq!(outlook.prospects[0].process, 2);
        assert_eq!(outlook.prospects[1].process, 1);
        near(outlook.prospects[0].chance, 9.5 / 9.975);
        near(outlook.prospects[1].chance, 0.475 / 9.975);
        let mut rng = FixedRng(1 << 62);
        let id = found(&mut market, &mut pops, &factuals, &mut rng).unwrap();
        assert_eq!(id, 2);
        assert_eq!(pops[&2].job.lines[0].process, 2);
        assert_eq!(market.founding_rest, 5);
        assert_eq!(market.founding_open, 0);
    }

    #[test]
    fn a_rich_pop_splits_off_five_households() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        let before = pops[&1].pre_growth_population;
        let outlook = market.founding_outlook(&pops, &factuals);
        near(outlook.chance, 1.0);
        assert_eq!(outlook.rest, 0);
        assert_eq!(outlook.prospects.len(), 1);
        near(outlook.prospects[0].chance, 1.0);
        assert_eq!(outlook.prospects[0].joins, None);
        let mut rng = FixedRng(0);
        let id = found(&mut market, &mut pops, &factuals, &mut rng).unwrap();

        assert_eq!(id, 2);
        assert_eq!(
            market.founding_day(),
            Some(FoundingDay {
                chance: 1.0,
                event: FoundingEvent::New { parent: 1, process: 1, id: 2 },
            }),
        );
        assert_eq!(market.founding_rest, 5);
        assert_eq!(market.founding_open, 0);
        assert!(market.pops.contains(&1) && market.pops.contains(&2));
        let parent = &pops[&1];
        let child = &pops[&2];
        near(parent.demographics.household.count, 20.0);
        near(child.demographics.household.count, 5.0);
        near(qty(parent, WOOD), 60.0);
        near(qty(parent, GRAIN), 60.0);
        near(qty(child, WOOD), 40.0);
        near(qty(child, GRAIN), 40.0);
        near(parent.desires[0][0].amount, 8.0);
        near(parent.desires[0][0].satisfaction, 8.0);
        near(child.desires[0][0].amount, 2.0);
        near(child.desires[0][0].satisfaction, 2.0);
        assert_eq!(parent.job.lines[0].process, 7);
        assert_eq!(parent.job.lines[0].target, Some(3.0));
        assert_eq!(parent.job.lines[1].process, 0);
        assert_eq!(parent.job.lines[1].target, None);
        assert_eq!(child.job.lines[0].process, 1);
        assert_eq!(child.job.lines[0].target, Some(0.0));
        assert!(child.job.lines[0].inputs.is_empty());
        assert_eq!(child.job.lines[1].process, 0);
        assert_eq!(child.job.lines[1].target, Some(0.0));
        assert_eq!(child.demographics.species, 3);
        assert_eq!(child.demographics.culture, 4);
        near(child.subsistence_rate, 0.4);
        assert!(matches!(child.subsistence_type, SubsistenceType::Nomadic));
        near(child.sentiment.happiness(), 1.0);
        near(child.pre_growth_population, child.demographics.total_population());
        assert_eq!(parent.pre_growth_population, before);
        assert!(parent.savings().is_empty() && child.savings().is_empty());
        assert_eq!(parent.migration, 0.0);
        assert_eq!(child.migration, 0.0);
    }

    #[test]
    fn labor_takes_the_second_line() {
        let factuals = world(1.0, vec![labor(5, 0.0)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        let mut rng = FixedRng(0);
        let id = found(&mut market, &mut pops, &factuals, &mut rng).unwrap();
        let child = &pops[&id];
        assert_eq!(child.job.lines[0].process, 0);
        assert_eq!(child.job.lines[0].target, Some(0.0));
        assert_eq!(child.job.lines[1].process, 5);
        assert_eq!(child.job.lines[1].target, Some(0.0));
    }

    #[test]
    fn the_rest_blocks_five_days_and_the_next_open_day_joins() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(40.0, 1000.0, 1000.0, 10.0))]);
        let mut rng = FixedRng(0);
        assert_eq!(found(&mut market, &mut pops, &factuals, &mut rng), Some(2));
        assert_eq!(market.founding_rest, 5);
        for left in (0..5).rev() {
            let mut rng = PanicRng;
            assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
            assert_eq!(market.founding_rest, left);
            assert_eq!(market.founding_open, 0);
            assert_eq!(pops.len(), 2);
        }
        // The new pop already bakes, so the next open day joins it and the chance keeps climbing.
        let mut rng = FixedRng(0);
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(pops.len(), 2);
        assert_eq!(market.founding_rest, 0);
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 30.0);
        near(pops[&2].demographics.household.count, 10.0);
        // 40 then 35. The leaving share is 0.25, then 10/35, and the second share joins the child.
        near(qty(&pops[&2], GRAIN), 250.0 + 750.0 * 10.0 / 35.0);
        near(qty(&pops[&2], WOOD), 250.0 + 750.0 * 10.0 / 35.0);
        near(qty(&pops[&1], GRAIN), 750.0 * 25.0 / 35.0);
        near(pops[&2].desires[0][0].amount, 2.5);
        near(pops[&2].desires[0][0].satisfaction, 2.5);
        near(pops[&1].desires[0][0].amount, 7.5);
        near(pops[&1].desires[0][0].satisfaction, 7.5);
        assert_eq!(pops[&1].job.lines[0].process, 7);
        assert_eq!(pops[&2].job.lines[0].process, 1);
    }

    #[test]
    fn a_short_input_pile_does_not_found() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 5.0, 100.0, 10.0))]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        assert_eq!(market.founding_rest, 0);
        near(pops[&1].demographics.household.count, 25.0);
        near(qty(&pops[&1], WOOD), 5.0);
        assert_eq!(pops.len(), 1);
    }

    #[test]
    fn a_short_desire_pile_does_not_found() {
        let factuals = world(1.0, vec![time_only(1)]);
        let mut market = market_of(&[1]);
        // Child day is 20. The 0.4 share of 70 is 28, under the 30 the gate asks.
        let mut pops = HashMap::from([(1, member(25.0, 0.0, 70.0, 100.0))]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 25.0);
        near(qty(&pops[&1], GRAIN), 70.0);
    }

    #[test]
    fn the_same_work_joins_the_lowest_member() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1, 2, 8]);
        let mut parent = member(25.0, 100.0, 100.0, 10.0);
        parent.job = Job::none();
        let mut early = member(10.0, 0.0, 0.0, 10.0);
        early.id = 2;
        early.desires[0][0].satisfaction = 4.0;
        early.job = Job::new([
            JobLine::new(1, Some(1.0), Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        let mut later = early.clone();
        later.id = 8;
        later.desires[0][0].satisfaction = 1.0;
        let mut pops = HashMap::from([(1, parent), (2, early), (8, later)]);
        let outlook = market.founding_outlook(&pops, &factuals);
        assert_eq!(outlook.prospects.len(), 1);
        assert_eq!(outlook.prospects[0].joins, Some(2));
        let mut rng = FixedRng(0);
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(
            market.founding_day(),
            Some(FoundingDay {
                chance: 1.0,
                event: FoundingEvent::Join { parent: 1, process: 1, dest: 2 },
            }),
        );
        assert_eq!(pops.len(), 3);
        assert_eq!(market.pops.len(), 3);
        assert_eq!(market.founding_rest, 0);
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 20.0);
        near(pops[&2].demographics.household.count, 15.0);
        near(pops[&8].demographics.household.count, 10.0);
        near(qty(&pops[&1], GRAIN), 60.0);
        near(qty(&pops[&1], WOOD), 60.0);
        near(qty(&pops[&2], GRAIN), 40.0);
        near(qty(&pops[&2], WOOD), 40.0);
        near(qty(&pops[&8], GRAIN), 0.0);
        near(pops[&1].desires[0][0].amount, 8.0);
        near(pops[&1].desires[0][0].satisfaction, 8.0);
        near(pops[&2].desires[0][0].amount, 15.0);
        near(pops[&2].desires[0][0].satisfaction, 6.0);
        near(pops[&8].desires[0][0].satisfaction, 1.0);
        assert_eq!(pops[&2].job.lines[0].process, 1);
        assert_eq!(pops[&2].job.lines[0].target, Some(1.0));
        assert_eq!(pops[&1].job.lines[0].process, 0);
    }

    #[test]
    fn a_different_culture_is_a_new_pop() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1, 2]);
        let mut parent = member(25.0, 100.0, 100.0, 10.0);
        parent.job = Job::none();
        let mut other = member(10.0, 0.0, 0.0, 10.0);
        other.id = 2;
        other.demographics.culture = 9;
        other.job = Job::new([
            JobLine::new(1, Some(1.0), Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        let mut pops = HashMap::from([(1, parent), (2, other)]);
        let mut rng = FixedRng(0);
        assert_eq!(found(&mut market, &mut pops, &factuals, &mut rng), Some(3));
        near(pops[&2].demographics.household.count, 10.0);
        near(qty(&pops[&2], GRAIN), 0.0);
        assert_eq!(pops[&3].demographics.culture, 4);
        assert_eq!(pops[&3].job.lines[0].process, 1);
        near(pops[&1].demographics.household.count, 20.0);
    }

    #[test]
    fn a_pop_under_the_minimum_does_not_found() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(24.0, 1000.0, 1000.0, 10.0))]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 24.0);
        assert_eq!(pops.len(), 1);
    }

    #[test]
    fn a_parent_keeps_at_least_one_household() {
        let mut factuals = world(1.0, vec![bake(1, BREAD)]);
        factuals.config.market.founding_min_households = 1.0;
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(5.5, 100.0, 100.0, 10.0))]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 5.5);

        pops.get_mut(&1).unwrap().demographics.household.count = 6.0;
        market.founding_open = 0;
        let mut rng = FixedRng(0);
        assert_eq!(found(&mut market, &mut pops, &factuals, &mut rng), Some(2));
        near(pops[&1].demographics.household.count, 1.0);
        near(pops[&2].demographics.household.count, 5.0);
        near(qty(&pops[&2], GRAIN), 100.0);
        near(qty(&pops[&1], GRAIN), 0.0);
    }

    #[test]
    fn a_zero_step_misses_without_a_draw() {
        let factuals = world(0.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        assert_eq!(market.founding_rest, 0);
        assert_eq!(
            market.founding_day(),
            Some(FoundingDay { chance: 0.0, event: FoundingEvent::Miss }),
        );
        near(market.founding_outlook(&pops, &factuals).chance, 0.0);
        near(pops[&1].demographics.household.count, 25.0);
    }

    #[test]
    fn a_high_draw_misses_and_the_next_certain_day_founds() {
        let factuals = world(0.5, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        let mut rng = FixedRng(u64::MAX);
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        assert_eq!(found(&mut market, &mut pops, &factuals, &mut rng), Some(2));
        assert_eq!(market.founding_rest, 5);
        assert_eq!(market.founding_open, 0);
    }

    #[test]
    fn copying_the_parent_is_not_a_founding() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        let mut pop = member(25.0, 100.0, 100.0, 10.0);
        pop.job = Job::new([
            JobLine::new(1, Some(2.0), Vec::new()),
            JobLine::new(0, None, Vec::new()),
        ]);
        let mut pops = HashMap::from([(1, pop)]);
        let mut rng = PanicRng;
        assert!(found(&mut market, &mut pops, &factuals, &mut rng).is_none());
        assert_eq!(market.founding_open, 1);
        near(pops[&1].demographics.household.count, 25.0);
        assert_eq!(pops[&1].job.lines[0].target, Some(2.0));
        near(qty(&pops[&1], WOOD), 100.0);
        assert_eq!(pops.len(), 1);
    }
}
