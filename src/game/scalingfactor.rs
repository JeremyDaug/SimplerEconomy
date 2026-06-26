/// # Desire Scalar
/// 
/// When a base amount targeted by a desire is being scaled, what part of the pop
/// does it scale off of. 
#[derive(Debug, Clone)]
pub enum ScalingFactor {
    /// 
    Fixed(f64),
    /// Scales with all of the members of a house.
    All,
    /// Scales by household, not members.
    Household,
    /// Scales by adults only.
    Adults,
    /// Scaled by children only.
    Children,
    /// Scaled by Elders only.
    Elders,
    /// Scaled by the effective labor output of the household.
    Labor
}