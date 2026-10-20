use crate::game::actor::Actor;

/// A message from an actor to a market they are in.
///
/// Positive `target_amount` is a buy. Negative is a sell.
/// `counter_offer` names a good they would take or give in return.
/// Priority, retry counts, and shop-tier ranking were the old queue and are gone.
#[derive(Debug, Clone, PartialEq)]
pub struct MarketOrder {
    /// Who is making this order.
    pub origin: Actor,
    /// What they are seeking.
    pub target: usize,
    /// How much they are seeking. Positive buys, negative sells.
    pub target_amount: f64,
    /// A good sought in return, when the order names one.
    pub counter_offer: Option<usize>,
    /// How much of `counter_offer` they want, when set.
    pub counter_offer_amount: Option<f64>,
    /// Raw AMV per unit of `target` this actor will trade at.
    ///
    /// A sell will not take less. A buy will not pay more. `0` means this
    /// order has not dictated one, and the card's raw AMV is the price.
    /// Salability does not scale it.
    pub accept_price: f64,
}

impl MarketOrder {
    /// Buy `amount` of `target`. Amount must be `>= 0`.
    pub fn buy(origin: Actor, target: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0 && amount.is_finite(), "buy amount must be >= 0");
        Self {
            origin,
            target,
            target_amount: amount,
            counter_offer: None,
            counter_offer_amount: None,
            accept_price: 0.0,
        }
    }

    /// Sell `amount` of `target`. Stored as a negative target amount.
    /// Amount must be `>= 0`.
    pub fn sell(origin: Actor, target: usize, amount: f64) -> Self {
        debug_assert!(amount >= 0.0 && amount.is_finite(), "sell amount must be >= 0");
        Self {
            origin,
            target,
            target_amount: -amount,
            counter_offer: None,
            counter_offer_amount: None,
            accept_price: 0.0,
        }
    }

    /// # With Accept Price
    ///
    /// Sets the raw AMV per unit this order will trade at.
    ///
    /// `price` is that AMV. A sell refuses less. A buy refuses more.
    /// Salability is not applied. Returns the order.
    pub fn with_accept_price(mut self, price: f64) -> Self {
        debug_assert!(price.is_finite(), "accept price must be finite");
        self.accept_price = price;
        self
    }

    /// Names a return good and, when `amount` is `Some`, how much of it.
    pub fn with_counter(mut self, good: usize, amount: Option<f64>) -> Self {
        if let Some(amount) = amount {
            debug_assert!(amount.is_finite(), "counter amount must be finite");
        }
        self.counter_offer = Some(good);
        self.counter_offer_amount = amount;
        self
    }
}
