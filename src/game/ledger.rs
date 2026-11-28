//! The coin ledger: each currency's sources, sinks, and transfers per day.

use std::collections::BTreeMap;

/// Relative slack on [`CoinFlows::balances`], for floating-point sums.
pub const LEDGER_TOLERANCE: f64 = 1e-9;

/// One currency's coin over one day.
///
/// Holdings count every pop and firm listed on the market. Sources add
/// coin, sinks remove it, and transfers only move it between local actors.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CoinFlows {
    /// Coin held at the start of the day.
    pub opening: f64,
    /// Coin held at the end of the day.
    pub closing: f64,
    /// Source. Coin made by production, whether a pop's job or a firm's line.
    pub minted: f64,
    /// Source. Coin the government printed. No V1 code writes this yet.
    pub printed: f64,
    /// Source. Coin paid in from outside the market. No V1 code writes this yet.
    pub border_in: f64,
    /// Sink. Coin paid out of the market. No V1 code writes this yet.
    pub border_out: f64,
    /// Sink. Coin lost to decay.
    pub decayed: f64,
    /// Sink. Coin destroyed by a process or consumed for a desire.
    pub destroyed: f64,
    /// Sink. Coin held by pops removed for falling under one household, when
    /// no other pop is left to merge into.
    pub pruned: f64,
    /// Coin paid from one local actor to another. Neither a source nor a sink.
    pub transfers: f64,
}

impl CoinFlows {
    /// # Sources
    ///
    /// Coin added today: minted, printed, and border in. Returns that sum.
    pub fn sources(&self) -> f64 {
        self.minted + self.printed + self.border_in
    }

    /// # Sinks
    ///
    /// Coin removed today: border out, decayed, destroyed, and pruned.
    /// Returns that sum.
    pub fn sinks(&self) -> f64 {
        self.border_out + self.decayed + self.destroyed + self.pruned
    }

    /// # Imbalance
    ///
    /// Closing coin less opening coin plus sources less sinks. Returns that
    /// difference, which is 0 when no coin was made or lost off the books.
    pub fn imbalance(&self) -> f64 {
        self.closing - (self.opening + self.sources() - self.sinks())
    }

    /// # Balances
    ///
    /// Whether [`Self::imbalance`] is within [`LEDGER_TOLERANCE`] of the
    /// day's largest of opening, closing, sources, sinks, and 1. Returns
    /// that check.
    pub fn balances(&self) -> bool {
        let scale = self
            .opening
            .max(self.closing)
            .max(self.sources())
            .max(self.sinks())
            .max(1.0);
        self.imbalance().abs() <= LEDGER_TOLERANCE * scale
    }
}

/// The coin flows of one day, by currency good.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LedgerDay {
    /// The turn this day ran as.
    pub turn: u64,
    /// Each currency's flows, keyed by good id.
    pub coins: BTreeMap<usize, CoinFlows>,
}

/// Every recorded day, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoinLedger {
    /// One entry per turn run.
    pub days: Vec<LedgerDay>,
}

#[cfg(test)]
mod coin_flows_should {
    use super::CoinFlows;

    #[test]
    fn balance_when_closing_is_opening_plus_sources_less_sinks() {
        let flows = CoinFlows {
            opening: 100.0,
            minted: 3.0,
            decayed: 0.1,
            pruned: 2.0,
            transfers: 40.0,
            closing: 100.9,
            ..CoinFlows::default()
        };
        assert!(flows.imbalance().abs() < 1e-12);
        assert!(flows.balances());
    }

    #[test]
    fn not_balance_when_coin_moves_off_the_books() {
        let flows = CoinFlows {
            opening: 100.0,
            closing: 100.5,
            ..CoinFlows::default()
        };
        assert!((flows.imbalance() - 0.5).abs() < 1e-12);
        assert!(!flows.balances());
    }
}
