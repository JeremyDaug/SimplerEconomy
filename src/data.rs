use std::collections::{HashMap, HashSet};

use crate::{culture::Culture, good::Good, process::Process, species::Species, want::Want};

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
    pub species: HashMap<usize, Species>,
    pub culture: HashMap<usize, Culture>,
}

impl Data {
    pub fn new() -> Self {
        Data {
            wants: HashMap::new(),
            goods: HashMap::new(),
            classes: HashMap::new(),
            processes: HashMap::new(),
            species: HashMap::new(),
            culture: HashMap::new(),
        }
    }

    /// # Get species
    /// 
    /// wrapper for get(id).expect()
    /// 
    /// # Panics
    /// 
    /// If the species id does not exist.
    pub fn get_species(&self, id: usize) -> &Species {
        self.species.get(&id)
        .expect(format!("Species '{} not found!'", id).as_str())
    }

    /// # Get Culture
    /// 
    /// wrapper for get(id).expect()
    /// 
    /// # Panics
    /// 
    /// If the culture id does not exist.
    pub fn get_culture(&self, id: usize) -> &Culture {
        self.culture.get(&id)
        .expect(format!("Culture '{} not found!'", id).as_str())
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
    
    /// # Get Good
    /// 
    /// Wrapper for get(id).expect().
    /// 
    /// # Panics
    /// 
    /// If the good id does not exist.
    pub fn get_good(&self, id: usize) -> &Good {
        self.goods.get(&id)
        .expect(format!("Good '{}' not found!", id).as_str())
    }
}