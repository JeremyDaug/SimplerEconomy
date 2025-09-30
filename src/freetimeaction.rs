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
    /// The actor is seeking employment elsewhere in the local market. The value 
    /// given is the desire of finding a new job. The higher the value the
    /// more likely (and larger the size) of the attempt to move elsewhere.
    SeekEmployment(f64),
    /// The actor is seeking to migrate to another market. The value given is 
    /// the desire of migrating out. If this is sent, then the pop REALLY wants to
    /// get out of dodge.
    SeekMigration(f64),
    /// The actor is wanting to change jobs, and has a desire to create a new firm.
    /// 
    /// The larger the value the more desire it has at it.
    CreateFirm(f64),
    /// The actor is getting more active in the community. Exactly what depends on
    /// the the breakdown of the population.
    GetActive,
}