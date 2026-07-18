//! Fakes for exercising the guardrail pipeline without a real RPC
//! endpoint or an interactive approval prompt. Intended for tests and
//! local dry runs — nothing in this module belongs in a code path that
//! can reach the real signer.

use crate::error::GuardError;
use crate::simulate::{SimulationOutcome, Simulator};
use crate::tx_request::TxRequest;

/// Always returns the same, canned [`SimulationOutcome`] regardless of
/// what transaction it is asked to simulate.
pub struct MockSimulator {
    pub outcome: SimulationOutcome,
}

impl MockSimulator {
    /// A canned successful simulation touching no accounts.
    pub fn success() -> MockSimulator {
        MockSimulator {
            outcome: SimulationOutcome {
                success: true,
                logs: vec!["Program log: mock success".to_string()],
                compute_units_consumed: 1_000,
                compute_unit_limit: 200_000,
                account_deltas: vec![],
                error: None,
            },
        }
    }

    /// A canned failed simulation with the given error message.
    pub fn failure(reason: impl Into<String>) -> MockSimulator {
        let reason = reason.into();
        MockSimulator {
            outcome: SimulationOutcome {
                success: false,
                logs: vec![format!("Program log: mock failure: {reason}")],
                compute_units_consumed: 0,
                compute_unit_limit: 200_000,
                account_deltas: vec![],
                error: Some(reason),
            },
        }
    }
}

impl Simulator for MockSimulator {
    fn simulate(&self, _tx: &TxRequest) -> Result<SimulationOutcome, GuardError> {
        Ok(self.outcome.clone())
    }
}
