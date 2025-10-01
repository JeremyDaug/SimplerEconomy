
/// # Offer Result
/// 
/// Offer result is a shared output for offers and 
#[derive(Debug, PartialEq)]
pub enum OfferResult {
    /// Accept, no further work needs done.
    Accept(AcceptReason),
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
    Reject(RejectReason),
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

/// # Reject Reason
/// 
/// The reason for why an offer was rejected. 
/// 
/// Used for both testing and updating AMV based on this reason.
#[derive(Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// Rejected because the AMV loss was way to large relative to 
    /// the AMV gained from the offer. 
    /// 
    /// The Hard Threshold is set by Constant::AMV_LOSS_HARD_THRESHOLD.
    HardThresholdFailure,
    /// The catch all reason for not accepting, IE, it did not meet the the thresholds
    /// for success.
    /// 
    /// Failed to increase Satisfaction, Density, or AMV
    NotAccepted,
}

/// # Accept Reason
/// 
/// The reason why we accepted the offer. Should only be a few reasons.
/// 
/// Currently for Logging and Testing purposes, but could also be used for market
/// adjustment as well.
#[derive(Debug, PartialEq, Eq)]
pub enum AcceptReason {
    /// The Price Hint offered was met, any more detailed reason is lost on us.
    /// 
    /// The goods requested via the price hint should get double counted on shifting
    /// AMV prices as it implies these goods are more desireable, accelerating their
    /// satisfaciton.
    PriceHint,
    /// Increases number of steps, no other reason needed.
    /// 
    /// Small steps are exaggereated here, while big steps get smaller emphasis.
    Steps,
    /// Satisfaction increased, no other reason needed.
    /// 
    /// Similar, but not quite equvialent to Steps.
    /// 
    /// Satisfaction means that more units of a good are brought in, benefiting big
    /// steps.
    Satisfaction,
    /// Increases Density of the Satisfaction.
    ///
    /// The amount of steps satisfied has stayed the same, but range has increased.
    /// 
    /// Benifits when satsifaction or steps declines, but the range decreases faster.
    Density,
    /// Steps, Satisfaction, and Density have remained the same, but AMV of the
    /// goods have increased.
    /// 
    /// The good being purchased this way should be increased in value while the
    /// goods purchasing via this method should have their value decline in value.
    /// 
    /// The change in value should be small (1/4th standard rate or something).
    AMV,
}