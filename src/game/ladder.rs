//! One sell book and a per-actor buy ladder.
//!
//! [`Market::buy_ladder`] replaces the eight-pass clear on the day path.
//! The morning card is the price. The buyer pays the freight.

use std::collections::HashMap;

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
    exchanged: HashMap<usize, f64>,
    paid: HashMap<usize, HashMap<usize, f64>>,
    haul_blocked: HashMap<usize, bool>,
}

impl Market {
    /// # Buy Ladder
    ///
    /// Sells once, then walks each pop through the buy ladder.
    ///
    /// `actors` holds the pops. `factuals` names goods, savings, and freight.
    /// `rng` draws a seller who can cover the whole order. The price is the
    /// card at the start of this call. Firms are not in the walk. A durability
    /// swap of saved currency follows the last actor. Nothing is returned.
    pub(crate) fn buy_ladder(
        &mut self,
        actors: &mut Actors,
        factuals: &Factuals,
        rng: &mut impl rand::RngCore,
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
    /// Nothing is returned.
    fn flush_tally(&mut self, history: &MarketHistory, tally: &Tally) {
        let mut goods: Vec<usize> = tally
            .offered
            .keys()
            .chain(tally.requested.keys())
            .copied()
            .collect();
        goods.sort_unstable();
        goods.dedup();
        for good in goods {
            let offered = tally.offered.get(&good).copied().unwrap_or(0.0);
            let requested = tally.requested.get(&good).copied().unwrap_or(0.0);
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
    }
}

/// # Listed Units
///
/// Whole units of `free` stock this actor posts.
///
/// A currency lists [`currency_holder_share`](crate::game::config::MarketConfig::currency_holder_share)
/// unless `producer` is set, which lists the producer share. Any other good
/// lists the whole free stock. Returns that count, floored.
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
/// One pop's ladder: buys, savings, production, then the later tiers.
///
/// A savings target that the purse cannot finish stops the walk. So does a
/// purse that cannot cover the flat fee. Nothing is returned.
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
    if buy_tier(
        actors, factuals, history, currencies, book, tally, id, 0, 1.0, &mut noted, rng,
    ) == Step::Stop
    {
        return;
    }
    if save_currency(actors, factuals, history, currencies, id, 1.0) == Step::Stop {
        return;
    }
    if buy_inputs(actors, factuals, history, currencies, book, tally, id, rng) == Step::Stop {
        return;
    }
    if save_currency(actors, factuals, history, currencies, id, 2.0) == Step::Stop {
        return;
    }
    if buy_tier(
        actors, factuals, history, currencies, book, tally, id, 1, 1.0, &mut noted, rng,
    ) == Step::Stop
    {
        return;
    }
    if save_currency(actors, factuals, history, currencies, id, 3.0) == Step::Stop {
        return;
    }
    buy_luxury(
        actors, factuals, history, currencies, book, tally, id, &mut noted, rng,
    );
}

/// # Buy Tier
///
/// Buys one level of each desire in `tier`, early desires first.
///
/// `levels` scales the desire amount. `noted` is the opening bid already
/// recorded for this actor. A desire that cannot be bought at all, while
/// currency remains, is left and the next desire runs. Returns
/// [`Step::Stop`] when the purse is finished.
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
    for index in order {
        let desire = actors.pop(id).desires[tier][index].clone();
        if fill_desire(
            actors, factuals, history, currencies, book, tally, id, &desire, levels, tier, noted,
            rng,
        ) == Step::Stop
        {
            return Step::Stop;
        }
    }
    Step::Continue
}

/// # Fill Desire
///
/// Buys `levels` of `desire` for pop `id`, highest satisfaction per AMV first.
///
/// The cap is [`allocate_desire`]. Whole units are floored. The part of the
/// opening bid the book cannot sell is recorded. `tier` and `noted` keep a
/// later bid for that desire to the increase over what was already recorded.
/// That good is then dropped and the remainder is allocated again. Returns
/// [`Step::Stop`] when the flat fee or the currency is gone.
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
    rng: &mut impl rand::RngCore,
) -> Step {
    if !desire.amount.is_finite() || desire.amount <= 0.0 || levels <= 0.0 {
        return Step::Continue;
    }
    let budget = desire.amount * levels;
    let mut blocked = Vec::new();
    // A good that filled what it could. Its stock on hand still counts.
    let mut capped = Vec::new();
    for _ in 0..64 {
        if !fee_open(actors.pop(id), factuals) || !has_currency(actors.pop(id), currencies, history)
        {
            return Step::Stop;
        }
        let goods = bucket_goods(
            actors.pop(id),
            history,
            book,
            desire,
            &blocked,
            &capped,
        );
        let Some(goods) = goods else {
            break;
        };
        let plan = allocate_desire(budget, &goods);
        if plan.is_empty() {
            break;
        }
        let mut retry = false;
        for plan_row in plan {
            let good = plan_row.id;
            let efficiency = desire
                .target
                .iter()
                .find(|target| target.good == good)
                .map(|target| target.efficiency)
                .unwrap_or(0.0);
            if efficiency <= 0.0 {
                blocked.push(good);
                retry = true;
                break;
            }
            let ask_units = (plan_row.ask / efficiency).floor();
            let buy_units = (plan_row.buy / efficiency)
                .floor()
                .min(stock_of(actors, book, good));
            // Units no seller listed are still a bid. A capped good was already recorded.
            if !capped.contains(&good) {
                let unmet = (ask_units - buy_units).max(0.0);
                if unmet >= 1.0 && tradeable(factuals, good) {
                    let key = (tier, desire.demo_desire_id, good);
                    let already = noted.get(&key).copied().unwrap_or(0.0);
                    let extra = (unmet - already).max(0.0);
                    if extra >= 1.0 {
                        *tally.requested.entry(good).or_insert(0.0) += extra;
                        actors.pop_mut(id).note_buy_offer(good, extra);
                        noted.insert(key, already + extra);
                    }
                }
            }
            if buy_units < 1.0 {
                if capped.contains(&good) {
                    continue;
                }
                // Stock already in hand still fills its share of the cap.
                let on_hand = actors.pop(id).working_units(good)
                    + actors.pop(id).meal.get(&good).copied().unwrap_or(0.0);
                if on_hand > 0.0 {
                    capped.push(good);
                } else {
                    blocked.push(good);
                }
                retry = true;
                break;
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
                buy_units,
                Purse::Working,
                None,
                false,
                rng,
            );
            if got < 1.0 {
                blocked.push(good);
                retry = true;
                break;
            }
            // A short unit rolls its satisfaction back into the next pass.
            if got * efficiency + 1e-6 < plan_row.buy {
                capped.push(good);
                retry = true;
                break;
            }
        }
        if !retry {
            break;
        }
    }
    if !fee_open(actors.pop(id), factuals) || !has_currency(actors.pop(id), currencies, history) {
        return Step::Stop;
    }
    Step::Continue
}

/// # Bucket Goods
///
/// The desire's goods ordered by satisfaction per AMV, skipping `blocked`.
///
/// `capped` goods stay in the bucket so stock on hand counts, and their
/// book is treated as empty. A non-positive card is left out. Returns
/// nothing when no good remains.
fn bucket_goods(
    pop: &Pop,
    history: &MarketHistory,
    book: &[Offer],
    desire: &Desire,
    blocked: &[usize],
    capped: &[usize],
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
        let on_hand = pop.working_units(target.good)
            + pop.meal.get(&target.good).copied().unwrap_or(0.0);
        let available = if capped.contains(&target.good) {
            0.0
        } else {
            stock_of_book(book, target.good) * target.efficiency
        };
        goods.push(BucketGood {
            id: target.good,
            efficiency: target.efficiency,
            max_share: target.cap.max(0.0),
            sat_on_hand: (on_hand * target.efficiency).max(0.0),
            sat_available: available,
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

/// # Buy Inputs
///
/// Buys the job's purchase list and sets the units aside.
///
/// The list is first cut to the runs the working purse can fund. A time
/// ceiling the purse cannot start is cleared. A list that still asks for a
/// whole unit after the buy stops the ladder. Returns that step.
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
        if !fee_open(actors.pop(id), factuals) || !has_currency(actors.pop(id), currencies, history)
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
        let _ = save_currency(actors, factuals, history, currencies, id, 4.0);
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
            return;
        }
        steps += batch;
        if save_currency(actors, factuals, history, currencies, id, 3.0 + steps) == Step::Stop {
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
        return savings_step(pop, factuals);
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
    cost + savings_step(pop, factuals)
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
/// Moves working currency into the saved pile until `tiers` savings steps are met.
///
/// `tiers` counts the levels already reached, including this one. The actor
/// stops when the purse cannot finish the target. A target already on hand
/// continues. Returns that step. A partial pile is not written.
fn save_currency(
    actors: &mut Actors,
    factuals: &Factuals,
    history: &MarketHistory,
    currencies: &[usize],
    id: usize,
    tiers: f64,
) -> Step {
    let target = savings_step(actors.pop(id), factuals) * tiers;
    if saved_currency_amv(actors.pop(id), history, currencies) + 1e-6 >= target {
        return Step::Continue;
    }
    let gap = target - saved_currency_amv(actors.pop(id), history, currencies);
    let coins = coins_of(actors.pop(id), currencies, history, Purse::Working, None, true);
    let mut pool = 0.0;
    for coin in &coins {
        pool += coin.amv * coin.have;
    }
    if pool + 1e-6 < gap {
        return Step::Stop;
    }
    let mut remain = gap;
    let mut order = coins;
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
        remain -= moved * coin.amv;
    }
    if saved_currency_amv(actors.pop(id), history, currencies) + 1e-6 >= target {
        Step::Continue
    } else {
        Step::Stop
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
                let have = actors.pop(id).saved_currency_units(source).floor();
                let affordable = (have * source_amv / price).floor();
                let units = affordable.min(stock_of(actors, book, target)).floor();
                if units < 1.0 {
                    continue;
                }
                let _ = exchange(
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
        match largest_slice(
            actors, factuals, history, currencies, buyer, seller, good, room, purse, only,
            spend_unlisted,
        ) {
            Ok(slice) => {
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
            Err(SliceMiss::Purse) => break,
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
/// The search starts at `room` and steps down. `spend_unlisted` lets the
/// buyer pay with tender the plan kept. Returns nothing when one unit
/// does not clear.
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
            spend_unlisted,
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
/// `spend_unlisted` lets the buyer pay with tender the plan kept. Returns
/// nothing when the purse or the freight cannot cover that count.
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
) -> Result<Slice, SliceMiss> {
    let due = history.price(good) * units;
    let pay_ids: Vec<usize> = match only {
        Some(id) => vec![id],
        None => currencies.to_vec(),
    };
    let buyer_coins = coins_of(
        actors.pop(buyer),
        &pay_ids,
        history,
        purse,
        Some(good),
        spend_unlisted,
    );
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
    let change = lay_change((paid - due).max(0.0), &seller_coins, smallest, good);
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
        actors.pop_mut(buyer).spend_freight(slice.bill, factuals);
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

/// # Build Book
///
/// One offer per pop per good, from working stock.
///
/// Saved stock is not listed. Time, skills, and untradeable goods are not
/// listed. A seller is listed even when the line loses holding. Returns the
/// offers.
fn build_book(
    actors: &Actors,
    factuals: &Factuals,
    order: &[usize],
    currencies: &[usize],
) -> Vec<Offer> {
    let holder = market_share(factuals, true);
    let producer = market_share(factuals, false);
    let mut book = Vec::new();
    for &id in order {
        let pop = actors.pop(id);
        let mut goods: Vec<usize> = pop.property.keys().copied().collect();
        goods.sort_unstable();
        for good in goods {
            if good == TIME {
                continue;
            }
            if factuals
                .get_good(good)
                .is_some_and(|row| !row.is_buyable() || row.is_skill())
            {
                continue;
            }
            let currency = currencies.contains(&good);
            let makes = pop.job.makes_good(factuals, good);
            // Tender the plan kept for a run stays off this book.
            let free = (pop.working_units(good) - pop.job.unlisted_of(good)).max(0.0);
            let units = listed_units(free, currency, makes, holder, producer);
            if units >= 1.0 {
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

/// # Market Share
///
/// The holder share when `holder` is set, otherwise the producer share.
fn market_share(factuals: &Factuals, holder: bool) -> f64 {
    if holder {
        factuals.config.market.currency_holder_share
    } else {
        factuals.config.market.currency_producer_share
    }
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

/// # Savings Step
///
/// AMV of one savings level for this pop.
fn savings_step(pop: &Pop, factuals: &Factuals) -> f64 {
    (factuals.config.pop.savings_amv * pop.household_count()).max(0.0)
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
/// True when the pop holds a whole unit of a currency with a positive card.
fn has_currency(pop: &Pop, currencies: &[usize], history: &MarketHistory) -> bool {
    currencies.iter().any(|&good| {
        history.price(good) > 0.0 && pop.working_units(good) >= 1.0
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

/// # Stock Of Book
///
/// Listed units of `good`, ignoring who holds them.
fn stock_of_book(book: &[Offer], good: usize) -> f64 {
    book.iter()
        .filter(|offer| offer.good == good)
        .map(|offer| offer.left)
        .sum()
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
    use crate::game::good::Good;
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

    /// # Build Book Leaves Off The Tender A Plan Kept
    ///
    /// Five grain with three kept lists two. Bread is not a tender, so its
    /// four units all list.
    #[test]
    fn build_book_leaves_off_the_tender_a_plan_kept() {
        let mut seller = Pop::new(1);
        seller.property.insert(1, PopPRow::new(5.0));
        seller.property.insert(2, PopPRow::new(4.0));
        seller.job.unlisted.insert(1, 3.0);
        let mut actors = Actors::new();
        actors.pops.insert(1, seller);
        let mut factuals = Factuals::new()
            .with_good(good(1, "grain", 0.0))
            .with_good(good(2, "bread", 0.0));
        factuals.config.market.currency_holder_share = 1.0;
        factuals.config.market.currency_producer_share = 1.0;

        let book = super::build_book(&actors, &factuals, &[1], &[1]);

        let grain = book.iter().find(|offer| offer.good == 1).map(|offer| offer.left);
        let bread = book.iter().find(|offer| offer.good == 2).map(|offer| offer.left);
        assert_eq!(grain, Some(2.0));
        assert_eq!(bread, Some(4.0));
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
            .with_good(good(2, "bread", 0.0))
            .with_good(good(9, "gold", 0.0));
        let mut rng = StdRng::seed_from_u64(1);

        market.market_day(&mut actors, &factuals, &mut rng);

        assert!(actors.pop(1).property.get(&2).is_none());
        assert!((actors.pop(1).saved.get(&9).copied().unwrap_or(0.0) - 1.0).abs() < 1e-9);
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

    #[test]
    fn an_earlier_seller_can_spend_the_gold_a_buyer_just_paid() {
        let mut rich = Pop::new(2);
        rich.property.insert(9, PopPRow::new(10.0));
        rich.property.insert(3, PopPRow::new(5.0));
        rich.desires[0].push(desire(1, 2, 1.0));
        let mut poor = Pop::new(1);
        poor.property.insert(2, PopPRow::new(5.0));
        poor.desires[0].push(desire(2, 3, 1.0));
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
            .with_good(good(2, "bread", 0.0))
            .with_good(good(3, "tools", 0.0))
            .with_good(good(9, "gold", 0.0));
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
        let process = Process::new(1, "bake", 0)
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

    #[test]
    fn an_empty_book_records_the_opening_bid_and_opens_the_card() {
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
                .map(|close| (close.offered, close.requested, close.exchanged))
                .unwrap_or((0.0, 0.0, 0.0))
        };
        assert_eq!(requested(2), (0.0, 3.0, 0.0));
        assert_eq!(requested(3), (0.0, 4.0, 0.0));
        assert!(market.goods[&2].amv > 1.0);
        assert!(market.goods[&3].amv > 1.0);
        let saved = actors.pop(1).saved.get(&9).copied().unwrap_or(0.0);
        assert!(saved + 1e-6 >= 4.0, "saved {saved}");
    }
}


