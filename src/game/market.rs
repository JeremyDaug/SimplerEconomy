#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use rand::RngCore;

use crate::game::actor::Actor;
use crate::game::firm::{Firm, FirmBudget};
use crate::game::good::TIME;
use crate::game::pop::{buy_from_offers, grid_down, Pop, SellOffer, ShopBook};
use crate::game::process::InputType;
use crate::game::config::{
    CURRENCY_STEP, IDLE_COST_MARGIN, MIN_PRICE, OWNER_DIVIDEND_RATE, OWNER_WAGE, MarketConfig,
};
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

/// Why a pop was removed today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopExit {
    /// Household count was under 1 after growth and the population cap.
    UnderOneHousehold,
}

/// A pop removed today.
///
/// `id` is the pop. `why` is the rule that removed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopRemoval {
    pub id: usize,
    pub why: PopExit,
}

/// The market. Member ids point at actors. Goods hold the day's ledger.
#[derive(Debug, Clone)]
pub struct Market {
    /// Unique id.
    pub id: usize,
    /// Pop ids present here.
    pub pops: HashSet<usize>,
    /// Firm ids present here.
    pub firms: HashSet<usize>,
    /// Per-good quantity record. Keyed by good id.
    pub goods: HashMap<usize, MarketGood>,
    /// Abstract market value of one unit. Keyed by good id.
    ///
    /// A missing good has no stored value.
    pub amv: HashMap<usize, f64>,
    /// Distance / size multiplier on deal bulk. 0 on a one-hex market.
    pub friction: f64,
    /// Tender goods. Can be any number of goods up to the number of goods marked as 'currency'.
    pub currencies: Vec<usize>,
    /// Pops removed today. [`Self::reset_day`] clears them.
    pop_removals: Vec<PopRemoval>,
    /// Rights in special goods. Empty means none are issued.
    ownership: Vec<OwnershipRight>,
}

/// A special good whose holders this market records.
///
/// `Firm` is the share of that firm. A holder is a pop. These rights are
/// not bought or sold here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OwnershipSubject {
    Firm(usize),
}

/// One pop's part of a special good.
#[derive(Debug, Clone, Copy, PartialEq)]
struct OwnershipRight {
    subject: OwnershipSubject,
    holder: Actor,
    /// Part of `subject`, above 0 and at most 1.
    fraction: f64,
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

impl Market {
    /// # New
    ///
    /// Empty market with this id.
    ///
    /// `id` is the market id. Members, goods, abstract values, ownership, and
    /// the tender list start empty. Friction starts at 0. Returns that market.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            pops: HashSet::new(),
            firms: HashSet::new(),
            goods: HashMap::new(),
            amv: HashMap::new(),
            friction: 0.0,
            currencies: Vec::new(),
            pop_removals: Vec::new(),
            ownership: Vec::new(),
        }
    }

    /// # Issue Ownership
    ///
    /// Adds `fraction` of `subject` to pop `pop`.
    ///
    /// A pop already holding `subject` keeps one row, and `fraction` is
    /// added to that row. `pop` is not 0. `fraction` is above 0 and at most
    /// 1. The issued total for `subject` stays within 1.
    pub fn issue_ownership(&mut self, subject: OwnershipSubject, pop: usize, fraction: f64) {
        debug_assert!(
            fraction.is_finite() && fraction > 0.0 && fraction <= 1.0,
            "fraction must be above 0 and at most 1, got {fraction}"
        );
        self.add_right(subject, pop, fraction);
    }

    /// # Transfer Ownership
    ///
    /// Moves `fraction` of `subject` from pop `from` to pop `to`.
    ///
    /// Both pops are not 0, and they are not the same pop. `from` holds at
    /// least `fraction`. A pop who already holds `subject` keeps one row.
    /// A remainder at or below `1e-9` drops that row. The move has no price.
    pub fn transfer_ownership(&mut self, subject: OwnershipSubject, from: usize, to: usize, fraction: f64) {
        debug_assert!(from != 0 && to != 0, "owner pop must not be 0");
        debug_assert!(from != to, "transfer needs two pops");
        debug_assert!(
            fraction.is_finite() && fraction > 0.0,
            "fraction must be above 0, got {fraction}"
        );
        let held = self.ownership_of(subject, Actor::Pop(from));
        debug_assert!(
            held + 1e-9 >= fraction,
            "pop {from} holds {held} and the transfer is {fraction}"
        );
        if let Some(row) = self
            .ownership
            .iter_mut()
            .find(|row| row.subject == subject && row.holder == Actor::Pop(from))
        {
            row.fraction -= fraction;
        }
        // A spent row is no longer a holder.
        self.ownership.retain(|row| {
            !(row.subject == subject && row.holder == Actor::Pop(from) && row.fraction <= 1e-9)
        });
        self.add_right(subject, to, fraction);
    }

    /// # Is Owned
    ///
    /// Whether at least one pop holds `subject`.
    ///
    /// No row for `subject` is unowned. Returns that answer.
    pub fn is_owned(&self, subject: OwnershipSubject) -> bool {
        self.ownership
            .iter()
            .any(|row| row.subject == subject && row.fraction > 0.0)
    }

    /// # Ownership Of
    ///
    /// The fraction of `subject` that `holder` owns.
    ///
    /// A holder with no row owns nothing. Returns that fraction.
    pub fn ownership_of(&self, subject: OwnershipSubject, holder: Actor) -> f64 {
        self.ownership
            .iter()
            .find(|row| row.subject == subject && row.holder == holder)
            .map(|row| row.fraction)
            .unwrap_or(0.0)
    }

    /// # Issued Ownership
    ///
    /// The fractions of `subject`, added up.
    ///
    /// No rows for `subject` is 0. Returns that total.
    pub fn issued_ownership(&self, subject: OwnershipSubject) -> f64 {
        self.ownership
            .iter()
            .filter(|row| row.subject == subject)
            .map(|row| row.fraction)
            .sum()
    }

    /// # Add Right
    ///
    /// Adds `fraction` of `subject` to pop `pop`.
    ///
    /// A pop already holding `subject` keeps one row. `pop` is not 0.
    /// The issued total for `subject` stays within 1.
    fn add_right(&mut self, subject: OwnershipSubject, pop: usize, fraction: f64) {
        debug_assert!(pop != 0, "owner pop must not be 0");
        let holder = Actor::Pop(pop);
        // One holder is one row for this good.
        if let Some(row) = self
            .ownership
            .iter_mut()
            .find(|row| row.subject == subject && row.holder == holder)
        {
            row.fraction += fraction;
        } else {
            self.ownership.push(OwnershipRight {
                subject,
                holder,
                fraction,
            });
        }
        debug_assert!(
            self.issued_ownership(subject) <= 1.0 + 1e-9,
            "issued ownership must stay within 1, got {}",
            self.issued_ownership(subject)
        );
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

    /// # Stored Amv
    ///
    /// The abstract market value of one unit of `good`, when one is stored.
    ///
    /// A stored value must be finite and above 0. Returns that value, or
    /// `None` when `good` has none.
    pub fn stored_amv(&self, good: usize) -> Option<f64> {
        let amv = self.amv.get(&good).copied()?;
        debug_assert!(amv.is_finite() && amv > 0.0, "amv must be > 0");
        Some(amv)
    }

    /// # Set Amv
    ///
    /// Stores `amv` as the abstract market value of one unit of `good`.
    ///
    /// `amv` must be finite and above 0.
    pub fn set_amv(&mut self, good: usize, amv: f64) {
        debug_assert!(amv.is_finite() && amv > 0.0, "amv must be > 0");
        self.amv.insert(good, amv);
    }

    /// # Priced Goods
    ///
    /// Goods with a stored abstract market value, lowest id first.
    ///
    /// Returns those ids.
    pub fn priced_goods(&self) -> Vec<usize> {
        let mut ids: Vec<usize> = self.amv.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// # Price
    ///
    /// The abstract market value of one unit of `good`.
    ///
    /// A stored value is that value. A good with none costs 1. Returns that value.
    pub fn price(&self, good: usize) -> f64 {
        self.stored_amv(good).unwrap_or(1.0)
    }

    /// # Pay Owners
    ///
    /// Pays each pop who holds a firm share on this market.
    ///
    /// `factuals` orders tender by durability. `pops` receive the payment.
    /// `firms` pay it. A holder's bill is that fraction of [`OWNER_WAGE`]
    /// plus that fraction of the firm's property value times
    /// [`OWNER_DIVIDEND_RATE`]. Property value is on-hand quantity at
    /// [`Self::price`], read before either payment leaves. The wage is paid
    /// first. Tender is spent least durable first, in [`CURRENCY_STEP`]
    /// units, and only up to the units on hand. Each firm's budget is
    /// cleared, then the wage and dividend that left are stored on it.
    /// A holder who is not in `pops` is skipped.
    /// A dead firm pays nothing.
    pub fn pay_owners(&self, factuals: &Factuals, pops: &mut [Pop], firms: &mut [Firm]) {
        let tender = self.payment_tender(factuals);
        let bills: Vec<(usize, f64, Vec<(usize, f64)>)> = firms
            .iter()
            .map(|firm| {
                let subject = OwnershipSubject::Firm(firm.id);
                let wealth = firm_wealth(firm, self);
                let holders = self
                    .ownership
                    .iter()
                    .filter_map(|row| match row.holder {
                        Actor::Pop(pop) if row.subject == subject && pop != 0 && row.fraction > 0.0 => {
                            Some((pop, row.fraction))
                        }
                        _ => None,
                    })
                    .collect();
                (firm.id, wealth, holders)
            })
            .collect();
        for (firm_id, wealth, holders) in bills {
            let Some(firm_at) = firms.iter().position(|firm| firm.id == firm_id) else {
                continue;
            };
            firms[firm_at].budget = FirmBudget::default();
            if firms[firm_at].is_dead() {
                continue;
            }
            for (pop, fraction) in holders {
                let Some(pop_at) = pops.iter().position(|pop_row| pop_row.id == pop) else {
                    continue;
                };
                // The stipend is paid before any share of wealth.
                let wage = pay_tender(
                    &mut firms[firm_at],
                    &mut pops[pop_at],
                    &tender,
                    fraction * OWNER_WAGE,
                );
                firms[firm_at].budget.wage += wage;
                let dividend = pay_tender(
                    &mut firms[firm_at],
                    &mut pops[pop_at],
                    &tender,
                    fraction * wealth * OWNER_DIVIDEND_RATE,
                );
                firms[firm_at].budget.dividend += dividend;
            }
        }
    }

    /// # Payment Tender
    ///
    /// This market's currencies, least durable first.
    ///
    /// `factuals` supplies durability. A missing good has durability 0.
    /// Equal durability keeps currency-list order. Returns each good and
    /// [`Self::price`].
    fn payment_tender(&self, factuals: &Factuals) -> Vec<(usize, f64)> {
        let mut tender: Vec<(usize, usize)> = self
            .currencies
            .iter()
            .copied()
            .enumerate()
            .map(|(index, good)| (good, index))
            .collect();
        tender.sort_by(|(left, left_at), (right, right_at)| {
            let durability = factuals
                .durability(*left)
                .partial_cmp(&factuals.durability(*right))
                .unwrap_or(std::cmp::Ordering::Equal);
            durability.then(left_at.cmp(right_at))
        });
        tender
            .into_iter()
            .map(|(good, _)| (good, self.price(good)))
            .collect()
    }

    /// # Market Day
    ///
    /// Runs this market's day.
    ///
    /// Pops clear yesterday's sales, run subsistence, and post sell offers.
    /// Owners are paid from firm stock, then firms clear yesterday's sales
    /// and post sell offers. That posted book is what consolidation records.
    /// Then firms and pops trade.
    /// Prices move in record keeping, after the rest of the day.
    ///
    /// `factuals` supplies the world. `pops` and `firms` are the members
    /// handed to this market. Firms shop in the order given, through
    /// [`Firm::market_day`]. Pops shop through [`buy_from_offers`],
    /// wealthiest household first. `rng` picks the sellers.
    /// Offers, bids, and completed exchanges are written after the shops.
    pub fn market_day(
        &mut self,
        factuals: &Factuals,
        pops: &mut [Pop],
        firms: &mut [Firm],
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
        // Owners are paid from opening stock, before the firm reserves or sells.
        self.pay_owners(factuals, pops, firms);
        // Firms list what they will sell the same morning.
        for firm in firms.iter_mut() {
            firm.clear_sold();
            offers.extend(firm.post_sale_offers(factuals));
        }
        // The book is the offers as posted. Later takes reduce what is left.
        let offered = posted_offers(&offers);
        // Firms shop in the order they were handed in.
        let mut book = ShopBook::default();
        for index in 0..firms.len() {
            let (before, rest) = firms.split_at_mut(index);
            let (firm, after) = rest.split_first_mut().expect("firm index is in range");
            book.absorb(firm.market_day(self, factuals, &mut offers, pops, before, after, rng));
        }
        // Pops shop the offers. A buy takes another actor's goods.
        book.absorb(buy_from_offers(pops, firms, self, factuals, &mut offers, rng));
        // Write the offers, the bids, and the completed exchanges.
        self.consolidate(&offered, &book);
    }

    /// # Consolidate
    ///
    /// Writes today's offers, bids, and exchanges onto this market.
    ///
    /// `offered` is the units posted for sale, by good. `book` is the units
    /// bid, the units exchanged, and the units listed after the morning
    /// post. Yesterday's offers, bids, and volume are replaced.
    /// A good offered today replaces its salability with the average of
    /// that share and today's volume over the units offered, posted plus
    /// listed later. A good not offered today keeps its salability.
    fn consolidate(&mut self, offered: &HashMap<usize, f64>, book: &ShopBook) {
        for row in self.goods.values_mut() {
            row.offered = 0.0;
            row.sought = 0.0;
            row.volume = 0.0;
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
        // Supply offered today averages the stored share with today's sell-through.
        for (good, row) in self.goods.iter_mut() {
            let listed = row.offered + book.raised.get(good).copied().unwrap_or(0.0);
            if listed > 0.0 {
                row.salability = (row.salability + row.volume / listed) / 2.0;
                debug_assert!(row.salability.is_finite() && row.salability <= 1.0 + 1e-9);
            }
        }
    }

    /// # Record Keeping
    ///
    /// Rolls member output, consumption, and rot into this market's goods.
    ///
    /// `pops` and `firms` are the world's actors. Only members listed on
    /// this market are counted. Production, consumption, and decay are
    /// replaced by today's sums. Stock, offers, and prices are left to their
    /// own writers. [`Self::move_prices`] runs after the rollup.
    pub fn record_keeping(
        &mut self,
        factuals: &Factuals,
        pops: &HashMap<usize, Pop>,
        firms: &HashMap<usize, Firm>,
    ) {
        for row in self.goods.values_mut() {
            row.production = 0.0;
            row.consumption = 0.0;
            row.decayed = 0.0;
        }
        for &id in &self.pops {
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
            let firm = firms
                .get(&id)
                .unwrap_or_else(|| panic!("firm {id} is not in the record set"));
            for (&good_id, row) in &firm.property {
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
        self.move_prices(factuals);
    }

    /// # Move Prices
    ///
    /// Moves one abstract value per good.
    ///
    /// `factuals` supplies the processes and `market.price_gain` and
    /// `market.price_cap`. A good with volume above 0 is priced by
    /// [`traded_price`]. A good with volume 0 and a stored value keeps its
    /// morning value unless [`raised_idle_price`] lifts it. A good with
    /// posted supply or demand and no value opens at [`opening_price`]. A
    /// value this pass writes is at least [`MIN_PRICE`]. Every value is read
    /// before any is written, so one good's new value does not feed the next.
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
            let cost = self.dearest_unit_cost(factuals, good);
            // A stored value only rises. Supply or demand with no value opens one.
            let next = if row.volume == 0.0 {
                match self.stored_amv(good) {
                    Some(amv) => raised_idle_price(amv, cost),
                    None if row.is_active() => Some(opening_price(cost)),
                    None => None,
                }
            } else {
                let morning = self.price(good);
                Some(traded_price(morning, row.sought, row.offered, gain, cap))
            };
            if let Some(amv) = next {
                priced.push((good, amv));
            }
        }
        for (good, amv) in priced {
            let amv = amv.max(MIN_PRICE);
            debug_assert!(amv.is_finite() && amv >= MIN_PRICE);
            self.amv.insert(good, amv);
        }
    }

    /// # Goods To Price
    ///
    /// Goods [`Self::move_prices`] reprices.
    ///
    /// A good with a stored abstract value comes first. A good that
    /// [`MarketGood::is_active`] follows. Time is left out. A good is listed
    /// once. Returns that list.
    fn goods_to_price(&self) -> Vec<usize> {
        let mut goods = Vec::new();
        let mut push = |id: usize| {
            if id != TIME && !goods.contains(&id) {
                goods.push(id);
            }
        };
        for &good in self.amv.keys() {
            push(good);
        }
        for (&good, row) in &self.goods {
            if row.is_active() {
                push(good);
            }
        }
        goods
    }

    /// # Dearest Unit Cost
    ///
    /// The highest input cost of one unit of `good`, in abstract value.
    ///
    /// Each process that outputs `good` costs the inputs that are not
    /// optional and not factors, at [`Self::price`], divided by how much
    /// of `good` one iteration makes.
    /// Other outputs are not credited. A method with no such cost is left
    /// out. Returns that cost, or `None` when every method is free or none
    /// make the good.
    fn dearest_unit_cost(&self, factuals: &Factuals, good: usize) -> Option<f64> {
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
                cost += input.amount * self.price(input.good);
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
    /// [`PopExit::UnderOneHousehold`]. The pop stays in `pops`. The caller
    /// drops the actor.
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

/// # Firm Wealth
///
/// On-hand property of `firm` at `market`'s prices.
///
/// Each row is `quantity` times [`Market::price`]. Returns that sum.
fn firm_wealth(firm: &Firm, market: &Market) -> f64 {
    firm.property
        .iter()
        .map(|(good, row)| row.quantity * market.price(*good))
        .sum()
}

/// # Pay Tender
///
/// Pays up to `bill` abstract value from `firm` to `pop`.
///
/// `tender` is each good and its price, least durable first. A take is
/// truncated with [`grid_down`] to [`CURRENCY_STEP`] and not above the
/// units on hand. A bill that truncates to 0 pays nothing. The units
/// leave the firm and are added to the pop. The unpaid part of `bill`
/// is not carried past this call. Returns the abstract value that left.
fn pay_tender(firm: &mut Firm, pop: &mut Pop, tender: &[(usize, f64)], bill: f64) -> f64 {
    if !(bill.is_finite() && bill > 0.0) {
        return 0.0;
    }
    let mut owed = bill;
    let mut paid = 0.0;
    for (good, price) in tender {
        if owed <= 1e-9 {
            break;
        }
        if !(price.is_finite() && *price > 0.0) {
            continue;
        }
        let open = firm
            .property
            .get(good)
            .map(|row| row.quantity)
            .unwrap_or(0.0);
        let units = grid_down((owed / price).min(open), CURRENCY_STEP);
        if units <= 0.0 {
            continue;
        }
        let taken = firm.release_good(*good, units);
        if taken <= 0.0 {
            continue;
        }
        pop.receive_payment(*good, taken);
        let value = taken * price;
        owed -= value;
        paid += value;
    }
    paid
}

/// # Opening Price
///
/// The first abstract value of a good that has supply or demand and no value.
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
/// The no-trade value when it is above `morning`.
///
/// `morning` is the abstract value before this pass. `cost` is
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
/// The next abstract value of a good that traded.
///
/// `price` is the abstract value before this step. `sought` and `offered`
/// are the good's own ledger. The value is multiplied by
/// `1 + gain * clamp((sought - offered) / max(sought, offered), cap, -cap)`.
/// A ledger with both sides at 0 leaves the value where it is. Returns that
/// value.
fn traded_price(price: f64, sought: f64, offered: f64, gain: f64, cap: f64) -> f64 {
    debug_assert!(price.is_finite() && price > 0.0);
    debug_assert!(sought.is_finite() && sought >= 0.0);
    debug_assert!(offered.is_finite() && offered >= 0.0);
    let scale = sought.max(offered);
    let step = if scale == 0.0 {
        0.0
    } else {
        ((sought - offered) / scale).clamp(-cap, cap)
    };
    let next = price * (1.0 + gain * step);
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
    use crate::game::household::Household;
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
    use crate::game::firm::{Firm, FirmPRow, ProductionLine};
    use crate::game::good::Good;
    use crate::game::pop::{Pop, ShopBook};
    use crate::game::pop_property::PopPRow;
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};
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
        // by today's bid, the posted offer, and the five loaves exchanged.
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
        let mut rng = StdRng::seed_from_u64(1);
        market.market_day(
            &factuals,
            &mut pops,
            &mut firms,
            &mut rng,
        );
        near(market.goods[&BREAD].sought, 5.0);
        near(market.goods[&BREAD].offered, 6.0);
        near(market.goods[&BREAD].volume, 5.0);
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

    #[test]
    fn salability_counts_coins_listed_after_the_post() {
        // Ten grain were posted. Coins paid in listed six more on the sellers,
        // and twelve were bought. The share is 12 over the 16 offered.
        let mut market = Market::new(1);
        market.goods.insert(GRAIN, MarketGood::new().with_salability(0.5));
        let offered = HashMap::from([(GRAIN, 10.0)]);
        let mut book = ShopBook::default();
        book.volume.insert(GRAIN, 12.0);
        book.raised.insert(GRAIN, 6.0);
        market.consolidate(&offered, &book);
        near(market.goods[&GRAIN].offered, 10.0);
        near(market.goods[&GRAIN].volume, 12.0);
        near(market.goods[&GRAIN].salability, (0.5 + 12.0 / 16.0) / 2.0);
    }

    #[test]
    fn a_firm_buys_the_gap_and_then_runs_it() {
        // No transport good, so the haul is free. The firm holds five grain
        // and asks for two runs. The pop holds five of the input. The firm
        // buys the two it needs, then one more from the buy pad, pays three
        // grain, and the run turns two of them into eight output.
        const INPUT: usize = 4;
        const OUTPUT: usize = 5;
        let mut factuals = Factuals::new();
        factuals.goods.insert(GRAIN, plain(GRAIN));
        factuals.goods.insert(INPUT, plain(INPUT));
        factuals.goods.insert(OUTPUT, plain(OUTPUT));
        factuals.processes.insert(
            1,
            Process::new(1, "mill", 0)
                .with_input(ProcessInput::new(INPUT, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(OUTPUT, 4.0, true)),
        );
        let mut market = Market::new(1);
        market.currencies.push(GRAIN);
        let mut seller = Pop::new(2);
        seller.property.insert(INPUT, PopPRow::new(5.0));
        let mut firm = Firm::new(1, "mill".into());
        firm.property.insert(GRAIN, FirmPRow::new().with_quantity(5.0));
        firm.production_line.push(ProductionLine {
            process: 1,
            target: Some(2.0),
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        });
        let mut pops = [seller];
        let mut firms = [firm];
        let mut rng = StdRng::seed_from_u64(1);
        market.market_day(
            &factuals,
            &mut pops,
            &mut firms,
            &mut rng,
        );
        near(firms[0].property[&INPUT].quantity, 3.0);
        near(firms[0].property[&INPUT].reserve, 3.0);
        near(firms[0].property[&GRAIN].quantity, 2.0);
        near(pops[0].property[&INPUT].quantity, 2.0);
        near(pops[0].property[&INPUT].sold, 3.0);
        near(pops[0].property[&GRAIN].quantity, 3.0);
        near(market.goods[&INPUT].offered, 5.0);
        near(market.goods[&INPUT].sought, 3.0);
        near(market.goods[&INPUT].volume, 3.0);
        assert_eq!(market.goods.get(&GRAIN).map(|row| row.offered).unwrap_or(0.0), 0.0);
        firms[0].produce(&factuals, Some(&market));
        near(firms[0].budget.input_cost, 2.0);
        near(firms[0].budget.output_value, 8.0);
        near(firms[0].production_line[0].attempted, 2.0);
        near(firms[0].production_line[0].completed, 2.0);
        near(firms[0].property[&INPUT].quantity, 1.0);
        near(firms[0].property[&INPUT].consumed, 2.0);
        near(firms[0].property[&OUTPUT].quantity, 8.0);
        near(firms[0].property[&OUTPUT].produced, 8.0);
        near(firms[0].property[&OUTPUT].fresh, 8.0);
    }

    #[test]
    fn a_buy_pad_does_not_spend_the_next_inputs_coin() {
        // Two required inputs cost one grain each. The purse holds two.
        // The pad asks for a spare unit only after both needs are covered.
        const INPUT_A: usize = 4;
        const INPUT_B: usize = 6;
        const OUTPUT: usize = 5;
        let mut factuals = Factuals::new();
        factuals.goods.insert(GRAIN, plain(GRAIN));
        factuals.goods.insert(INPUT_A, plain(INPUT_A));
        factuals.goods.insert(INPUT_B, plain(INPUT_B));
        factuals.goods.insert(OUTPUT, plain(OUTPUT));
        factuals.processes.insert(
            1,
            Process::new(1, "mill", 0)
                .with_input(ProcessInput::new(INPUT_A, 1.0, true, InputType::Destroyed, false))
                .with_input(ProcessInput::new(INPUT_B, 1.0, true, InputType::Destroyed, false))
                .with_output(ProcessOutput::new(OUTPUT, 1.0, true)),
        );
        let mut market = Market::new(1);
        market.currencies.push(GRAIN);
        let mut seller = Pop::new(2);
        seller.property.insert(INPUT_A, PopPRow::new(5.0));
        seller.property.insert(INPUT_B, PopPRow::new(5.0));
        let mut firm = Firm::new(1, "mill".into());
        firm.property.insert(GRAIN, FirmPRow::new().with_quantity(2.0));
        firm.production_line.push(ProductionLine {
            process: 1,
            target: Some(1.0),
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        });
        let mut pops = [seller];
        let mut firms = [firm];
        let mut rng = StdRng::seed_from_u64(1);
        market.market_day(
            &factuals,
            &mut pops,
            &mut firms,
            &mut rng,
        );
        near(firms[0].property[&INPUT_A].quantity, 1.0);
        near(firms[0].property[&INPUT_B].quantity, 1.0);
        near(firms[0].property[&GRAIN].quantity, 0.0);
    }

    #[test]
    fn a_firm_sale_records_the_price_it_sold_at() {
        // Bread is 3. The firm lists six and the buyer takes some.
        // The budget is the units sold times that price.
        let mut factuals = Factuals::new();
        factuals.goods.insert(GRAIN, plain(GRAIN));
        factuals.goods.insert(BREAD, plain(BREAD));
        factuals.processes.insert(
            1,
            Process::new(1, "bake", 0).with_output(ProcessOutput::new(BREAD, 1.0, true)),
        );
        let mut market = Market::new(1);
        market.currencies.push(GRAIN);
        market.set_amv(BREAD, 3.0);
        let mut buyer = Pop::new(1);
        buyer.property.insert(GRAIN, PopPRow::new(20.0));
        buyer.desires[0].push(bread_desire(4.0));
        let mut firm = Firm::new(1, "bakery".into());
        firm.property.insert(BREAD, FirmPRow::new().with_quantity(6.0));
        firm.production_line.push(ProductionLine {
            process: 1,
            target: Some(0.0),
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        });
        let mut pops = [buyer];
        let mut firms = [firm];
        let mut rng = StdRng::seed_from_u64(1);
        market.market_day(
            &factuals,
            &mut pops,
            &mut firms,
            &mut rng,
        );
        let sold = firms[0].property[&BREAD].sold;
        assert!(sold > 0.0);
        near(firms[0].budget.sold_value, sold * 3.0);
        near(firms[0].budget.wage, 0.0);
        near(firms[0].budget.dividend, 0.0);
    }
}

#[cfg(test)]
mod record_keeping_should {
    use std::collections::HashMap;

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
    fn member_flows_replace_the_day_and_skip_non_members() {
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.firms.insert(3);
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
            },
        );
        market.goods.insert(9, super::MarketGood {
            production: 3.0,
            stock: 2.0,
            ..super::MarketGood::new()
        });


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
        let mut firm = Firm::new(3, "shop".into());
        firm.property.insert(
            1,
            FirmPRow {
                produced: 8.0,
                consumed: 1.5,
                spent: 0.25,
                lost: 0.25,
                quantity: 3.0,
                ..FirmPRow::default()
            },
        );
        let mut elsewhere = Firm::new(4, "other".into());
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
        market.record_keeping(&Factuals::new(), &pops, &firms);

        let good = &market.goods[&1];
        near(good.production, 12.0);
        near(good.consumption, 4.75);
        near(good.decayed, 0.75);
        near(good.stock, 4.0);
        near(good.offered, 6.0);
        near(good.volume, 2.0);
        near(good.salability, 0.25);
        near(market.goods[&2].production, 5.0);
        near(market.goods[&2].consumption, 0.0);
        near(market.goods[&9].production, 0.0);
        near(market.goods[&9].stock, 2.0);
        // Offered six, sought none. The morning value of 1 falls by the capped step.
        near(market.stored_amv(1).unwrap(), 0.98);
        assert!(market.stored_amv(2).is_none());
    }
}

#[cfg(test)]
mod move_prices_should {
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
    fn a_shortage_moves_the_amv() {
        let mut market = Market::new(1);
        market.set_amv(5, 0.5);
        market.goods.insert(
            5,
            super::MarketGood {
                stock: 3.0,
                sought: 8.0,
                offered: 4.0,
                volume: 1.0,
                ..super::MarketGood::new()
            },
        );
        market.move_prices(&Factuals::new());

        // The whole clamped step lands on the one value.
        near(market.stored_amv(5).unwrap(), 0.51);
        near(market.price(5), 0.51);
        market.set_amv(1, 2.0);
        near(market.price(1), 2.0);
        near(market.price(5), 0.51);
        near(market.stored_amv(5).unwrap(), 0.51);

        let mut fresh = Market::new(2);
        fresh.goods.insert(
            6,
            super::MarketGood {
                sought: 8.0,
                offered: 4.0,
                volume: 1.0,
                ..super::MarketGood::new()
            },
        );
        fresh.move_prices(&Factuals::new());
        // A missing value starts at 1 and takes the whole step.
        near(fresh.stored_amv(6).unwrap(), 1.02);
        near(market.goods[&5].stock, 3.0);
        near(market.goods[&5].offered, 4.0);
    }

    #[test]
    fn a_small_gap_uses_the_configured_gain() {
        let mut market = Market::new(1);
        market.set_amv(5, 0.5);
        market.goods.insert(
            5,
            super::MarketGood {
                sought: 10.0,
                offered: 9.0,
                volume: 1.0,
                ..super::MarketGood::new()
            },
        );
        let mut factuals = Factuals::new();
        factuals.config.market.price_gain = 0.05;
        market.move_prices(&factuals);
        near(market.stored_amv(5).unwrap(), 0.5025);
    }

    #[test]
    fn no_trade_raises_a_cheap_price_to_the_dearest_cost() {
        let mut market = Market::new(1);
        market.set_amv(1, 0.5);
        market.set_amv(8, 1.0 / 3.0);
        market.goods.insert(
            2,
            super::MarketGood {
                sought: 5.0,
                offered: 1.0,
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

        near(market.stored_amv(2).unwrap(), 2.2);
        near(market.stored_amv(1).unwrap(), 0.5);
        near(market.stored_amv(8).unwrap(), 1.0 / 3.0);
        assert!(market.stored_amv(6).is_none());
        assert!(market.stored_amv(7).is_none());
        assert!(market.stored_amv(9).is_none());
    }

    #[test]
    fn no_trade_leaves_a_price_above_the_dearest_cost() {
        let mut market = Market::new(1);
        // Good 2 is 10 per unit. Good 4 is 0.4. Both methods cost 2.2 after the margin.
        market.set_amv(1, 0.5);
        market.set_amv(2, 10.0);
        market.set_amv(4, 0.4);
        market.goods.insert(2, super::MarketGood::new());
        market.goods.insert(4, super::MarketGood::new());
        let mut factuals = Factuals::new();
        factuals.processes.insert(1, method(1, vec![input(1, 4.0, false)], 2, 1.0));
        factuals.processes.insert(2, method(2, vec![input(1, 4.0, false)], 4, 1.0));
        market.move_prices(&factuals);

        assert_eq!(market.stored_amv(2).unwrap(), 10.0);
        near(market.stored_amv(4).unwrap(), 2.2);
        assert_eq!(market.stored_amv(1).unwrap(), 0.5);
    }

    #[test]
    fn a_written_price_stays_at_least_a_thousandth() {
        let mut market = Market::new(1);
        // Good 2 sits under both its method and the floor. Good 5 has no method.
        // Good 3 traded and the surplus step would cross under the floor.
        market.set_amv(2, 0.0001);
        market.set_amv(3, 0.001);
        market.set_amv(5, 0.0004);
        market.goods.insert(2, super::MarketGood::new());
        market.goods.insert(
            3,
            super::MarketGood {
                sought: 0.0,
                offered: 10.0,
                volume: 1.0,
                ..super::MarketGood::new()
            },
        );
        market.goods.insert(5, super::MarketGood::new());
        let mut factuals = Factuals::new();
        // 0.0002 of an unpriced input makes one unit. With the margin that is 0.00022.
        factuals.processes.insert(1, method(1, vec![input(9, 0.0002, false)], 2, 1.0));
        market.move_prices(&factuals);

        near(market.stored_amv(2).unwrap(), 0.001);
        near(market.stored_amv(3).unwrap(), 0.001);
        near(market.stored_amv(5).unwrap(), 0.001);
    }

    #[test]
    fn supply_or_demand_opens_a_price() {
        let mut market = Market::new(1);
        // Good 8 already costs 10. Good 2 is demanded with no value. Good 6 is
        // offered and costs less than 1. Good 7 is demanded and no method
        // charges. Good 4 has neither. Time is demanded and stays unpriced.
        market.set_amv(8, 10.0);
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
        // 4 of good 1 at the missing value of 1 is a unit cost of 4.
        factuals.processes.insert(1, method(1, vec![input(1, 4.0, false)], 2, 1.0));
        // Half a unit of an unpriced input is a unit cost of 0.5.
        factuals.processes.insert(2, method(2, vec![input(9, 0.5, false)], 6, 1.0));
        factuals.processes.insert(3, method(3, vec![input(1, 2.0, false)], 8, 1.0));
        market.move_prices(&factuals);

        near(market.stored_amv(2).unwrap(), 4.4);
        near(market.stored_amv(6).unwrap(), 0.55);
        near(market.stored_amv(7).unwrap(), 1.0);
        assert_eq!(market.stored_amv(8).unwrap(), 10.0);
        assert!(market.stored_amv(4).is_none());
        assert!(market.stored_amv(0).is_none());

        // The opened value stays once it is the dearest cost plus the margin.
        market.move_prices(&factuals);
        near(market.stored_amv(6).unwrap(), 0.55);
    }
}

#[cfg(test)]
mod ownership_should {
    use super::{Market, OwnershipSubject};
    use crate::game::actor::Actor;

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    #[test]
    fn a_new_market_has_no_owners() {
        let market = Market::new(1);
        let firm = OwnershipSubject::Firm(1);
        assert!(!market.is_owned(firm));
        near(market.issued_ownership(firm), 0.0);
        near(market.ownership_of(firm, Actor::Pop(30)), 0.0);
    }

    #[test]
    fn issuing_records_the_pop_and_a_second_grant_adds() {
        let mut market = Market::new(1);
        let firm = OwnershipSubject::Firm(1);
        market.issue_ownership(firm, 30, 0.25);
        market.issue_ownership(firm, 4, 0.5);
        market.issue_ownership(firm, 30, 0.25);
        assert!(market.is_owned(firm));
        near(market.ownership_of(firm, Actor::Pop(30)), 0.5);
        near(market.ownership_of(firm, Actor::Pop(4)), 0.5);
        near(market.ownership_of(firm, Actor::Firm(1)), 0.0);
        near(market.issued_ownership(firm), 1.0);
    }

    #[test]
    fn a_transfer_moves_the_part_and_another_firm_stays_separate() {
        let mut market = Market::new(1);
        let firm = OwnershipSubject::Firm(1);
        let other = OwnershipSubject::Firm(7);
        market.issue_ownership(firm, 30, 1.0);
        market.issue_ownership(other, 2, 1.0);
        market.transfer_ownership(firm, 30, 8, 0.25);
        near(market.ownership_of(firm, Actor::Pop(30)), 0.75);
        near(market.ownership_of(firm, Actor::Pop(8)), 0.25);
        near(market.issued_ownership(firm), 1.0);
        near(market.ownership_of(other, Actor::Pop(2)), 1.0);
        near(market.issued_ownership(other), 1.0);
        assert!(!market.is_owned(OwnershipSubject::Firm(8)));
    }
}

#[cfg(test)]
mod pay_should {
    use std::collections::{HashMap, HashSet};

    use super::{Market, OwnershipSubject};
    use crate::game::factuals::Factuals;
    use crate::game::firm::{Firm, FirmPRow};
    use crate::game::good::Good;
    use crate::game::pop::Pop;

    const COIN: usize = 45;
    const TOOL: usize = 9;
    const GRAIN: usize = 1;

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    fn held(firm: &Firm, good: usize) -> f64 {
        firm.property.get(&good).map(|row| row.quantity).unwrap_or(0.0)
    }

    fn purse(pop: &Pop, good: usize) -> f64 {
        pop.property.get(&good).map(|row| row.quantity).unwrap_or(0.0)
    }

    fn stocked(coins: f64, tools: f64) -> Firm {
        let mut firm = Firm::new(1, "agri".into());
        if coins > 0.0 {
            firm.property
                .insert(COIN, FirmPRow::new().with_quantity(coins));
        }
        if tools > 0.0 {
            firm.property
                .insert(TOOL, FirmPRow::new().with_quantity(tools));
        }
        firm
    }

    fn priced() -> Market {
        let mut market = Market::new(1);
        market.currencies.push(COIN);
        market.set_amv(COIN, 2.0);
        market.set_amv(TOOL, 5.0);
        market
    }

    fn good(id: usize, decay_rate: f64) -> Good {
        Good {
            id,
            name: format!("g{id}"),
            class: None,
            decay_rate,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: vec![],
        }
    }

    #[test]
    fn one_owner_is_paid_the_wage_and_one_percent() {
        // 100 coins at 2 and 10 tools at 5 is 250. The bill is 1 + 2.5.
        let mut market = priced();
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 1.0);
        let mut firms = [stocked(100.0, 10.0)];
        let mut pops = [Pop::new(30)];
        market.pay_owners(&Factuals::new(), &mut pops, &mut firms);
        near(held(&firms[0], COIN), 98.25);
        near(held(&firms[0], TOOL), 10.0);
        near(purse(&pops[0], COIN), 1.75);
        near(firms[0].budget.wage, 1.0);
        near(firms[0].budget.dividend, 2.5);
        let row = pops[0].property.get(&COIN).unwrap();
        near(row.produced, 0.0);
        near(row.fresh, 0.0);
        let firm_row = firms[0].property.get(&COIN).unwrap();
        near(firm_row.consumed, 0.0);
        near(firm_row.spent, 0.0);
    }

    #[test]
    fn two_owners_are_paid_their_fractions() {
        let mut market = priced();
        market.issue_ownership(OwnershipSubject::Firm(1), 4, 0.25);
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 0.75);
        let mut firms = [stocked(100.0, 10.0)];
        let mut pops = [Pop::new(4), Pop::new(30)];
        market.pay_owners(&Factuals::new(), &mut pops, &mut firms);
        near(purse(&pops[0], COIN), 0.437);
        near(purse(&pops[1], COIN), 1.312);
        near(held(&firms[0], COIN), 98.251);
        near(held(&firms[0], TOOL), 10.0);
    }

    #[test]
    fn a_short_pile_pays_the_wage_and_leaves_the_dividend() {
        // Half a coin is the whole wage. The dividend stays unpaid.
        let mut market = priced();
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 1.0);
        let mut firms = [stocked(0.5, 10.0)];
        let mut pops = [Pop::new(30)];
        market.pay_owners(&Factuals::new(), &mut pops, &mut firms);
        near(held(&firms[0], COIN), 0.0);
        near(held(&firms[0], TOOL), 10.0);
        near(purse(&pops[0], COIN), 0.5);
        near(firms[0].budget.wage, 1.0);
        near(firms[0].budget.dividend, 0.0);
    }

    #[test]
    fn a_firm_with_no_tender_pays_nothing() {
        let mut market = Market::new(1);
        market.set_amv(TOOL, 5.0);
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 1.0);
        let mut firms = [stocked(0.0, 10.0)];
        let mut pops = [Pop::new(30)];
        market.pay_owners(&Factuals::new(), &mut pops, &mut firms);
        near(held(&firms[0], TOOL), 10.0);
        assert!(pops[0].property.is_empty());
    }

    #[test]
    fn a_missing_owner_is_skipped() {
        let mut market = priced();
        market.issue_ownership(OwnershipSubject::Firm(1), 99, 1.0);
        let mut firms = [stocked(100.0, 10.0)];
        let mut pops = [Pop::new(30)];
        market.pay_owners(&Factuals::new(), &mut pops, &mut firms);
        near(held(&firms[0], COIN), 100.0);
        near(held(&firms[0], TOOL), 10.0);
        assert!(pops[0].property.is_empty());
    }

    #[test]
    fn the_least_durable_tender_is_spent_first() {
        // Grain decays faster, so the wage spends it before the coins.
        let mut factuals = Factuals::new();
        factuals.goods.insert(GRAIN, good(GRAIN, 0.05));
        factuals.goods.insert(COIN, good(COIN, 0.001));
        let mut market = Market::new(1);
        market.currencies.push(COIN);
        market.currencies.push(GRAIN);
        market.set_amv(GRAIN, 1.0);
        market.set_amv(COIN, 2.0);
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 1.0);
        let mut firm = Firm::new(1, "agri".into());
        firm.property
            .insert(GRAIN, FirmPRow::new().with_quantity(0.5));
        firm.property
            .insert(COIN, FirmPRow::new().with_quantity(100.0));
        let mut firms = [firm];
        let mut pops = [Pop::new(30)];
        market.pay_owners(&factuals, &mut pops, &mut firms);
        near(held(&firms[0], GRAIN), 0.0);
        near(held(&firms[0], COIN), 98.748);
        near(purse(&pops[0], GRAIN), 0.5);
        near(purse(&pops[0], COIN), 1.252);
    }

    #[test]
    fn a_dead_firm_pays_nothing() {
        let mut market = priced();
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 1.0);
        let mut firm = stocked(100.0, 10.0);
        firm.dead = true;
        firm.budget.sold_value = 9.0;
        let mut firms = [firm];
        let mut pops = [Pop::new(30)];
        market.pay_owners(&Factuals::new(), &mut pops, &mut firms);
        near(held(&firms[0], COIN), 100.0);
        near(held(&firms[0], TOOL), 10.0);
        near(firms[0].budget.wage, 0.0);
        near(firms[0].budget.dividend, 0.0);
        near(firms[0].budget.sold_value, 0.0);
        assert!(pops[0].property.is_empty());
    }
}
