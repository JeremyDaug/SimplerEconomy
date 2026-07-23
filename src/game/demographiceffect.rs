/// # Demographic Effect
/// 
/// Demographic effects are 
#[derive(Debug, Clone)]
pub enum DemographicEffect {
    /// Modify Adults in a household.
    Adults(f64),
    /// Modify Elder count in a household.
    Elders(f64),
    /// Modify Child count in a household.
    Children(f64),
    /// Modify Adult Efficiency in a household, discourage Positive values as adults 
    /// already have 1.0 efficiency.
    AdultEfficiency(f64),
    /// Modify Elder Labor Efficiency.
    ElderEfficiency(f64),
    /// Modify Child Labor Efficiency
    ChildEfficiency(f64),
    /// Modify Birth rate up or down.
    BirthRate(f64),
    /// Modify Mortality Rate up or down.
    MortalityRate(f64),
    /// Modify research rate up or down.
    ResearchRate(f64),
    /// Modify culture rate up or down.
    CultureRate(f64),
}