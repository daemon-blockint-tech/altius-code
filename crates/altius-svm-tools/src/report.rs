use std::path::PathBuf;

/// Output of a successful [`crate::SvmToolchain::build`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildArtifacts {
    /// Compiled `.so` program binaries, one per program in the workspace.
    pub program_paths: Vec<PathBuf>,
    /// Anchor's generated IDL, when the toolchain produces one.
    pub idl_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCaseResult {
    pub name: String,
    pub passed: bool,
    pub compute_units_consumed: Option<u64>,
}

/// Structured result of a test run, so the agent reads test outcomes as
/// data rather than parsing raw terminal output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestReport {
    pub cases: Vec<TestCaseResult>,
    pub logs: Vec<String>,
    pub raw_stdout: String,
    pub raw_stderr: String,
}

impl TestReport {
    pub fn all_passed(&self) -> bool {
        self.cases.iter().all(|c| c.passed)
    }
}

/// Parses the `test <name> ... ok` / `test <name> ... FAILED` lines that
/// `cargo test`'s default (human) output produces. This is line-based
/// text scanning, not a structured format (cargo's JSON test output is
/// still unstable), so it only captures pass/fail per test name —
/// per-test compute unit counts would have to come from parsing program
/// logs separately and are left `None` here.
pub fn parse_cargo_test_output(stdout: &str) -> Vec<TestCaseResult> {
    let mut cases = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("test ") else {
            continue;
        };
        let Some((name, status)) = rest.rsplit_once(" ... ") else {
            continue;
        };
        let status = status.trim();
        if status == "ok" || status.starts_with("FAILED") {
            cases.push(TestCaseResult {
                name: name.to_string(),
                passed: status == "ok",
                compute_units_consumed: None,
            });
        }
    }
    cases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_passing_and_failing_cargo_test_lines() {
        let stdout = "\
running 3 tests
test tests::adds_two ... ok
test tests::rejects_overflow ... FAILED
test tests::ignored_case ... ignored

failures:

test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out
";
        let cases = parse_cargo_test_output(stdout);
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0].name, "tests::adds_two");
        assert!(cases[0].passed);
        assert_eq!(cases[1].name, "tests::rejects_overflow");
        assert!(!cases[1].passed);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintFinding {
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    pub file: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LintReport {
    pub findings: Vec<LintFinding>,
}

impl LintReport {
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Error)
    }
}
