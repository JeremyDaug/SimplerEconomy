use crate::game::factuals::Factuals;

#[derive(Debug, Clone)]
pub struct Institution {
    pub id: usize,
}

impl Institution {
    /// End-of-day bookkeeping for this institution.
    /// Only external input is factuals.
    pub fn record_keeping(&mut self, factuals: &Factuals) {
        let _ = (self, factuals);
        todo!("Institution record keeping")
    }
}
