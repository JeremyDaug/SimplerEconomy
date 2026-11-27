use serde::{Deserialize, Serialize};

/// # Actor
///
/// A common enum which stores values for any part which can act in our system.
///
/// Pops, Firms, Institutions, and Nations (Players).
///
/// All work the same, the tag, and the ID of the actor.
///
/// World data writes that tag in snake case with the id, as `{ pop = 1 }`.
/// A credit good reads it on [`crate::game::good::GoodTag::Credit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Pop(usize),
    Firm(usize),
    Institution(usize),
    Nation(usize),
}
