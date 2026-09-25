/// # Plot
/// 
/// The types of land plots available to us.
#[derive(Debug, Clone)]
pub enum Plot {
    Flat,
    Hill,
    Mountain,
    Forest,
    ForestHill,
    ForestMountain,
    Coastal,
    Sea,
}