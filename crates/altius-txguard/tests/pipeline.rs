//! End-to-end exercises of the five-stage guardrail pipeline described in
//! `docs/specs/FASE-0_SVM_INTEGRATION_SPEC.md` §6: policy, mandatory
//! simulation, diff, approval, audit log. These tests are the acceptance
//! evidence for the "no transaction reaches a signer without simulation
//! and approval" invariant the spec calls out as non-negotiable.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use altius_signer::{KeypairFileSigner, Signer as _, SignerClient, SignerServer};
use altius_svm_detect::Cluster;
use altius_txguard::{
    verify_chain, AutoApprove, FailClosed, GuardError, PolicyConfig, SimulationOutcome, Simulator,
    TxGuard, TxKind, TxOutcome, TxRequest,
};
use ed25519_dalek::{SigningKey, Verifier};
use rand::rngs::SysRng;
use rand_core::UnwrapErr;
use solana_instruction::{AccountMeta, Instruction};
use solana_message::Message;
use solana_pubkey::Pubkey;

/// Wraps another `Simulator` and counts how many times `simulate` was
/// actually invoked, so tests can prove a rejected-at-policy transaction
/// never reaches simulation at all.
struct CountingSimulator<S: Simulator> {
    inner: S,
    calls: Arc<AtomicUsize>,
}

impl<S: Simulator> Simulator for CountingSimulator<S> {
    fn simulate(&self, tx: &TxRequest) -> Result<SimulationOutcome, GuardError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.simulate(tx)
    }
}

/// A real, single-instruction message: `payer` signs and is the only
/// account touched, invoking a throwaway program id.
fn benign_invoke(cluster: Cluster, payer: Pubkey) -> TxRequest {
    let program_id = Pubkey::new_unique();
    let instruction =
        Instruction::new_with_bytes(program_id, &[], vec![AccountMeta::new(payer, true)]);
    let message = Message::new(&[instruction], Some(&payer));
    TxRequest::new(
        "call the ping instruction",
        cluster,
        TxKind::Invoke {
            instruction_name: "ping".to_string(),
        },
        message,
    )
}

#[test]
fn benign_devnet_invoke_is_approved_without_a_signer() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("txlog").join("audit.jsonl");

    let mut guard = TxGuard::new(
        PolicyConfig::default(),
        altius_txguard::testing::MockSimulator::success(),
        AutoApprove,
        altius_txguard::AuditLogger::open(&audit_path).unwrap(),
    );

    let outcome = guard
        .submit(benign_invoke(Cluster::Devnet, Pubkey::new_unique()))
        .unwrap();
    assert_eq!(outcome, TxOutcome::ApprovedNoSigner);
    verify_chain(&audit_path).unwrap();
    assert_eq!(count_entries(&audit_path), 1);
}

#[test]
fn policy_rejection_never_reaches_simulation() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let calls = Arc::new(AtomicUsize::new(0));

    let mut guard = TxGuard::new(
        PolicyConfig::default(), // localnet/devnet only
        CountingSimulator {
            inner: altius_txguard::testing::MockSimulator::success(),
            calls: Arc::clone(&calls),
        },
        AutoApprove,
        altius_txguard::AuditLogger::open(&audit_path).unwrap(),
    );

    // Testnet is not in the default allowed_clusters list.
    let err = guard
        .submit(benign_invoke(Cluster::Testnet, Pubkey::new_unique()))
        .unwrap_err();
    assert!(matches!(err, GuardError::PolicyRejected(_)));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "simulate() must not run after a policy rejection"
    );
    verify_chain(&audit_path).unwrap();
    assert_eq!(count_entries(&audit_path), 1);
}

#[test]
fn failed_simulation_blocks_approval_and_signing() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");

    let mut guard = TxGuard::new(
        PolicyConfig::default(),
        altius_txguard::testing::MockSimulator::failure("insufficient funds"),
        AutoApprove,
        altius_txguard::AuditLogger::open(&audit_path).unwrap(),
    );

    let err = guard
        .submit(benign_invoke(Cluster::Devnet, Pubkey::new_unique()))
        .unwrap_err();
    assert!(matches!(err, GuardError::SimulationFailed(reason) if reason == "insufficient funds"));
    verify_chain(&audit_path).unwrap();
}

#[test]
fn mainnet_is_denied_even_with_auto_approve_wired() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");

    // A policy that (mis)configures mainnet as allowed, paired with the
    // auto-approve channel a careless integration might wire everywhere.
    let policy = PolicyConfig {
        allowed_clusters: vec![Cluster::MainnetBeta],
        ..PolicyConfig::default()
    };
    let mut guard = TxGuard::new(
        policy,
        altius_txguard::testing::MockSimulator::success(),
        AutoApprove,
        altius_txguard::AuditLogger::open(&audit_path).unwrap(),
    );

    let payer = Pubkey::new_unique();
    let program_id = Pubkey::new_unique();
    let instruction =
        Instruction::new_with_bytes(program_id, &[], vec![AccountMeta::new(payer, true)]);
    let message = Message::new(&[instruction], Some(&payer));
    let tx = TxRequest::new(
        "upgrade the program",
        Cluster::MainnetBeta,
        TxKind::Upgrade,
        message,
    );

    let err = guard.submit(tx).unwrap_err();
    assert!(matches!(err, GuardError::ApprovalDenied(_)));
    verify_chain(&audit_path).unwrap();
}

#[test]
fn headless_fail_closed_denies_even_a_benign_devnet_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");

    let mut guard = TxGuard::new(
        PolicyConfig::default(),
        altius_txguard::testing::MockSimulator::success(),
        FailClosed,
        altius_txguard::AuditLogger::open(&audit_path).unwrap(),
    );

    let err = guard
        .submit(benign_invoke(Cluster::Devnet, Pubkey::new_unique()))
        .unwrap_err();
    assert!(matches!(err, GuardError::ApprovalDenied(_)));
}

/// Full happy path with a real signer process on the other end of a Unix
/// socket: policy continues, simulation succeeds, AutoApprove approves a
/// benign devnet invoke, and the assembled `Transaction`'s signature
/// verifies against the signer's public key over the exact message bytes
/// that were signed.
#[test]
fn approved_transaction_is_actually_signed_by_the_isolated_signer() {
    let dir = tempfile::tempdir().unwrap();
    let keypair_path = dir.path().join("id.json");
    let socket_path = dir.path().join("signer.sock");
    let audit_path = dir.path().join("audit.jsonl");

    let signing_key = SigningKey::generate(&mut UnwrapErr(SysRng));
    std::fs::write(
        &keypair_path,
        serde_json::to_string(&signing_key.to_keypair_bytes().to_vec()).unwrap(),
    )
    .unwrap();

    let backend = KeypairFileSigner::load(&keypair_path).unwrap();
    let expected_pubkey = backend.pubkey();
    let server = SignerServer::new(&socket_path, backend);
    thread::spawn(move || {
        let _ = server.run();
    });
    wait_for_socket(&socket_path);

    let mut guard = TxGuard::new(
        PolicyConfig::default(),
        altius_txguard::testing::MockSimulator::success(),
        AutoApprove,
        altius_txguard::AuditLogger::open(&audit_path).unwrap(),
    )
    .with_signer(SignerClient::new(&socket_path));

    let payer = Pubkey::from(expected_pubkey.0);
    let tx = benign_invoke(Cluster::Devnet, payer);
    let message_bytes = tx.message.serialize();
    let outcome = guard.submit(tx).unwrap();

    let TxOutcome::Signed { transaction } = outcome else {
        panic!("expected a signed outcome, got {outcome:?}");
    };
    let signer_index = transaction
        .message
        .signer_keys()
        .iter()
        .position(|k| **k == payer)
        .expect("payer must be a signer");
    let signature = transaction.signatures[signer_index];

    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&expected_pubkey.0).unwrap();
    let dalek_sig = ed25519_dalek::Signature::from_bytes(&signature.into());
    assert!(verifying_key.verify(&message_bytes, &dalek_sig).is_ok());

    verify_chain(&audit_path).unwrap();
}

fn wait_for_socket(path: &Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("signer socket never appeared at {path:?}");
}

fn count_entries(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}
