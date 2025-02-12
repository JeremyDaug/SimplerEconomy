use std::collections::{HashMap, HashSet};

use crate::{good::Good, process::Process, want::Want};

/// # Data
pub struct Data {
    pub wants: HashMap<usize, Want>,
    pub goods: HashMap<usize, Good>,
    /// Classses
    /// 
    /// A Shorthand function to help find the goods within a class.
    /// 
    /// Needs to be updated when inserting new good.
    pub classes: HashMap<usize, HashSet<usize>>,
    pub processes: HashMap<usize, Process>,
}

impl Data {
    pub fn new() -> Self {
        Data {
            wants: HashMap::new(),
            goods: HashMap::new(),
            classes: HashMap::new(),
            processes: HashMap::new()
        }
    }

    /// # Get Class
    /// 
    /// Wrapper for get(id).expect("Class 'id' not fonud!").
    /// 
    /// # Panics
    /// 
    /// If the class id does not exist.
    pub fn get_class(&self, id: usize) -> &HashSet<usize> {
        self.classes.get(&id)
        .expect(format!("Class '{}' not found!", id).as_str())
    }
}