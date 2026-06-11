use bevy::utils::default;

/// # Firm Organization
/// 
/// This defines how a firm organizes itself internally, and restricts what a firm can 
/// or can't do. 
/// 
/// Rather than names, it gives level of control and influence.
/// 
/// Sub-firms can have different rules, from each other within a company, but the 
/// superior sub-firm node's rules are obeyed first.
/// 
/// ## Note
/// 
/// This is currently just a placeholder, and it's logic will not be used for now.
/// 
/// Most of what this would do is based around the planning and intermarket phase of 
/// the game.
#[derive(Debug, Clone, Default)]
pub struct FirmOrganization {
    /// How the firm determines prices and tha associated values
    pub price_org: f32,
    /// How much of it's resources are shared with other sub-firms in it's company.
    pub resource_sharing: f32,
    /// How independent the sub-firm is from the wider company. This acts as a cap on 
    /// the control
    /// the rest of the has over this sub-firm.
    pub independence: f32,
    /// How centralized management is. An almost, but not quite, opposite to 
    /// independence. Represents how centralized management of the company is.
    pub management: f32,
}

impl FirmOrganization {
    pub fn empty() -> Self {
        Self {
            price_org: 0.0,
            resource_sharing: 0.0,
            independence: 0.0,
            management: 0.0,
        }
    }
}