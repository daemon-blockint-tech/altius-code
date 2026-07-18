use std::str::FromStr;

use altius_signer::SignerClient;
use altius_svm_detect::{detect, Cluster};
use altius_svm_tools::DeploymentPlan;
use altius_txguard::{
    ApprovalChannel, AuditLogger, AutoApprove, FailClosed, GuardError, PolicyConfig, RpcSimulator,
    TxGuard, TxOutcome, TxRequest,
};
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;

use crate::cli::DeployArgs;
use crate::error::CliError;
use crate::rpc_endpoint::default_rpc_url;
use crate::terminal_approval::TerminalApproval;
use crate::toolchain_for::toolchain_for;

pub fn run_deploy(args: &DeployArgs) -> Result<(), CliError> {
    let project = detect(&args.project)?
        .ok_or_else(|| CliError::NotAnSvmProject(args.project.display().to_string()))?;

    let cluster = match &args.cluster {
        Some(raw) => Cluster::from_str(raw)?,
        None => project.default_cluster,
    };
    let rpc_url = args
        .rpc_url
        .clone()
        .unwrap_or_else(|| default_rpc_url(cluster).to_string());
    let rpc_client = RpcClient::new(rpc_url.clone());

    let socket_path = args
        .signer_socket
        .clone()
        .or_else(|| std::env::var("ALTIUS_SIGNER_SOCKET").ok().map(Into::into))
        .ok_or(CliError::MissingSignerSocket)?;
    let signer = SignerClient::new(&socket_path);
    let payer = Pubkey::from(signer.pubkey()?.0);

    let recent_blockhash = rpc_client
        .get_latest_blockhash()
        .map_err(|e| CliError::Rpc {
            rpc_url: rpc_url.clone(),
            reason: e.to_string(),
        })?;

    let toolchain = toolchain_for(project.framework, &args.project);
    let plan: DeploymentPlan = toolchain.deploy(cluster, payer, recent_blockhash, args.upgrade)?;

    println!(
        "deployment plan: buffer {}, program {}, {} write step(s)",
        plan.buffer_pubkey,
        plan.program_pubkey,
        plan.write_chunks.len()
    );

    let policy = load_policy(&args.project)?;

    if args.dry_run {
        return run_dry(&plan, &rpc_url, &policy);
    }

    let audit_path = args
        .project
        .join(".altius")
        .join("txlog")
        .join("audit.jsonl");

    for tx in plan_steps(plan) {
        let description = tx.description.clone();
        let outcome = if args.yes {
            submit_one(
                &policy,
                &rpc_url,
                AutoApprove,
                &audit_path,
                &socket_path,
                tx,
            )?
        } else {
            submit_one(
                &policy,
                &rpc_url,
                TerminalApproval,
                &audit_path,
                &socket_path,
                tx,
            )?
        };

        match outcome {
            TxOutcome::Signed { transaction } => {
                let signature = rpc_client
                    .send_and_confirm_transaction(&transaction)
                    .map_err(|e| CliError::Rpc {
                        rpc_url: rpc_url.clone(),
                        reason: e.to_string(),
                    })?;
                println!("{description}: confirmed {signature}");
            }
            TxOutcome::ApprovedNoSigner => {
                println!("{description}: approved (no signer configured, nothing broadcast)");
            }
        }
    }

    Ok(())
}

fn load_policy(project_root: &std::path::Path) -> Result<PolicyConfig, CliError> {
    let policy_path = project_root.join("altius.toml");
    if !policy_path.is_file() {
        return Ok(PolicyConfig::default());
    }
    let contents = std::fs::read_to_string(&policy_path)?;
    Ok(PolicyConfig::from_toml_str(&contents)?)
}

fn plan_steps(plan: DeploymentPlan) -> Vec<TxRequest> {
    let mut steps = Vec::with_capacity(2 + plan.write_chunks.len());
    steps.push(plan.create_buffer);
    steps.extend(plan.write_chunks);
    steps.push(plan.finalize);
    steps
}

/// Builds a fresh, single-use `TxGuard` for one transaction and submits
/// it. A new guard per step (rather than one long-lived guard) keeps this
/// generic over whichever concrete `ApprovalChannel` the caller picked
/// (`AutoApprove` for `--yes`, `TerminalApproval` otherwise) without
/// needing dynamic dispatch; `SignerClient` and `RpcSimulator` are both
/// cheap to (re)construct, since neither holds a persistent connection.
fn submit_one(
    policy: &PolicyConfig,
    rpc_url: &str,
    approval: impl ApprovalChannel,
    audit_path: &std::path::Path,
    socket_path: &std::path::Path,
    tx: TxRequest,
) -> Result<TxOutcome, CliError> {
    let mut guard = TxGuard::new(
        policy.clone(),
        RpcSimulator::new(rpc_url.to_string()),
        approval,
        AuditLogger::open(audit_path)?,
    )
    .with_signer(SignerClient::new(socket_path));
    Ok(guard.submit(tx)?)
}

/// `--dry-run`: run policy, mandatory simulation, and diff reporting for
/// every step and print the results, without ever approving, signing, or
/// broadcasting anything. `FailClosed` guarantees that even if this is
/// later wired up with a real signer by mistake, nothing gets signed —
/// every step is expected to end in `ApprovalDenied`.
fn run_dry(plan: &DeploymentPlan, rpc_url: &str, policy: &PolicyConfig) -> Result<(), CliError> {
    let audit_path =
        std::env::temp_dir().join(format!("altius-dry-run-{}.jsonl", std::process::id()));

    for tx in [&plan.create_buffer]
        .into_iter()
        .chain(plan.write_chunks.iter())
        .chain([&plan.finalize])
    {
        let mut guard = TxGuard::new(
            policy.clone(),
            RpcSimulator::new(rpc_url.to_string()),
            FailClosed,
            AuditLogger::open(&audit_path)?,
        );
        match guard.submit(tx.clone()) {
            Ok(_) => unreachable!("FailClosed never approves a transaction"),
            Err(GuardError::ApprovalDenied(_)) => {
                println!(
                    "{}: policy + simulation ok (dry run, not submitted)",
                    tx.description
                );
            }
            Err(other) => return Err(other.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn deploy_args(project: PathBuf) -> DeployArgs {
        DeployArgs {
            project,
            cluster: None,
            rpc_url: None,
            signer_socket: None,
            upgrade: false,
            yes: false,
            dry_run: false,
        }
    }

    #[test]
    fn deploy_rejects_a_non_svm_project_before_touching_the_network_or_a_signer() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "not a program").unwrap();

        // No --signer-socket, no ALTIUS_SIGNER_SOCKET, no reachable RPC —
        // if detection didn't short-circuit first, this would fail with a
        // signer or RPC error instead, which the assertion below rules out.
        let err = run_deploy(&deploy_args(dir.path().to_path_buf())).unwrap_err();
        assert!(matches!(err, CliError::NotAnSvmProject(_)));
    }

    #[test]
    fn load_policy_returns_defaults_when_altius_toml_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let policy = load_policy(dir.path()).unwrap();
        assert_eq!(
            policy.allowed_clusters,
            PolicyConfig::default().allowed_clusters
        );
    }

    #[test]
    fn load_policy_reads_project_overrides() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("altius.toml"), "max_lamports_out = 42\n").unwrap();
        let policy = load_policy(dir.path()).unwrap();
        assert_eq!(policy.max_lamports_out, 42);
    }
}
