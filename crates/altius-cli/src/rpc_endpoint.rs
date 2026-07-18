use altius_svm_detect::Cluster;

/// The well-known public RPC endpoint for `cluster`, used when the user
/// doesn't pass `--rpc-url` explicitly. Real deployments to devnet or
/// mainnet-beta should usually point at a dedicated RPC provider instead
/// of the public endpoint (rate limits are tight), but this is a
/// reasonable default to get started.
pub fn default_rpc_url(cluster: Cluster) -> &'static str {
    match cluster {
        Cluster::Localnet => "http://127.0.0.1:8899",
        Cluster::Devnet => "https://api.devnet.solana.com",
        Cluster::Testnet => "https://api.testnet.solana.com",
        Cluster::MainnetBeta => "https://api.mainnet-beta.solana.com",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn localnet_points_at_the_default_local_validator_port() {
        assert_eq!(default_rpc_url(Cluster::Localnet), "http://127.0.0.1:8899");
    }

    #[test]
    fn every_cluster_has_a_distinct_endpoint() {
        let urls: Vec<&str> = Cluster::ALL.iter().map(|c| default_rpc_url(*c)).collect();
        let mut unique = urls.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(urls.len(), unique.len());
    }
}
