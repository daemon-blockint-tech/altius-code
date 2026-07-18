use std::collections::HashMap;

use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use solana_rpc_client_api::config::{
    RpcSimulateTransactionAccountsConfig, RpcSimulateTransactionConfig,
};
use solana_rpc_client_api::response::UiAccountEncoding;

use crate::error::GuardError;
use crate::simulate::{AccountDelta, SimulationOutcome, Simulator};
use crate::tx_assembly::assemble_for_simulation;
use crate::tx_request::TxRequest;

/// A per-instruction compute budget Solana assumes when a transaction
/// doesn't request one explicitly via the compute budget program.
const DEFAULT_COMPUTE_UNITS_PER_INSTRUCTION: u64 = 200_000;

/// Real, RPC-backed [`Simulator`]: calls `simulateTransaction` with
/// `sigVerify: false` and `replaceRecentBlockhash: true`, exactly as
/// Phase 0 spec §6 stage 2 describes for localnet/devnet, and for the
/// local-fork half of the mainnet dual simulation (point this at a
/// forked `solana-test-validator`'s RPC URL — see
/// [`crate::test_validator::TestValidator`]).
pub struct RpcSimulator {
    client: RpcClient,
}

impl RpcSimulator {
    pub fn new(rpc_url: impl ToString) -> RpcSimulator {
        RpcSimulator {
            client: RpcClient::new(rpc_url),
        }
    }

    /// Wraps an already-constructed client — this is how tests inject
    /// `RpcClient::new_mock_with_mocks(...)` to exercise this type's
    /// response-mapping logic without any real network I/O.
    pub fn from_client(client: RpcClient) -> RpcSimulator {
        RpcSimulator { client }
    }

    fn rpc_error(&self, reason: impl std::fmt::Display) -> GuardError {
        GuardError::Rpc {
            rpc_url: self.client.url(),
            reason: reason.to_string(),
        }
    }
}

impl Simulator for RpcSimulator {
    fn simulate(&self, tx: &TxRequest) -> Result<SimulationOutcome, GuardError> {
        let transaction = assemble_for_simulation(tx);
        let addresses: Vec<Pubkey> = tx.message.account_keys.clone();

        // Pre-fetch account state so we can report ownership changes,
        // account creation, and account closure — `simulateTransaction`
        // itself only ever gives us post-simulation state.
        let pre_accounts = self
            .client
            .get_multiple_accounts(&addresses)
            .map_err(|e| self.rpc_error(e))?;

        let config = RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: true,
            accounts: Some(RpcSimulateTransactionAccountsConfig {
                addresses: addresses.iter().map(|p| p.to_string()).collect(),
                encoding: Some(UiAccountEncoding::Base64),
            }),
            ..Default::default()
        };
        let response = self
            .client
            .simulate_transaction_with_config(&transaction, config)
            .map_err(|e| self.rpc_error(e))?;
        let result = response.value;

        let compute_unit_limit =
            DEFAULT_COMPUTE_UNITS_PER_INSTRUCTION * tx.message.instructions.len().max(1) as u64;

        let post_accounts: HashMap<String, _> = result
            .accounts
            .clone()
            .unwrap_or_default()
            .into_iter()
            .zip(addresses.iter())
            .filter_map(|(account, pubkey)| account.map(|a| (pubkey.to_string(), a)))
            .collect();

        let mut account_deltas = Vec::new();
        for (index, pubkey) in addresses.iter().enumerate() {
            let pre = pre_accounts.get(index).and_then(|a| a.as_ref());
            let post = post_accounts.get(&pubkey.to_string());

            let lamports_before = pre.map(|a| a.lamports).unwrap_or(0);
            let lamports_after = result
                .post_balances
                .as_ref()
                .and_then(|balances| balances.get(index))
                .copied()
                .unwrap_or_else(|| post.map(|a| a.lamports).unwrap_or(lamports_before));
            let owner_before = pre.map(|a| a.owner.to_string()).unwrap_or_default();
            let owner_after = post
                .map(|a| a.owner.clone())
                .unwrap_or_else(|| owner_before.clone());

            let existed_before = pre.is_some();
            let exists_after = post.is_some() && lamports_after > 0;

            if lamports_before == lamports_after
                && owner_before == owner_after
                && existed_before == exists_after
            {
                continue;
            }

            account_deltas.push(AccountDelta {
                pubkey: pubkey.to_string(),
                lamports_before,
                lamports_after,
                owner_before,
                owner_after,
                created: !existed_before && exists_after,
                closed: existed_before && !exists_after,
            });
        }

        Ok(SimulationOutcome {
            success: result.err.is_none(),
            logs: result.logs.unwrap_or_default(),
            compute_units_consumed: result.units_consumed.unwrap_or(0),
            compute_unit_limit,
            account_deltas,
            error: result.err.map(|e| format!("{e:?}")),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx_request::TxKind;
    use altius_svm_detect::Cluster;
    use serde_json::json;
    use solana_instruction::{AccountMeta, Instruction};
    use solana_message::Message;
    use solana_rpc_client_api::request::RpcRequest;
    use std::collections::HashMap as StdHashMap;

    fn sample_tx() -> TxRequest {
        let payer = Pubkey::new_unique();
        let target = Pubkey::new_unique();
        let program_id = Pubkey::new_unique();
        let instruction = Instruction::new_with_bytes(
            program_id,
            &[],
            vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(target, false),
            ],
        );
        let message = Message::new(&[instruction], Some(&payer));
        TxRequest::new(
            "test invoke",
            Cluster::Devnet,
            TxKind::Invoke {
                instruction_name: "ping".into(),
            },
            message,
        )
    }

    #[test]
    fn maps_a_successful_simulation_response() {
        let tx = sample_tx();
        let mut mocks: HashMap<RpcRequest, serde_json::Value> = StdHashMap::new();
        mocks.insert(
            RpcRequest::GetMultipleAccounts,
            json!({"context": {"slot": 1}, "value": [null, null]}),
        );
        mocks.insert(
            RpcRequest::SimulateTransaction,
            json!({
                "context": {"slot": 1},
                "value": {
                    "err": null,
                    "logs": ["Program log: ok"],
                    "accounts": null,
                    "unitsConsumed": 1234,
                    "loadedAccountsDataSize": null,
                    "returnData": null,
                    "innerInstructions": null,
                    "replacementBlockhash": null,
                    "fee": null,
                    "preBalances": null,
                    "postBalances": null,
                    "preTokenBalances": null,
                    "postTokenBalances": null,
                    "loadedAddresses": null
                }
            }),
        );
        let client = RpcClient::new_mock_with_mocks("succeeds".to_string(), mocks);
        let simulator = RpcSimulator::from_client(client);

        let outcome = simulator.simulate(&tx).unwrap();
        assert!(outcome.success);
        assert_eq!(outcome.compute_units_consumed, 1234);
        assert_eq!(outcome.logs, vec!["Program log: ok".to_string()]);
    }

    #[test]
    fn maps_a_failed_simulation_response() {
        let tx = sample_tx();
        let mut mocks: HashMap<RpcRequest, serde_json::Value> = StdHashMap::new();
        mocks.insert(
            RpcRequest::GetMultipleAccounts,
            json!({"context": {"slot": 1}, "value": [null, null]}),
        );
        mocks.insert(
            RpcRequest::SimulateTransaction,
            json!({
                "context": {"slot": 1},
                "value": {
                    "err": "InsufficientFundsForFee",
                    "logs": ["Program log: not enough funds"],
                    "accounts": null,
                    "unitsConsumed": 0,
                    "loadedAccountsDataSize": null,
                    "returnData": null,
                    "innerInstructions": null,
                    "replacementBlockhash": null,
                    "fee": null,
                    "preBalances": null,
                    "postBalances": null,
                    "preTokenBalances": null,
                    "postTokenBalances": null,
                    "loadedAddresses": null
                }
            }),
        );
        let client = RpcClient::new_mock_with_mocks("succeeds".to_string(), mocks);
        let simulator = RpcSimulator::from_client(client);

        let outcome = simulator.simulate(&tx).unwrap();
        assert!(!outcome.success);
        assert!(outcome.error.is_some());
    }
}
