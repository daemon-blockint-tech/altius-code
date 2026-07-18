mod rules;

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::ToolError;
use crate::report::LintReport;

const SKIPPED_DIRS: [&str; 4] = ["target", ".git", ".anchor", "node_modules"];
const MAX_WALK_DEPTH: usize = 8;

/// Runs all six v1 SVM lint rules over every `.rs` file under
/// `project_root` (skipping build/vendor directories) and collects the
/// findings.
pub(crate) fn run_all(project_root: &Path) -> Result<LintReport, ToolError> {
    let mut findings = Vec::new();
    for path in collect_rust_files(project_root)? {
        let contents = fs::read_to_string(&path)?;
        findings.extend(rules::missing_signer_check(&contents, &path));
        findings.extend(rules::missing_owner_check(&contents, &path));
        findings.extend(rules::arbitrary_cpi(&contents, &path));
        findings.extend(rules::unvalidated_writable_account(&contents, &path));
        findings.extend(rules::lamports_overflow_risk(&contents, &path));
        findings.extend(rules::close_without_zeroing(&contents, &path));
    }
    Ok(LintReport { findings })
}

fn collect_rust_files(root: &Path) -> Result<Vec<PathBuf>, ToolError> {
    let mut out = Vec::new();
    walk(root, 0, &mut out)?;
    Ok(out)
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> Result<(), ToolError> {
    if depth > MAX_WALK_DEPTH || !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if SKIPPED_DIRS.contains(&name) {
                continue;
            }
            walk(&path, depth + 1, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_issues_across_nested_files_and_skips_target() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("programs/foo/src")).unwrap();
        fs::write(
            dir.path().join("programs/foo/src/lib.rs"),
            "let payer = next_account_info(iter)?;",
        )
        .unwrap();

        fs::create_dir_all(dir.path().join("target/deploy")).unwrap();
        fs::write(
            dir.path().join("target/deploy/decoy.rs"),
            "let payer = next_account_info(iter)?;",
        )
        .unwrap();

        let report = run_all(dir.path()).unwrap();
        // Two rules (signer + owner) should both fire on the one real
        // source file, and the decoy under target/ must be ignored.
        assert_eq!(report.findings.len(), 2);
        assert!(report
            .findings
            .iter()
            .all(|f| !f.file.to_string_lossy().contains("target")));
    }
}
