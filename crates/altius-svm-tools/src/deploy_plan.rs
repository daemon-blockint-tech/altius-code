use std::fs;
use std::path::Path;

use altius_svm_detect::Cluster;
use altius_txguard::{TxKind, TxRequest};
use solana_hash::Hash;
use solana_keypair::{read_keypair_file, write_keypair_file, Keypair};
use solana_loader_v3_interface::instruction as loader_instruction;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_message::Message;
use solana_pubkey::Pubkey;
use solana_rent::Rent;
use solana_signer::Signer as SolanaSigner;

use crate::error::ToolError;

/// Max bytes of program data carried by a single `Write` instruction.
/// Chosen conservatively so one write transaction (payer + this
/// instruction's overhead) stays under Solana's 1232-byte packet limit —
/// the same order of magnitude the Solana CLI and Anchor use for buffer
/// writes.
pub const WRITE_CHUNK_SIZE: usize = 900;

/// The ordered sequence of transactions needed to get `program_bytes`
/// onto a buffer account and into a program: create the buffer, write it
/// in chunks, then deploy (first time) or upgrade (redeploy). Every
/// field is a [`TxRequest`] that has not been submitted or signed — each
/// one still has to pass through `altius_txguard::TxGuard::submit`
/// individually, in this order, since a later step depends on an earlier
/// one having actually landed on-chain.
#[derive(Debug)]
pub struct DeploymentPlan {
    pub create_buffer: TxRequest,
    pub write_chunks: Vec<TxRequest>,
    pub finalize: TxRequest,
    pub buffer_pubkey: Pubkey,
    pub program_pubkey: Pubkey,
}

/// Loads `<deploy_dir>/<program_name>-keypair.json` if it already exists,
/// or generates and persists a fresh one — the same convention Anchor
/// and the Solana CLI use for a program's own keypair. This keypair is
/// not the protected wallet key: it only proves the caller controls the
/// address a program lives at, and — matching existing tooling — lives
/// in the project's `target/deploy` directory rather than being treated
/// as long-term secret material.
pub fn load_or_generate_program_keypair(
    deploy_dir: &Path,
    program_name: &str,
) -> Result<Keypair, ToolError> {
    let path = deploy_dir.join(format!("{program_name}-keypair.json"));
    if path.is_file() {
        return read_keypair_file(&path).map_err(|e| ToolError::Keypair(e.to_string()));
    }
    fs::create_dir_all(deploy_dir)?;
    let keypair = Keypair::new();
    write_keypair_file(&keypair, &path).map_err(|e| ToolError::Keypair(e.to_string()))?;
    Ok(keypair)
}

/// Builds the full [`DeploymentPlan`] for either an initial deploy
/// (`is_upgrade = false`) or a redeploy of an already-existing program
/// (`is_upgrade = true`). `max_data_len` should leave headroom above
/// `program_bytes.len()` for future upgrades — Anchor and the Solana CLI
/// both default to roughly twice the current program size.
#[allow(clippy::too_many_arguments)]
pub fn build_deployment_plan(
    program_bytes: &[u8],
    payer: Pubkey,
    program_keypair: &Keypair,
    max_data_len: usize,
    cluster: Cluster,
    recent_blockhash: Hash,
    is_upgrade: bool,
) -> Result<DeploymentPlan, ToolError> {
    let buffer_keypair = Keypair::new();
    let buffer_pubkey = buffer_keypair.pubkey();
    let program_pubkey = program_keypair.pubkey();
    let rent = Rent::default();

    let create_buffer = build_create_buffer_tx(
        program_bytes,
        &payer,
        &buffer_keypair,
        &rent,
        cluster,
        recent_blockhash,
    )?;

    let write_chunks = build_write_chunk_txs(
        program_bytes,
        &payer,
        &buffer_pubkey,
        cluster,
        recent_blockhash,
    );

    let finalize = if is_upgrade {
        build_upgrade_tx(
            &program_pubkey,
            &buffer_pubkey,
            &payer,
            cluster,
            recent_blockhash,
        )
    } else {
        build_deploy_tx(
            &payer,
            program_keypair,
            &buffer_pubkey,
            max_data_len,
            &rent,
            cluster,
            recent_blockhash,
        )?
    };

    Ok(DeploymentPlan {
        create_buffer,
        write_chunks,
        finalize,
        buffer_pubkey,
        program_pubkey,
    })
}

fn build_create_buffer_tx(
    program_bytes: &[u8],
    payer: &Pubkey,
    buffer_keypair: &Keypair,
    rent: &Rent,
    cluster: Cluster,
    recent_blockhash: Hash,
) -> Result<TxRequest, ToolError> {
    let buffer_pubkey = buffer_keypair.pubkey();
    let buffer_lamports =
        rent.minimum_balance(UpgradeableLoaderState::size_of_buffer(program_bytes.len()));
    let instructions = loader_instruction::create_buffer(
        payer,
        &buffer_pubkey,
        payer,
        buffer_lamports,
        program_bytes.len(),
    )
    .map_err(|e| ToolError::InstructionBuild(e.to_string()))?;

    let message = Message::new_with_blockhash(&instructions, Some(payer), &recent_blockhash);
    let buffer_signature = buffer_keypair.sign_message(&message.serialize());

    let mut tx = TxRequest::new(
        format!(
            "create buffer account {buffer_pubkey} ({} bytes) on {cluster}",
            program_bytes.len()
        ),
        cluster,
        TxKind::Deploy,
        message,
    );
    tx.extra_signatures.push((buffer_pubkey, buffer_signature));
    Ok(tx)
}

fn build_write_chunk_txs(
    program_bytes: &[u8],
    payer: &Pubkey,
    buffer_pubkey: &Pubkey,
    cluster: Cluster,
    recent_blockhash: Hash,
) -> Vec<TxRequest> {
    program_bytes
        .chunks(WRITE_CHUNK_SIZE)
        .enumerate()
        .map(|(chunk_index, chunk)| {
            let offset = (chunk_index * WRITE_CHUNK_SIZE) as u32;
            let instruction =
                loader_instruction::write(buffer_pubkey, payer, offset, chunk.to_vec());
            let message =
                Message::new_with_blockhash(&[instruction], Some(payer), &recent_blockhash);
            TxRequest::new(
                format!(
                    "write {} bytes at offset {offset} to buffer {buffer_pubkey}",
                    chunk.len()
                ),
                cluster,
                TxKind::Deploy,
                message,
            )
        })
        .collect()
}

fn build_upgrade_tx(
    program_pubkey: &Pubkey,
    buffer_pubkey: &Pubkey,
    payer: &Pubkey,
    cluster: Cluster,
    recent_blockhash: Hash,
) -> TxRequest {
    let instruction =
        loader_instruction::upgrade(program_pubkey, buffer_pubkey, payer, payer, false);
    let message = Message::new_with_blockhash(&[instruction], Some(payer), &recent_blockhash);
    TxRequest::new(
        format!("upgrade program {program_pubkey} from buffer {buffer_pubkey} on {cluster}"),
        cluster,
        TxKind::Upgrade,
        message,
    )
}

fn build_deploy_tx(
    payer: &Pubkey,
    program_keypair: &Keypair,
    buffer_pubkey: &Pubkey,
    max_data_len: usize,
    rent: &Rent,
    cluster: Cluster,
    recent_blockhash: Hash,
) -> Result<TxRequest, ToolError> {
    let program_pubkey = program_keypair.pubkey();
    let program_lamports = rent.minimum_balance(UpgradeableLoaderState::size_of_program());
    let instructions = loader_instruction::deploy_with_max_program_len(
        payer,
        &program_pubkey,
        buffer_pubkey,
        payer,
        program_lamports,
        max_data_len,
        false,
    )
    .map_err(|e| ToolError::InstructionBuild(e.to_string()))?;

    let message = Message::new_with_blockhash(&instructions, Some(payer), &recent_blockhash);
    let program_signature = program_keypair.sign_message(&message.serialize());

    let mut tx = TxRequest::new(
        format!("deploy program {program_pubkey} from buffer {buffer_pubkey} on {cluster}"),
        cluster,
        TxKind::Deploy,
        message,
    );
    tx.extra_signatures
        .push((program_pubkey, program_signature));
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_program(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 256) as u8).collect()
    }

    #[test]
    fn initial_deploy_plan_has_expected_signers_and_chunk_count() {
        let program_bytes = synthetic_program(2_500); // spans 3 chunks at 900 bytes each
        let payer = Pubkey::new_unique();
        let program_keypair = Keypair::new();
        let plan = build_deployment_plan(
            &program_bytes,
            payer,
            &program_keypair,
            program_bytes.len() * 2,
            Cluster::Devnet,
            Hash::default(),
            false,
        )
        .unwrap();

        assert_eq!(plan.write_chunks.len(), 3);
        assert_eq!(plan.program_pubkey, program_keypair.pubkey());

        // create_buffer needs both the payer and the buffer keypair.
        let create_buffer_signers = plan.create_buffer.message.signer_keys();
        assert!(create_buffer_signers.contains(&&payer));
        assert!(create_buffer_signers.contains(&&plan.buffer_pubkey));
        assert_eq!(plan.create_buffer.extra_signatures.len(), 1);
        assert_eq!(plan.create_buffer.extra_signatures[0].0, plan.buffer_pubkey);

        // write chunks only need the payer/authority to sign.
        for chunk in &plan.write_chunks {
            assert_eq!(chunk.message.signer_keys(), vec![&payer]);
        }

        // finalize (first deploy) needs both the payer and the program keypair.
        let finalize_signers = plan.finalize.message.signer_keys();
        assert!(finalize_signers.contains(&&payer));
        assert!(finalize_signers.contains(&&plan.program_pubkey));
        assert_eq!(plan.finalize.extra_signatures.len(), 1);
        assert_eq!(plan.finalize.extra_signatures[0].0, plan.program_pubkey);
    }

    #[test]
    fn upgrade_plan_finalize_only_needs_the_payer() {
        let program_bytes = synthetic_program(500);
        let payer = Pubkey::new_unique();
        let program_keypair = Keypair::new();
        let plan = build_deployment_plan(
            &program_bytes,
            payer,
            &program_keypair,
            program_bytes.len() * 2,
            Cluster::Devnet,
            Hash::default(),
            true,
        )
        .unwrap();

        assert_eq!(plan.finalize.message.signer_keys(), vec![&payer]);
        assert!(plan.finalize.extra_signatures.is_empty());
        assert_eq!(plan.write_chunks.len(), 1);
    }

    #[test]
    fn program_keypair_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_generate_program_keypair(dir.path(), "my_program").unwrap();
        let second = load_or_generate_program_keypair(dir.path(), "my_program").unwrap();
        assert_eq!(first.pubkey(), second.pubkey());
    }
}
