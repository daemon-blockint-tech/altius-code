use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Execution budget enforced by the supervisor / graph runtime.
///
/// All fields are optional upper bounds. `None` means "no limit for this axis"
/// (callers may still apply their own defaults).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    /// Maximum graph node executions for a run.
    pub max_steps: Option<u64>,
    /// Maximum LLM tokens (prompt + completion) for a run.
    pub max_tokens: Option<u64>,
    /// Maximum wall-clock duration for a run.
    #[serde(with = "duration_secs_opt")]
    pub max_wall_time: Option<Duration>,
    /// Maximum estimated spend in USD for a run.
    pub max_dollars: Option<f64>,
    /// Maximum parallel worker branches during fan-out.
    pub max_parallel: Option<usize>,
}

impl Budget {
    pub fn unlimited() -> Self {
        Self::default()
    }

    pub fn with_max_steps(mut self, steps: u64) -> Self {
        self.max_steps = Some(steps);
        self
    }

    pub fn with_max_tokens(mut self, tokens: u64) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    pub fn with_max_wall_time(mut self, duration: Duration) -> Self {
        self.max_wall_time = Some(duration);
        self
    }

    pub fn with_max_dollars(mut self, dollars: f64) -> Self {
        self.max_dollars = Some(dollars);
        self
    }

    pub fn with_max_parallel(mut self, parallel: usize) -> Self {
        self.max_parallel = Some(parallel);
        self
    }

    /// Returns true when `steps` would exceed `max_steps` (if set).
    pub fn steps_exceeded(&self, steps: u64) -> bool {
        self.max_steps.map(|max| steps > max).unwrap_or(false)
    }

    /// Returns true when `tokens` would exceed `max_tokens` (if set).
    pub fn tokens_exceeded(&self, tokens: u64) -> bool {
        self.max_tokens.map(|max| tokens > max).unwrap_or(false)
    }

    /// Cap for fan-out width (defaults to 8 when unset).
    pub fn parallel_limit(&self) -> usize {
        self.max_parallel.unwrap_or(8).max(1)
    }
}

mod duration_secs_opt {
    use std::time::Duration;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(d) => serializer.serialize_some(&d.as_secs_f64()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt = Option::<f64>::deserialize(deserializer)?;
        Ok(opt.map(Duration::from_secs_f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_builder_and_checks() {
        let budget = Budget::unlimited()
            .with_max_steps(3)
            .with_max_tokens(100)
            .with_max_parallel(2);
        assert!(!budget.steps_exceeded(3));
        assert!(budget.steps_exceeded(4));
        assert!(budget.tokens_exceeded(101));
        assert_eq!(budget.parallel_limit(), 2);
    }
}
