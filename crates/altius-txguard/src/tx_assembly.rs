use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_transaction::Transaction;

use crate::error::GuardError;
use crate::tx_request::TxRequest;

/// Builds a [`Transaction`] from `tx.message`, filling in `signatures` by
/// matching each provided pubkey against the message's signer list (in
/// whatever order they were passed — order in `signatures` does not need
/// to match `message.signer_keys()`).
///
/// When `allow_missing` is `false`, every required signer must have a
/// matching signature or this returns [`GuardError::IncompleteSignatures`]
/// naming the ones still missing — this is the strict path used right
/// before a transaction is actually submitted. When `true`, missing
/// signers are left as the default (all-zero) `Signature` — this is what
/// mandatory simulation uses, matching `simulateTransaction`'s
/// `sigVerify: false` mode, which does not require real signatures at all
/// (see Phase 0 spec §6 stage 2).
pub fn assemble_transaction(
    tx: &TxRequest,
    signatures: &[(Pubkey, Signature)],
    allow_missing: bool,
) -> Result<Transaction, GuardError> {
    let mut transaction = Transaction::new_unsigned(tx.message.clone());

    let mut missing = Vec::new();
    for (index, signer) in tx.message.signer_keys().iter().enumerate() {
        match signatures.iter().find(|(pubkey, _)| pubkey == *signer) {
            Some((_, signature)) => transaction.signatures[index] = *signature,
            None => missing.push(signer.to_string()),
        }
    }

    if !allow_missing && !missing.is_empty() {
        return Err(GuardError::IncompleteSignatures(missing));
    }

    Ok(transaction)
}

/// Convenience wrapper combining `tx.extra_signatures` with the wallet's
/// signature obtained separately (from `altius-signer`, post-approval).
pub fn assemble_signed_transaction(
    tx: &TxRequest,
    wallet_pubkey: Pubkey,
    wallet_signature: Signature,
) -> Result<Transaction, GuardError> {
    let mut signatures = tx.extra_signatures.clone();
    signatures.push((wallet_pubkey, wallet_signature));
    assemble_transaction(tx, &signatures, false)
}

/// Assembles a transaction suitable for mandatory simulation: every
/// required signer that doesn't already have a signature in
/// `tx.extra_signatures` gets a placeholder all-zero signature, which is
/// exactly what `simulateTransaction` with `sigVerify: false` expects —
/// it does not check signatures at all.
pub fn assemble_for_simulation(tx: &TxRequest) -> Transaction {
    assemble_transaction(tx, &tx.extra_signatures, true)
        .expect("allow_missing=true never returns IncompleteSignatures")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx_request::TxKind;
    use altius_svm_detect::Cluster;
    use solana_instruction::{AccountMeta, Instruction};
    use solana_message::Message;

    fn two_signer_message() -> (Message, Pubkey, Pubkey) {
        let payer = Pubkey::new_unique();
        let other_signer = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();
        let instruction = Instruction::new_with_bytes(
            program_id,
            &[],
            vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(other_signer, true),
            ],
        );
        let message = Message::new(&[instruction], Some(&payer));
        (message, payer, other_signer)
    }

    #[test]
    fn simulation_assembly_fills_placeholders_for_every_missing_signer() {
        let (message, _, _) = two_signer_message();
        let tx = TxRequest::new("test", Cluster::Devnet, TxKind::Deploy, message);
        let assembled = assemble_for_simulation(&tx);
        assert_eq!(assembled.signatures.len(), 2);
        assert!(assembled
            .signatures
            .iter()
            .all(|s| *s == Signature::default()));
    }

    #[test]
    fn strict_assembly_fails_when_a_signer_is_missing() {
        let (message, payer, _other_signer) = two_signer_message();
        let tx = TxRequest::new("test", Cluster::Devnet, TxKind::Deploy, message);
        let err = assemble_transaction(&tx, &[(payer, Signature::default())], false).unwrap_err();
        assert!(matches!(err, GuardError::IncompleteSignatures(missing) if missing.len() == 1));
    }

    #[test]
    fn strict_assembly_succeeds_once_every_signer_is_present() {
        let (message, payer, other_signer) = two_signer_message();
        let tx = TxRequest::new("test", Cluster::Devnet, TxKind::Deploy, message);
        let signatures = vec![
            (payer, Signature::from([1u8; 64])),
            (other_signer, Signature::from([2u8; 64])),
        ];
        let assembled = assemble_transaction(&tx, &signatures, false).unwrap();
        let payer_index = assembled
            .message
            .signer_keys()
            .iter()
            .position(|k| **k == payer)
            .unwrap();
        let other_index = assembled
            .message
            .signer_keys()
            .iter()
            .position(|k| **k == other_signer)
            .unwrap();
        assert_eq!(
            assembled.signatures[payer_index],
            Signature::from([1u8; 64])
        );
        assert_eq!(
            assembled.signatures[other_index],
            Signature::from([2u8; 64])
        );
    }

    #[test]
    fn assemble_signed_transaction_combines_extra_and_wallet_signatures() {
        let (message, payer, other_signer) = two_signer_message();
        let mut tx = TxRequest::new("test", Cluster::Devnet, TxKind::Deploy, message);
        tx.extra_signatures
            .push((other_signer, Signature::from([9u8; 64])));

        let assembled =
            assemble_signed_transaction(&tx, payer, Signature::from([7u8; 64])).unwrap();
        assert!(assembled.signatures.contains(&Signature::from([7u8; 64])));
        assert!(assembled.signatures.contains(&Signature::from([9u8; 64])));
    }
}
