use crate::game::actor::Actor;

/// # Market Order
/// 
/// A Market order is a message from an actor (player, firm, institution, pop) to a 
/// market they are in. It covers what they want, and need.
/// 
/// It contains: 
/// - Who sent the order.
/// - What they want.
/// - how much they are seeking.
/// 
/// How much they are looking for can be positive or negative. If positive, they are
/// looking to buy. If negative, they'll sell.
/// 
/// It can also include:
/// - Their AMV target for acceptance.
/// - A good they are seeking in return.
/// - The amount of the other good they are requesting.
/// 
/// These last parts are only allowed if the actor has access to Buy and Sell orders.
/// Pops do not have access to this, but firms, institutions, and states do.
#[derive(Debug, Clone, PartialEq)]
pub struct MarketOrder {
    /// Who is making this order.
    pub origin: Actor,
    /// What they are seeking.
    pub target: usize,
    /// How much they are seeking.
    pub target_amount: f64,

    /// The AMV being targeted for the offer maker.
    pub amv_target: Option<f64>,
    /// A good that would be sought out in return for their offer.
    pub counter_offer: Option<usize>,
    /// The amount of their counter_offer good.
    pub counter_offer_amount: Option<f64>,
}

impl MarketOrder {
    pub fn buy_order(buyer: Actor, target: usize, target_amount: f64,
    amv_target: f64, counter_offer: usize, counter_offer_amount: f64) -> Self {
        debug_assert!(target_amount > 0.0, "Buy Orders must have positive target amounts.");
        debug_assert!(counter_offer_amount < 0.0, "Counter Offers in buy Orders must be negative.");

        Self {
            origin: buyer,
            target,
            target_amount,
            amv_target: Some(amv_target),
            counter_offer: Some(counter_offer),
            counter_offer_amount: Some(counter_offer_amount),
        }
    }

    pub fn sell_order(seller: Actor, target: usize, target_amount: f64,
    amv_target: f64, counter_offer: usize, counter_offer_amount: f64) -> Self {
        debug_assert!(target_amount < 0.0, "Sell Orders must have Negative target amounts.");
        debug_assert!(counter_offer_amount > 0.0, "Counter Offers in Sell Orders must be Positive.");

        Self {
            origin: seller,
            target,
            target_amount,
            amv_target: Some(amv_target),
            counter_offer: Some(counter_offer),
            counter_offer_amount: Some(counter_offer_amount),
        }
    }

    pub fn offer_order(seller: Actor, target: usize, target_amount: f64) -> Self {
        debug_assert!(target_amount < 0.0, "Offer Orders must have negative target amounts.");

        Self {
            origin: seller,
            target,
            target_amount,
            amv_target: None,
            counter_offer: None,
            counter_offer_amount: None,
        }
    }

    pub fn request_order(buyer: Actor, target: usize, target_amount: f64) -> Self {
        debug_assert!(target_amount > 0.0, "Request Orders must have positive target amounts.");

        Self {
            origin: buyer,
            target,
            target_amount,
            amv_target: None,
            counter_offer: None,
            counter_offer_amount: None,
        }
    }

    pub fn is_buy_order(&self) -> bool {
        if self.amv_target.is_some() && self.counter_offer.is_some() && self.counter_offer_amount.is_some() {
            // check that the target amount is positive, and the counter_offer_amount is negative 
            if self.target_amount > 0.0 && self.counter_offer_amount.unwrap() < 0.0 {
                true
            } else {
                false
            }
        } else if self.amv_target.is_none() && self.counter_offer.is_none() && self.counter_offer_amount.is_none() {
            // if they are none, then it can't be a buy order.
            false
        } else {
            unreachable!("Market Orders cannot mix it's optionals.");
        }
    }

    pub fn is_sell_order(&self) -> bool {
        if self.amv_target.is_some() && self.counter_offer.is_some() && self.counter_offer_amount.is_some() {
            // check that the target amount is negative, and the counter_offer_amount is positive 
            if self.target_amount < 0.0 && self.counter_offer_amount.unwrap() > 0.0 {
                true
            } else {
                false
            }
        } else if self.amv_target.is_none() && self.counter_offer.is_none() && self.counter_offer_amount.is_none() {
            // if they are none, then it can't be a buy order.
            false
        } else {
            unreachable!("Market Orders cannot mix it's optionals.");
        }
    }

    pub fn is_offer_order(&self) -> bool {
        if self.amv_target.is_some() && self.counter_offer.is_some() && self.counter_offer_amount.is_some() {
            // if they are some, then it can't be a offer or request order.
            false
        } else if self.amv_target.is_none() && self.counter_offer.is_none() && self.counter_offer_amount.is_none() {
            if self.target_amount < 0.0 {
                true
            } else {
                false
            }
        } else {
            unreachable!("Market Orders cannot mix it's optionals.");
        }
    }

    pub fn is_request_order(&self) -> bool {
        if self.amv_target.is_some() && self.counter_offer.is_some() && self.counter_offer_amount.is_some() {
            // if they are some, then it can't be a offer or request order.
            false
        } else if self.amv_target.is_none() && self.counter_offer.is_none() && self.counter_offer_amount.is_none() {
            if self.target_amount > 0.0 {
                true
            } else {
                false
            }
        } else {
            unreachable!("Market Orders cannot mix it's optionals.");
        }
    }
}