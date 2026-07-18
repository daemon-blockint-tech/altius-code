use std::fmt;

use crate::simulate::SimulationOutcome;

/// Human-readable summary of what a simulated transaction would do,
/// derived from a [`SimulationOutcome`] and shown to whoever (or
/// whatever) makes the approval decision at stage 4 of the pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    pub lamport_deltas: Vec<(String, i128)>,
    pub accounts_created: Vec<String>,
    pub accounts_closed: Vec<String>,
    /// (pubkey, owner before, owner after)
    pub owner_changes: Vec<(String, String, String)>,
    pub compute_units_consumed: u64,
    pub compute_unit_limit: u64,
}

impl DiffReport {
    pub fn from_simulation(outcome: &SimulationOutcome) -> DiffReport {
        let mut lamport_deltas = Vec::new();
        let mut accounts_created = Vec::new();
        let mut accounts_closed = Vec::new();
        let mut owner_changes = Vec::new();

        for delta in &outcome.account_deltas {
            let net = delta.lamports_after as i128 - delta.lamports_before as i128;
            if net != 0 {
                lamport_deltas.push((delta.pubkey.clone(), net));
            }
            if delta.created {
                accounts_created.push(delta.pubkey.clone());
            }
            if delta.closed {
                accounts_closed.push(delta.pubkey.clone());
            }
            if delta.owner_before != delta.owner_after {
                owner_changes.push((
                    delta.pubkey.clone(),
                    delta.owner_before.clone(),
                    delta.owner_after.clone(),
                ));
            }
        }

        DiffReport {
            lamport_deltas,
            accounts_created,
            accounts_closed,
            owner_changes,
            compute_units_consumed: outcome.compute_units_consumed,
            compute_unit_limit: outcome.compute_unit_limit,
        }
    }
}

impl fmt::Display for DiffReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "compute units: {}/{}",
            self.compute_units_consumed, self.compute_unit_limit
        )?;

        if self.lamport_deltas.is_empty() {
            writeln!(f, "lamport changes: none")?;
        } else {
            writeln!(f, "lamport changes:")?;
            for (pubkey, delta) in &self.lamport_deltas {
                writeln!(f, "  {pubkey}: {delta:+}")?;
            }
        }

        if !self.accounts_created.is_empty() {
            writeln!(f, "accounts created: {}", self.accounts_created.join(", "))?;
        }
        if !self.accounts_closed.is_empty() {
            writeln!(f, "accounts closed: {}", self.accounts_closed.join(", "))?;
        }
        if !self.owner_changes.is_empty() {
            writeln!(f, "owner changes:")?;
            for (pubkey, before, after) in &self.owner_changes {
                writeln!(f, "  {pubkey}: {before} -> {after}")?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulate::AccountDelta;

    #[test]
    fn summarizes_lamport_and_owner_changes() {
        let outcome = SimulationOutcome {
            success: true,
            logs: vec![],
            compute_units_consumed: 5_000,
            compute_unit_limit: 200_000,
            account_deltas: vec![
                AccountDelta {
                    pubkey: "Payer1111111111111111111111111111111111111".into(),
                    lamports_before: 1_000_000,
                    lamports_after: 900_000,
                    owner_before: "11111111111111111111111111111111".into(),
                    owner_after: "11111111111111111111111111111111".into(),
                    created: false,
                    closed: false,
                },
                AccountDelta {
                    pubkey: "NewAccount111111111111111111111111111111111".into(),
                    lamports_before: 0,
                    lamports_after: 100_000,
                    owner_before: "11111111111111111111111111111111".into(),
                    owner_after: "Prog11111111111111111111111111111111111111".into(),
                    created: true,
                    closed: false,
                },
            ],
            error: None,
        };

        let diff = DiffReport::from_simulation(&outcome);
        assert_eq!(diff.lamport_deltas.len(), 2);
        assert_eq!(
            diff.accounts_created,
            vec!["NewAccount111111111111111111111111111111111"]
        );
        assert_eq!(diff.owner_changes.len(), 1);

        let rendered = diff.to_string();
        assert!(rendered.contains("compute units: 5000/200000"));
        assert!(rendered.contains("accounts created"));
    }
}
