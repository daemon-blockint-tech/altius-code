use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::GuardError;

/// Hash of an entry that doesn't exist — what the first real entry's
/// `prev_hash` must equal. A sha256 digest is 32 bytes, i.e. 64 hex
/// characters; built with `repeat` rather than a literal so the length
/// can't drift from that.
fn genesis_hash() -> String {
    "0".repeat(64)
}

/// One line of the append-only audit log. Every transaction the pipeline
/// evaluates produces exactly one entry, whether it was ultimately
/// rejected, denied, or approved — see Phase 0 spec §6 stage 5.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub sequence: u64,
    pub timestamp_unix: u64,
    pub prev_hash: String,
    pub tx_description: String,
    pub cluster: String,
    pub tx_kind: String,
    pub policy_decision: String,
    pub simulation_success: Option<bool>,
    pub outcome: String,
    pub final_signature: Option<String>,
}

impl AuditEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tx_description: String,
        cluster: String,
        tx_kind: String,
        policy_decision: String,
        simulation_success: Option<bool>,
        outcome: String,
        final_signature: Option<String>,
    ) -> AuditEntry {
        AuditEntry {
            sequence: 0,
            timestamp_unix: unix_now(),
            prev_hash: String::new(),
            tx_description,
            cluster,
            tx_kind,
            policy_decision,
            simulation_success,
            outcome,
            final_signature,
        }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Appends [`AuditEntry`] records to a JSONL file, hash-chaining each one
/// to the last so the file is tamper-evident: editing or deleting any
/// line breaks [`verify_chain`] for every entry after it.
pub struct AuditLogger {
    path: PathBuf,
    last_hash: String,
    next_sequence: u64,
}

impl AuditLogger {
    /// Opens (creating if necessary) the audit log at `path`, replaying
    /// any existing entries to pick up where the chain left off.
    pub fn open(path: impl Into<PathBuf>) -> Result<AuditLogger, GuardError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let (last_hash, next_sequence) = if path.exists() {
            replay(&path)?
        } else {
            (genesis_hash(), 0)
        };

        Ok(AuditLogger {
            path,
            last_hash,
            next_sequence,
        })
    }

    /// Fills in `sequence` and `prev_hash`, serializes the entry as one
    /// JSON line, appends it, and advances the chain.
    pub fn append(&mut self, mut entry: AuditEntry) -> Result<(), GuardError> {
        entry.sequence = self.next_sequence;
        entry.prev_hash = self.last_hash.clone();

        let line = serde_json::to_string(&entry)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")?;

        self.last_hash = sha256_hex(line.as_bytes());
        self.next_sequence += 1;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn replay(path: &Path) -> Result<(String, u64), GuardError> {
    let contents = fs::read_to_string(path)?;
    let mut last_hash = genesis_hash();
    let mut count = 0u64;
    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        last_hash = sha256_hex(line.as_bytes());
        count += 1;
    }
    Ok((last_hash, count))
}

/// Recomputes the hash chain over the entries in `path` and confirms
/// every entry's `prev_hash` matches the hash of the entry before it,
/// starting from the all-zero genesis hash. Returns `Err` naming the first entry
/// where the chain breaks; a bare deserialization failure (a line isn't
/// valid `AuditEntry` JSON at all) is reported the same way, since a
/// tampered or truncated line is exactly what this function exists to
/// catch.
pub fn verify_chain(path: impl AsRef<Path>) -> Result<(), GuardError> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)?;
    let mut expected_prev = genesis_hash();

    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: AuditEntry =
            serde_json::from_str(line).map_err(|e| GuardError::AuditChainBroken {
                path: path.display().to_string(),
                reason: format!("entry {index} is not valid JSON: {e}"),
            })?;
        if entry.prev_hash != expected_prev {
            return Err(GuardError::AuditChainBroken {
                path: path.display().to_string(),
                reason: format!(
                    "entry {index} (sequence {}) has prev_hash {} but expected {}",
                    entry.sequence, entry.prev_hash, expected_prev
                ),
            });
        }
        expected_prev = sha256_hex(line.as_bytes());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(desc: &str) -> AuditEntry {
        AuditEntry::new(
            desc.to_string(),
            "devnet".to_string(),
            "Deploy".to_string(),
            "Continue".to_string(),
            Some(true),
            "approved".to_string(),
            None,
        )
    }

    #[test]
    fn appends_and_verifies_a_clean_chain() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("txlog").join("audit.jsonl");

        let mut logger = AuditLogger::open(&log_path).unwrap();
        logger.append(sample_entry("first")).unwrap();
        logger.append(sample_entry("second")).unwrap();
        logger.append(sample_entry("third")).unwrap();

        verify_chain(&log_path).unwrap();

        let contents = fs::read_to_string(&log_path).unwrap();
        assert_eq!(contents.lines().count(), 3);
    }

    #[test]
    fn reopening_continues_the_same_chain() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.jsonl");

        let mut logger = AuditLogger::open(&log_path).unwrap();
        logger.append(sample_entry("first")).unwrap();
        drop(logger);

        let mut reopened = AuditLogger::open(&log_path).unwrap();
        reopened.append(sample_entry("second")).unwrap();

        verify_chain(&log_path).unwrap();
        let contents = fs::read_to_string(&log_path).unwrap();
        assert_eq!(contents.lines().count(), 2);
    }

    #[test]
    fn detects_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.jsonl");

        let mut logger = AuditLogger::open(&log_path).unwrap();
        logger.append(sample_entry("first")).unwrap();
        logger.append(sample_entry("second")).unwrap();

        // Tamper with the first line's description in place.
        let mut contents = fs::read_to_string(&log_path).unwrap();
        contents = contents.replace("\"first\"", "\"tampered\"");
        fs::write(&log_path, contents).unwrap();

        let err = verify_chain(&log_path).unwrap_err();
        assert!(matches!(err, GuardError::AuditChainBroken { .. }));
    }
}
