use std::io::{self, BufRead, Write};

use altius_txguard::{ApprovalChannel, ApprovalDecision, DiffReport, GuardError, TxRequest};

/// Prints the transaction description and diff report to stdout, then
/// blocks on a `y`/`n` line from stdin. This is the human in the loop
/// Phase 0 spec §6 stage 4 requires for mainnet and irreversible
/// transactions — there is deliberately no way to configure this channel
/// to skip the prompt for those.
pub struct TerminalApproval;

impl ApprovalChannel for TerminalApproval {
    fn request_approval(
        &self,
        tx: &TxRequest,
        diff: &DiffReport,
        requires_manual: bool,
    ) -> Result<ApprovalDecision, GuardError> {
        println!("\n--- {} ({}) ---", tx.description, tx.cluster);
        if requires_manual {
            println!("(policy flags this transaction as requiring manual approval)");
        }
        print!("{diff}");
        print!("Approve and sign? [y/N] ");
        io::stdout().flush().ok();

        let mut line = String::new();
        io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(GuardError::Io)?;

        if line.trim().eq_ignore_ascii_case("y") {
            Ok(ApprovalDecision::Approved)
        } else {
            Ok(ApprovalDecision::Denied {
                reason: "declined at the interactive prompt".to_string(),
            })
        }
    }
}
