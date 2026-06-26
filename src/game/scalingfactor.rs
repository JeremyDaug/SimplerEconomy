/// # Desire Scalar
/// 
/// When a base amount targeted by a desire is being scaled, what part of the pop
/// does it scale off of. 
/// 
/// All of them include an f64 value, which acts as an additional multiplier on the
/// factor that causes it to scale. 
/// 
/// So, `ScalingFactor::Elders(3.0)` means that the effect is scaled by 3x the elder 
/// population.
#[derive(Debug, Clone)]
pub enum ScalingFactor {
    /// Fixed scalar value, allows multiplication by the value contained.
    Fixed(f64),
    /// Scales with all of the members of a house.
    All(f64),
    /// Scales by household, not members.
    Household(f64),
    /// Scales by adults only.
    Adults(f64),
    /// Scaled by children only.
    Children(f64),
    /// Scaled by Elders only.
    Elders(f64),
    /// Scaled by the effective labor output of the household.
    Labor(f64)
}