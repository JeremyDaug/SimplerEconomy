/// # Actor
/// 
/// A common enum which stores values for any part which can act in our system.
/// 
/// Pops, Firms, Institutions, and States (Players).
/// 
/// All work the same, the tag, and the ID of the actor.
#[derive(Debug, Clone, Copy)]
pub enum Actor {
    Pop(usize),
    Firm(usize),
    Institution(usize),
    State(usize),
}