use std::fmt;

use crate::known_programs::{format_lamports, format_pubkey_label, label_program_id};
use crate::simulate::SimulationOutcome;
use crate::tx_request::{TxKind, TxRequest};

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
    /// Program ids invoked by the transaction message (deduplicated, in order).
    pub invoked_programs: Vec<String>,
    /// Coarse action label from the unsigned request.
    pub action_summary: Option<String>,
}

impl DiffReport {
    pub fn from_simulation(outcome: &SimulationOutcome) -> DiffReport {
        Self::from_simulation_and_tx(outcome, None)
    }

    pub fn from_simulation_and_tx(
        outcome: &SimulationOutcome,
        tx: Option<&TxRequest>,
    ) -> DiffReport {
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

        let (invoked_programs, action_summary) = tx
            .map(|request| {
                (
                    invoked_program_ids(request),
                    Some(summarize_action(request)),
                )
            })
            .unwrap_or_default();

        DiffReport {
            lamport_deltas,
            accounts_created,
            accounts_closed,
            owner_changes,
            compute_units_consumed: outcome.compute_units_consumed,
            compute_unit_limit: outcome.compute_unit_limit,
            invoked_programs,
            action_summary,
        }
    }
}

fn invoked_program_ids(tx: &TxRequest) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut programs = Vec::new();
    for instruction in &tx.message.instructions {
        let Some(program_id) = tx
            .message
            .account_keys
            .get(instruction.program_id_index as usize)
        else {
            continue;
        };
        let id = program_id.to_string();
        if seen.insert(id.clone()) {
            programs.push(id);
        }
    }
    programs
}

fn summarize_action(tx: &TxRequest) -> String {
    match &tx.kind {
        TxKind::Deploy => "Deploy program".into(),
        TxKind::Upgrade => "Upgrade program".into(),
        TxKind::SetAuthority => "Set authority".into(),
        TxKind::CloseAccount => "Close account".into(),
        TxKind::Transfer { lamports } => format!("Transfer {} lamports", lamports),
        TxKind::Payment { lamports } => format!("Payment {} lamports", lamports),
        TxKind::Invoke { instruction_name } => format!("Invoke `{instruction_name}`"),
    }
}

impl fmt::Display for DiffReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(action) = &self.action_summary {
            writeln!(f, "action: {action}")?;
        }

        if !self.invoked_programs.is_empty() {
            writeln!(f, "programs invoked:")?;
            for program_id in &self.invoked_programs {
                match label_program_id(program_id) {
                    Some(label) => writeln!(f, "  {program_id} ({label})")?,
                    None => writeln!(f, "  {program_id}")?,
                }
            }
        }

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
                writeln!(
                    f,
                    "  {}: {}",
                    format_pubkey_label(pubkey),
                    format_lamports(*delta)
                )?;
            }
        }

        if !self.accounts_created.is_empty() {
            writeln!(f, "accounts created:")?;
            for pubkey in &self.accounts_created {
                writeln!(f, "  {}", format_pubkey_label(pubkey))?;
            }
        }
        if !self.accounts_closed.is_empty() {
            writeln!(f, "accounts closed:")?;
            for pubkey in &self.accounts_closed {
                writeln!(f, "  {}", format_pubkey_label(pubkey))?;
            }
        }
        if !self.owner_changes.is_empty() {
            writeln!(f, "owner changes:")?;
            for (pubkey, before, after) in &self.owner_changes {
                writeln!(
                    f,
                    "  {}: {} -> {}",
                    format_pubkey_label(pubkey),
                    format_pubkey_label(before),
                    format_pubkey_label(after)
                )?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulate::AccountDelta;
    use altius_svm_detect::Cluster;
    use solana_message::Message;
    use solana_pubkey::Pubkey;

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
                    lamports_before: 1_000_000_000,
                    lamports_after: 500_000_000,
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
                    owner_after: "BPFLoaderUpgradeab1e11111111111111111111111".into(),
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
        assert!(rendered.contains("System Program"));
        assert!(rendered.contains("0.500000000 SOL"));
        assert!(rendered.contains("BPF Loader Upgradeable"));
    }

    #[test]
    fn includes_invoked_program_labels_from_tx() {
        let system = Pubkey::new_from_array([0u8; 32]);
        let token = Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
        let payer = Pubkey::new_unique();
        let message = Message::new(
            &[
                solana_instruction::Instruction::new_with_bytes(system, &[], vec![]),
                solana_instruction::Instruction::new_with_bytes(token, &[], vec![]),
            ],
            Some(&payer),
        );
        let tx = TxRequest::new(
            "transfer tokens",
            Cluster::Devnet,
            TxKind::Transfer { lamports: 1 },
            message,
        );
        let outcome = SimulationOutcome {
            success: true,
            logs: vec![],
            compute_units_consumed: 1,
            compute_unit_limit: 200_000,
            account_deltas: vec![],
            error: None,
        };

        let diff = DiffReport::from_simulation_and_tx(&outcome, Some(&tx));
        assert_eq!(diff.invoked_programs.len(), 2);
        let rendered = diff.to_string();
        assert!(rendered.contains("programs invoked"));
        assert!(rendered.contains("System Program"));
        assert!(rendered.contains("Token Program"));
        assert!(rendered.contains("action: Transfer"));
    }
}
