//! Building a [`TxRequest`] for an x402 requirement and settling it through
//! [`TxGuard`] — the only code path in this crate that can lead to a
//! signature, and it goes through every guardrail stage.

use std::str::FromStr;

use altius_txguard::{ApprovalChannel, Simulator, TxGuard, TxKind, TxOutcome, TxRequest};
use base64::Engine;
use serde::{Deserialize, Serialize};
use solana_hash::Hash;
use solana_message::Message;
use solana_pubkey::Pubkey;

use crate::challenge::PaymentRequirements;
use crate::error::{PaymentError, PaymentResult};

/// Build an unsigned [`TxRequest`] (kind [`TxKind::Payment`]) paying the
/// given requirement from `payer`. Only native-SOL `exact` requirements are
/// supported; SPL-token settlement is a deliberate later extension.
pub fn build_payment_request(
    requirement: &PaymentRequirements,
    payer: &Pubkey,
    recent_blockhash: Hash,
) -> PaymentResult<TxRequest> {
    if !requirement.is_native_sol() {
        return Err(PaymentError::UnsupportedAsset(requirement.asset.clone()));
    }
    let cluster = requirement.cluster()?;
    let lamports = requirement.lamports()?;
    let pay_to = Pubkey::from_str(&requirement.pay_to)
        .map_err(|error| PaymentError::InvalidPayTo(error.to_string()))?;

    let instruction = solana_system_interface::instruction::transfer(payer, &pay_to, lamports);
    let message = Message::new_with_blockhash(&[instruction], Some(payer), &recent_blockhash);

    Ok(TxRequest::new(
        format!(
            "x402 payment of {lamports} lamports to {pay_to} for {}",
            requirement.resource
        ),
        cluster,
        TxKind::Payment { lamports },
        message,
    ))
}

/// The `X-PAYMENT` request-header payload proving settlement, per the x402
/// `exact` scheme: a base64-wrapped JSON envelope carrying the fully signed,
/// bincode-serialized Solana transaction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentProof {
    pub x402_version: u64,
    pub scheme: String,
    pub network: String,
    pub payload: ProofPayload,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofPayload {
    /// Base64 of the bincode-serialized signed transaction.
    pub transaction: String,
}

impl PaymentProof {
    /// Encode as the `X-PAYMENT` header value.
    pub fn to_header_value(&self) -> PaymentResult<String> {
        let json = serde_json::to_vec(self)
            .map_err(|error| PaymentError::InvalidProof(error.to_string()))?;
        Ok(base64::engine::general_purpose::STANDARD.encode(json))
    }

    /// Decode an `X-PAYMENT` header value (used by tests and any future
    /// facilitator-side verification).
    pub fn from_header_value(value: &str) -> PaymentResult<Self> {
        let json = base64::engine::general_purpose::STANDARD
            .decode(value)
            .map_err(|error| PaymentError::InvalidProof(error.to_string()))?;
        serde_json::from_slice(&json).map_err(|error| PaymentError::InvalidProof(error.to_string()))
    }
}

/// Settle a payment requirement through the guardrail pipeline.
///
/// This is the only way `altius-payments` produces a signed transaction:
/// [`TxGuard::submit`] runs policy (Payment is in the default
/// `deny_instructions`, so it always at least requires approval), mandatory
/// simulation, diff, approval, and audit logging. If the guard has no signer
/// (dry-run configurations), this fails with [`PaymentError::NoSigner`]
/// rather than pretending settlement happened.
pub fn settle_via_guard<Sim: Simulator, App: ApprovalChannel>(
    guard: &mut TxGuard<Sim, App>,
    requirement: &PaymentRequirements,
    payer: &Pubkey,
    recent_blockhash: Hash,
) -> PaymentResult<PaymentProof> {
    let request = build_payment_request(requirement, payer, recent_blockhash)?;
    match guard.submit(request)? {
        TxOutcome::Signed { transaction } => {
            let bytes = bincode::serialize(&transaction)
                .map_err(|error| PaymentError::InvalidProof(error.to_string()))?;
            Ok(PaymentProof {
                x402_version: crate::challenge::SUPPORTED_X402_VERSION,
                scheme: requirement.scheme.clone(),
                network: requirement.network.clone(),
                payload: ProofPayload {
                    transaction: base64::engine::general_purpose::STANDARD.encode(bytes),
                },
            })
        }
        TxOutcome::ApprovedNoSigner => Err(PaymentError::NoSigner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::challenge::PaymentChallenge;
    use altius_svm_detect::Cluster;
    use altius_txguard::testing::MockSimulator;
    use altius_txguard::{
        ApprovalDecision, AuditLogger, DiffReport, FailClosed, GuardError, PolicyConfig,
    };

    fn devnet_requirement() -> PaymentRequirements {
        let body = r#"{
            "x402Version": 1,
            "accepts": [{
                "scheme": "exact",
                "network": "solana-devnet",
                "maxAmountRequired": "10000",
                "resource": "https://api.example.com/v1/report",
                "payTo": "7VHUFJHWu2CuExkJcJrzhQPJ2oygupTWkL2A2For4BmE",
                "asset": ""
            }]
        }"#;
        PaymentChallenge::parse(body)
            .unwrap()
            .select_solana_requirement()
            .unwrap()
            .clone()
    }

    #[test]
    fn builds_a_payment_tx_request() {
        let payer = Pubkey::new_unique();
        let request =
            build_payment_request(&devnet_requirement(), &payer, Hash::default()).unwrap();
        assert_eq!(request.cluster, Cluster::Devnet);
        assert_eq!(request.kind, TxKind::Payment { lamports: 10_000 });
        assert!(request.description.contains("x402 payment"));
        // Exactly one signer: the payer wallet, signed only via TxGuard.
        assert_eq!(request.message.header.num_required_signatures, 1);
    }

    #[test]
    fn refuses_spl_assets() {
        let mut requirement = devnet_requirement();
        requirement.asset = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into();
        let payer = Pubkey::new_unique();
        assert!(matches!(
            build_payment_request(&requirement, &payer, Hash::default()),
            Err(PaymentError::UnsupportedAsset(_))
        ));
    }

    #[test]
    fn refuses_bad_pay_to_address() {
        let mut requirement = devnet_requirement();
        requirement.pay_to = "not-a-pubkey".into();
        let payer = Pubkey::new_unique();
        assert!(matches!(
            build_payment_request(&requirement, &payer, Hash::default()),
            Err(PaymentError::InvalidPayTo(_))
        ));
    }

    #[test]
    fn headless_fail_closed_guard_denies_payment() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = TxGuard::new(
            PolicyConfig::default(),
            MockSimulator::success(),
            FailClosed,
            AuditLogger::open(dir.path().join("txlog.jsonl")).unwrap(),
        );
        let payer = Pubkey::new_unique();
        let err = settle_via_guard(&mut guard, &devnet_requirement(), &payer, Hash::default())
            .unwrap_err();
        assert!(matches!(
            err,
            PaymentError::Guard(GuardError::ApprovalDenied(_))
        ));
    }

    /// Approval channel that approves everything — only exists to prove the
    /// no-signer configuration cannot fake a settlement proof.
    struct ApproveAll;

    impl ApprovalChannel for ApproveAll {
        fn request_approval(
            &self,
            _tx: &TxRequest,
            _diff: &DiffReport,
            _requires_manual: bool,
        ) -> Result<ApprovalDecision, GuardError> {
            Ok(ApprovalDecision::Approved)
        }
    }

    #[test]
    fn approved_payment_without_signer_yields_no_proof() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = TxGuard::new(
            PolicyConfig::default(),
            MockSimulator::success(),
            ApproveAll,
            AuditLogger::open(dir.path().join("txlog.jsonl")).unwrap(),
        );
        let payer = Pubkey::new_unique();
        let err = settle_via_guard(&mut guard, &devnet_requirement(), &payer, Hash::default())
            .unwrap_err();
        assert!(matches!(err, PaymentError::NoSigner));
    }

    #[test]
    fn oversized_payment_is_rejected_by_policy() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = TxGuard::new(
            PolicyConfig {
                max_lamports_out: 100,
                ..PolicyConfig::default()
            },
            MockSimulator::success(),
            FailClosed,
            AuditLogger::open(dir.path().join("txlog.jsonl")).unwrap(),
        );
        let payer = Pubkey::new_unique();
        let err = settle_via_guard(&mut guard, &devnet_requirement(), &payer, Hash::default())
            .unwrap_err();
        assert!(matches!(
            err,
            PaymentError::Guard(GuardError::PolicyRejected(_))
        ));
    }

    #[test]
    fn proof_header_round_trips() {
        let proof = PaymentProof {
            x402_version: 1,
            scheme: "exact".into(),
            network: "solana-devnet".into(),
            payload: ProofPayload {
                transaction: "AAAA".into(),
            },
        };
        let header = proof.to_header_value().unwrap();
        assert_eq!(PaymentProof::from_header_value(&header).unwrap(), proof);
    }
}
