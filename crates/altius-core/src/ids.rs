use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AltiusError;

macro_rules! uuid_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a new random identifier.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wrap an existing UUID.
            pub fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            /// Borrow the inner UUID.
            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }

            /// Consume into the inner UUID.
            pub fn into_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = AltiusError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s)
                    .map(Self)
                    .map_err(|source| AltiusError::InvalidId {
                        kind: stringify!($name),
                        value: s.to_owned(),
                        source,
                    })
            }
        }

        impl From<Uuid> for $name {
            fn from(value: Uuid) -> Self {
                Self(value)
            }
        }
    };
}

uuid_newtype!(
    /// Identifies a single fleet run (one user prompt / task execution).
    RunId
);
uuid_newtype!(
    /// Identifies a single graph step within a run.
    StepId
);
uuid_newtype!(
    /// Correlates logs / traces across agents and tools for one logical request.
    CorrelationId
);

/// Stable role or worker identifier (e.g. `"router"`, `"explorer"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for AgentId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for AgentId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for AgentId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_round_trips_display_parse() {
        let id = RunId::new();
        let parsed: RunId = id.to_string().parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn agent_id_displays() {
        let id = AgentId::new("router");
        assert_eq!(id.as_str(), "router");
        assert_eq!(id.to_string(), "router");
    }
}
