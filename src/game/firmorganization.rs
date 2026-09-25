/// How a firm is run inside a company.
///
/// Empty until company rules are written. Parent, children, and level on
/// [`crate::game::firm::Firm`] are the links that stay.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FirmOrganization {}

impl FirmOrganization {
    pub fn empty() -> Self {
        Self {}
    }
}
