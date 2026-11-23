use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// # Actor
///
/// A common enum which stores values for any part which can act in our system.
///
/// Pops, Firms, Institutions, and Nations (Players).
///
/// All work the same, the tag, and the ID of the actor. A file names one of
/// those and the id. It does not build the pop, firm, institution, or nation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Pop(usize),
    Firm(usize),
    Institution(usize),
    Nation(usize),
}

/// Failed to load an actor reference from a file.
#[derive(Debug)]
pub enum ActorLoadError {
    Io(std::io::Error),
    Toml(toml::de::Error),
}

impl fmt::Display for ActorLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "read actor: {err}"),
            Self::Toml(err) => write!(f, "parse actor: {err}"),
        }
    }
}

impl std::error::Error for ActorLoadError {}

impl Actor {
    /// # Load From Path
    ///
    /// `path` is a TOML file.
    ///
    /// Reads the file and passes the text to [`Self::load_from_toml`].
    /// Returns that actor, or the first error.
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ActorLoadError> {
        let text = std::fs::read_to_string(path.as_ref()).map_err(ActorLoadError::Io)?;
        Self::load_from_toml(&text)
    }

    /// # Load From Toml
    ///
    /// `text` is one actor.
    ///
    /// The table is `pop`, `firm`, `institution`, or `nation`, and the value
    /// is the id. A credit tag on a good uses the same table. Returns that
    /// actor, or the parse error.
    pub fn load_from_toml(text: &str) -> Result<Self, ActorLoadError> {
        toml::from_str(text).map_err(ActorLoadError::Toml)
    }
}

#[cfg(test)]
mod load_should {
    use super::Actor;

    #[test]
    fn load_from_toml_reads_each_kind() {
        assert_eq!(Actor::load_from_toml("pop = 3").unwrap(), Actor::Pop(3));
        assert_eq!(Actor::load_from_toml("firm = 1").unwrap(), Actor::Firm(1));
        assert_eq!(
            Actor::load_from_toml("institution = 8").unwrap(),
            Actor::Institution(8)
        );
        assert_eq!(Actor::load_from_toml("nation = 2").unwrap(), Actor::Nation(2));
    }
}
