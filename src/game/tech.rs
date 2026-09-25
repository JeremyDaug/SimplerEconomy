/// # Technology
/// 
/// A node in our technology tree. 
/// 
/// Unlocks processes, goods, and many other things.
#[derive(Debug, Clone)]
pub struct Technology {
    pub id: usize,
    pub name: String,
    pub description: String,
    pub cost: f64,
    pub parents: Vec<usize>,
    pub children: Vec<usize>,
    pub tier: usize,
    
    // unlocks
    pub processes: Vec<usize>,
    pub goods: Vec<usize>,
}