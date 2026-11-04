//! One sell book and a per-actor buy ladder.
//!
//! [`Market::buy_ladder`] replaces the eight-pass clear on the day path.
//! The morning card is the price. The buyer pays the freight.

use std::collections::{HashMap, HashSet};

use crate::game::actors::Actors;
use crate::game::deal::freight_bill;
use crate::game::desire::Desire;
use crate::game::factuals::Factuals;
use crate::game::good::TIME;
use crate::game::market::{tradeable, MassMiss, Market, MarketGood, MarketHistory};
use crate::game::pop::{DayFlow, Pop, SaveHold};
use crate::game::util::{random_index, whole_units_up};

/// Denominator of the desire cap. The best remaining good takes
/// `1 - 1 / (DESIRE_CAP_DENOM * goods left)`.
const DESIRE_CAP_DENOM: f64 = 2.0;

/// Luxury levels bought in one batch, as a divisor of the levels the purse can fund.
const LUXURY_BATCH_DIVISOR: f64 = 2.0;

/// Where the payment for one slice comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purse {
    /// Working currency.
    Working,
    /// Saved currency, excluding the meal and production inputs.
    Saved,
}

/// One seller's remaining units on the morning book.
struct Offer {
    seller: usize,
    good: usize,
    left: f64,
}

/// A tender the buyer or the seller can spend.
#[derive(Debug, Clone, Copy)]
struct Coin {
    id: usize,
    amv: f64,
    have: f64,
}

/// One good in a desire bucket, priced and capped for [`allocate_desire`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct BucketGood {
    /// Good id.
    pub id: usize,
    /// Satisfaction per unit.
    pub efficiency: f64,
    /// Fraction of the desire this good may fill. `1` is the whole desire.
    pub max_share: f64,
    /// Satisfaction already on hand.
    pub sat_on_hand: f64,
    /// Satisfaction the book can still sell.
    pub sat_available: f64,
}

/// The ladder keeps walking, or the actor is done for the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Continue,
    Stop,
}

/// Running totals for one good's book.
#[derive(Debug, Default)]
struct Tally {
    offered: HashMap<usize, f64>,
    requested: HashMap<usize, f64>,
    /// One-unit wants posted with no purchase, by good.
    unpaid: HashMap<usize, f64>,
    /// Pop and good pairs that already bid today.
    floored: HashSet<(usize, usize)>,
    exchanged: HashMap<usize, f64>,
    paid: HashMap<usize, HashMap<usize, f64>>,
    haul_blocked: HashMap<usize, bool>,
    /// Goods a buyer covered for one unit.
    affordable: HashSet<usize>,
    /// Goods a buyer asked for and could not cover one unit.
    broke: HashSet<usize>,
    /// Units listed at the open, before a reservation or a sale.
    initial: HashMap<usize, f64>,
    /// Units pulled off the book for a reservation.
    retracted: HashMap<usize, f64>,
    /// Units buyers would purchase if the purse were not the limit.
    maximal: HashMap<usize, f64>,
    /// Units actually purchased.
    real: HashMap<usize, f64>,
}

impl Market {
    /// # Buy Ladder
    ///
    /// Sells once, then walks each pop through the buy ladder.
    ///
    /// `actors` holds the pops. `factuals` names goods, savings, and freight.
    /// `rng` draws a seller who can cover the whole order. `_sought` is
    /// yesterday's unfilled bid. The morning book lists holdings and does
    /// not read it. The price is the card at the start of this call. Firms
    /// are not in the walk. A durability swap of saved currency follows the
    /// last actor. Nothing is returned.
    pub(crate) fn buy_ladder(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        rng: &mut impl rand::RngCore,
        _sought: &HashMap<usize, f64>,
    ) {
        let history = self.history();
        let currencies = currency_ids(self);
        let order = actor_order(actors, &self.pops, &history, &currencies);
        let mut book = build_book(actors, factuals, &order, &currencies);
        for offer in &book {
            actors
                .pop_mut(offer.seller)
                .note_sell_offer(offer.good, offer.left);
        }
        let mut tally = Tally::default();
        for offer in &book {
            *tally.offered.entry(offer.good).or_insert(0.0) += offer.left;
            *tally.initial.entry(offer.good).or_insert(0.0) += offer.left;
        }
        // Saved stock was listed with everything else. It is already reserved.
        for &id in &order {
            let saved: Vec<(usize, f64)> = actors
                .pop(id)
                .saved
                .iter()
                .map(|(&good, &units)| (good, units))
                .collect();
            for (good, units) in saved {
                retract(&mut book, &mut tally, id, good, units);
            }
        }
        for id in &order {
            walk_actor(
                actors,
                factuals,
                &history,
                &currencies,
                &mut book,
                &mut tally,
                *id,
                rng,
            );
        }
        swap_savings(
            actors,
            factuals,
            &history,
            &currencies,
            &order,
            &mut book,
            &mut tally,
            rng,
        );
        // Grain spent as tender, or set aside, is no longer an unsold offer.
        restate_offers(&book, &mut tally);
        let mut excess: HashMap<usize, f64> = HashMap::new();
        for offer in &book {
            if offer.left > 0.0 {
                *excess.entry(offer.good).or_insert(0.0) += offer.left;
            }
        }
        self.store_book_facts(
            std::mem::take(&mut tally.initial),
            &tally.retracted,
            excess,
            std::mem::take(&mut tally.maximal),
            std::mem::take(&mut tally.real),
        );
        self.flush_tally(&history, &tally);
    }

    /// # Flush Tally
    ///
    /// Writes the day's book onto the mass-close rows the night prices from.
    ///
    /// `history` is the morning card. `tally` is the offer, the ask, and what
    /// moved. A good that traded records that card. A haul that blocked a
    /// two-sided book is stored as [`MassMiss::Haul`]. A bid for a good that
    /// is not on the card opens a default row, so the night can price it.
    /// A good that was asked, and that no buyer covered for one unit, is
    /// stored for the night write.
    /// Nothing is returned.
    fn flush_tally(&mut self, history: &MarketHistory, tally: &Tally) {
        let mut goods: Vec<usize> = tally
            .offered
            .keys()
            .chain(tally.requested.keys())
            .chain(tally.unpaid.keys())
            .copied()
            .collect();
        goods.sort_unstable();
        goods.dedup();
        for good in goods {
            let offered = tally.offered.get(&good).copied().unwrap_or(0.0);
            let unpaid = tally.unpaid.get(&good).copied().unwrap_or(0.0);
            let requested = tally.requested.get(&good).copied().unwrap_or(0.0) + unpaid;
            if unpaid > 0.0 {
                self.add_unpaid_want(good, unpaid);
            }
            let exchanged = tally.exchanged.get(&good).copied().unwrap_or(0.0);
            let price = (exchanged >= 1.0).then(|| history.price(good));
            let miss = (exchanged < 1.0
                && offered >= 1.0
                && requested >= 1.0
                && tally.haul_blocked.get(&good).copied().unwrap_or(false))
            .then_some(MassMiss::Haul);
            self.remember_mass_close(good, offered, requested, exchanged, price, miss);
            // A bid opens a card row when the good is not listed yet.
            if requested >= 1.0 {
                self.goods.entry(good).or_insert_with(MarketGood::new);
            }
            if exchanged >= 1.0 {
                let empty = HashMap::new();
                let paid = tally.paid.get(&good).unwrap_or(&empty);
                self.note_mass_exchange(good, history.price(good), exchanged, paid);
            }
        }
        // Asked, and no buyer covered one unit.
        let mut unaffordable = HashSet::new();
        for good in tally.broke.difference(&tally.affordable) {
            unaffordable.insert(*good);
        }
        self.set_unaffordable(unaffordable);
    }
}

/// # Listed Units
///
/// Amount of `free` stock this actor posts.
///
/// A currency lists [`currency_holder_share`](crate::game::config::MarketConfig::currency_holder_share)
/// unless `producer` is set, which lists the producer share. Any other good
/// lists the free stock, floored. A pile under one unit lists nothing.
/// Returns that count.
pub(crate) fn listed_units(
    free: f64,
    currency: bool,
    producer: bool,
    holder_share: f64,
    producer_share: f64,
) -> f64 {
    if !free.is_finite() || free < 1.0 {
        return 0.0;
    }
    let free = free.floor();
    if !currency {
        return free;
    }
    let share = if producer { producer_share } else { holder_share };
    (free * share).floor().max(0.0)
}

/// # Desire Cap Share
///
/// Fraction of the remaining satisfaction the best remaining good may take.
///
/// `goods` is how many goods are still in the bucket. One good takes all of
/// the remainder. Two goods cap the best at three quarters. Five goods cap
/// the best at nine tenths.
pub(crate) fn desire_cap_share(goods: usize) -> f64 {
    if goods <= 1 {
        1.0
    } else {
        1.0 - 1.0 / (DESIRE_CAP_DENOM * goods as f64)
    }
}

/// # Luxury Batch
///
/// Levels to buy at once from the levels `affordable` the purse can fund.
///
/// The batch is half of that count, floored, and at least one when one level
/// fits. A purse that cannot fund one level buys none.
pub(crate) fn luxury_batch(affordable: f64) -> f64 {
    if !affordable.is_finite() || affordable < 1.0 {
        0.0
    } else {
        (affordable / LUXURY_BATCH_DIVISOR).floor().max(1.0)
    }
}

/// One good's opening bid and the part the book can sell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DesirePlan {
    /// Good id.
    pub id: usize,
    /// Satisfaction of the opening bid, before the book cuts it.
    pub ask: f64,
    /// Satisfaction the book can sell. The ladder buys this.
    pub buy: f64,
}

/// # Allocate Desire
///
/// Opening bid and purchasable satisfaction for each good, best first.
///
/// `budget` is the satisfaction still open. `goods` is the bucket in the
/// order to buy. The best remaining good takes [`desire_cap_share`] of the
/// budget, and satisfaction already on hand counts toward that cap. That
/// cap is the opening bid. The buy is the part of the bid the book can
/// fill. A shortfall stays in the budget for the next good. The last good
/// takes the remainder. A good with no opening bid is left out. Returns
/// those rows.
pub(crate) fn allocate_desire(budget: f64, goods: &[BucketGood]) -> Vec<DesirePlan> {
    if !budget.is_finite() || budget <= 1e-9 || goods.is_empty() {
        return Vec::new();
    }
    let cap_sat = budget * desire_cap_share(goods.len());
    let good = &goods[0];
    let cap_sat = cap_sat.min(budget * good.max_share.max(0.0));
    let need = (cap_sat - good.sat_on_hand.max(0.0)).max(0.0);
    let buy = need.min(good.sat_available.max(0.0)).max(0.0);
    let used = (good.sat_on_hand.max(0.0) + buy).min(budget);
    let mut out = Vec::new();
    if need > 1e-9 {
        out.push(DesirePlan {
            id: good.id,
            ask: need,
            buy,
        });
    }
    if goods.len() == 1 {
        return out;
    }
    out.extend(allocate_desire((budget - used).max(0.0), &goods[1..]));
    out
}

/// # Walk Actor
///
/// One pop's buy walk, then a bid on each currency that walk did not post.
///
/// A reservable savings target the purse cannot finish stops the walk. A
/// cheaper coin with nothing more durable for sale stays working, and the
/// walk continues. A purse that cannot cover the flat fee also stops the
/// walk. Nothing is returned.
fn walk_actor(
    actors: &mut Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    book: &mut Vec<Offer>,
    tally: &mut Tally,
    id: usize,
    rng: &mut impl rand::RngCore,
) {
    if !actors.pops.contains_key(&id) {
        return;
    }
    // Opening bids already recorded, by tier, desire, and good.
    let mut noted: HashMap<(usize, usize, usize), f64> = HashMap::new();
    walk_purchases(
        actors, factuals, history, currencies, book, tally, id, &mut noted, rng,
    );
    // A currency this pop did not bid still puts one unit on the book.
    note_currency_wants(actors, factuals, tally, currencies, id);
}

/// # Walk Purchases
///
/// Buys the job's inputs, the basic meal, and savings for one pop.
///
/// The arguments are the same ones [`walk_actor`] received. `noted` is the
/// opening bid already recorded. The job's inputs are bought before the
/// basic desires. Those desires then run in their stored order, then one
/// savings level, then another. A basic level the purse cannot buy stops
/// before that save. The input buy has already run. Common buys its
/// desires, saves, buys any inputs still open, and saves again. An open
/// common level still saves and still buys those inputs. A savings target
/// the purse cannot finish still buys the job's inputs. A pop with no line
/// running then leaves the later tiers unbought. A line already set to run
/// keeps buying them. Luxury stays on its own shorter walk. Nothing is returned.
fn walk_purchases(
    actors: &mut Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    book: &mut Vec<Offer>,
    tally: &mut Tally,
    id: usize,
    noted: &mut HashMap<(usize, usize, usize), f64>,
    rng: &mut impl rand::RngCore,
) {
    let producing = is_producing(actors.pop(id));
    for tier in 0..2 {
        // The job is funded before the basic meal. Common still buys desires first.
        let mut inputs = Step::Continue;
        if tier == 0 {
            inputs = buy_inputs(actors, factuals, history, currencies, book, tally, id, rng);
        }
        let bought = buy_tier(
            actors, factuals, history, currencies, book, tally, id, tier, 1.0, noted, rng,
        );
        // An unpaid basic level stops the later tiers. The input buy already ran.
        if tier == 0 && bought == Step::Stop {
            wish_unreached(actors.pop(id), history, tally, tier);
            return;
        }
        // One level after this tier's desires. A missed save still leaves the input buy.
        let level = tier as f64 * 2.0 + 1.0;
        let saved = save_currency(actors, factuals, history, currencies, book, tally, id, level);
        if tier != 0 {
            inputs = buy_inputs(actors, factuals, history, currencies, book, tally, id, rng);
        }
        if inputs == Step::Stop {
            wish_unreached(actors.pop(id), history, tally, tier);
            return;
        }
        // A running line spends the coin the save left in the purse.
        if !producing && saved == Step::Stop {
            wish_unreached(actors.pop(id), history, tally, tier);
            return;
        }
        if save_currency(actors, factuals, history, currencies, book, tally, id, level + 1.0)
            == Step::Stop
            && !producing
        {
            wish_unreached(actors.pop(id), history, tally, tier);
            return;
        }
    }
    buy_luxury(actors, factuals, history, currencies, book, tally, id, noted, rng);
}

/// # Is Producing
///
/// True when a line is set to run today.
///
/// `pop` is the buyer. A finite target above zero counts. Returns that answer.
fn is_producing(pop: &Pop) -> bool {
    pop.job.lines.iter().any(|line| {
        line.target
            .is_some_and(|target| target.is_finite() && target > 0.0)
    })
}

/// # Note Currency Wants
///
/// Bids one unit of each currency this pop has not already bid.
///
/// `currencies` are the filled tender slots. `tally` is the day's book.
/// A good already bid by `id` is left as it stands. Nothing is returned.
fn note_currency_wants(
    actors: &mut Actors,
    factuals: &Factuals,
    tally: &mut Tally,
    currencies: &[usize],
    id: usize,
) {
    for &good in currencies {
        note_unpaid_want(actors, factuals, tally, id, good);
    }
}

/// # Note Desire Wants
///
/// Bids one unit of each good in `desire` that this pop has not already bid.
///
/// `tier` and `noted` are the opening bid already recorded for that desire.
/// A good already in either is left as it stands. Nothing is returned.
fn note_desire_wants(
    actors: &mut Actors,
    factuals: &Factuals,
    tally: &mut Tally,
    id: usize,
    desire: &Desire,
    tier: usize,
    noted: &HashMap<(usize, usize, usize), f64>,
) {
    let mut goods: Vec<usize> = desire.target.iter().map(|target| target.good).collect();
    goods.sort_unstable();
    goods.dedup();
    for good in goods {
        if noted.contains_key(&(tier, desire.demo_desire_id, good)) {
            continue;
        }
        note_unpaid_want(actors, factuals, tally, id, good);
    }
}

/// # Note Unpaid Want
///
/// Bids one unit of `good` for pop `id` without buying it.
///
/// `tally` is the day's book. A good this pop already bid is left as it
/// stands, and so is a good that cannot be traded. The unit is stored apart
/// from a purchase bid. The good is noted broke. Nothing is returned.
fn note_unpaid_want(
    actors: &mut Actors,
    factuals: &Factuals,
    tally: &mut Tally,
    id: usize,
    good: usize,
) {
    if !tradeable(factuals, good) || !tally.floored.insert((id, good)) {
        return;
    }
    *tally.unpaid.entry(good).or_insert(0.0) += 1.0;
    tally.broke.insert(good);
    actors.pop_mut(id).note_buy_offer(good, 1.0);
}

/// # Buy Tier
///
/// Buys one level of each desire in `tier`, early desires first.
///
/// `levels` scales the desire amount. `noted` is the opening bid already
/// recorded for this actor. Each desire records the buy it would make if
/// the purse were not the limit, then reserves or buys inside the bucket.
/// A desire that does not finish does not skip the later desires. On a
/// basic stop those later desires do not add another unpaid bid. Returns
/// [`Step::Stop`] when the tier is unfinished.
fn buy_tier(
    actors: &mut Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    book: &mut Vec<Offer>,
    tally: &mut Tally,
    id: usize,
    tier: usize,
    levels: f64,
    noted: &mut HashMap<(usize, usize, usize), f64>,
    rng: &mut impl rand::RngCore,
) -> Step {
    let mut order: Vec<usize> = (0..actors.pop(id).desires.get(tier).map(|row| row.len()).unwrap_or(0))
        .collect();
    order.sort_by(|&left, &right| {
        actors.pop(id).desires[tier][left].cmp_order(&actors.pop(id).desires[tier][right])
    });
    let mut stopped = false;
    let mut note_shortfall = true;
    for index in order {
        let desire = actors.pop(id).desires[tier][index].clone();
        wish_desire(actors.pop(id), history, tally, &desire, levels);
        if fill_desire(
            actors,
            factuals,
            history,
            currencies,
            book,
            tally,
            id,
            &desire,
            levels,
            tier,
            noted,
            note_shortfall,
            rng,
        ) == Step::Stop
        {
            stopped = true;
            // The desire the walk stops on already bid. A later one does not.
            if tier == 0 {
                note_shortfall = false;
            }
        }
    }
    if stopped { Step::Stop } else { Step::Continue }
}

/// # Fill Desire
///
/// Fills `levels` of `desire` for pop `id`, highest satisfaction per AMV first.
///
/// The winning good is chosen before any stock is set aside. Units already
/// held are reserved, and that reservation leaves the sell line. The rest
/// of the slice is the order, and an empty book still posts it. A buy that
/// cannot finish leaves that good and the next good in the bucket is tried.
/// The slice and each good's minimum share round up to a whole unit.
/// `tier` and `noted` are the bid already recorded for this desire.
/// `note_shortfall` posts one unpaid unit when the purse cannot buy. A
/// later desire in a stopped basic tier leaves that bid off. Returns
/// [`Step::Stop`] when a basic level is still open and the purse cannot
/// buy, and when a common or luxury level is still open. A funded basic
/// gap continues. A currency in this desire can be paid with other goods.
fn fill_desire(
    actors: &mut Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    book: &mut Vec<Offer>,
    tally: &mut Tally,
    id: usize,
    desire: &Desire,
    levels: f64,
    tier: usize,
    noted: &mut HashMap<(usize, usize, usize), f64>,
    note_shortfall: bool,
    rng: &mut impl rand::RngCore,
) -> Step {
    if !desire.amount.is_finite() || desire.amount <= 0.0 || levels <= 0.0 {
        return Step::Continue;
    }
    let level_budget = desire.amount * levels;
    let targets: Vec<usize> = desire.target.iter().map(|target| target.good).collect();
    let mut blocked = Vec::new();
    let mut open = level_budget;
    for _ in 0..64 {
        if open <= 1e-9 {
            break;
        }
        let Some(pick) = pick_slice(history, desire, open, level_budget, &blocked) else {
            break;
        };
        if pick.units < 1.0 {
            blocked.push(pick.good);
            continue;
        }
        // Own stock covers what it can. The sell line drops with it.
        let meal = actors.pop(id).meal.get(&pick.good).copied().unwrap_or(0.0);
        let working = actors.pop(id).working_units(pick.good);
        let (reserve, buy) = cover_units(pick.units, meal, working);
        let moved = actors
            .pop_mut(id)
            .stash_working(pick.good, reserve, SaveHold::Meal);
        retract(book, tally, id, pick.good, moved);
        let mut got = 0.0;
        if buy >= 1.0 {
            let funded = fee_open(actors.pop(id), factuals)
                && is_walk_funded(actors, factuals, history, currencies, book, id, &targets);
            if funded {
                // The slice is the order. An empty book still posts it.
                got = exchange(
                    actors,
                    factuals,
                    history,
                    currencies,
                    book,
                    tally,
                    id,
                    pick.good,
                    buy,
                    Purse::Working,
                    None,
                    false,
                    rng,
                );
                *tally.real.entry(pick.good).or_insert(0.0) += got;
            }
        }
        // Meal already set aside covers this slice too. A short buy leaves the rest open.
        let applied = (meal.max(0.0) + moved + got).min(pick.units.max(0.0));
        open = (open - applied * pick.efficiency).max(0.0);
        blocked.push(pick.good);
    }
    let purse_closed = !fee_open(actors.pop(id), factuals)
        || !is_walk_funded(actors, factuals, history, currencies, book, id, &targets);
    if open > 1e-6 && purse_closed && note_shortfall {
        // The purse stopped on this desire. One unit of each good still bids.
        note_desire_wants(actors, factuals, tally, id, desire, tier, noted);
    }
    if open <= 1e-6 {
        return Step::Continue;
    }
    // Common and luxury report the open level. The walk still moves on.
    // Basic stops only when the purse cannot buy.
    if tier > 0 || purse_closed {
        Step::Stop
    } else {
        Step::Continue
    }
}

/// One bucket good chosen for this pass, and the whole units that cover it.
struct SlicePick {
    good: usize,
    efficiency: f64,
    units: f64,
}

/// # Rank Bucket
///
/// The desire's goods ordered by satisfaction per AMV, skipping `blocked`.
///
/// Stock on hand does not change the rank or the slice. A non-positive
/// card is left out. Returns nothing when no good remains.
fn rank_bucket(
    history: &MarketHistory,
    desire: &Desire,
    blocked: &[usize],
) -> Option<Vec<BucketGood>> {
    let mut goods = Vec::new();
    for target in &desire.target {
        if target.efficiency <= 0.0 || blocked.contains(&target.good) {
            continue;
        }
        let price = history.price(target.good);
        if !price.is_finite() || price <= 0.0 {
            continue;
        }
        goods.push(BucketGood {
            id: target.good,
            efficiency: target.efficiency,
            max_share: target.cap.max(0.0),
            sat_on_hand: 0.0,
            sat_available: f64::MAX / 4.0,
        });
    }
    goods.sort_by(|left, right| {
        let score = |good: &BucketGood| good.efficiency / history.price(good.id);
        score(right)
            .partial_cmp(&score(left))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.id.cmp(&right.id))
    });
    if goods.is_empty() { None } else { Some(goods) }
}

/// # Minimum Share
///
/// Smallest share of a bucket each good is aimed at.
///
/// `goods` is how many targets have a positive efficiency. One good has no
/// separate floor. Any wider bucket floors each good at
/// `1 / (2 * goods)`. Returns that share.
fn minimum_share(goods: usize) -> f64 {
    if goods <= 1 {
        0.0
    } else {
        1.0 / (DESIRE_CAP_DENOM * goods as f64)
    }
}

/// # Slice Units
///
/// Whole units that cover `slice` satisfaction at `efficiency`.
///
/// The count rounds away from zero, so a fractional eat still has a whole
/// unit on hand. A non-positive slice or efficiency is 0. Returns that count.
fn slice_units(slice: f64, efficiency: f64) -> f64 {
    if !slice.is_finite() || slice <= 0.0 || !efficiency.is_finite() || efficiency <= 0.0 {
        return 0.0;
    }
    whole_units_up(slice / efficiency)
}

/// # Cover Units
///
/// Units to reserve and units still to buy for a slice of `target` units.
///
/// `meal` is already set aside. `working` is free stock. The reserve is
/// the part of the gap that stock covers. The buy is the rest, rounded up
/// to a whole unit. Returns `(reserve, buy)`.
fn cover_units(target: f64, meal: f64, working: f64) -> (f64, f64) {
    let gap = (target - meal.max(0.0)).max(0.0);
    let reserve = gap.min(working.max(0.0));
    let buy = whole_units_up((gap - reserve).max(0.0));
    (reserve, buy)
}

/// # Pick Slice
///
/// The next bucket good and the whole units that cover its slice.
///
/// `open` is the satisfaction still unfilled. `level_budget` is the full
/// level, used for the minimum share. `blocked` goods are skipped. The
/// best remaining good takes its cap, and at least its minimum share when
/// that share sits inside the cap. Both amounts round up to a whole unit.
/// Returns nothing when no priced good remains.
fn pick_slice(
    history: &MarketHistory,
    desire: &Desire,
    open: f64,
    level_budget: f64,
    blocked: &[usize],
) -> Option<SlicePick> {
    let goods = rank_bucket(history, desire, blocked)?;
    let plan = allocate_desire(open, &goods);
    let row = plan.first()?;
    let target = desire.target.iter().find(|target| target.good == row.id)?;
    if target.efficiency <= 0.0 {
        return None;
    }
    let cap = target.cap.max(0.0);
    let goods_n = desire
        .target
        .iter()
        .filter(|target| target.efficiency > 0.0)
        .count();
    let min_sat = level_budget * minimum_share(goods_n).min(cap);
    let slice = row.ask.max(min_sat);
    Some(SlicePick {
        good: row.id,
        efficiency: target.efficiency,
        units: slice_units(slice, target.efficiency),
    })
}

/// # Wish Desire
///
/// Adds the units `pop` would buy of `desire` if the purse were not the limit.
///
/// The bucket order matches [`fill_desire`]. Stock on hand is reserved in
/// the count and is not a buy. Each later good sees that reservation.
/// `levels` scales the amount. Nothing is returned.
fn wish_desire(
    pop: &Pop,
    history: &MarketHistory,
    tally: &mut Tally,
    desire: &Desire,
    levels: f64,
) {
    if !desire.amount.is_finite() || desire.amount <= 0.0 || levels <= 0.0 {
        return;
    }
    let level_budget = desire.amount * levels;
    let mut open = level_budget;
    let mut blocked = Vec::new();
    let mut have: HashMap<usize, f64> = HashMap::new();
    for target in &desire.target {
        let meal = pop.meal.get(&target.good).copied().unwrap_or(0.0);
        have.insert(target.good, pop.working_units(target.good) + meal.max(0.0));
    }
    for _ in 0..64 {
        if open <= 1e-9 {
            break;
        }
        let Some(pick) = pick_slice(history, desire, open, level_budget, &blocked) else {
            break;
        };
        if pick.units < 1.0 {
            blocked.push(pick.good);
            continue;
        }
        let owned = have.get(&pick.good).copied().unwrap_or(0.0);
        let (reserve, buy) = cover_units(pick.units, 0.0, owned);
        if let Some(left) = have.get_mut(&pick.good) {
            *left = (*left - reserve).max(0.0);
        }
        if buy >= 1.0 {
            *tally.maximal.entry(pick.good).or_insert(0.0) += buy;
        }
        // Money is not the limit here, so the buy is treated as filled.
        open = (open - pick.units * pick.efficiency).max(0.0);
        blocked.push(pick.good);
    }
}

/// # Wish Tier
///
/// Records one unlimited buy of every desire in `tier`.
///
/// `levels` scales each amount. A missing tier does nothing. Nothing is returned.
fn wish_tier(pop: &Pop, history: &MarketHistory, tally: &mut Tally, tier: usize, levels: f64) {
    let Some(desires) = pop.desires.get(tier) else {
        return;
    };
    for desire in desires {
        wish_desire(pop, history, tally, desire, levels);
    }
}

/// # Wish Unreached
///
/// Records the unlimited buy of the tiers `pop` did not walk.
///
/// `stopped` is the tier the walk left. Tier 0 also records common.
/// Luxury records one level above the stock already held. Nothing is returned.
fn wish_unreached(pop: &Pop, history: &MarketHistory, tally: &mut Tally, stopped: usize) {
    if stopped < 1 {
        wish_tier(pop, history, tally, 1, 1.0);
    }
    wish_tier(pop, history, tally, 2, levels_held(pop) + 1.0);
}

/// # Reserve Planned Inputs
///
/// Stashes the morning plan's input reserve and takes it off the book.
///
/// `actors` holds the pop. `book` is today's sell book. `id` is the pop.
/// The reserve is cleared, so a later tier does not stash it again.
/// Nothing is returned.
fn reserve_planned_inputs(actors: &mut Actors, book: &mut [Offer], tally: &mut Tally, id: usize) {
    let pending: Vec<(usize, f64)> = actors
        .pop(id)
        .job
        .input_reserve
        .iter()
        .map(|(&good, &units)| (good, units))
        .collect();
    actors.pop_mut(id).job.input_reserve.clear();
    for (good, units) in pending {
        let moved = actors
            .pop_mut(id)
            .stash_working(good, units, SaveHold::Input);
        retract(book, tally, id, good, moved);
    }
}

/// # Buy Inputs
///
/// Buys the job's purchase list and sets the units aside.
///
/// Inputs the morning plan already named move out of working stock and off
/// the book first. The list is then cut to the runs the working purse can
/// fund. A time ceiling the purse cannot start is cleared. A list that
/// still asks for a whole unit after the buy stops the ladder. Returns
/// that step.
fn buy_inputs(
    actors: &mut Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    book: &mut Vec<Offer>,
    tally: &mut Tally,
    id: usize,
    rng: &mut impl rand::RngCore,
) -> Step {
    reserve_planned_inputs(actors, book, tally, id);
    // The list before the purse cut is the buy money would not limit.
    let wanted: Vec<(usize, f64)> = actors
        .pop(id)
        .job
        .shopping_keys()
        .into_iter()
        .map(|good| (good, whole_units_up(actors.pop(id).job.shopping_of(good))))
        .filter(|(_, units)| *units >= 1.0)
        .collect();
    for (good, units) in &wanted {
        *tally.maximal.entry(*good).or_insert(0.0) += units;
    }
    let on_hand = funding_on_hand(actors.pop(id));
    actors
        .pop_mut(id)
        .job
        .fit_shopping_to_purse(&on_hand, factuals, history, currencies);
    let goods = actors.pop(id).job.shopping_keys();
    for good in goods {
        let need = whole_units_up(actors.pop(id).job.shopping_of(good));
        if need < 1.0 {
            continue;
        }
        if !fee_open(actors.pop(id), factuals)
            || !is_walk_funded(actors, factuals, history, currencies, book, id, &[good])
        {
            return Step::Stop;
        }
        let got = exchange(
            actors,
            factuals,
            history,
            currencies,
            book,
            tally,
            id,
            good,
            need,
            Purse::Working,
            None,
            true,
            rng,
        );
        *tally.real.entry(good).or_insert(0.0) += got;
        let parked = actors.pop_mut(id).stash_working(good, got, SaveHold::Input);
        actors.pop_mut(id).job.note_purchase(good, parked);
    }
    if actors.pop(id).job.shopping_unfilled() {
        Step::Stop
    } else {
        Step::Continue
    }
}

/// # Buy Luxury
///
/// Buys half the luxury levels the purse can fund, then saves that many steps.
///
/// The walk repeats when the savings target is met and another level fits.
/// A batch that does not finish saves the step already reached and leaves.
/// An empty luxury tier saves the fourth flat step once. Nothing is returned.
fn buy_luxury(
    actors: &mut Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    book: &mut Vec<Offer>,
    tally: &mut Tally,
    id: usize,
    noted: &mut HashMap<(usize, usize, usize), f64>,
    rng: &mut impl rand::RngCore,
) {
    let empty = actors
        .pop(id)
        .desires
        .get(2)
        .is_none_or(|tier| tier.is_empty());
    if empty {
        let _ = save_currency(actors, factuals, history, currencies, book, tally, id, 4.0);
        return;
    }
    let mut steps = 0.0;
    for _ in 0..64 {
        let cost = luxury_level_cost(actors.pop(id), history, currencies, factuals);
        if cost <= 1e-9 {
            break;
        }
        let affordable = (purse_amv(actors.pop(id), history, currencies) / cost).floor();
        let batch = luxury_batch(affordable);
        if batch < 1.0 {
            // One more level is the buy the purse did not fund.
            wish_tier(actors.pop(id), history, tally, 2, levels_held(actors.pop(id)) + 1.0);
            break;
        }
        // Levels already on hand stay in the budget, so this batch is new stock.
        let held = levels_held(actors.pop(id));
        if buy_tier(
            actors,
            factuals,
            history,
            currencies,
            book,
            tally,
            id,
            2,
            held + batch,
            noted,
            rng,
        ) == Step::Stop
        {
            // The batch is not a finished level. Save the step already reached.
            let _ = save_currency(
                actors, factuals, history, currencies, book, tally, id, 3.0 + steps,
            );
            return;
        }
        steps += batch;
        if save_currency(actors, factuals, history, currencies, book, tally, id, 3.0 + steps)
            == Step::Stop
        {
            return;
        }
    }
}

/// # Luxury Level Cost
///
/// AMV of one more luxury level plus one savings step.
///
/// The basket is what [`allocate_desire`] would buy if the book were deep,
/// priced on `history`. Returns that cost. An empty tier is the savings step.
fn luxury_level_cost(
    pop: &Pop,
    history: &MarketHistory,
    _currencies: &[usize],
    factuals: &Factuals,
) -> f64 {
    let mut cost = 0.0;
    let Some(tier) = pop.desires.get(2) else {
        return savings_step(pop, factuals, history);
    };
    for desire in tier {
        if desire.amount <= 0.0 {
            continue;
        }
        let mut goods = Vec::new();
        for target in &desire.target {
            let price = history.price(target.good);
            if target.efficiency <= 0.0 || !price.is_finite() || price <= 0.0 {
                continue;
            }
            let on_hand = pop.working_units(target.good)
                + pop.meal.get(&target.good).copied().unwrap_or(0.0);
            // Whole levels already held do not cheapen the next level.
            let raw = on_hand * target.efficiency;
            let toward_next = raw - (raw / desire.amount).floor() * desire.amount;
            goods.push(BucketGood {
                id: target.good,
                efficiency: target.efficiency,
                max_share: target.cap.max(0.0),
                sat_on_hand: toward_next.max(0.0),
                sat_available: f64::MAX / 4.0,
            });
        }
        goods.sort_by(|left, right| {
            let score = |good: &BucketGood| good.efficiency / history.price(good.id);
            score(right)
                .partial_cmp(&score(left))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.id.cmp(&right.id))
        });
        for plan in allocate_desire(desire.amount, &goods) {
            let efficiency = desire
                .target
                .iter()
                .find(|target| target.good == plan.id)
                .map(|target| target.efficiency)
                .unwrap_or(1.0);
            cost += plan.buy / efficiency * history.price(plan.id);
        }
    }
    cost + savings_step(pop, factuals, history)
}

/// # Levels Held
///
/// Luxury levels the stock on hand already covers, across every luxury desire.
///
/// The count is the smallest desire, so a later batch still buys for the
/// desire that is behind. A tier with no positive desire is `0`.
fn levels_held(pop: &Pop) -> f64 {
    let Some(tier) = pop.desires.get(2) else {
        return 0.0;
    };
    let mut held = f64::INFINITY;
    let mut any = false;
    for desire in tier {
        if !desire.amount.is_finite() || desire.amount <= 0.0 {
            continue;
        }
        any = true;
        let mut sat = 0.0;
        for target in &desire.target {
            let on_hand = pop.working_units(target.good)
                + pop.meal.get(&target.good).copied().unwrap_or(0.0);
            sat += on_hand * target.efficiency.max(0.0);
        }
        held = held.min((sat / desire.amount).floor().max(0.0));
    }
    if any { held } else { 0.0 }
}

/// # Save Currency
///
/// Moves reservable working currency into the saved pile until `tiers` steps are met.
///
/// `tiers` counts the levels already reached, including this one. `book` is
/// today's sell book. A coin is reservable through [`is_reservable_coin`].
/// Tender the plan kept off the sell book stays in the purse. A cheaper coin
/// with nothing more durable for sale stays in the purse, and that step
/// continues. A short reservable pool is written up to the gap. The actor
/// stops when that pool leaves the level short. A target already on hand
/// continues. An empty reservable pool continues and writes nothing.
/// Returns that step. Units that move also leave `book`.
fn save_currency(
    actors: &mut Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    book: &mut [Offer],
    tally: &mut Tally,
    id: usize,
    tiers: f64,
) -> Step {
    let target = savings_step(actors.pop(id), factuals, history) * tiers;
    if saved_currency_amv(actors.pop(id), history, currencies) + 1e-6 >= target {
        return Step::Continue;
    }
    let gap = target - saved_currency_amv(actors.pop(id), history, currencies);
    // Tender kept for a run stays working, so this pile does not spend it.
    let coins = coins_of(actors.pop(id), currencies, history, Purse::Working, None, false);
    // Grain is not parked for a gold swap when gold is not for sale.
    let mut reservable = Vec::new();
    for coin in coins {
        if is_reservable_coin(factuals, currencies, actors, book, coin.id) {
            reservable.push(coin);
        }
    }
    // With nothing reservable, the purse stays and the walk continues.
    if reservable.is_empty() {
        return Step::Continue;
    }
    let mut remain = gap;
    let mut order = reservable;
    order.sort_by(|left, right| {
        left.amv
            .partial_cmp(&right.amv)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.id.cmp(&right.id))
    });
    for coin in order {
        if remain <= 1e-6 {
            break;
        }
        let n = (remain / coin.amv).ceil().min(coin.have).max(1.0);
        let moved = actors
            .pop_mut(id)
            .stash_working(coin.id, n, SaveHold::Currency);
        retract(book, tally, id, coin.id, moved);
        remain -= moved * coin.amv;
    }
    if saved_currency_amv(actors.pop(id), history, currencies) + 1e-6 >= target {
        Step::Continue
    } else {
        Step::Stop
    }
}

/// # Is Reservable Coin
///
/// True when `coin` may be moved into the savings pile.
///
/// `factuals` supplies durability. `currencies` are the tender slots.
/// `actors` and `book` are today's sell book. A coin with nothing strictly
/// more durable in the slots is reservable. A cheaper coin is reservable
/// when a more durable one still has a unit on offer, so the swap can buy
/// it. A cheaper coin with nothing more durable for sale stays in the purse.
/// Returns that answer.
fn is_reservable_coin(
    factuals: &Factuals,
    currencies: &[usize],
    actors: &Actors,
    book: &[Offer],
    coin: usize,
) -> bool {
    let own = durability(factuals, coin);
    let mut upgrade_exists = false;
    for &other in currencies {
        if other == coin {
            continue;
        }
        if durability(factuals, other) <= own + 1e-12 {
            continue;
        }
        if stock_of(actors, book, other) >= 1.0 {
            return true;
        }
        upgrade_exists = true;
    }
    !upgrade_exists
}

/// # Restate Offers
///
/// Sets each good's recorded offer to what is still for sale plus what exchanged.
///
/// `book` is the sell book after the walk and the swap. `tally` is the
/// morning listing. Units spent as tender, and units moved into savings,
/// have already left `book`. They are not an unsold offer. Nothing is returned.
fn restate_offers(book: &[Offer], tally: &mut Tally) {
    let mut left: HashMap<usize, f64> = HashMap::new();
    for offer in book {
        if offer.left > 0.0 {
            *left.entry(offer.good).or_insert(0.0) += offer.left;
        }
    }
    let mut goods: Vec<usize> = tally
        .offered
        .keys()
        .chain(left.keys())
        .copied()
        .collect();
    goods.sort_unstable();
    goods.dedup();
    for good in goods {
        let exchanged = tally.exchanged.get(&good).copied().unwrap_or(0.0);
        let remain = left.get(&good).copied().unwrap_or(0.0);
        tally.offered.insert(good, remain + exchanged);
    }
}

/// # Swap Savings
///
/// Buys a strictly more durable currency with saved currency.
///
/// Higher AMV per unit breaks a tie between two more-durable coins. The meal
/// and production inputs stay put. The same match and freight apply. Nothing
/// is returned.
fn swap_savings(
    actors: &mut Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    order: &[usize],
    book: &mut Vec<Offer>,
    tally: &mut Tally,
    rng: &mut impl rand::RngCore,
) {
    for &id in order {
        let mut sources: Vec<usize> = currencies
            .iter()
            .copied()
            .filter(|&good| actors.pop(id).saved_currency_units(good) >= 1.0)
            .collect();
        sources.sort_by(|&left, &right| {
            durability(factuals, left)
                .partial_cmp(&durability(factuals, right))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.cmp(&right))
        });
        for source in sources {
            let mut targets: Vec<usize> = currencies
                .iter()
                .copied()
                .filter(|&good| {
                    good != source
                        && durability(factuals, good) > durability(factuals, source) + 1e-12
                        && stock_of(actors, book, good) >= 1.0
                })
                .collect();
            targets.sort_by(|&left, &right| {
                durability(factuals, right)
                    .partial_cmp(&durability(factuals, left))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        history
                            .price(right)
                            .partial_cmp(&history.price(left))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then(left.cmp(&right))
            });
            for target in targets {
                let price = history.price(target);
                let source_amv = history.price(source);
                if price <= 0.0 || source_amv <= 0.0 {
                    continue;
                }
                let have = actors.pop(id).saved_currency_units(source);
                let affordable = have * source_amv / price;
                let units = affordable.min(stock_of(actors, book, target)).floor();
                if units < 1.0 {
                    continue;
                }
                // The durable coin the savings can reach. A deeper purse is not priced here.
                *tally.maximal.entry(target).or_insert(0.0) += units;
                let got = exchange(
                    actors,
                    factuals,
                    history,
                    currencies,
                    book,
                    tally,
                    id,
                    target,
                    units,
                    Purse::Saved,
                    Some(source),
                    false,
                    rng,
                );
                *tally.real.entry(target).or_insert(0.0) += got;
            }
        }
    }
}

/// # Exchange
///
/// Buys up to `units` of `good` for pop `buyer`.
///
/// A seller who can cover the remainder is drawn at random. Otherwise the
/// biggest seller fills what they can, and the next seller pays the flat fee
/// again. `purse` is where payment comes from. `only` limits that purse to
/// one currency. `spend_unlisted` lets that payment use tender the plan kept.
/// The buyer pays the freight. Returns the units bought.
fn exchange(
    actors: &mut Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    book: &mut Vec<Offer>,
    tally: &mut Tally,
    buyer: usize,
    good: usize,
    units: f64,
    purse: Purse,
    only: Option<usize>,
    spend_unlisted: bool,
    rng: &mut impl rand::RngCore,
) -> f64 {
    let price = history.price(good);
    if !price.is_finite() || price <= 0.0 {
        return 0.0;
    }
    let mut need = units.floor();
    if need < 1.0 {
        return 0.0;
    }
    *tally.requested.entry(good).or_insert(0.0) += need;
    tally.floored.insert((buyer, good));
    actors.pop_mut(buyer).note_buy_offer(good, need);
    let mut bought = 0.0;
    let mut skipped = Vec::new();
    while need >= 1.0 {
        let Some(seller) = pick_seller(actors, book, good, buyer, need, &skipped, rng) else {
            break;
        };
        let room = stock_of_seller(actors, book, seller, good).min(need).floor();
        if room < 1.0 {
            skipped.push(seller);
            continue;
        }
        // A seller who cannot make change is skipped. The buyer's own purse
        // or freight stops the order.
        // A currency is paid with other goods when the tender purse cannot cover it.
        let pay_with_goods = currencies.contains(&good);
        match largest_slice(
            actors, factuals, history, currencies, buyer, seller, good, room, purse, only,
            spend_unlisted, pay_with_goods,
        ) {
            Ok(slice) => {
                // This buyer covered one unit.
                if slice.units >= 1.0 {
                    tally.affordable.insert(good);
                }
                commit_slice(actors, factuals, book, buyer, seller, good, &slice, purse);
                *tally.exchanged.entry(good).or_insert(0.0) += slice.units;
                let paid = tally.paid.entry(good).or_insert_with(HashMap::new);
                for &(id, qty) in &slice.pay {
                    *paid.entry(id).or_insert(0.0) += qty;
                }
                bought += slice.units;
                need -= slice.units;
            }
            Err(SliceMiss::Freight) => {
                tally.haul_blocked.insert(good, true);
                break;
            }
            Err(SliceMiss::Purse) => {
                // One unit did not clear.
                tally.broke.insert(good);
                break;
            }
            Err(SliceMiss::Change) => skipped.push(seller),
        }
    }
    bought
}

/// Why one slice could not be planned.
enum SliceMiss {
    /// The buyer cannot cover the price in whole coins.
    Purse,
    /// The buyer cannot cover the freight.
    Freight,
    /// The seller cannot return the change this payment needs.
    Change,
}

/// A slice that clears: units, payment, change, and the freight bill.
struct Slice {
    units: f64,
    pay: Vec<(usize, f64)>,
    change: Vec<(usize, f64)>,
    bill: f64,
}

/// # Largest Slice
///
/// The most units of `good` this buyer can pay for, freight included.
///
/// The search starts at `room` and steps down by whole units.
/// `spend_unlisted` lets the buyer pay with tender the plan kept.
/// `pay_with_goods` lets other goods cover a currency. Returns nothing
/// when one unit does not clear.
fn largest_slice(
    actors: &Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    buyer: usize,
    seller: usize,
    good: usize,
    room: f64,
    purse: Purse,
    only: Option<usize>,
    spend_unlisted: bool,
    pay_with_goods: bool,
) -> Result<Slice, SliceMiss> {
    let mut lo = 1.0;
    let mut hi = room.floor();
    let mut best: Option<Slice> = None;
    let mut miss = SliceMiss::Purse;
    while lo <= hi {
        let mid = ((lo + hi) / 2.0).floor();
        if mid < 1.0 {
            break;
        }
        match plan_slice(
            actors, factuals, history, currencies, buyer, seller, good, mid, purse, only,
            spend_unlisted, pay_with_goods,
        ) {
            Ok(slice) => {
                best = Some(slice);
                lo = mid + 1.0;
            }
            Err(reason) => {
                miss = reason;
                if mid <= 1.0 {
                    break;
                }
                hi = mid - 1.0;
            }
        }
    }
    best.ok_or(miss)
}

/// # Plan Slice
///
/// Payment, change, and freight for `units` of `good`, without moving stock.
///
/// `spend_unlisted` lets the buyer pay with tender the plan kept.
/// `pay_with_goods` is set when `good` is a currency. A currency purse that
/// cannot cover the bill then pays with other goods, even when another
/// tender is in stock. The buyer pays in whole coins. A good that is
/// not a currency is paid only in tender. Returns nothing when the purse
/// or the freight cannot cover that count.
fn plan_slice(
    actors: &Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    buyer: usize,
    seller: usize,
    good: usize,
    units: f64,
    purse: Purse,
    only: Option<usize>,
    spend_unlisted: bool,
    pay_with_goods: bool,
) -> Result<Slice, SliceMiss> {
    let due = history.price(good) * units;
    let pay_ids: Vec<usize> = match only {
        Some(id) => vec![id],
        None => currencies.to_vec(),
    };
    let mut buyer_coins = coins_of(
        actors.pop(buyer),
        &pay_ids,
        history,
        purse,
        Some(good),
        spend_unlisted,
    );
    // The currency purse cannot cover it, so other goods pay.
    if pay_with_goods && lay_tender(due, &buyer_coins).is_none() {
        buyer_coins.extend(goods_coins(
            actors.pop(buyer),
            factuals,
            history,
            currencies,
            purse,
            spend_unlisted,
        ));
    }
    let Some(pay) = lay_tender(due, &buyer_coins) else {
        return Err(SliceMiss::Purse);
    };
    let paid = paid_value(&pay, &buyer_coins);
    let smallest = smallest_amv(&pay, &buyer_coins);
    let seller_coins = coins_of(
        actors.pop(seller),
        currencies,
        history,
        Purse::Working,
        Some(good),
        false,
    );
    let paid_in_goods = pay.iter().any(|(id, _)| !currencies.contains(id));
    let change = if paid_in_goods {
        Vec::new()
    } else {
        lay_change((paid - due).max(0.0), &seller_coins, smallest, good)
    };
    let bill = freight_for(factuals, history.friction, good, units, &pay, &change);
    let cover = cover_after(actors.pop(buyer), factuals, purse, &pay);
    if bill > 0.0 && cover + 1e-6 < bill {
        return Err(SliceMiss::Freight);
    }
    // Change has to sit in the seller's working stock.
    for &(id, qty) in &change {
        if actors.pop(seller).working_units(id) + 1e-6 < qty {
            return Err(SliceMiss::Change);
        }
    }
    Ok(Slice {
        units,
        pay,
        change,
        bill,
    })
}

/// # Commit Slice
///
/// Moves `slice` between `buyer` and `seller` and drops those units off the book.
///
/// The buyer pays the freight. Payment follows `purse`. Change returns to
/// that same purse. Nothing is returned.
fn commit_slice(
    actors: &mut Actors,
    factuals: &Factuals,
    book: &mut [Offer],
    buyer: usize,
    seller: usize,
    good: usize,
    slice: &Slice,
    purse: Purse,
) {
    for &(id, qty) in &slice.pay {
        let moved = match purse {
            Purse::Working => actors.pop_mut(buyer).take_working(id, qty),
            Purse::Saved => actors.pop_mut(buyer).spend_saved_currency(id, qty),
        };
        actors.pop_mut(seller).give_working(id, moved);
        actors.pop_mut(buyer).note_flow(DayFlow::Paid, id, moved);
        actors.pop_mut(seller).note_flow(DayFlow::Received, id, moved);
        reduce_book(book, buyer, id, moved);
    }
    let moved = actors.pop_mut(seller).take_working(good, slice.units);
    match purse {
        Purse::Working => actors.pop_mut(buyer).give_working(good, moved),
        Purse::Saved => actors.pop_mut(buyer).give_saved(good, moved),
    }
    actors.pop_mut(buyer).note_flow(DayFlow::Got, good, moved);
    actors.pop_mut(seller).note_flow(DayFlow::Gave, good, moved);
    reduce_book(book, seller, good, moved);
    for &(id, qty) in &slice.change {
        let back = actors.pop_mut(seller).take_working(id, qty);
        match purse {
            Purse::Working => actors.pop_mut(buyer).give_working(id, back),
            Purse::Saved => actors.pop_mut(buyer).give_saved(id, back),
        }
        actors.pop_mut(buyer).note_change_in(id, back);
        actors.pop_mut(seller).note_change_out(id, back);
        reduce_book(book, seller, id, back);
    }
    if slice.bill > 0.0 {
        // Freight leaves working stock, so the live offer drops with it.
        let ids: Vec<usize> = actors.pop(buyer).property.keys().copied().collect();
        let before: HashMap<usize, f64> = ids
            .iter()
            .map(|&good| (good, actors.pop(buyer).working_units(good)))
            .collect();
        actors.pop_mut(buyer).spend_freight(slice.bill, factuals);
        for (good, prev) in before {
            let spent = (prev - actors.pop(buyer).working_units(good)).max(0.0);
            reduce_book(book, buyer, good, spent);
        }
    }
}

/// # Lay Tender
///
/// Whole coins that cover `due`, largest denomination first.
///
/// One extra coin covers a remainder. Returns nothing when the coins cannot
/// reach `due`.
fn lay_tender(due: f64, coins: &[Coin]) -> Option<Vec<(usize, f64)>> {
    if !due.is_finite() || due <= 0.0 {
        return Some(Vec::new());
    }
    let mut order = coins.to_vec();
    order.sort_by(|left, right| {
        right
            .amv
            .partial_cmp(&left.amv)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.id.cmp(&right.id))
    });
    let mut remain = due;
    let mut pay = Vec::new();
    for coin in &order {
        if remain <= 1e-6 {
            break;
        }
        if !coin.amv.is_finite() || coin.amv <= 0.0 {
            continue;
        }
        let n = (remain / coin.amv).floor().min(coin.have);
        if n >= 1.0 {
            pay.push((coin.id, n));
            remain -= n * coin.amv;
        }
    }
    if remain > 1e-6 {
        let extra = covering_spare(coins, &pay, remain)?;
        if let Some(row) = pay.iter_mut().find(|(id, _)| *id == extra) {
            row.1 += 1.0;
        } else {
            pay.push((extra, 1.0));
        }
    }
    Some(pay)
}

/// # Covering Spare
///
/// The lowest-AMV coin that still has a unit after `pay` and covers `remain`.
///
/// A coin whose unit is short of `remain` is skipped. Returns nothing when
/// no spare coin covers it.
fn covering_spare(coins: &[Coin], pay: &[(usize, f64)], remain: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for coin in coins {
        let used = pay
            .iter()
            .find(|(id, _)| *id == coin.id)
            .map(|(_, qty)| *qty)
            .unwrap_or(0.0);
        if coin.have - used < 1.0 || coin.amv + 1e-6 < remain {
            continue;
        }
        match best {
            None => best = Some((coin.id, coin.amv)),
            Some((_, amv)) if coin.amv < amv => best = Some((coin.id, coin.amv)),
            Some((id, amv)) if (coin.amv - amv).abs() <= 1e-12 && coin.id < id => {
                best = Some((coin.id, coin.amv));
            }
            Some(_) => {}
        }
    }
    best.map(|(id, _)| id)
}

/// # Lay Change
///
/// Smaller coins the seller can return against `over`.
///
/// A coin counts when its AMV is strictly below `smallest_paid`. The count
/// is floored, so the seller keeps a remainder that does not divide. `skip`
/// is the good being sold and is not returned as change.
fn lay_change(over: f64, coins: &[Coin], smallest_paid: f64, skip: usize) -> Vec<(usize, f64)> {
    if !over.is_finite() || over <= 1e-6 || !smallest_paid.is_finite() {
        return Vec::new();
    }
    let mut order: Vec<Coin> = coins
        .iter()
        .copied()
        .filter(|coin| coin.id != skip && coin.amv + 1e-9 < smallest_paid)
        .collect();
    order.sort_by(|left, right| {
        right
            .amv
            .partial_cmp(&left.amv)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut remain = over;
    let mut out = Vec::new();
    for coin in order {
        let n = (remain / coin.amv).floor().min(coin.have);
        if n >= 1.0 {
            out.push((coin.id, n));
            remain -= n * coin.amv;
        }
    }
    out
}

/// # Pick Seller
///
/// A random seller who can cover `need`, or the biggest seller otherwise.
///
/// The buyer is skipped. A tie on size keeps the lower id. Returns nothing
/// when no seller has a unit.
fn pick_seller(
    actors: &Actors,
    book: &[Offer],
    good: usize,
    buyer: usize,
    need: f64,
    skip: &[usize],
    rng: &mut impl rand::RngCore,
) -> Option<usize> {
    let mut covers = Vec::new();
    let mut best: Option<(usize, f64)> = None;
    for offer in book.iter().filter(|offer| {
        offer.good == good && offer.seller != buyer && !skip.contains(&offer.seller)
    }) {
        let stock = stock_of_seller(actors, book, offer.seller, good);
        if stock < 1.0 {
            continue;
        }
        if stock + 1e-9 >= need {
            covers.push(offer.seller);
        }
        match best {
            None => best = Some((offer.seller, stock)),
            Some((_, size)) if stock > size + 1e-9 => best = Some((offer.seller, stock)),
            Some((id, size)) if (stock - size).abs() <= 1e-9 && offer.seller < id => {
                best = Some((offer.seller, stock));
            }
            Some(_) => {}
        }
    }
    if !covers.is_empty() {
        covers.sort_unstable();
        covers.dedup();
        return Some(covers[random_index(rng, covers.len())]);
    }
    best.map(|(id, _)| id)
}

/// # Listed Holding
///
/// True when `good` stays off the morning book.
///
/// An untradeable good stays off. Time and a skill list even when the
/// good is untradeable. A good missing from `factuals` lists. Returns
/// that answer.
fn stays_off_book(factuals: &Factuals, good: usize) -> bool {
    if good == TIME {
        return false;
    }
    match factuals.get_good(good) {
        Some(row) if row.is_skill() => false,
        Some(row) => !row.is_buyable(),
        None => false,
    }
}

/// # Build Book
///
/// One offer per pop per good, from working stock and saved stock.
///
/// Time and skills are listed. An untradeable good that is not a skill
/// stays off. A currency lists the whole pile. Tender the plan kept is
/// listed with the rest. A positive holding lists, including a fraction.
/// Returns the offers.
fn build_book(
    actors: &Actors,
    factuals: &Factuals,
    order: &[usize],
    _currencies: &[usize],
) -> Vec<Offer> {
    let mut book = Vec::new();
    for &id in order {
        let pop = actors.pop(id);
        let mut goods: Vec<usize> = pop
            .property
            .keys()
            .chain(pop.saved.keys())
            .copied()
            .collect();
        goods.sort_unstable();
        goods.dedup();
        for good in goods {
            if stays_off_book(factuals, good) {
                continue;
            }
            let saved = pop.saved.get(&good).copied().unwrap_or(0.0).max(0.0);
            let units = pop.working_units(good).max(0.0) + saved;
            if units.is_finite() && units > 0.0 {
                book.push(Offer {
                    seller: id,
                    good,
                    left: units,
                });
            }
        }
    }
    book
}

/// # Actor Order
///
/// Pop ids, highest currency AMV per household first.
///
/// A tie keeps the lower id. Owned currency is working stock plus the saved
/// pile.
fn actor_order(
    actors: &Actors,
    pops: &std::collections::HashSet<usize>,
    history: &MarketHistory,
    currencies: &[usize],
) -> Vec<usize> {
    let mut ids: Vec<usize> = pops.iter().copied().filter(|id| actors.pops.contains_key(id)).collect();
    ids.sort_by(|&left, &right| {
        liquidity(actors.pop(right), history, currencies)
            .partial_cmp(&liquidity(actors.pop(left), history, currencies))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.cmp(&right))
    });
    ids
}

/// # Liquidity
///
/// Currency AMV this pop owns, divided by its household count.
fn liquidity(pop: &Pop, history: &MarketHistory, currencies: &[usize]) -> f64 {
    let mut value = 0.0;
    for &good in currencies {
        let owned = pop.working_units(good) + pop.saved.get(&good).copied().unwrap_or(0.0);
        value += owned.max(0.0) * history.price(good).max(0.0);
    }
    value / pop.household_count()
}

/// # Tier Spread
///
/// Mean card price of the desire tiers, each tier weighted the same.
///
/// `pop` supplies the desires. `history` prices each good. A tier's price
/// is the mean of its goods, each good once. A non-positive price is left
/// out. A tier with no priced good is left out of the mean. A pop with
/// none returns 1, so one savings level stays one unit of AMV per
/// household. Returns that price.
fn tier_spread(pop: &Pop, history: &MarketHistory) -> f64 {
    let mut sum = 0.0;
    let mut tiers = 0.0;
    for desires in &pop.desires {
        let mut goods = Vec::new();
        for desire in desires {
            for target in &desire.target {
                if target.efficiency <= 0.0 || goods.contains(&target.good) {
                    continue;
                }
                goods.push(target.good);
            }
        }
        let mut price_sum = 0.0;
        let mut count = 0.0;
        for good in goods {
            let price = history.price(good);
            if price.is_finite() && price > 0.0 {
                price_sum += price;
                count += 1.0;
            }
        }
        if count > 0.0 {
            let mean = price_sum / count;
            if mean.is_finite() {
                sum += mean;
                tiers += 1.0;
            }
        }
    }
    if tiers > 0.0 && sum.is_finite() {
        sum / tiers
    } else {
        1.0
    }
}

/// # Savings Step
///
/// AMV of one savings level for this pop.
///
/// The level is [`crate::game::config::PopConfig::savings_amv`] times
/// [`tier_spread`] times the household count. The levels add. A product
/// that is not finite is 0. Returns that AMV.
fn savings_step(pop: &Pop, factuals: &Factuals, history: &MarketHistory) -> f64 {
    let step = factuals.config.pop.savings_amv * tier_spread(pop, history) * pop.household_count();
    if step.is_finite() {
        step.max(0.0)
    } else {
        0.0
    }
}

/// # Purse AMV
///
/// Working currency valued on `history`.
fn purse_amv(pop: &Pop, history: &MarketHistory, currencies: &[usize]) -> f64 {
    coins_of(pop, currencies, history, Purse::Working, None, false)
        .iter()
        .map(|coin| coin.amv * coin.have)
        .sum()
}

/// # Saved Currency AMV
///
/// Saved currency, excluding the meal and production inputs, valued on `history`.
fn saved_currency_amv(pop: &Pop, history: &MarketHistory, currencies: &[usize]) -> f64 {
    coins_of(pop, currencies, history, Purse::Saved, None, false)
        .iter()
        .map(|coin| coin.amv * coin.have)
        .sum()
}

/// # Funding On Hand
///
/// Working stock plus production inputs, for the purchase cap.
///
/// `pop` is the buyer. Saved currency that is not an input stays out.
/// Returns units by good.
fn funding_on_hand(pop: &Pop) -> HashMap<usize, f64> {
    let mut on_hand = HashMap::new();
    for &good in pop.property.keys() {
        let qty = pop.working_units(good);
        if qty > 0.0 {
            on_hand.insert(good, qty);
        }
    }
    for (&good, &qty) in &pop.job.input_hold {
        if qty > 0.0 {
            *on_hand.entry(good).or_insert(0.0) += qty;
        }
    }
    on_hand
}

/// # Has Currency
///
/// True when the pop holds a whole coin of a currency with a positive card.
fn has_currency(pop: &Pop, currencies: &[usize], history: &MarketHistory) -> bool {
    currencies.iter().any(|&good| {
        history.price(good) > 0.0 && pop.working_units(good) >= 1.0
    })
}

/// # Is Other Tender Stocked
///
/// True when a tender other than `good` still has a unit on the book.
///
/// `actors` and `book` are the morning sell book. `currencies` are the
/// tender slots. `good` is the currency being bought. Returns that answer.
fn is_other_tender_stocked(
    actors: &Actors,
    book: &[Offer],
    currencies: &[usize],
    good: usize,
) -> bool {
    currencies
        .iter()
        .any(|&id| id != good && stock_of(actors, book, id) >= 1.0)
}

/// # Is Goods Payment
///
/// True when `good` may pay for a currency.
///
/// `factuals` names the good. A currency is left out by the caller. A
/// skill, a transport good, and an untradeable good are left out. A missing
/// good is left out. Returns that answer.
fn is_goods_payment(factuals: &Factuals, good: usize) -> bool {
    factuals.get_good(good).is_some_and(|row| {
        row.is_buyable() && !row.is_skill() && !row.is_transport()
    })
}

/// # Goods Coins
///
/// Other goods `pop` can spend from `purse` on a currency.
///
/// `factuals` names those goods. `history` prices them. `currencies` are
/// the tender slots and are left out. A saved purse spends nothing here.
/// Working units [`crate::game::job::Job::unlisted_of`] keeps are left out
/// unless `spend_unlisted` is set. Returns one coin per good, lowest id first.
fn goods_coins(
    pop: &Pop,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    purse: Purse,
    spend_unlisted: bool,
) -> Vec<Coin> {
    if purse != Purse::Working {
        return Vec::new();
    }
    let mut ids: Vec<usize> = pop.property.keys().copied().collect();
    ids.sort_unstable();
    let mut coins = Vec::new();
    for id in ids {
        if currencies.contains(&id) || !is_goods_payment(factuals, id) {
            continue;
        }
        let amv = history.price(id);
        if !amv.is_finite() || amv <= 0.0 {
            continue;
        }
        let mut have = pop.working_units(id);
        if !spend_unlisted {
            have = (have - pop.job.unlisted_of(id)).max(0.0);
        }
        let have = have.floor();
        if have >= 1.0 {
            coins.push(Coin { id, amv, have });
        }
    }
    coins
}

/// # Is Walk Funded
///
/// True when `id` can pay for a buy of one of `targets`.
///
/// A whole coin of a priced currency is enough for any buy. Other goods
/// pay for a currency in `targets` that still has a unit on the book, even
/// when another tender is in stock. They also fund the walk when some
/// currency is in stock and no other tender is. `actors`, `factuals`,
/// `history`, `currencies`, and `book` are that morning. Returns that answer.
fn is_walk_funded(
    actors: &Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    book: &[Offer],
    id: usize,
    targets: &[usize],
) -> bool {
    let pop = actors.pop(id);
    if has_currency(pop, currencies, history) {
        return true;
    }
    let goods_pay = goods_coins(pop, factuals, history, currencies, Purse::Working, true)
        .iter()
        .any(|coin| coin.have >= 1.0);
    if !goods_pay {
        return false;
    }
    // This buy's currency can be paid with those goods.
    if targets.iter().any(|&good| {
        currencies.contains(&good) && stock_of(actors, book, good) >= 1.0
    }) {
        return true;
    }
    currencies.iter().any(|&good| {
        stock_of(actors, book, good) >= 1.0
            && !is_other_tender_stocked(actors, book, currencies, good)
    })
}

/// # Fee Open
///
/// True when the pop can pay the flat buy fee.
///
/// A world with no transport good charges nothing.
fn fee_open(pop: &Pop, factuals: &Factuals) -> bool {
    if !factuals.goods.values().any(|good| good.is_transport()) {
        return true;
    }
    let flat = factuals.config.market.transaction_cost;
    flat <= 0.0 || pop.transport_cover(factuals) + 1e-6 >= flat
}

/// # Coins Of
///
/// Currencies `pop` can spend from `purse`.
///
/// `skip` is left out, so the good being bought is not spent as its own price.
/// Working units [`crate::game::job::Job::unlisted_of`] keeps are left out
/// unless `spend_unlisted` is set.
fn coins_of(
    pop: &Pop,
    currencies: &[usize],
    history: &MarketHistory,
    purse: Purse,
    skip: Option<usize>,
    spend_unlisted: bool,
) -> Vec<Coin> {
    let mut coins = Vec::new();
    for &id in currencies {
        if Some(id) == skip {
            continue;
        }
        let amv = history.price(id);
        if !amv.is_finite() || amv <= 0.0 {
            continue;
        }
        let mut have = match purse {
            Purse::Working => pop.working_units(id),
            Purse::Saved => pop.saved_currency_units(id),
        };
        if purse == Purse::Working && !spend_unlisted {
            have = (have - pop.job.unlisted_of(id)).max(0.0);
        }
        let have = have.floor();
        if have >= 1.0 {
            coins.push(Coin { id, amv, have });
        }
    }
    coins
}

/// # Paid Value
///
/// AMV of `pay` at the coin values in `coins`.
fn paid_value(pay: &[(usize, f64)], coins: &[Coin]) -> f64 {
    pay.iter()
        .map(|(id, qty)| {
            let amv = coins
                .iter()
                .find(|coin| coin.id == *id)
                .map(|coin| coin.amv)
                .unwrap_or(0.0);
            qty * amv
        })
        .sum()
}

/// # Smallest AMV
///
/// The lowest AMV among coins in `pay`, or infinity when `pay` is empty.
fn smallest_amv(pay: &[(usize, f64)], coins: &[Coin]) -> f64 {
    pay.iter()
        .filter(|(_, qty)| *qty >= 1.0)
        .filter_map(|(id, _)| coins.iter().find(|coin| coin.id == *id).map(|coin| coin.amv))
        .fold(f64::INFINITY, f64::min)
}

/// # Freight For
///
/// The transport bill for the good, the payment, and the change.
///
/// `friction` is the market's bulk multiplier. A world with no transport
/// good bills nothing.
fn freight_for(
    factuals: &Factuals,
    friction: f64,
    good: usize,
    units: f64,
    pay: &[(usize, f64)],
    change: &[(usize, f64)],
) -> f64 {
    let mut basket = HashMap::from([(good, units)]);
    for &(id, qty) in pay.iter().chain(change.iter()) {
        if qty > 0.0 && id != good {
            *basket.entry(id).or_insert(0.0) += qty;
        }
    }
    freight_bill(factuals, friction, &basket)
}

/// # Cover After
///
/// Transport cover still on hand once `pay` has left working stock.
///
/// A saved purse does not spend working transport as the tender.
fn cover_after(pop: &Pop, factuals: &Factuals, purse: Purse, pay: &[(usize, f64)]) -> f64 {
    let mut cover = pop.transport_cover(factuals);
    if purse == Purse::Working {
        for &(id, qty) in pay {
            if let Some(good) = factuals.get_good(id) {
                cover -= good.transport_cover(qty);
            }
        }
    }
    cover.max(0.0)
}

/// # Currency Ids
///
/// Filled tender slots, in slot order, each good once.
fn currency_ids(market: &Market) -> Vec<usize> {
    let mut ids = Vec::new();
    for id in market.currencies().into_iter().flatten() {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// # Durability
///
/// `1 - decay` for `good`, or 1 when the good is missing.
fn durability(factuals: &Factuals, good: usize) -> f64 {
    factuals
        .get_good(good)
        .map(|row| row.durability())
        .unwrap_or(1.0)
}

/// # Stock Of
///
/// Units of `good` the book can still sell, capped by working stock.
fn stock_of(actors: &Actors, book: &[Offer], good: usize) -> f64 {
    book.iter()
        .filter(|offer| offer.good == good)
        .map(|offer| stock_of_seller(actors, book, offer.seller, good))
        .sum()
}

/// # Stock Of Seller
///
/// Units of `good` one seller can still deliver.
fn stock_of_seller(actors: &Actors, book: &[Offer], seller: usize, good: usize) -> f64 {
    let listed: f64 = book
        .iter()
        .filter(|offer| offer.seller == seller && offer.good == good)
        .map(|offer| offer.left)
        .sum();
    listed
        .min(actors.pop(seller).working_units(good))
        .floor()
        .max(0.0)
}

/// # Listed Left
///
/// Units of `good` still on `actor`'s line.
fn listed_left(book: &[Offer], actor: usize, good: usize) -> f64 {
    book.iter()
        .filter(|offer| offer.seller == actor && offer.good == good)
        .map(|offer| offer.left)
        .sum()
}

/// # Retract
///
/// Drops `units` of `good` from `actor`'s offer and records the pull.
///
/// `tally` keeps the reservation. A line shorter than `units` records only
/// what it held. Nothing is returned.
fn retract(book: &mut [Offer], tally: &mut Tally, actor: usize, good: usize, units: f64) {
    if !units.is_finite() || units <= 0.0 {
        return;
    }
    let before = listed_left(book, actor, good);
    reduce_book(book, actor, good, units);
    let gone = (before - listed_left(book, actor, good)).max(0.0);
    if gone > 0.0 {
        *tally.retracted.entry(good).or_insert(0.0) += gone;
    }
}

/// # Reduce Book
///
/// Drops `units` of `good` from `actor`'s offer.
fn reduce_book(book: &mut [Offer], actor: usize, good: usize, units: f64) {
    let mut left = units;
    for offer in book
        .iter_mut()
        .filter(|offer| offer.seller == actor && offer.good == good)
    {
        if left <= 0.0 {
            break;
        }
        let take = left.min(offer.left).max(0.0);
        offer.left -= take;
        left -= take;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use crate::game::actors::Actors;
    use crate::game::demographic_source::DemographicSource;
    use crate::game::desire::{Desire, DesireTarget, DesireTargetType};
    use crate::game::factuals::Factuals;
    use crate::game::good::{Good, GoodTag, TIME};
    use crate::game::job::{Job, JobLine};
    use crate::game::ladder::{
        allocate_desire, desire_cap_share, listed_units, luxury_batch, BucketGood,
    };
    use crate::game::market::{Market, MarketGood};
    use crate::game::pop::Pop;
    use crate::game::pop_property::PopPRow;
    use crate::game::process::{InputType, Process, ProcessInput, ProcessOutput};
    use crate::game::scalingfactor::ScalingFactor;

    fn good(id: usize, name: &str, decay: f64) -> Good {
        Good {
            id,
            name: name.to_string(),
            class: None,
            decay_rate: decay,
            decay_result: HashMap::new(),
            mass: 1.0,
            volume: 0.0,
            tags: HashSet::new(),
            categories: Vec::new(),
        }
    }

    fn desire(id: usize, good_id: usize, amount: f64) -> Desire {
        Desire {
            source: DemographicSource::Species(0),
            demo_desire_id: id,
            priority: 0,
            target: vec![DesireTarget::new(good_id, DesireTargetType::Consume, 1.0)],
            amount,
            satisfaction: 0.0,
            target_taken: vec![0.0],
            category: None,
            effect: vec![],
            scalar: ScalingFactor::Fixed(1.0),
            decay: 0.0,
        }
    }

    fn at(market: &mut Market, id: usize, amv: f64) {
        market
            .goods
            .insert(id, MarketGood::new().with_amv(amv).with_salability(1.0));
    }

    fn bucket(id: usize, available: f64) -> BucketGood {
        BucketGood {
            id,
            efficiency: 1.0,
            max_share: 1.0,
            sat_on_hand: 0.0,
            sat_available: available,
        }
    }

    #[test]
    fn desire_cap_rolls_a_short_book_into_the_next_good() {
        assert!((desire_cap_share(2) - 0.75).abs() < 1e-12);
        assert!((desire_cap_share(5) - 0.9).abs() < 1e-12);
        let two = allocate_desire(10.0, &[bucket(1, 100.0), bucket(2, 100.0)]);
        assert_eq!((two[0].id, two[0].buy, two[0].ask), (1, 7.5, 7.5));
        assert_eq!((two[1].id, two[1].buy, two[1].ask), (2, 2.5, 2.5));
        let short = allocate_desire(10.0, &[bucket(1, 4.0), bucket(2, 100.0)]);
        assert_eq!((short[0].id, short[0].buy), (1, 4.0));
        assert!((short[0].ask - 7.5).abs() < 1e-12);
        assert_eq!((short[1].id, short[1].buy, short[1].ask), (2, 6.0, 6.0));
        let empty = allocate_desire(10.0, &[bucket(1, 0.0), bucket(2, 0.0)]);
        assert!((empty[0].ask - 7.5).abs() < 1e-12);
        assert!(empty[0].buy.abs() < 1e-12);
        assert!((empty[1].ask - 10.0).abs() < 1e-12);
        assert!(empty[1].buy.abs() < 1e-12);
        let five = allocate_desire(
            10.0,
            &[
                bucket(1, 100.0),
                bucket(2, 100.0),
                bucket(3, 100.0),
                bucket(4, 100.0),
                bucket(5, 100.0),
            ],
        );
        assert!((five[0].buy - 9.0).abs() < 1e-12);
    }

    #[test]
    fn luxury_batch_buys_half_the_levels_and_at_least_one() {
        assert_eq!(luxury_batch(0.5), 0.0);
        assert_eq!(luxury_batch(1.0), 1.0);
        assert_eq!(luxury_batch(5.0), 2.0);
    }

    /// # Coins Of Leaves The Tender A Plan Kept
    ///
    /// Five grain with three kept spends two, unless the caller is paying
    /// for a run the plan already funded.
    #[test]
    fn coins_of_leaves_the_tender_a_plan_kept() {
        let mut pop = Pop::new(1);
        pop.property.insert(1, PopPRow::new(5.0));
        pop.job.unlisted.insert(1, 3.0);
        let mut history = crate::game::market::MarketHistory::new();
        history.prices.insert(1, 1.0);

        let kept = super::coins_of(&pop, &[1], &history, super::Purse::Working, None, false);
        let open = super::coins_of(&pop, &[1], &history, super::Purse::Working, None, true);

        assert_eq!(kept.len(), 1);
        assert!((kept[0].have - 2.0).abs() < 1e-9);
        assert_eq!(open.len(), 1);
        assert!((open[0].have - 5.0).abs() < 1e-9);
    }

    /// # Build Book Lists The Tender A Plan Kept
    ///
    /// Five grain lists five, including the three the plan kept. Bread lists
    /// its four units.
    #[test]
    fn build_book_lists_the_tender_a_plan_kept() {
        let mut seller = Pop::new(1);
        seller.property.insert(1, PopPRow::new(5.0));
        seller.property.insert(2, PopPRow::new(4.0));
        seller.job.unlisted.insert(1, 3.0);
        let mut actors = Actors::new();
        actors.pops.insert(1, seller);
        let factuals = Factuals::new()
            .with_good(good(1, "grain", 0.0))
            .with_good(good(2, "bread", 0.0));

        let book = super::build_book(&actors, &factuals, &[1], &[1]);

        let grain = book.iter().find(|offer| offer.good == 1).map(|offer| offer.left);
        let bread = book.iter().find(|offer| offer.good == 2).map(|offer| offer.left);
        assert_eq!(grain, Some(5.0));
        assert_eq!(bread, Some(4.0));
    }

    /// A holder lists the whole currency pile. Yesterday's bid does not change it.
    #[test]
    fn build_book_lists_the_whole_currency_pile() {
        let mut seller = Pop::new(1);
        seller.property.insert(1, PopPRow::new(10.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, seller);
        let mut factuals = Factuals::new().with_good(good(1, "grain", 0.0));
        factuals.config.market.currency_holder_share = 0.5;
        factuals.config.market.currency_producer_share = 1.0;
        let book = super::build_book(&actors, &factuals, &[1], &[1]);
        let listed = book.iter().find(|offer| offer.good == 1).map(|offer| offer.left);
        assert_eq!(listed, Some(10.0));
    }

    #[test]
    fn listed_units_keeps_a_holder_share_and_a_full_producer_share() {
        assert_eq!(listed_units(10.0, true, false, 0.5, 1.0), 5.0);
        assert_eq!(listed_units(10.0, true, true, 0.5, 1.0), 10.0);
        assert_eq!(listed_units(10.4, false, false, 0.5, 1.0), 10.0);
        assert_eq!(listed_units(0.4, false, false, 0.5, 1.0), 0.0);
    }

    #[test]
    fn savings_stop_keeps_a_finished_level_and_skips_the_later_buy() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(9, PopPRow::new(1.0));
        buyer.desires[1].push(desire(1, 2, 1.0));
        let mut seller = Pop::new(2);
        seller.property.insert(2, PopPRow::new(5.0));
        // A running line, so this stall is not an evacuation at the end of the day.
        // Complexity 0.5 leaves the trade score at 0, so the bread stays here.
        seller.property.insert(8, PopPRow::new(2.0));
        seller.job = Job::new(0, vec![JobLine::new(4, Some(1.0), vec![])]);
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 2, 1.0);
        at(&mut market, 9, 1.0);
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let factuals = Factuals::new()
            .with_process(
                Process::new(4, "shape", 0)
                    .with_complexity(0.5)
                    .with_input(ProcessInput::new(8, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(11, 2.0, true)),
            )
            .with_good(good(2, "bread", 0.0))
            .with_good(good(8, "clay", 0.0))
            .with_good(good(9, "gold", 0.0))
            .with_good(good(11, "ware", 0.0));
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        assert!(actors.pop(1).property.get(&2).is_none());
        assert!((actors.pop(1).saved.get(&9).copied().unwrap_or(0.0) - 1.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&2].quantity - 5.0).abs() < 1e-9);
    }

    /// # A Running Line Reaches The Next Tier After A Partial Save
    ///
    /// The save needs 5.5 and the purse holds 4, so those 4 are parked.
    /// The line is set to run, so the walk still reaches the common bread.
    /// The purse is that partial, and the bread stays with the seller.
    #[test]
    fn a_running_line_reaches_the_next_tier_after_a_partial_save() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(9, PopPRow::new(4.0));
        buyer.desires[0].push(desire(1, 2, 1.0));
        buyer.desires[1].push(desire(3, 4, 1.0));
        buyer.job = Job::new(0, vec![JobLine::new(5, Some(1.0), vec![])]);
        let mut seller = Pop::new(2);
        seller.property.insert(4, PopPRow::new(5.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 2, 10.0);
        at(&mut market, 4, 1.0);
        at(&mut market, 9, 1.0);
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let factuals = Factuals::new()
            .with_good(good(2, "grain", 0.0))
            .with_good(good(4, "bread", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut rng = StdRng::seed_from_u64(1);

        market.buy_ladder(&mut actors, &factuals, &mut rng, &HashMap::new());

        let saved = actors.pop(1).saved.get(&9).copied().unwrap_or(0.0);
        let bread = actors.pop(2).property.get(&4).map(|row| row.quantity).unwrap_or(0.0);
        assert!((saved - 4.0).abs() < 1e-9, "saved {saved}");
        assert!((bread - 5.0).abs() < 1e-9, "seller bread {bread}");
    }

    /// # A Short Purse Saves The Coins It Holds
    ///
    /// One savings level is 3. The purse holds 1 gold, so that coin is parked
    /// and the level stays short. The pop is not running a line, so the
    /// common bread stays with the seller.
    #[test]
    fn a_short_purse_saves_the_coins_it_holds() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(9, PopPRow::new(1.0));
        buyer.desires[1].push(desire(1, 2, 1.0));
        let mut seller = Pop::new(2);
        seller.property.insert(2, PopPRow::new(5.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 2, 1.0);
        at(&mut market, 9, 1.0);
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut factuals = Factuals::new()
            .with_good(good(2, "bread", 0.0))
            .with_good(good(9, "gold", 0.0));
        factuals.config.pop.savings_amv = 3.0;
        let mut rng = StdRng::seed_from_u64(1);

        market.buy_ladder(&mut actors, &factuals, &mut rng, &HashMap::new());

        let saved = actors.pop(1).saved.get(&9).copied().unwrap_or(0.0);
        assert!((saved - 1.0).abs() < 1e-9, "saved {saved}");
        assert!(actors.pop(1).working_units(9) < 1e-9);
        assert!((actors.pop(2).property[&2].quantity - 5.0).abs() < 1e-9);
    }

    #[test]
    fn savings_are_one_amv_per_household_and_add_across_levels() {
        let mut buyer = Pop::new(1);
        buyer.demographics.household.count = 2.0;
        buyer.property.insert(9, PopPRow::new(10.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 9, 1.0);
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        let factuals = Factuals::new().with_good(good(9, "gold", 0.0));
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let saved = actors.pop(1).saved.get(&9).copied().unwrap_or(0.0);
        assert!(saved + 1e-9 >= 8.0);
    }

    /// # Savings Follow The Mean Price Of The Tiers
    ///
    /// Basic goods at 1 and 3 average 2. The common good at 5 stands alone.
    /// The two tiers average 3.5. Four levels for one household save 14.
    #[test]
    fn savings_follow_the_mean_price_of_the_tiers() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(9, PopPRow::new(20.0));
        buyer.desires[0].push(desire(1, 2, 1.0));
        buyer.desires[0].push(desire(2, 3, 1.0));
        buyer.desires[1].push(desire(3, 4, 1.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 2, 1.0);
        at(&mut market, 3, 3.0);
        at(&mut market, 4, 5.0);
        at(&mut market, 9, 1.0);
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        let factuals = Factuals::new()
            .with_good(good(2, "grain", 0.0))
            .with_good(good(3, "bread", 0.0))
            .with_good(good(4, "beer", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let saved = actors.pop(1).saved.get(&9).copied().unwrap_or(0.0);
        assert!((saved - 14.0).abs() < 1e-6, "saved {saved}");
    }

    #[test]
    fn an_earlier_seller_can_spend_the_gold_a_buyer_just_paid() {
        let mut rich = Pop::new(2);
        rich.property.insert(9, PopPRow::new(10.0));
        rich.property.insert(3, PopPRow::new(5.0));
        rich.desires[0].push(desire(1, 2, 1.0));
        let mut poor = Pop::new(1);
        poor.property.insert(2, PopPRow::new(5.0));
        poor.desires[0].push(desire(2, 3, 1.0));
        // Still running after the gold is spent, so this pop does not evacuate.
        poor.property.insert(8, PopPRow::new(2.0));
        poor.job = Job::new(0, vec![JobLine::new(4, Some(1.0), vec![])]);
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 2, 1.0);
        at(&mut market, 3, 1.0);
        at(&mut market, 9, 1.0);
        let mut actors = Actors::new();
        actors.pops.insert(1, poor);
        actors.pops.insert(2, rich);
        let factuals = Factuals::new()
            .with_process(
                Process::new(4, "shape", 0)
                    .with_input(ProcessInput::new(8, 1.0, true, InputType::Destroyed, false))
                    .with_output(ProcessOutput::new(11, 2.0, true)),
            )
            .with_good(good(2, "bread", 0.0))
            .with_good(good(3, "tools", 0.0))
            .with_good(good(8, "clay", 0.0))
            .with_good(good(9, "gold", 0.0))
            .with_good(good(11, "ware", 0.0));
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        assert!(actors.pop(2).desires[0][0].satisfaction > 0.0);
        assert!(actors.pop(1).desires[0][0].satisfaction > 0.0);
        assert!((actors.pop(2).property[&3].quantity - 4.0).abs() < 1e-9);
    }

    #[test]
    fn a_second_seller_charges_the_flat_fee_again() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(9, PopPRow::new(20.0));
        buyer.property.insert(7, PopPRow::new(5.0));
        buyer.desires[0].push(desire(1, 2, 4.0));
        let mut first = Pop::new(2);
        first.property.insert(2, PopPRow::new(2.0));
        let mut second = Pop::new(3);
        second.property.insert(2, PopPRow::new(2.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.pops.insert(3);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 2, 1.0);
        at(&mut market, 9, 1.0);
        let mut cart = good(7, "cart", 0.0);
        cart.set_transport_efficiency(1.0);
        cart.mass = 0.0;
        let factuals = Factuals::new()
            .with_good(good(2, "bread", 0.0))
            .with_good(good(9, "gold", 0.0))
            .with_good(cart);
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, first);
        actors.pops.insert(3, second);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        assert!((actors.pop(1).property[&7].quantity - 3.0).abs() < 1e-9);
        assert!(actors.pop(2).property[&2].quantity.abs() < 1e-9);
        assert!(actors.pop(3).property[&2].quantity.abs() < 1e-9);
    }

    #[test]
    fn change_returns_smaller_coins_and_the_seller_keeps_the_remainder() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(4, PopPRow::new(1.0));
        buyer.desires[0].push(desire(1, 2, 1.0));
        let mut seller = Pop::new(2);
        seller.property.insert(2, PopPRow::new(1.0));
        seller.property.insert(5, PopPRow::new(4.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(4), Some(5), None, None];
        at(&mut market, 2, 1.0);
        at(&mut market, 4, 10.0);
        at(&mut market, 5, 2.0);
        let factuals = Factuals::new()
            .with_good(good(2, "bread", 0.0))
            .with_good(good(4, "gold", 0.0))
            .with_good(good(5, "silver", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let buyer = actors.pop(1);
        let silver = buyer.property.get(&5).map(|row| row.quantity).unwrap_or(0.0)
            + buyer.saved.get(&5).copied().unwrap_or(0.0);
        assert!((silver - 4.0).abs() < 1e-9);
        assert!(buyer.property.get(&4).map(|row| row.quantity).unwrap_or(0.0) < 1e-9);
        assert!(buyer.saved.get(&4).copied().unwrap_or(0.0) < 1e-9);
        let seller = actors.pop(2);
        let gold = seller.property.get(&4).map(|row| row.quantity).unwrap_or(0.0)
            + seller.saved.get(&4).copied().unwrap_or(0.0);
        assert!((gold - 1.0).abs() < 1e-9);
        assert!(seller.property.get(&5).map(|row| row.quantity).unwrap_or(0.0) < 1e-9);
    }

    /// Wood buys the grain. Gold is a tender and it is not on the book, so
    /// grain cannot be paid with itself.
    #[test]
    fn grain_is_bought_with_wood_when_gold_is_not_in_stock() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(7, PopPRow::new(5.0));
        buyer.desires[0].push(desire(1, 1, 2.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(20.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(1), Some(4), None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 4, 1.0);
        at(&mut market, 7, 1.0);
        let factuals = Factuals::new()
            .with_good(good(1, "grain", 0.12))
            .with_good(good(4, "gold", 0.001))
            .with_good(good(7, "wood", 0.18));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.buy_ladder(&mut actors, &factuals, &mut rng, &HashMap::new());

        let grain = actors.pop(1).working_units(1)
            + actors.pop(1).saved.get(&1).copied().unwrap_or(0.0);
        let wood = actors.pop(1).working_units(7)
            + actors.pop(1).saved.get(&7).copied().unwrap_or(0.0);
        let seller_grain = actors.pop(2).working_units(1)
            + actors.pop(2).saved.get(&1).copied().unwrap_or(0.0);
        let seller_wood = actors.pop(2).working_units(7)
            + actors.pop(2).saved.get(&7).copied().unwrap_or(0.0);
        assert!(grain >= 1.0, "grain {grain}");
        assert!(wood <= 4.0, "wood {wood}");
        assert!(seller_grain <= 19.0, "seller grain {seller_grain}");
        assert!(seller_wood >= 1.0, "seller wood {seller_wood}");
    }

    /// The buyer holds grain and nothing else. Grain does not pay for grain.
    #[test]
    fn grain_is_not_bought_with_itself_when_gold_is_not_in_stock() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(1, PopPRow::new(5.0));
        buyer.desires[0].push(desire(1, 1, 2.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(20.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(1), Some(4), None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 4, 1.0);
        let factuals = Factuals::new()
            .with_good(good(1, "grain", 0.12))
            .with_good(good(4, "gold", 0.001));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.buy_ladder(&mut actors, &factuals, &mut rng, &HashMap::new());

        let buyer_grain = actors.pop(1).working_units(1)
            + actors.pop(1).saved.get(&1).copied().unwrap_or(0.0);
        let seller_grain = actors.pop(2).working_units(1)
            + actors.pop(2).saved.get(&1).copied().unwrap_or(0.0);
        assert!((buyer_grain - 5.0).abs() < 1e-6, "buyer grain {buyer_grain}");
        assert!((seller_grain - 20.0).abs() < 1e-6, "seller grain {seller_grain}");
    }

    /// Gold is on the book. Wood still pays for the grain, and it does not pay for bread.
    #[test]
    fn wood_buys_grain_when_gold_is_in_stock_and_does_not_buy_bread() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(7, PopPRow::new(5.0));
        buyer.desires[0].push(desire(1, 1, 2.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(20.0));
        seller.property.insert(4, PopPRow::new(20.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(1), Some(4), None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 4, 1.0);
        at(&mut market, 7, 1.0);
        let factuals = Factuals::new()
            .with_good(good(1, "grain", 0.12))
            .with_good(good(4, "gold", 0.001))
            .with_good(good(7, "wood", 0.18));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.buy_ladder(&mut actors, &factuals, &mut rng, &HashMap::new());

        let buyer_wood = actors.pop(1).working_units(7)
            + actors.pop(1).saved.get(&7).copied().unwrap_or(0.0);
        let seller_wood = actors.pop(2).working_units(7)
            + actors.pop(2).saved.get(&7).copied().unwrap_or(0.0);
        let grain_sold = market
            .mass_closes()
            .iter()
            .find(|close| close.good == 1)
            .map(|close| close.exchanged)
            .unwrap_or(0.0);
        assert!(grain_sold >= 1.0, "grain sold {grain_sold}");
        assert!(buyer_wood <= 4.0, "buyer wood {buyer_wood}");
        assert!(seller_wood >= 1.0, "seller wood {seller_wood}");

        let mut bread_buyer = Pop::new(1);
        bread_buyer.property.insert(7, PopPRow::new(5.0));
        bread_buyer.desires[0].push(desire(1, 2, 2.0));
        let mut bread_seller = Pop::new(2);
        bread_seller.property.insert(2, PopPRow::new(20.0));
        bread_seller.property.insert(4, PopPRow::new(20.0));
        let mut bread_market = Market::new(1);
        bread_market.pops.insert(1);
        bread_market.pops.insert(2);
        bread_market.currencies = [Some(1), Some(4), None, None];
        at(&mut bread_market, 2, 1.0);
        at(&mut bread_market, 4, 1.0);
        at(&mut bread_market, 7, 1.0);
        let bread_factuals = Factuals::new()
            .with_good(good(1, "grain", 0.12))
            .with_good(good(2, "bread", 0.0))
            .with_good(good(4, "gold", 0.001))
            .with_good(good(7, "wood", 0.18));
        let mut bread_actors = Actors::new();
        bread_actors.pops.insert(1, bread_buyer);
        bread_actors.pops.insert(2, bread_seller);
        let mut bread_rng = StdRng::seed_from_u64(1);

        bread_market.buy_ladder(&mut bread_actors, &bread_factuals, &mut bread_rng, &HashMap::new());

        let bread = bread_actors.pop(1).working_units(2);
        let wood = bread_actors.pop(1).working_units(7);
        assert!(bread < 1.0, "bread {bread}");
        assert!((wood - 5.0).abs() < 1e-6, "wood {wood}");
    }

    #[test]
    fn durability_swap_moves_saved_grain_into_gold_and_leaves_inputs() {
        let mut buyer = Pop::new(1);
        buyer.saved.insert(1, 6.0);
        buyer.job.input_hold.insert(1, 2.0);
        let mut seller = Pop::new(2);
        seller.property.insert(4, PopPRow::new(10.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(1), Some(4), None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 4, 1.0);
        let mut factuals = Factuals::new()
            .with_good(good(1, "grain", 0.01))
            .with_good(good(4, "gold", 0.0));
        factuals.config.market.currency_holder_share = 1.0;
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        // The input units return for production. The swap spends the rest.
        let grain = actors
            .pop(1)
            .property
            .get(&1)
            .map(|row| row.quantity)
            .unwrap_or(0.0);
        let saved_grain = actors.pop(1).saved.get(&1).copied().unwrap_or(0.0);
        let saved_gold = actors.pop(1).saved.get(&4).copied().unwrap_or(0.0);
        let sellers_gold = actors.pop(2).property.get(&4).map(|row| row.quantity).unwrap_or(0.0)
            + actors.pop(2).saved.get(&4).copied().unwrap_or(0.0);
        assert!((grain - 1.98).abs() < 1e-6, "grain {grain}");
        assert!(saved_grain < 1e-6, "saved grain {saved_grain}");
        assert!((saved_gold - 4.0).abs() < 1e-6, "saved gold {saved_gold}");
        assert!((sellers_gold - 6.0).abs() < 1e-6, "seller gold {sellers_gold}");
    }

    #[test]
    fn the_sell_book_lists_working_stock_and_not_the_saved_pile() {
        let mut holder = Pop::new(1);
        holder.property.insert(1, PopPRow::new(3.0));
        holder.saved.insert(1, 5.0);
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 9, 1.0);
        let mut actors = Actors::new();
        actors.pops.insert(1, holder);
        let factuals = Factuals::new().with_good(good(1, "grain", 0.0));
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let offered = market
            .mass_closes()
            .iter()
            .find(|close| close.good == 1)
            .map(|close| close.offered)
            .unwrap_or(0.0);
        assert!((offered - 3.0).abs() < 1e-9);
        assert!((market.initial_offering().get(&1).copied().unwrap_or(0.0) - 8.0).abs() < 1e-9);
        assert!((market.effective_offering().get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
        assert!((market.excess_supply().get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
        let stock = market.goods.get(&1).map(|good| good.stock).unwrap_or(0.0);
        assert!(
            (stock - market.initial_offering().get(&1).copied().unwrap_or(0.0)).abs() > 1.0,
            "market stock {stock} is a different figure from the morning offering"
        );
    }

    /// Time and a skill list. Another untradeable good stays off.
    #[test]
    fn build_book_lists_time_and_skills_and_leaves_other_untradeable_goods() {
        let mut seller = Pop::new(1);
        seller.property.insert(TIME, PopPRow::new(6.0));
        seller.property.insert(28, PopPRow::new(1.0));
        seller.property.insert(8, PopPRow::new(4.0));
        seller.saved.insert(1, 2.0);
        let mut actors = Actors::new();
        actors.pops.insert(1, seller);
        let mut skill = good(28, "farming", 0.05);
        skill.tags.insert(GoodTag::Skill);
        skill.tags.insert(GoodTag::Untradeable);
        let mut relic = good(8, "relic", 0.0);
        relic.tags.insert(GoodTag::Untradeable);
        let factuals = Factuals::new()
            .with_good(good(TIME, "time", 1.0))
            .with_good(skill)
            .with_good(relic)
            .with_good(good(1, "grain", 0.0));
        let book = super::build_book(&actors, &factuals, &[1], &[]);
        let listed = |good_id: usize| {
            book.iter()
                .find(|offer| offer.good == good_id)
                .map(|offer| offer.left)
        };
        assert_eq!(listed(TIME), Some(6.0));
        assert_eq!(listed(28), Some(1.0));
        assert_eq!(listed(1), Some(2.0));
        assert_eq!(listed(8), None);
    }

    /// Bread is cheaper per satisfaction, so grain on hand stays for sale.
    #[test]
    fn a_cheaper_bread_is_bought_before_grain_on_hand_is_reserved() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(1, PopPRow::new(10.0));
        buyer.property.insert(9, PopPRow::new(20.0));
        let mut food = desire(1, 1, 4.0);
        food.target.push(DesireTarget::new(3, DesireTargetType::Consume, 2.0));
        food.target_taken = vec![0.0, 0.0];
        buyer.desires[0].push(food);
        let mut seller = Pop::new(2);
        seller.property.insert(3, PopPRow::new(10.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 1, 2.0);
        at(&mut market, 3, 2.0);
        at(&mut market, 9, 1.0);
        let factuals = Factuals::new()
            .with_good(good(1, "grain", 0.0))
            .with_good(good(3, "bread", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let grain = actors.pop(1).working_units(1) + actors.pop(1).meal.get(&1).copied().unwrap_or(0.0);
        let bread_left = actors.pop(2).working_units(3);
        assert!((grain - 10.0).abs() < 1e-6, "grain {grain}");
        assert!((bread_left - 8.0).abs() < 1e-6, "bread left {bread_left}");
        assert!((market.real_order().get(&3).copied().unwrap_or(0.0) - 2.0).abs() < 1e-6);
        assert!((market.maximal_order().get(&3).copied().unwrap_or(0.0) - 2.0).abs() < 1e-9);
    }

    /// A fractional slice rounds up so the minimum is a whole unit.
    #[test]
    fn a_fractional_slice_rounds_up_to_a_whole_unit() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(9, PopPRow::new(20.0));
        let mut food = desire(1, 1, 1.0);
        food.target[0].efficiency = 0.4;
        buyer.desires[0].push(food);
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(5.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 9, 1.0);
        let factuals = Factuals::new()
            .with_good(good(1, "grain", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        assert!((market.real_order().get(&1).copied().unwrap_or(0.0) - 3.0).abs() < 1e-6);
        assert!((actors.pop(2).working_units(1) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn luxury_buys_another_level_only_after_that_batch_is_saved() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(9, PopPRow::new(30.0));
        buyer.desires[2].push(desire(1, 6, 1.0));
        let mut seller = Pop::new(2);
        seller.property.insert(6, PopPRow::new(40.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 6, 1.0);
        at(&mut market, 9, 1.0);
        let factuals = Factuals::new()
            .with_good(good(6, "jewelry", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let left = actors.pop(2).property[&6].quantity;
        let bought = 40.0 - left;
        let saved = actors.pop(1).saved.get(&9).copied().unwrap_or(0.0);
        assert!(bought >= 2.0, "bought {bought} saved {saved}");
        assert!(
            saved + 1e-6 >= 3.0 + bought,
            "bought {bought} saved {saved}"
        );
    }

    #[test]
    fn produce_spends_the_inputs_the_ladder_set_aside() {
        // Four runs have to fit in four units of time, so each run spends little.
        let process = Process::new(1, "bake", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let mut maker = Pop::new(1);
        maker.job = Job::new(1, vec![JobLine::new(1, None, vec![])]);
        maker.property.insert(0, PopPRow::new(4.0));
        maker.property.insert(1, PopPRow::new(2.0));
        maker.property.insert(9, PopPRow::new(10.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(10.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 0, 1.0);
        at(&mut market, 1, 1.0);
        at(&mut market, 2, 10.0);
        at(&mut market, 9, 1.0);
        let factuals = Factuals::new()
            .with_process(process)
            .with_good(good(0, "time", 0.0))
            .with_good(good(1, "grain", 0.0))
            .with_good(good(2, "bread", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, maker);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        assert!((actors.pop(1).property[&2].quantity - 4.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&1].quantity - 8.0).abs() < 1e-9);
    }

    /// # Work Is Funded Before The Basic Meal
    ///
    /// Two gold can buy the line's grain or the meal's jewelry. The job
    /// takes the grain. The jewelry stays with the seller.
    #[test]
    fn work_is_funded_before_the_basic_meal() {
        let process = Process::new(1, "bake", 0)
            .with_complexity(0.01)
            .with_input(ProcessInput::new(0, 1.0, true, InputType::Destroyed, false))
            .with_input(ProcessInput::new(1, 1.0, true, InputType::Destroyed, false))
            .with_output(ProcessOutput::new(2, 1.0, true));
        let mut maker = Pop::new(1);
        maker.job = Job::new(1, vec![JobLine::new(1, Some(2.0), vec![])]);
        maker.property.insert(0, PopPRow::new(2.0));
        maker.property.insert(9, PopPRow::new(2.0));
        maker.desires[0].push(desire(1, 6, 10.0));
        let mut seller = Pop::new(2);
        seller.property.insert(1, PopPRow::new(10.0));
        seller.property.insert(6, PopPRow::new(10.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 0, 1.0);
        at(&mut market, 1, 1.0);
        at(&mut market, 2, 10.0);
        at(&mut market, 6, 1.0);
        at(&mut market, 9, 1.0);
        let factuals = Factuals::new()
            .with_process(process)
            .with_good(good(0, "time", 0.0))
            .with_good(good(1, "grain", 0.0))
            .with_good(good(2, "bread", 0.0))
            .with_good(good(6, "jewelry", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, maker);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.buy_ladder(&mut actors, &factuals, &mut rng, &HashMap::new());

        let held = actors
            .pop(1)
            .job
            .input_hold
            .get(&1)
            .copied()
            .unwrap_or(0.0);
        assert!((held - 2.0).abs() < 1e-9, "grain held {held}");
        assert!((actors.pop(2).property[&1].quantity - 8.0).abs() < 1e-9);
        assert!((actors.pop(2).property[&6].quantity - 10.0).abs() < 1e-9);
    }

    /// # An Empty Book Still Posts The Slice
    ///
    /// Gold can pay, and nobody lists bread or beer. The order is the slice.
    /// Nothing traded, so both cards stay at 1. The wish stays on the maximal
    /// order. The funded gap still saves.
    #[test]
    fn an_empty_book_posts_the_slice_and_still_saves() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(9, PopPRow::new(20.0));
        let mut want = desire(1, 2, 4.0);
        want.target
            .push(DesireTarget::new(3, DesireTargetType::Consume, 1.0));
        want.target_taken.push(0.0);
        buyer.desires[0].push(want);
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 9, 1.0);
        let factuals = Factuals::new()
            .with_good(good(2, "bread", 0.0))
            .with_good(good(3, "beer", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let requested = |id: usize| {
            market
                .mass_closes()
                .iter()
                .find(|close| close.good == id)
                .map(|close| close.requested)
                .unwrap_or(0.0)
        };
        assert!((requested(2) - 3.0).abs() < 1e-9, "bread {}", requested(2));
        assert!((requested(3) - 4.0).abs() < 1e-9, "beer {}", requested(3));
        assert!((market.maximal_order().get(&2).copied().unwrap_or(0.0) - 3.0).abs() < 1e-9);
        assert!((market.maximal_order().get(&3).copied().unwrap_or(0.0) - 1.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - 1.0).abs() < 1e-9, "bread {}", market.goods[&2].amv);
        assert!((market.goods[&3].amv - 1.0).abs() < 1e-9, "beer {}", market.goods[&3].amv);
        let saved = actors.pop(1).saved.get(&9).copied().unwrap_or(0.0);
        assert!(saved + 1e-6 >= 4.0, "saved {saved}");
    }

    /// # An Open Common Level Still Saves
    ///
    /// Grain on hand finishes basic. Nobody lists the common bread. The
    /// walk still posts that slice, then reaches the later savings levels.
    #[test]
    fn an_open_common_level_still_saves() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(1, PopPRow::new(4.0));
        buyer.property.insert(9, PopPRow::new(20.0));
        buyer.desires[0].push(desire(1, 1, 1.0));
        buyer.desires[1].push(desire(2, 2, 4.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 2, 1.0);
        at(&mut market, 9, 1.0);
        let factuals = Factuals::new()
            .with_good(good(1, "grain", 0.0))
            .with_good(good(2, "bread", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let requested = market
            .mass_closes()
            .iter()
            .find(|close| close.good == 2)
            .map(|close| close.requested)
            .unwrap_or(0.0);
        assert!((requested - 4.0).abs() < 1e-9, "bread {requested}");
        let saved = actors.pop(1).saved.get(&9).copied().unwrap_or(0.0);
        assert!(saved + 1e-6 >= 4.0, "saved {saved}");
    }

    /// # An Empty Luxury Book Posts One Batch
    ///
    /// The purse can fund many jewelry levels and nobody lists it. The
    /// order is that one batch, not a stack of the same batch. The wish
    /// stays on the maximal order, and the walk still saves.
    #[test]
    fn an_empty_luxury_book_posts_one_batch_and_still_saves() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(9, PopPRow::new(30.0));
        buyer.desires[2].push(desire(1, 6, 1.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.currencies = [Some(9), None, None, None];
        at(&mut market, 6, 1.0);
        at(&mut market, 9, 1.0);
        let factuals = Factuals::new()
            .with_good(good(6, "jewelry", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let requested = market
            .mass_closes()
            .iter()
            .find(|close| close.good == 6)
            .map(|close| close.requested)
            .unwrap_or(0.0);
        assert!((requested - 6.0).abs() < 1e-9, "jewelry {requested}");
        assert!(market.maximal_order().get(&6).copied().unwrap_or(0.0) >= 1.0);
        let saved = actors.pop(1).saved.get(&9).copied().unwrap_or(0.0);
        assert!(saved + 1e-6 >= 4.0, "saved {saved}");
    }

    /// # Cheap Grain Still Buys Bread
    ///
    /// Six grain at a twentieth cannot finish one savings step. Gold is a
    /// currency and nobody holds it, so the grain stays in the purse and
    /// buys the bread. The grain that paid is not left on the offer.
    #[test]
    fn cheap_grain_still_buys_bread_when_gold_is_not_for_sale() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(1, PopPRow::new(6.0));
        buyer.desires[1].push(desire(1, 2, 1.0));
        let mut seller = Pop::new(2);
        seller.property.insert(2, PopPRow::new(5.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(1), Some(4), None, None];
        at(&mut market, 1, 0.05);
        at(&mut market, 2, 0.05);
        at(&mut market, 4, 1.95);
        let mut factuals = Factuals::new()
            .with_good(good(1, "grain", 0.12))
            .with_good(good(2, "bread", 0.0))
            .with_good(good(4, "gold", 0.001));
        factuals.config.market.currency_holder_share = 1.0;
        factuals.config.market.currency_producer_share = 1.0;
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        assert!(actors.pop(1).desires[1][0].satisfaction > 0.0);
        assert!((actors.pop(2).property[&2].quantity - 4.0).abs() < 1e-9);
        let saved_grain = actors.pop(1).saved.get(&1).copied().unwrap_or(0.0);
        assert!(saved_grain < 1.0, "saved grain {saved_grain}");
        let grain = market
            .mass_closes()
            .iter()
            .find(|close| close.good == 1)
            .map(|close| close.offered)
            .unwrap_or(0.0);
        assert!(
            (grain - 5.0).abs() < 1e-9,
            "grain offer {grain}, spent tender should leave the book"
        );
    }

    /// # Grain Is Reserved When Gold Is On Offer
    ///
    /// Four grain can finish the savings steps, and gold is for sale, so the
    /// grain is reserved and the swap turns it into gold.
    #[test]
    fn grain_is_reserved_when_gold_is_on_offer() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(1, PopPRow::new(4.0));
        let mut seller = Pop::new(2);
        seller.property.insert(4, PopPRow::new(10.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(1), Some(4), None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 4, 1.0);
        let mut factuals = Factuals::new()
            .with_good(good(1, "grain", 0.12))
            .with_good(good(4, "gold", 0.001));
        factuals.config.market.currency_holder_share = 1.0;
        factuals.config.market.currency_producer_share = 1.0;
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let saved_gold = actors.pop(1).saved.get(&4).copied().unwrap_or(0.0);
        let saved_grain = actors.pop(1).saved.get(&1).copied().unwrap_or(0.0);
        assert!(saved_gold >= 1.0, "saved gold {saved_gold}");
        assert!(saved_grain < 1.0, "saved grain {saved_grain}");
    }

    /// # A Broke Pop Still Bids One
    ///
    /// No coin and nothing on the book. The desire the walk stops on bids
    /// one unit of each of its goods. Each currency bids one unit. The next
    /// desire does not. Nothing traded, so bread, grain, and gold stay.
    #[test]
    fn a_broke_pop_bids_one_unit_for_the_desire_and_each_currency() {
        let mut food = desire(1, 1, 4.0);
        food.target
            .push(DesireTarget::new(2, DesireTargetType::Consume, 2.0));
        food.target_taken.push(0.0);
        let mut buyer = Pop::new(1);
        buyer.desires[0].push(food);
        buyer.desires[0].push(desire(2, 3, 4.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.currencies = [Some(1), Some(4), None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 4, 1.0);
        let factuals = Factuals::new()
            .with_good(good(1, "grain", 0.12))
            .with_good(good(2, "bread", 0.0))
            .with_good(good(3, "water", 0.0))
            .with_good(good(4, "gold", 0.001));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        let requested = |id: usize| {
            market
                .mass_closes()
                .iter()
                .find(|close| close.good == id)
                .map(|close| close.requested)
                .unwrap_or(0.0)
        };
        assert!((requested(1) - 1.0).abs() < 1e-9, "grain {}", requested(1));
        assert!((requested(2) - 1.0).abs() < 1e-9, "bread {}", requested(2));
        assert!(requested(3) < 1.0, "water {}", requested(3));
        assert!((requested(4) - 1.0).abs() < 1e-9, "gold {}", requested(4));
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&2].amv - 1.0).abs() < 1e-9);
    }

    /// # An Unaffordable Bread Offer Falls
    ///
    /// One grain cannot buy bread at 100. The desire orders four and the
    /// seller listed one. Nothing was bought. The night takes twice the
    /// shortage rise off the bread card, and salability loses one percent.
    /// The four sit on the maximal order. The buyer's grain is parked
    /// toward the savings level and leaves the offer, so grain and gold
    /// keep the AMV they opened at.
    #[test]
    fn an_unaffordable_bread_offer_falls_instead_of_rising() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(1, PopPRow::new(1.0));
        buyer.desires[0].push(desire(1, 2, 4.0));
        let mut seller = Pop::new(2);
        seller.property.insert(2, PopPRow::new(1.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(1), Some(4), None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 2, 100.0);
        at(&mut market, 4, 1.0);
        let factuals = Factuals::new()
            .with_good(good(1, "grain", 0.0))
            .with_good(good(2, "bread", 0.0))
            .with_good(good(4, "gold", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.buy_ladder(&mut actors, &factuals, &mut rng, &HashMap::new());
        market.record_keeping(&factuals);

        let close = market
            .mass_closes()
            .iter()
            .find(|close| close.good == 2)
            .expect("bread was quoted");
        assert!((close.offered - 1.0).abs() < 1e-9, "offered {}", close.offered);
        assert!((close.requested - 4.0).abs() < 1e-9, "requested {}", close.requested);
        assert!(close.exchanged < 1.0, "exchanged {}", close.exchanged);
        let maximal = market.maximal_order().get(&2).copied().unwrap_or(0.0);
        let real = market.real_order().get(&2).copied().unwrap_or(0.0);
        assert!((maximal - 4.0).abs() < 1e-9, "maximal {maximal}");
        assert!(real < 1.0, "real {real}");
        // One offered against four asked. The drag is twice three quarters
        // of the idle step, taken off the price. The salability loss stops
        // at one percent.
        let dragged = 100.0 * (2.0 * 0.03 * (3.0 / 4.0));
        assert!(
            (market.goods[&2].amv - (100.0 - dragged)).abs() < 1e-9,
            "bread {}",
            market.goods[&2].amv
        );
        assert!(
            (market.goods[&2].salability - 0.99).abs() < 1e-9,
            "salability {}",
            market.goods[&2].salability
        );
        let grain = market
            .mass_closes()
            .iter()
            .find(|close| close.good == 1)
            .expect("grain was quoted");
        assert!(grain.offered < 1e-9, "grain offered {}", grain.offered);
        assert!(
            (grain.requested - 2.0).abs() < 1e-9,
            "grain requested {}",
            grain.requested
        );
        assert!(
            (market.goods[&1].amv - 1.0).abs() < 1e-9,
            "grain {}",
            market.goods[&1].amv
        );
        assert!(
            (market.goods[&4].amv - 1.0).abs() < 1e-9,
            "gold {}",
            market.goods[&4].amv
        );
    }

    /// # A Bread Offer Someone Can Pay Stays
    ///
    /// One grain buys the one bread at the card. The desire still orders
    /// four. That clearing trade pulls the card back to the price that
    /// traded, so the card stays. The four sit on the maximal order.
    #[test]
    fn a_bread_offer_someone_can_pay_does_not_fall() {
        let mut buyer = Pop::new(1);
        buyer.property.insert(1, PopPRow::new(1.0));
        buyer.desires[0].push(desire(1, 2, 4.0));
        let mut seller = Pop::new(2);
        seller.property.insert(2, PopPRow::new(1.0));
        let mut market = Market::new(1);
        market.pops.insert(1);
        market.pops.insert(2);
        market.currencies = [Some(1), Some(4), None, None];
        at(&mut market, 1, 1.0);
        at(&mut market, 2, 1.0);
        at(&mut market, 4, 1.0);
        let factuals = Factuals::new()
            .with_good(good(1, "grain", 0.0))
            .with_good(good(2, "bread", 0.0))
            .with_good(good(4, "gold", 0.0));
        let mut actors = Actors::new();
        actors.pops.insert(1, buyer);
        actors.pops.insert(2, seller);
        let mut rng = StdRng::seed_from_u64(1);

        market.buy_ladder(&mut actors, &factuals, &mut rng, &HashMap::new());
        market.record_keeping(&factuals);

        let close = market
            .mass_closes()
            .iter()
            .find(|close| close.good == 2)
            .expect("bread was quoted");
        assert!((close.offered - 1.0).abs() < 1e-9, "offered {}", close.offered);
        assert!((close.requested - 4.0).abs() < 1e-9, "requested {}", close.requested);
        assert!((close.exchanged - 1.0).abs() < 1e-9, "exchanged {}", close.exchanged);
        let maximal = market.maximal_order().get(&2).copied().unwrap_or(0.0);
        assert!((maximal - 4.0).abs() < 1e-9, "maximal {maximal}");
        assert!((market.goods[&2].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&1].amv - 1.0).abs() < 1e-9);
        assert!((market.goods[&4].amv - 1.0).abs() < 1e-9);
    }
}


