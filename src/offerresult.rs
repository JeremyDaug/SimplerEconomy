
/// # Offer Result
/// 
/// Offer result is a shared output for offers and 
#[derive(Debug, PartialEq)]
pub enum OfferResult {
    /// Accept, no further work needs done.
    Accept,
    /// Accepts, but wants to return change of the AMV given.
    /// 
    /// Call a followup to make change for this, if possible. 
    /// Value included is the AMV to return.
    AcceptWithChange(f64),
    /// Blanket Rejection, don't even try again. Usually called
    /// 
    /// When the offer is way to low (50% of request or less), or
    /// if the person is busy enough that they don't want to expend
    /// more effort.
    Reject,
    /// Soft rejection, requests for more AMV to matter, lets the
    /// responder reply with a better offer or not.
    /// 
    /// Note: This may not be used as pop offers that can't make
    /// the AMV imply it can't offer any more, and AMV
    /// is a shared value between them.
    ShortBy(f64),
    /// Soft rejection, but gives a hint as to what it would accept.
    /// 
    /// This is used when one of the goods in the goods either hinted
    /// at is particularly desired by the rejector, having extra
    /// satisfaction relative to it's amv value.
    /// 
    /// This would be found by comparing the average Sat Gain/AMV by other
    /// goods, relative to the possible requested good's Sat Gain/AMV.
    /// 
    /// Note: This will likely be not implemented yet, and will probably
    /// be put in before ShortBy as it's more useful.
    Request(usize, f64),
}