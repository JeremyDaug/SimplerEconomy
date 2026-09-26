use std::collections::{HashMap, HashSet};

use crate::game::actor::Actor;
use crate::game::actors::Actors;
use crate::game::deal::{matched_on, DealResponse, ProposedDeal, SellerBook};
use crate::game::marketorder::MarketOrder;
use crate::game::factuals::Factuals;
use crate::game::good::GoodTag;

/// Dead zone around zero for a stored AMV.
///
/// AMV may be negative. A value inside `(-AMV_EPSILON, AMV_EPSILON)` is not
/// stored: it bounces to `AMV_EPSILON` on the other side of zero from the
/// previous sign. This is not a configured price floor.
pub const AMV_EPSILON: f64 = 1e-9;

/// Salability of a good with no recorded value yet.
pub const SALABILITY_DEFAULT: f64 = 0.4;

/// Salability clamp. `0..=1` is illiquid to par. Above 1 is at-par and currency.
pub const SALABILITY_MAX: f64 = 2.0;

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
    /// Distance / size multiplier on deal bulk. 0 on a one-hex market.
    pub friction: f64,
}

impl Market {
    /// Empty market with this id.
    pub fn new(id: usize) -> Self {
        Self {
            id,
            pops: HashSet::new(),
            firms: HashSet::new(),
            institution_ids: HashSet::new(),
            goods: HashMap::new(),
            friction: 0.0,
        }
    }

    /// Sets the market friction factor. Must be `>= 0.0`.
    pub fn with_friction(mut self, friction: f64) -> Self {
        debug_assert!(friction >= 0.0, "friction must be >= 0.0");
        self.friction = friction;
        self
    }

    /// End-of-day market bookkeeping. Not written yet.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Market record keeping")
    }

    /// Aggregate emigration and hiring pressure for this region. Not written yet.
    pub fn sum_migratory_pressure(&mut self, actors: &Actors, factuals: &Factuals) {
        let _ = (self, actors, factuals);
        todo!("Market sum migratory pressure (positive / negative / net, migrant pool)")
    }

    /// Snapshot of stored AMV and salability.
    pub fn history(&self) -> MarketHistory {
        let mut history = MarketHistory::new();
        for (&good_id, good) in &self.goods {
            history.prices.insert(good_id, good.amv);
            history.salability.insert(good_id, good.salability);
        }
        history.friction = self.friction;
        history
    }

    /// # Match Deals
    ///
    /// Collects sell orders, then picks a buyer at random from those who still
    /// have a buy, and a seller at random from the valid matches.
    /// The buyer sees that seller's offers and requests and either proposes
    /// a basket or abandons. The seller accepts or rejects. An accepted
    /// basket is finalized, freight included. The buyer pays that freight
    /// from transport they already hold and from transport the basket buys.
    ///
    /// After every meeting both sides reevaluate their orders. A rejected or
    /// abandoned pair is not tried again this call.
    pub fn match_deals(
        &self,
        actors: &mut Actors,
        factuals: &Factuals,
        rng: &mut impl rand::RngCore,
    ) -> Vec<ProposedDeal> {
        let history = self.history();
        let members = self.members();
        let mut sells = Vec::new();
        for actor in &members {
            sells.extend(listed_sells(actors, *actor, &history, factuals));
        }

        let mut tried: HashSet<(Actor, Actor, usize)> = HashSet::new();
        let mut deals = Vec::new();
        loop {
            let mut candidates = Vec::new();
            for buyer in &members {
                for buy in actors.get(*buyer).buy_orders(&history) {
                    if buy.target_amount < 1.0 || !tradeable(factuals, buy.target) {
                        continue;
                    }
                    let matches: Vec<MarketOrder> = sells
                        .iter()
                        .filter(|sell| {
                            matched_on(&buy, sell)
                                && !tried.contains(&(*buyer, sell.origin, buy.target))
                        })
                        .cloned()
                        .collect();
                    if !matches.is_empty() {
                        candidates.push((*buyer, buy, matches));
                    }
                }
            }
            if candidates.is_empty() {
                break;
            }
            let pick = crate::game::util::random_index(rng, candidates.len());
            let (buyer, buy, matches) = candidates.swap_remove(pick);
            let sell = matches[crate::game::util::random_index(rng, matches.len())].clone();
            tried.insert((buyer, sell.origin, buy.target));
            if let Some(deal) = self.meet(actors, buyer, &sell, &history, factuals, rng) {
                deals.push(deal);
            }
            replace_sells(&mut sells, actors, buyer, &history, factuals);
            replace_sells(&mut sells, actors, sell.origin, &history, factuals);
        }
        deals
    }

    /// One meeting. The buyer proposes from the seller's book. The seller
    /// accepts or rejects. Both sides then rewrite their orders.
    fn meet(
        &self,
        actors: &mut Actors,
        buyer: Actor,
        sell: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
        rng: &mut impl rand::RngCore,
    ) -> Option<ProposedDeal> {
        let seller = sell.origin;
        let book = SellerBook {
            seller,
            offers: listed_sells(actors, seller, history, factuals),
            requests: actors.get(seller).buy_orders(history),
        };
        let mut accepted = None;
        if let Some(proposal) = actors.get(buyer).propose(sell.target, &book, history, factuals) {
            if actors.get(seller).evaluate(&proposal, history, factuals) == DealResponse::Accept
                && is_valid_exchange(actors, &proposal)
            {
                actors.get_mut(buyer).finalize(&proposal, factuals);
                actors.get_mut(seller).finalize(&proposal, factuals);
                accepted = Some(proposal);
            }
        }
        actors.get_mut(buyer).reevaluate(history, rng);
        actors.get_mut(seller).reevaluate(history, rng);
        accepted
    }

    /// Member actors, pops then firms then institutions, each id ascending.
    fn members(&self) -> Vec<Actor> {
        let mut actors = Vec::new();
        let mut ids: Vec<usize> = self.pops.iter().copied().collect();
        ids.sort_unstable();
        actors.extend(ids.iter().copied().map(Actor::Pop));
        ids.clear();
        ids.extend(self.firms.iter().copied());
        ids.sort_unstable();
        actors.extend(ids.iter().copied().map(Actor::Firm));
        ids.clear();
        ids.extend(self.institution_ids.iter().copied());
        ids.sort_unstable();
        actors.extend(ids.iter().copied().map(Actor::Institution));
        actors
    }
}

fn listed_sells(
    actors: &Actors,
    actor: Actor,
    history: &MarketHistory,
    factuals: &Factuals,
) -> Vec<MarketOrder> {
    actors
        .get(actor)
        .sell_orders(history)
        .into_iter()
        .filter(|order| order.target_amount < 0.0 && tradeable(factuals, order.target))
        .collect()
}

/// Both sides can spare the goods the basket moves.
fn is_valid_exchange(actors: &Actors, proposal: &ProposedDeal) -> bool {
    proposal.goods.iter().all(|(&good, &qty)| {
        if qty > 0.0 {
            actors.get(proposal.seller).free_units(good) >= qty
        } else if qty < 0.0 {
            actors.get(proposal.buyer).free_units(good) >= -qty
        } else {
            true
        }
    })
}

fn replace_sells(
    sells: &mut Vec<MarketOrder>,
    actors: &Actors,
    actor: Actor,
    history: &MarketHistory,
    factuals: &Factuals,
) {
    sells.retain(|order| order.origin != actor);
    sells.extend(listed_sells(actors, actor, history, factuals));
}

fn tradeable(factuals: &Factuals, good: usize) -> bool {
    factuals
        .goods
        .get(&good)
        .is_none_or(|row| !row.tags.contains(&GoodTag::Untradeable))
}

/// A saved price snapshot for one market.
#[derive(Debug, Clone)]
pub struct MarketHistory {
    /// Last known AMV per good.
    pub prices: HashMap<usize, f64>,
    /// Last known salability per good.
    pub salability: HashMap<usize, f64>,
    /// Copied from [`Market::friction`].
    pub friction: f64,
    /// Used when a good has no recorded salability.
    pub default_salability: f64,
}

impl Default for MarketHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl MarketHistory {
    pub fn new() -> Self {
        Self {
            prices: HashMap::new(),
            salability: HashMap::new(),
            friction: 0.0,
            default_salability: SALABILITY_DEFAULT,
        }
    }

    /// AMV for `good_id`, or 1.0 if this snapshot has none.
    pub fn price(&self, good_id: usize) -> f64 {
        self.prices.get(&good_id).copied().unwrap_or(1.0)
    }

    /// Salability for `good_id`, or this snapshot's default if missing.
    pub fn salability(&self, good_id: usize) -> f64 {
        self.salability
            .get(&good_id)
            .copied()
            .unwrap_or(self.default_salability)
    }
}

/// Per-market price snapshots plus pop-id to market-id.
#[derive(Debug, Clone, Default)]
pub struct MarketLookups {
    pub histories: HashMap<usize, MarketHistory>,
    pub pop_to_market: HashMap<usize, usize>,
}

impl MarketLookups {
    pub fn new() -> Self {
        Self::default()
    }

    /// One history per market, and each member pop id mapped to that market id.
    pub fn from_markets(markets: &HashMap<usize, Market>) -> Self {
        let mut histories = HashMap::new();
        let mut pop_to_market = HashMap::new();
        for market in markets.values() {
            histories.insert(market.id, market.history());
            for &pop_id in &market.pops {
                pop_to_market.insert(pop_id, market.id);
            }
        }
        Self {
            histories,
            pop_to_market,
        }
    }

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

/// If `new` is inside the AMV dead zone, land [`AMV_EPSILON`] on the other
/// side of 0 from `old`. Otherwise return `new`.
fn bounce_away_from_zero(old: f64, new: f64) -> f64 {
    debug_assert!(new.is_finite(), "new AMV must be finite");
    if new.abs() >= AMV_EPSILON {
        new
    } else if old >= 0.0 {
        -AMV_EPSILON
    } else {
        AMV_EPSILON
    }
}

/// Stored price and quantity for one good in a market.
#[derive(Debug, Clone)]
pub struct MarketGood {
    /// Abstract market value. May be negative. Not stored as zero.
    /// Assign through [`Self::set_amv`] so the dead zone is applied.
    pub amv: f64,
    /// How readily the good trades. Clamped to `0.0..=`[`SALABILITY_MAX`].
    pub salability: f64,
    /// Units made today.
    pub production: f64,
    /// Units consumed today.
    pub consumption: f64,
    /// Units already in the market from yesterday.
    pub stock: f64,
}

impl Default for MarketGood {
    fn default() -> Self {
        Self {
            amv: 1.0,
            salability: SALABILITY_DEFAULT,
            production: 0.0,
            consumption: 0.0,
            stock: 0.0,
        }
    }
}

impl MarketGood {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets AMV. Values inside the dead zone bounce past zero.
    pub fn set_amv(&mut self, amv: f64) {
        self.amv = bounce_away_from_zero(self.amv, amv);
    }

    /// [`Self::set_amv`] as a builder. Starts from the current AMV.
    pub fn with_amv(mut self, amv: f64) -> Self {
        self.set_amv(amv);
        self
    }

    /// Sets salability, clamped to `0.0..=`[`SALABILITY_MAX`].
    pub fn set_salability(&mut self, salability: f64) {
        debug_assert!(salability.is_finite(), "salability must be finite");
        self.salability = salability.clamp(0.0, SALABILITY_MAX);
    }

    pub fn with_salability(mut self, salability: f64) -> Self {
        self.set_salability(salability);
        self
    }

    pub fn set_production(&mut self, production: f64) {
        debug_assert!(production >= 0.0, "production must be >= 0");
        self.production = production;
    }

    pub fn set_consumption(&mut self, consumption: f64) {
        debug_assert!(consumption >= 0.0, "consumption must be >= 0");
        self.consumption = consumption;
    }

    pub fn set_stock(&mut self, stock: f64) {
        debug_assert!(stock >= 0.0, "stock must be >= 0");
        self.stock = stock;
    }
}
