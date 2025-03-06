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

    /// # Try Add Good
    /// 
    /// A helper which allows us to add a good to our data and 
    /// add it to both wants and classes in the process.
    /// 
    /// If unable to add, it returns Err instead of OK().
    pub fn try_add_good(&mut self, good: Good) -> Result<(), String>{
        // Keep out duplicates.
        if !self.goods.contains_key(&good.id) {
            return Err(format!("Good '{}' already exists in data.", good.id));
        }
        // check class membership is valid.
        if let Some(class_id) = good.class {
            // if class doesn't exist and it doesn't point to itself, 
            if !self.goods.contains_key(&class_id) && class_id != good.id {
                return Err(format!("Class base Good '{}' in Good '{}' does not currently exist. Be sure it exists before adding a new member of said class.", 
                class_id, good.id))
            }
        }
        // check wants it references exist
        for (want, _) in good.consumption_wants.iter() {
            if !self.wants.contains_key(want) {
                return Err(format!("Want '{}' does not currently exist in Data.", want));
            }
        }
        for (want, _) in good.use_wants.iter() {
            if !self.wants.contains_key(want) {
                return Err(format!("Want '{}' does not currently exist in Data.", want));
            }
        }
        for (want, _) in good.own_wants.iter() {
            if !self.wants.contains_key(want) {
                return Err(format!("Want '{}' does not currently exist in Data.", want));
            }
        }
        // all checks done and no invalid data, add to data and make connections.
        if let Some(class_id) = good.class {
            if class_id == good.id {
                // if we are the class base good, add whole cloth.
                let mut newset = HashSet::new();
                newset.insert(class_id);
                self.classes.insert(class_id, newset);
            } else {
                self.classes.get_mut(&class_id).unwrap().insert(good.id);
            }
        }
        // add want connections
        for (want, _) in good.consumption_wants.iter() {
            self.wants.get_mut(want).unwrap().consumption_sources.push(good.id);
        }
        for (want, _) in good.use_wants.iter() {
            self.wants.get_mut(want).unwrap().use_sources.push(good.id);
        }
        for (want, _) in good.own_wants.iter() {
            self.wants.get_mut(want).unwrap().ownership_sources.push(good.id);
        }
        self.goods.insert(good.id, good);
        Ok(())
    }

    /// # Add Good
    /// 
    /// Hard Add good, panics if addition fails, sending the failure message out.
    pub fn add_good(&mut self, good: Good) {
        match self.try_add_good(good) {
            Ok(_) => {},
            Err(msg) => assert!(false, "{}", msg),
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
    
    /// # Get Want
    /// 
    /// Wrapper for get(id).expect()
    /// 
    /// # Panics
    /// 
    /// If the want does not exist.
    pub fn get_want(&self, id: usize) -> &Want {
        self.wants.get(&id)
        .expect(format!("Want '{}' not found!", id).as_str())
    }
}