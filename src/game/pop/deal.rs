use crate::game::actor::Actor;
use crate::game::deal::{
    collect_tenders, deal_goods_tradeable, evaluate_amv_floor, form_buy_proposal,
    transport_cover_on_hand, transport_spend_plan, with_transport_budget, DealMaker,
    DealResponse, ProposedDeal,
};
use crate::game::factuals::Factuals;
use crate::game::market::MarketHistory;
use crate::game::marketorder::MarketOrder;
use crate::game::pop_property::PopPRow;

use super::Pop;

impl DealMaker for Pop {
    /// # Buy
    ///
    /// Returns a proposed basket as buyer, or `None` if no tender can be named.
    /// Uses free stock above `shop_target.max(reserved)`, minus units listed
    /// on offer orders in `current_orders`. Seller's named counter first (any
    /// salability), then other excess by salability. Highly salable goods
    /// (and that counter) cover the fill first; lower salability only if
    /// those cannot. Shrinks the fill if still short. Does not move stock.
    fn buy(
        &self,
        own_order: &MarketOrder,
        other_order: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> Option<ProposedDeal> {
        debug_assert_eq!(own_order.origin, Actor::Pop(self.id));
        let targeted_good = own_order.target;
        let live = pop_live_tenders(self, targeted_good, history, factuals);
        let deal = form_buy_proposal(
            Actor::Pop(self.id),
            own_order,
            other_order,
            history,
            factuals.config.deal.high_salability,
            |good| pop_tenderable(self, good, targeted_good, factuals),
            &live,
        )?;
        with_transport_budget(
            deal,
            Actor::Pop(self.id),
            own_order,
            other_order,
            history,
            factuals,
            |good| pop_tenderable(self, good, targeted_good, factuals),
            &live,
            transport_cover_on_hand(
                self.property.iter().map(|(id, row)| (*id, row.quantity)),
                factuals,
            ),
        )
    }

    /// # Evaluate
    ///
    /// Returns Accept or Reject for this deal as this pop.
    /// Keep must meet the pop AMV floor. Desire / shop-target goods skip
    /// salability; other received goods are haircut. Buyers accept windfalls.
    /// Does not move stock.
    fn evaluate(
        &self,
        deal: &ProposedDeal,
        own_order: &MarketOrder,
        other_order: &MarketOrder,
        history: &MarketHistory,
        factuals: &Factuals,
    ) -> DealResponse {
        let _ = other_order;
        debug_assert_eq!(own_order.origin, Actor::Pop(self.id));
        let Some(role) = deal.role_of(Actor::Pop(self.id)) else {
            debug_assert!(false, "pop must be a party to the deal");
            return DealResponse::Reject;
        };
        if !deal_goods_tradeable(deal, factuals) {
            return DealResponse::Reject;
        }
        evaluate_amv_floor(
            deal,
            role,
            history,
            factuals.config.deal.pop_amv_min_keep,
            factuals.config.deal.pop_amv_min_keep,
            false,
            |good| pop_uses_good(self, good),
        )
    }

    /// # Finalize
    ///
    /// Applies `deal` to this pop's on-hand `quantity`. Seller adds the map,
    /// buyer subtracts it. Creates a row at 0 if the good is new. Does not
    /// edit orders, reserved, or shop/save targets.
    fn finalize(&mut self, deal: &ProposedDeal, history: &MarketHistory) {
        let _ = history;
        let Some(role) = deal.role_of(Actor::Pop(self.id)) else {
            debug_assert!(false, "pop must be a party to the deal");
            return;
        };
        for (&good, _) in &deal.goods {
            let delta = deal.signed_qty(role, good);
            if delta == 0.0 {
                continue;
            }
            let row = self
                .property
                .entry(good)
                .or_insert_with(|| PopPRow::new(0.0));
            row.quantity += delta;
            debug_assert!(row.quantity >= 0.0, "quantity must be >= 0.0");
        }
    }

    fn pay_transport(&mut self, amount: f64, factuals: &Factuals) {
        let on_hand: Vec<(usize, f64)> = self
            .property
            .iter()
            .map(|(id, row)| (*id, row.quantity))
            .collect();
        for (id, sub) in transport_spend_plan(amount, factuals, on_hand) {
            if let Some(row) = self.property.get_mut(&id) {
                row.quantity = (row.quantity - sub).max(0.0);
            }
        }
    }
}

/// Returns true if this pop has a shop_target or desire target for `good`.
/// Those goods skip the salability haircut when received.
fn pop_uses_good(pop: &Pop, good: usize) -> bool {
    if pop
        .property
        .get(&good)
        .is_some_and(|row| row.shop_target > 0.0)
    {
        return true;
    }
    pop.desires.iter().flatten().any(|desire| {
        desire.target.iter().any(|target| target.good == good)
    })
}

/// Returns how many units of `good` this pop can tender (0 if it is `targeted_good`).
/// Free stock above `shop_target.max(reserved)`, minus listed offer qty.
fn pop_tenderable(pop: &Pop, good: usize, targeted_good: usize, factuals: &Factuals) -> f64 {
    if good == targeted_good {
        return 0.0;
    }
    if !factuals.find_good(good).is_buyable() {
        return 0.0;
    }
    let Some(row) = pop.property.get(&good) else {
        return 0.0;
    };
    let keep = row.shop_target.max(row.reserved);
    let listed: f64 = pop
        .current_orders
        .iter()
        .filter(|order| order.target == good && order.target_amount < 0.0)
        .map(|order| -order.target_amount)
        .sum();
    (row.quantity - keep - listed).max(0.0)
}

/// Returns this pop's tenderable goods as `(id, qty)`, highest salability first.
fn pop_live_tenders(
    pop: &Pop,
    targeted_good: usize,
    history: &MarketHistory,
    factuals: &Factuals,
) -> Vec<(usize, f64)> {
    collect_tenders(pop.property.keys().copied(), history, |good| {
        pop_tenderable(pop, good, targeted_good, factuals)
    })
}
