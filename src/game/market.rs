#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use rand::RngCore;

use crate::game::actor::Actor;
use crate::game::firm::{Firm, FirmBudget};
use crate::game::good::TIME;
use crate::game::institution::Institution;
use crate::game::job::{Job, JobLine};
use crate::game::map::Map;
use crate::game::players::Players;
use crate::game::pop::{buy_from_offers, grid_down, line_value, Pop, SellOffer, ShopBook};
use crate::game::process::{InputType, Process};
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
    /// The firm this pop owned is dead. `to` received the households.
    FirmDead { to: Vec<usize> },
}

/// A pop removed today.
///
/// `id` is the pop. `why` is the rule that removed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopRemoval {
    pub id: usize,
    pub why: PopExit,
}

/// A local market. Member ids point at actors. Goods hold the day's ledger.
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
    /// Rights in special goods. Empty means none are issued.
    ownership: Vec<OwnershipRight>,
}

/// A special good whose holders this market records.
///
/// `Firm` is the share of that firm. `Land` is the share of that land.
/// A holder is a pop. These rights are not bought or sold here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OwnershipSubject {
    Firm(usize),
    Land(usize),
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
    /// Households left `parent` to form pop `id`, which restarted `firm`.
    Firm { parent: usize, id: usize, firm: usize },
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
    /// The dead firm this split would restart.
    ///
    /// Set only on a firm row. `process` is then 0 and `joins` is none.
    pub firm: Option<usize>,
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
    /// `id` is the market id. Members, goods, abstract values, ownership, and
    /// the tender list start empty. Friction starts at 0. Returns that market.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            pops: HashSet::new(),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods: HashMap::new(),
            amv: HashMap::new(),
            friction: 0.0,
            currencies: Vec::new(),
            founding_rest: 0,
            founding_open: 0,
            founding_day: None,
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

    /// # Release Dead Firms
    ///
    /// Moves each dead member firm's owners into another pop.
    ///
    /// `firms` are the world's firms. `pops` are the world's pops. A firm
    /// that is not dead is left. An owner joins the lowest-id pop of the
    /// same species, or any other member when none share the species.
    /// Another owner of a dead firm is not a destination. The destination
    /// takes the households, the goods, and every other right that owner
    /// held. The dead firm's own right is dropped. A firm with no other
    /// pop keeps its owners. A removed pop is recorded as
    /// [`PopExit::FirmDead`].
    pub fn release_dead_firms(
        &mut self, firms: &HashMap<usize, Firm>, pops: &mut HashMap<usize, Pop>,
    ) {
        let mut dead: Vec<usize> = self
            .firms
            .iter()
            .copied()
            .filter(|id| firms.get(id).is_some_and(|firm| firm.is_dead()))
            .collect();
        dead.sort_unstable();
        if dead.is_empty() {
            return;
        }
        let mut leaving = HashSet::new();
        for id in &dead {
            for owner in self.firm_owner_pops(*id) {
                leaving.insert(owner);
            }
        }
        let dead_set: HashSet<usize> = dead.iter().copied().collect();
        for id in dead {
            for owner in self.firm_owner_pops(id) {
                let Some(pop) = pops.get(&owner) else {
                    continue;
                };
                let Some(dest) = self.owner_destination(pop, &leaving, pops) else {
                    continue;
                };
                self.fold_owner(owner, dest, &dead_set, pops);
            }
        }
    }

    /// # Firm Owner Pops
    ///
    /// Pop ids that hold `firm`, lowest id first.
    ///
    /// A holder that is not a pop is left out. A fraction at or below 0 is
    /// left out. Returns those ids.
    fn firm_owner_pops(&self, firm: usize) -> Vec<usize> {
        let subject = OwnershipSubject::Firm(firm);
        let mut owners: Vec<usize> = self
            .ownership
            .iter()
            .filter_map(|row| match row.holder {
                Actor::Pop(pop) if row.subject == subject && pop != 0 && row.fraction > 0.0 => {
                    Some(pop)
                }
                _ => None,
            })
            .collect();
        owners.sort_unstable();
        owners.dedup();
        owners
    }

    /// # Owner Destination
    ///
    /// The pop `owner` joins when its firm dies.
    ///
    /// `leaving` are owners of a dead firm, and they are not destinations.
    /// `pops` are the world's pops. The same species is taken first. The
    /// lowest id wins. No other member returns nothing.
    fn owner_destination(
        &self, owner: &Pop, leaving: &HashSet<usize>, pops: &HashMap<usize, Pop>,
    ) -> Option<usize> {
        let mut same = Vec::new();
        let mut other = Vec::new();
        for &id in &self.pops {
            if id == owner.id || leaving.contains(&id) {
                continue;
            }
            let Some(pop) = pops.get(&id) else {
                continue;
            };
            if pop.demographics.household.count < 1.0 {
                continue;
            }
            if pop.demographics.species == owner.demographics.species {
                same.push(id);
            } else {
                other.push(id);
            }
        }
        same.sort_unstable();
        other.sort_unstable();
        same.into_iter().next().or_else(|| other.into_iter().next())
    }

    /// # Fold Owner
    ///
    /// Moves pop `owner` and its goods onto pop `dest`.
    ///
    /// `dead` are the firms being released. Their rights are dropped. Every
    /// other right this pop held moves to `dest`. The households are
    /// absorbed. The pop is unlisted and recorded as [`PopExit::FirmDead`].
    fn fold_owner(
        &mut self, owner: usize, dest: usize, dead: &HashSet<usize>, pops: &mut HashMap<usize, Pop>,
    ) {
        let Some(mut origin) = pops.remove(&owner) else {
            return;
        };
        let count = origin.demographics.household.count;
        if count <= 0.0 {
            pops.insert(owner, origin);
            return;
        }
        let snapshot = origin.demographics.household;
        let (stock, saved) = origin.belonging_snapshot();
        {
            let dest_pop = pops
                .get_mut(&dest)
                .unwrap_or_else(|| panic!("pop {dest} is not in the release set"));
            origin.shift_belongings_to(dest_pop, 1.0, &stock, &saved);
            dest_pop.demographics.household.absorb(&snapshot, count);
            dest_pop.migration += count;
            dest_pop.records.migration_in += count;
        }
        let rights: Vec<(OwnershipSubject, f64)> = self
            .ownership
            .iter()
            .filter(|row| row.holder == Actor::Pop(owner) && row.fraction > 0.0)
            .map(|row| (row.subject, row.fraction))
            .collect();
        self.ownership.retain(|row| row.holder != Actor::Pop(owner));
        for (subject, fraction) in rights {
            if let OwnershipSubject::Firm(firm) = subject {
                if dead.contains(&firm) {
                    continue;
                }
            }
            self.add_right(subject, dest, fraction);
        }
        self.pops.remove(&owner);
        self.pop_removals.push(PopRemoval {
            id: owner,
            why: PopExit::FirmDead { to: vec![dest] },
        });
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

    /// # Take Firm
    ///
    /// Gives pop `pop` the whole of firm `firm`.
    ///
    /// Every right in that firm is dropped first, including one this pop
    /// already holds. The issued total is 1.
    fn take_firm(&mut self, firm: usize, pop: usize) {
        let subject = OwnershipSubject::Firm(firm);
        self.ownership.retain(|row| row.subject != subject);
        self.issue_ownership(subject, pop, 1.0);
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
    /// A holder who is not in `pops` is skipped. A land share is not paid.
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
    /// Then early player, institutions, firms, pops, and late player trade.
    /// Prices move in record keeping, after the rest of the day.
    ///
    /// `factuals` supplies the world. `pops` and `firms` are the members
    /// handed to this market. Firms shop in the order given, through
    /// [`Firm::market_day`]. Pops shop through [`buy_from_offers`],
    /// wealthiest household first. `institutions` is every institution.
    /// This market calls the ones in `institution_ids`, in id order.
    /// `players` buy first and again last. `rng` picks the sellers.
    /// Offers, bids, and completed exchanges are written after the shops.
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
        // Owners are paid from opening stock, before the firm reserves or sells.
        self.pay_owners(factuals, pops, firms);
        // Firms list what they will sell the same morning.
        for firm in firms.iter_mut() {
            firm.clear_sold();
            offers.extend(firm.post_sale_offers(factuals));
        }
        // The book is the offers as posted. Later takes reduce what is left.
        let offered = posted_offers(&offers);
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
        // Firms shop in the order they were handed in.
        let mut book = ShopBook::default();
        for index in 0..firms.len() {
            let (before, rest) = firms.split_at_mut(index);
            let (firm, after) = rest.split_first_mut().expect("firm index is in range");
            book.absorb(firm.market_day(self, factuals, &mut offers, pops, before, after, rng));
        }
        // Pops shop the offers. A buy takes another actor's goods.
        book.absorb(buy_from_offers(pops, firms, self, factuals, &mut offers, rng));
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
    /// bid and the units exchanged. Yesterday's offers, bids, and volume
    /// are replaced.
    /// A good with posted supply replaces its salability with the average of
    /// that share and today's volume over the posted supply. A good with no
    /// posted supply keeps its salability.
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
    /// `pops` are the world's pops. `firms` are the world's firms. `factuals`
    /// supplies processes, goods, and the founding knobs. `rng` draws the
    /// attempt, the firm chance, and the choice. `next_id` is the id of a pop
    /// this call creates. Savings are still set aside.
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
    /// the chance to 0. Its line starts at target 0.
    ///
    /// A hit can restart a firm instead, when [`lowest_dead_firm`] finds one
    /// and [`wealthy_parents`] is not empty. [`FIRM_FOUNDING_CHANCE`] of those
    /// hits call [`Self::restart_firm`]. The other hits found a pop as above.
    /// That draw happens only then, before the pop choice. Returns the new
    /// pop's id, or nothing when no pop is created.
    pub fn found_pop(
        &mut self,
        pops: &mut HashMap<usize, Pop>,
        firms: &mut HashMap<usize, Firm>,
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
        // Half of these hits restart a firm. The rest found a pop.
        if let Some(firm_id) = lowest_dead_firm(self, firms) {
            let parents = wealthy_parents(self, pops, factuals);
            if !parents.is_empty() && unit_draw(rng) < FIRM_FOUNDING_CHANCE {
                return Some(self.restart_firm(
                    pops, firms, factuals, rng, next_id, chance, firm_id, &parents,
                ));
            }
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

    /// # Restart Firm
    ///
    /// Turns `firm` back on under a new owner pop.
    ///
    /// `parents` are the wealthy members, in id order. `rng` picks one of
    /// them with equal weight. `next_id` is the id of the pop this call
    /// creates. `chance` is the attempt chance stored on the day.
    /// [`MarketConfig::founding_households`] households leave with the usual
    /// double headcount share.
    ///
    /// The child has no craft. [`FIRM_FOUNDING_SAVINGS`] of its savings moves
    /// onto the firm's stock and is not marked fresh. The priced savings
    /// cache is the savings it carried, then scaled by the share it keeps.
    /// Any right in `firm` is dropped, and the child is issued the whole
    /// firm. `firm` is no longer dead. Its target is left as it was. The
    /// wait resets to [`FOUNDING_REST_DAYS`] and the open count to 0.
    /// Returns the new pop's id.
    fn restart_firm(
        &mut self,
        pops: &mut HashMap<usize, Pop>,
        firms: &mut HashMap<usize, Firm>,
        factuals: &Factuals,
        rng: &mut dyn RngCore,
        next_id: usize,
        chance: f64,
        firm_id: usize,
        parents: &[usize],
    ) -> usize {
        let index = weighted_index(rng, &vec![1.0; parents.len()])
            .unwrap_or_else(|| panic!("a wealthy parent is waiting"));
        let parent = parents[index];
        let count = pops
            .get(&parent)
            .unwrap_or_else(|| panic!("pop {parent} is not in the founding set"))
            .demographics
            .household
            .count;
        let leaving = factuals.config.market.founding_households;
        let fraction = (2.0 * leaving / count).min(1.0);
        let choice = FoundingChoice {
            parent,
            process: 0,
            dest: None,
            fraction,
            leaving,
        };
        let id = fresh_id(pops, next_id);
        move_founders(pops, parent, FoundingDest::Own { id }, &choice);
        // The cache matches the savings the child carried, then the gift scales it.
        let value: f64 = pops
            .get(&id)
            .unwrap_or_else(|| panic!("pop {id} is not in the founding set"))
            .savings()
            .iter()
            .map(|(&good, amount)| *amount * self.price(good))
            .sum();
        let taken = {
            let child = pops
                .get_mut(&id)
                .unwrap_or_else(|| panic!("pop {id} is not in the founding set"));
            child.set_effective_savings(value);
            child.take_savings_share(FIRM_FOUNDING_SAVINGS)
        };
        let firm = firms
            .get_mut(&firm_id)
            .unwrap_or_else(|| panic!("firm {firm_id} is not in the founding set"));
        firm.dead = false;
        for (good, amount) in taken {
            // Kickstart stock is not a fresh receipt.
            firm.property.entry(good).or_default().quantity += amount;
        }
        self.take_firm(firm_id, id);
        self.pops.insert(id);
        self.founding_rest = FOUNDING_REST_DAYS;
        self.founding_open = 0;
        self.founding_day = Some(FoundingDay {
            chance,
            event: FoundingEvent::Firm { parent, id, firm: firm_id },
        });
        id
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
    /// `pops` are the world's pops. `firms` are the world's firms.
    /// `factuals` supplies processes and the knobs. The chance is
    /// [`attempt_chance`] for the open days so far, including while
    /// [`Self::founding_rest`] is counting down. Each prospect is one parent
    /// and process from [`ready_founders`]. Its chance is the share
    /// [`choose_founding`] would give that pair. When [`lowest_dead_firm`]
    /// finds one and [`wealthy_parents`] is not empty, each of those chances
    /// is multiplied by one minus [`FIRM_FOUNDING_CHANCE`], and each wealthy
    /// member gets an equal share of [`FIRM_FOUNDING_CHANCE`] aimed at that
    /// firm. Prospects are highest chance first, then parent id, then
    /// process id. Returns that outlook.
    pub fn founding_outlook(
        &self, pops: &HashMap<usize, Pop>, firms: &HashMap<usize, Firm>, factuals: &Factuals,
    ) -> FoundingOutlook {
        let chance = attempt_chance(self.founding_open, factuals.config.market.founding_drought_step);
        let ready = ready_founders(self, pops, factuals);
        let dead = lowest_dead_firm(self, firms);
        let wealthy = wealthy_parents(self, pops, factuals);
        let firm_open = dead.is_some() && !wealthy.is_empty();
        let pop_scale = if firm_open { 1.0 - FIRM_FOUNDING_CHANCE } else { 1.0 };
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
                        chance: parent_share * weight / total * pop_scale,
                        joins: dest,
                        firm: None,
                    });
                }
            }
        }
        if let Some(firm_id) = dead.filter(|_| firm_open) {
            let share = FIRM_FOUNDING_CHANCE / wealthy.len() as f64;
            for parent in wealthy {
                prospects.push(FoundingProspect {
                    parent,
                    process: 0,
                    chance: share,
                    joins: None,
                    firm: Some(firm_id),
                });
            }
        }
        prospects.sort_by(|left, right| {
            right.chance.total_cmp(&left.chance)
                .then(left.parent.cmp(&right.parent))
                .then(left.process.cmp(&right.process))
        });
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
/// How many times the cheapest new pop the leaving share must be worth.
const FIRM_FOUNDING_WEALTH: f64 = 3.0;
/// Chance a hit restarts a dead firm when a wealthy parent can leave.
const FIRM_FOUNDING_CHANCE: f64 = 0.5;
/// Share of the new owner's savings given to the restarted firm.
const FIRM_FOUNDING_SAVINGS: f64 = 0.5;

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
    /// A new pop that owns a restarted firm.
    Own { id: usize },
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

/// # Lowest Dead Firm
///
/// The lowest-id dead firm that is a member of `market`.
///
/// `firms` are the world's firms. A member missing from that map is left
/// out. A firm that is not dead is left out. Returns that id, or nothing
/// when every member firm is still working.
fn lowest_dead_firm(market: &Market, firms: &HashMap<usize, Firm>) -> Option<usize> {
    let mut ids: Vec<usize> = market
        .firms
        .iter()
        .copied()
        .filter(|id| firms.get(id).is_some_and(|firm| firm.is_dead()))
        .collect();
    ids.sort_unstable();
    ids.into_iter().next()
}

/// # Wealthy Parents
///
/// Members who can leave to restart a firm, lowest id first.
///
/// `pops` are the world's pops. `factuals` supplies the household knobs.
/// A member [`is_wealthy`] keeps. Returns those ids.
fn wealthy_parents(market: &Market, pops: &HashMap<usize, Pop>, factuals: &Factuals) -> Vec<usize> {
    let leaving = factuals.config.market.founding_households;
    let minimum = factuals.config.market.founding_min_households;
    let mut ids: Vec<usize> = market.pops.iter().copied().collect();
    ids.sort_unstable();
    ids.retain(|id| {
        pops.get(id).is_some_and(|pop| is_wealthy(pop, market, factuals, leaving, minimum))
    });
    ids
}

/// # Is Wealthy
///
/// Whether `pop` is large enough to split and rich enough to restart a firm.
///
/// `leaving` households must go, and at least one must stay. The household
/// count is also at least `minimum`. The leaving share is twice that
/// headcount's part of the pop, and not more than the pop has. That share's
/// wealth has to be at least [`FIRM_FOUNDING_WEALTH`] times [`founding_bar`].
/// A bar at or below `1e-9` does not count. Returns that check.
fn is_wealthy(pop: &Pop, market: &Market, factuals: &Factuals, leaving: f64, minimum: f64) -> bool {
    let count = pop.demographics.household.count;
    if count + 1e-9 < minimum || count + 1e-9 < leaving + 1.0 {
        return false;
    }
    let fraction = (2.0 * leaving / count).min(1.0);
    let bar = founding_bar(pop, market, factuals, leaving, count);
    if bar <= 1e-9 {
        return false;
    }
    let wealth = pop.wealth_per_household(Some(market)) * count * fraction;
    wealth + 1e-9 >= bar * FIRM_FOUNDING_WEALTH
}

/// # Founding Bar
///
/// The priced cost of the cheapest way for `pop` to found a pop.
///
/// `leaving` households out of `count` are the new pop. The cost is
/// [`FOUNDING_INPUT_RUNS`] of one product process's required destroyed or
/// consumed inputs, other than time, plus [`FOUNDING_DESIRE_DAYS`] of that
/// new pop's basic desires. The process with the lowest input cost is the
/// one that counts. With no product process the cost is those desire days.
/// Returns that cost.
fn founding_bar(pop: &Pop, market: &Market, factuals: &Factuals, leaving: f64, count: f64) -> f64 {
    let child_day = pop.day_savings(market) * (leaving / count);
    let desire = FOUNDING_DESIRE_DAYS * child_day;
    let mut processes: Vec<&Process> = factuals.processes.values().collect();
    processes.sort_by_key(|process| process.id);
    let mut cheapest = None;
    for process in processes {
        if !has_product(process) {
            continue;
        }
        let cost = input_runs_cost(process, market);
        cheapest = Some(cheapest.map_or(cost, |low: f64| low.min(cost)));
    }
    match cheapest {
        Some(cost) => cost + desire,
        None => desire,
    }
}

/// # Input Runs Cost
///
/// The priced cost of [`FOUNDING_INPUT_RUNS`] of `process` on `market`.
///
/// Required destroyed or consumed inputs count. Time, optional inputs, and
/// other input kinds do not. A missing price counts as 1. Returns that cost.
fn input_runs_cost(process: &Process, market: &Market) -> f64 {
    let mut cost = 0.0;
    for input in &process.inputs {
        if input.good == TIME || input.is_optional() {
            continue;
        }
        if !matches!(input.input_type, InputType::Destroyed | InputType::Consumed) {
            continue;
        }
        cost += input.amount * FOUNDING_INPUT_RUNS * market.price(input.good);
    }
    cost
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
        FoundingDest::Own { id } => {
            let mut child = new_owner_from(parent, id, choice.leaving);
            let mut parent = pops
                .remove(&parent_id)
                .unwrap_or_else(|| panic!("pop {parent_id} is not in the founding set"));
            scale_desire_size(&mut parent, kept);
            parent.demographics.household.count -= choice.leaving;
            parent.shift_belongings_to(&mut child, choice.fraction, &stock, &saved);
            pops.insert(parent_id, parent);
            pops.insert(id, child);
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

/// # New Owner From
///
/// A pop of `leaving` households split from `parent` to own a firm.
///
/// Species, culture, subsistence, sentiment, and desires are copied. Desire
/// amounts and satisfaction are that household share of the parent. Property
/// and savings start empty. Both job lines are process 0 at target 0, so
/// the pop does not craft. Returns that pop.
fn new_owner_from(parent: &Pop, id: usize, leaving: f64) -> Pop {
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
    let line = JobLine::new(0, Some(0.0), Vec::new());
    child.job = Job::new([line.clone(), line]);
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

    use hexx::Hex;

    use super::{Market, MarketGood};
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::firm::{Firm, FirmPRow, ProductionLine};
    use crate::game::good::Good;
    use crate::game::institution::Institution;
    use crate::game::players::Players;
    use crate::game::pop::Pop;
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
        let mut firm = Firm::new(1, "mill".into(), 1, Hex::new(0, 0));
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
        let mut firm = Firm::new(1, "mill".into(), 1, Hex::new(0, 0));
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
        let mut firm = Firm::new(1, "bakery".into(), 1, Hex::new(0, 0));
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
        let mut firm = Firm::new(3, "shop".into(), 1, Hex::new(0, 0));
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
mod founding_should {
    use std::collections::{HashMap, HashSet};

    use rand::RngCore;

    use hexx::Hex;

    use super::{
        founding_score, founding_weights, unmet_goods, FoundingDay, FoundingEvent, FoundingScore, Market,
        MarketGood, OwnershipSubject,
    };
    use crate::game::actor::Actor;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::firm::{Firm, FirmPRow, ProductionLine};
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
        market.found_pop(pops, &mut HashMap::new(), factuals, rng, 2)
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
        let outlook = market.founding_outlook(&pops, &HashMap::new(), &factuals);
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
        let outlook = market.founding_outlook(&pops, &HashMap::new(), &factuals);
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
        let outlook = market.founding_outlook(&pops, &HashMap::new(), &factuals);
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
        near(market.founding_outlook(&pops, &HashMap::new(), &factuals).chance, 0.0);
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

    fn dead_mill(id: usize, wood: f64) -> Firm {
        let mut firm = Firm::new(id, format!("mill {id}"), 1, Hex::new(0, 0));
        firm.dead = true;
        firm.property.insert(WOOD, FirmPRow::new().with_quantity(wood));
        firm.production_line.push(ProductionLine {
            process: 1,
            target: Some(4.0),
            inputs: vec![],
            attempted: 0.0,
            completed: 0.0,
        });
        firm
    }

    fn saved(pop: &Pop, good: usize) -> f64 {
        pop.savings().get(&good).copied().unwrap_or(0.0)
    }

    fn firm_qty(firm: &Firm, good: usize) -> f64 {
        firm.property.get(&good).map(|row| row.quantity).unwrap_or(0.0)
    }

    /// A rich parent with 10 grain parked, two dead member firms, and one dead outsider.
    fn wealthy_restart() -> (Market, HashMap<usize, Pop>, HashMap<usize, Firm>, Factuals) {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        market.firms.extend([2, 5]);
        let mut pop = member(25.0, 100.0, 100.0, 10.0);
        pop.move_to_savings(GRAIN, 10.0);
        let pops = HashMap::from([(1, pop)]);
        let firms = HashMap::from([
            (1, dead_mill(1, 3.0)),
            (2, dead_mill(2, 7.0)),
            (5, dead_mill(5, 1.0)),
        ]);
        market.issue_ownership(OwnershipSubject::Firm(2), 4, 1.0);
        market.issue_ownership(OwnershipSubject::Land(9), 4, 1.0);
        (market, pops, firms, factuals)
    }

    #[test]
    fn a_wealthy_pop_restarts_the_lowest_dead_firm() {
        // This draw is 0, so the hit takes the firm half.
        let (mut market, mut pops, mut firms, factuals) = wealthy_restart();
        let mut rng = FixedRng(0);
        let id = market.found_pop(&mut pops, &mut firms, &factuals, &mut rng, 2).unwrap();

        assert_eq!(id, 2);
        assert_eq!(
            market.founding_day(),
            Some(FoundingDay {
                chance: 1.0,
                event: FoundingEvent::Firm { parent: 1, id: 2, firm: 2 },
            }),
        );
        assert_eq!(market.founding_rest, 5);
        assert_eq!(market.founding_open, 0);
        assert!(market.pops.contains(&2));
        near(pops[&1].demographics.household.count, 20.0);
        near(pops[&2].demographics.household.count, 5.0);
        near(qty(&pops[&1], WOOD), 60.0);
        near(qty(&pops[&2], WOOD), 40.0);
        // 10 grain left property for savings. The child keeps 36, and half of its 4 saved grain goes to the firm.
        near(qty(&pops[&1], GRAIN), 54.0);
        near(qty(&pops[&2], GRAIN), 36.0);
        near(saved(&pops[&1], GRAIN), 6.0);
        near(saved(&pops[&2], GRAIN), 2.0);
        near(pops[&2].effective_savings(), 2.0);
        assert_eq!(pops[&1].job.lines[0].process, 7);
        assert_eq!(pops[&1].job.lines[0].target, Some(3.0));
        assert_eq!(pops[&2].job.lines[0].process, 0);
        assert_eq!(pops[&2].job.lines[0].target, Some(0.0));
        assert_eq!(pops[&2].job.lines[1].process, 0);
        assert_eq!(pops[&2].job.lines[1].target, Some(0.0));
        assert!(!firms[&2].is_dead());
        assert!(firms[&5].is_dead());
        assert!(firms[&1].is_dead());
        assert_eq!(firms[&2].production_line[0].target, Some(4.0));
        near(firm_qty(&firms[&2], WOOD), 7.0);
        near(firm_qty(&firms[&2], GRAIN), 2.0);
        near(firms[&2].property[&GRAIN].fresh, 0.0);
        let firm = OwnershipSubject::Firm(2);
        near(market.ownership_of(firm, Actor::Pop(2)), 1.0);
        near(market.ownership_of(firm, Actor::Pop(4)), 0.0);
        near(market.issued_ownership(firm), 1.0);
        near(market.ownership_of(OwnershipSubject::Land(9), Actor::Pop(4)), 1.0);
    }

    #[test]
    fn a_high_firm_draw_founds_a_pop_and_leaves_the_firm_dead() {
        // This draw is 0.5, and the firm path takes the half below that.
        let (mut market, mut pops, mut firms, factuals) = wealthy_restart();
        let mut rng = FixedRng(1 << 63);
        let id = market.found_pop(&mut pops, &mut firms, &factuals, &mut rng, 2).unwrap();

        assert_eq!(id, 2);
        assert_eq!(
            market.founding_day(),
            Some(FoundingDay {
                chance: 1.0,
                event: FoundingEvent::New { parent: 1, process: 1, id: 2 },
            }),
        );
        assert_eq!(pops[&2].job.lines[0].process, 1);
        assert_eq!(pops[&2].job.lines[0].target, Some(0.0));
        near(saved(&pops[&2], GRAIN), 4.0);
        near(pops[&1].demographics.household.count, 20.0);
        near(pops[&2].demographics.household.count, 5.0);
        assert!(firms[&2].is_dead());
        assert_eq!(firms[&2].production_line[0].target, Some(4.0));
        near(firm_qty(&firms[&2], WOOD), 7.0);
        near(firm_qty(&firms[&2], GRAIN), 0.0);
        let firm = OwnershipSubject::Firm(2);
        near(market.ownership_of(firm, Actor::Pop(4)), 1.0);
        near(market.ownership_of(firm, Actor::Pop(2)), 0.0);
        near(market.issued_ownership(firm), 1.0);
    }

    #[test]
    fn a_pop_under_three_times_the_bar_does_not_restart() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        market.firms.insert(2);
        let mut pops = HashMap::from([(1, member(25.0, 20.0, 10.0, 10.0))]);
        let mut firms = HashMap::from([(2, dead_mill(2, 7.0))]);
        let outlook = market.founding_outlook(&pops, &firms, &factuals);
        assert_eq!(outlook.prospects.len(), 1);
        assert_eq!(outlook.prospects[0].firm, None);
        near(outlook.prospects[0].chance, 1.0);
        let mut rng = FixedRng(0);
        let id = market.found_pop(&mut pops, &mut firms, &factuals, &mut rng, 2).unwrap();

        assert_eq!(id, 2);
        assert_eq!(pops[&2].job.lines[0].process, 1);
        assert!(firms[&2].is_dead());
        near(market.issued_ownership(OwnershipSubject::Firm(2)), 0.0);
        assert!(market.pops.contains(&2));
    }

    #[test]
    fn outlook_splits_a_hit_between_the_pop_and_the_dead_firm() {
        let factuals = world(1.0, vec![bake(1, BREAD)]);
        let mut market = market_of(&[1]);
        market.firms.extend([1, 2, 5]);
        let pops = HashMap::from([(1, member(25.0, 100.0, 100.0, 10.0))]);
        let mut live = Firm::new(1, "live".into(), 1, Hex::new(0, 0));
        live.dead = false;
        let firms = HashMap::from([(1, live), (2, dead_mill(2, 7.0)), (5, dead_mill(5, 1.0))]);
        let outlook = market.founding_outlook(&pops, &firms, &factuals);
        let firm_rows: Vec<_> = outlook.prospects.iter().filter(|row| row.firm.is_some()).collect();
        let pop_rows: Vec<_> = outlook.prospects.iter().filter(|row| row.firm.is_none()).collect();
        assert_eq!(firm_rows.len(), 1);
        assert_eq!(firm_rows[0].firm, Some(2));
        assert_eq!(firm_rows[0].parent, 1);
        assert_eq!(firm_rows[0].process, 0);
        assert_eq!(firm_rows[0].joins, None);
        near(firm_rows[0].chance, 0.5);
        assert_eq!(pop_rows.len(), 1);
        assert_eq!(pop_rows[0].process, 1);
        near(pop_rows[0].chance, 0.5);
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
    fn a_transfer_moves_the_part_and_a_land_share_stays_separate() {
        let mut market = Market::new(1);
        let firm = OwnershipSubject::Firm(1);
        let land = OwnershipSubject::Land(7);
        market.issue_ownership(firm, 30, 1.0);
        market.issue_ownership(land, 2, 1.0);
        market.transfer_ownership(firm, 30, 8, 0.25);
        near(market.ownership_of(firm, Actor::Pop(30)), 0.75);
        near(market.ownership_of(firm, Actor::Pop(8)), 0.25);
        near(market.issued_ownership(firm), 1.0);
        near(market.ownership_of(land, Actor::Pop(2)), 1.0);
        near(market.issued_ownership(land), 1.0);
        assert!(!market.is_owned(OwnershipSubject::Land(8)));
    }
}

#[cfg(test)]
mod pay_should {
    use std::collections::{HashMap, HashSet};

    use hexx::Hex;

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
        let mut firm = Firm::new(1, "agri".into(), 1, Hex::new(0, 0));
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
    fn a_land_share_is_not_paid() {
        let mut market = priced();
        market.issue_ownership(OwnershipSubject::Land(7), 2, 1.0);
        let mut firms = [stocked(100.0, 10.0)];
        let mut pops = [Pop::new(2)];
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
        let mut firm = Firm::new(1, "agri".into(), 1, Hex::new(0, 0));
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

#[cfg(test)]
mod release_should {
    use std::collections::HashMap;

    use hexx::Hex;

    use super::{Market, OwnershipSubject, PopExit};
    use crate::game::firm::Firm;
    use crate::game::pop::{Pop, PopPRow};

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    fn dead_firm(id: usize) -> Firm {
        let mut firm = Firm::new(id, format!("mill {id}"), 1, Hex::new(0, 0));
        firm.dead = true;
        firm
    }

    fn pop(id: usize, species: usize, grain: f64) -> Pop {
        let mut pop = Pop::new(id);
        pop.demographics.species = species;
        pop.demographics.household.count = 4.0;
        if grain > 0.0 {
            pop.property.insert(1, PopPRow::new(grain));
        }
        pop
    }

    #[test]
    fn a_dead_firms_owner_joins_another_pop() {
        let mut market = Market::new(1);
        market.firms.insert(1);
        market.pops.insert(6);
        market.pops.insert(30);
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 1.0);
        market.issue_ownership(OwnershipSubject::Land(4), 30, 1.0);
        let firms = HashMap::from([(1, dead_firm(1))]);
        let mut pops = HashMap::from([(6, pop(6, 0, 1.0)), (30, pop(30, 0, 5.0))]);
        market.release_dead_firms(&firms, &mut pops);
        assert!(!pops.contains_key(&30));
        assert!(!market.pops.contains(&30));
        assert!(market.pops.contains(&6));
        near(pops[&6].demographics.household.count, 8.0);
        near(pops[&6].property[&1].quantity, 6.0);
        near(pops[&6].migration, 4.0);
        assert!(!market.is_owned(OwnershipSubject::Firm(1)));
        near(market.ownership_of(OwnershipSubject::Land(4), crate::game::actor::Actor::Pop(6)), 1.0);
        assert_eq!(market.pop_removals().len(), 1);
        assert_eq!(market.pop_removals()[0].id, 30);
        assert_eq!(market.pop_removals()[0].why, PopExit::FirmDead { to: vec![6] });
    }

    #[test]
    fn owners_of_dead_firms_do_not_join_each_other() {
        let mut market = Market::new(1);
        market.firms.insert(1);
        market.firms.insert(2);
        market.pops.extend([6, 30, 32]);
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 1.0);
        market.issue_ownership(OwnershipSubject::Firm(2), 32, 1.0);
        let firms = HashMap::from([(1, dead_firm(1)), (2, dead_firm(2))]);
        let mut pops = HashMap::from([
            (6, pop(6, 0, 0.0)),
            (30, pop(30, 0, 2.0)),
            (32, pop(32, 0, 3.0)),
        ]);
        market.release_dead_firms(&firms, &mut pops);
        assert!(pops.contains_key(&6));
        assert!(!pops.contains_key(&30));
        assert!(!pops.contains_key(&32));
        near(pops[&6].demographics.household.count, 12.0);
        near(pops[&6].property[&1].quantity, 5.0);
    }

    #[test]
    fn the_same_species_is_taken_before_a_lower_id() {
        let mut market = Market::new(1);
        market.firms.insert(1);
        market.pops.extend([1, 5, 30]);
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 1.0);
        let firms = HashMap::from([(1, dead_firm(1))]);
        let mut pops = HashMap::from([
            (1, pop(1, 2, 0.0)),
            (5, pop(5, 1, 0.0)),
            (30, pop(30, 1, 0.0)),
        ]);
        market.release_dead_firms(&firms, &mut pops);
        assert!(pops.contains_key(&5));
        assert!(pops.contains_key(&1));
        assert!(!pops.contains_key(&30));
        near(pops[&5].demographics.household.count, 8.0);
        near(pops[&1].demographics.household.count, 4.0);
    }

    #[test]
    fn a_firm_with_no_other_pop_keeps_its_owner() {
        let mut market = Market::new(1);
        market.firms.insert(1);
        market.pops.insert(30);
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 1.0);
        let firms = HashMap::from([(1, dead_firm(1))]);
        let mut pops = HashMap::from([(30, pop(30, 0, 5.0))]);
        market.release_dead_firms(&firms, &mut pops);
        assert!(pops.contains_key(&30));
        assert!(market.pops.contains(&30));
        near(market.ownership_of(OwnershipSubject::Firm(1), crate::game::actor::Actor::Pop(30)), 1.0);
        assert!(market.pop_removals().is_empty());
    }

    #[test]
    fn a_living_firm_keeps_its_owner() {
        let mut market = Market::new(1);
        market.firms.insert(1);
        market.pops.extend([6, 30]);
        market.issue_ownership(OwnershipSubject::Firm(1), 30, 1.0);
        let mut firm = dead_firm(1);
        firm.dead = false;
        let firms = HashMap::from([(1, firm)]);
        let mut pops = HashMap::from([(6, pop(6, 0, 0.0)), (30, pop(30, 0, 5.0))]);
        market.release_dead_firms(&firms, &mut pops);
        assert!(pops.contains_key(&30));
        near(pops[&30].property[&1].quantity, 5.0);
        near(market.ownership_of(OwnershipSubject::Firm(1), crate::game::actor::Actor::Pop(30)), 1.0);
    }
}
