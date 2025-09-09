use crate::item::Item;

#[derive(Debug)]
pub enum FreeTimeAction {
    /// Do nothing, a holding action, typically means it needs to come back.
    /// 
    /// Includes the amount of time remaining in the pop so that the market can 
    /// decide to either close them out for the day forcefully, or put them back
    /// in the rotation for acting.
    Nothing ( f64 ),
    /// Actor is completely done and cannot logic up another reason to act today.
    /// 
    /// This is typical when they run out of extra time to spend on anything.
    End,
    /// The Actor is seeking a good and sending out a buy order.
    /// 
    /// It includes a good and the amount being sought.
    /// 
    /// The market records the buy order and searches for the good. The amount
    /// is not actually used to find sell orders.
    BuyOrder { good: Item, amount: f64 },
}