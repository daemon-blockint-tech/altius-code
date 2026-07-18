//! Six v1 heuristic checks from Phase 0 spec §4. Each rule is a plain
//! substring/pattern scan over a whole file's text, not a dataflow
//! analysis — good enough to flag a file worth a human's attention, not a
//! guarantee that flagged code is wrong or that unflagged code is safe.
//! Findings are `Severity::Warning` except `close_without_zeroing`,
//! which is `Severity::Error` per the spec (an account revival bug is a
//! direct fund-loss vector, not a style nit).

use std::path::Path;

use crate::report::{LintFinding, Severity};

fn finding(
    rule_id: &str,
    severity: Severity,
    message: impl Into<String>,
    file: &Path,
) -> LintFinding {
    LintFinding {
        rule_id: rule_id.to_string(),
        severity,
        message: message.into(),
        file: file.to_path_buf(),
    }
}

/// Flags a file that pulls accounts via `next_account_info` but never
/// checks `.is_signer` anywhere in it.
pub(crate) fn missing_signer_check(contents: &str, file: &Path) -> Vec<LintFinding> {
    if contents.contains("next_account_info") && !contents.contains("is_signer") {
        return vec![finding(
            "svm-missing-signer-check",
            Severity::Warning,
            "reads accounts via next_account_info but never checks `.is_signer` anywhere in \
             this file; confirm every account that authorizes an action is verified as a signer",
            file,
        )];
    }
    vec![]
}

/// Flags a file that pulls accounts via `next_account_info` but never
/// checks account ownership (`.owner` or the common `check_id(...)`
/// helper).
pub(crate) fn missing_owner_check(contents: &str, file: &Path) -> Vec<LintFinding> {
    if contents.contains("next_account_info")
        && !contents.contains(".owner")
        && !contents.contains("check_id(")
    {
        return vec![finding(
            "svm-missing-owner-check",
            Severity::Warning,
            "reads accounts via next_account_info but never checks account ownership (`.owner` \
             / `check_id`); an attacker could supply an account owned by a different program",
            file,
        )];
    }
    vec![]
}

/// Flags a file that performs a CPI (`invoke`/`invoke_signed`) without
/// any visible reference to `program_id`, the common way a CPI target is
/// checked against an expected program.
pub(crate) fn arbitrary_cpi(contents: &str, file: &Path) -> Vec<LintFinding> {
    let calls_cpi = contents.contains("invoke(") || contents.contains("invoke_signed(");
    if calls_cpi && !contents.contains("program_id") {
        return vec![finding(
            "svm-arbitrary-cpi",
            Severity::Warning,
            "performs a cross-program invocation (invoke/invoke_signed) with no visible check \
             against an expected `program_id`; confirm the CPI target isn't attacker-supplied",
            file,
        )];
    }
    vec![]
}

/// Flags a file that mutates an account's lamports without ever checking
/// `.is_writable`.
pub(crate) fn unvalidated_writable_account(contents: &str, file: &Path) -> Vec<LintFinding> {
    if contents.contains("lamports.borrow_mut") && !contents.contains("is_writable") {
        return vec![finding(
            "svm-unvalidated-writable-account",
            Severity::Warning,
            "mutates an account's lamports without checking `.is_writable`; confirm the account \
             was declared writable and validated before mutation",
            file,
        )];
    }
    vec![]
}

/// Flags a file that does lamport arithmetic without `checked_*` /
/// `saturating_*` helpers, where a plain `+`/`-` can panic (debug) or
/// silently wrap (release) on overflow/underflow.
pub(crate) fn lamports_overflow_risk(contents: &str, file: &Path) -> Vec<LintFinding> {
    let touches_lamports = contents.contains("lamports()") || contents.contains(".lamports");
    let uses_checked_math = contents.contains("checked_add")
        || contents.contains("checked_sub")
        || contents.contains("saturating_add")
        || contents.contains("saturating_sub");
    if touches_lamports && !uses_checked_math {
        return vec![finding(
            "svm-lamports-overflow-risk",
            Severity::Warning,
            "performs lamport arithmetic without `checked_*`/`saturating_*`; a plain `+`/`-` can \
             panic (debug) or wrap (release) on overflow/underflow",
            file,
        )];
    }
    vec![]
}

/// Flags a file that appears to close an account (drains its lamports to
/// zero) without also zeroing the account's data buffer — the classic
/// "account revival" bug: stale data can be reused if the account is
/// recreated in the same transaction or slot.
pub(crate) fn close_without_zeroing(contents: &str, file: &Path) -> Vec<LintFinding> {
    let drains_lamports = contents.contains("lamports.borrow_mut() = 0")
        || contents.contains("borrow_mut_lamports()? = 0");
    if drains_lamports && !contents.contains("fill(0)") {
        return vec![finding(
            "svm-close-without-zeroing",
            Severity::Error,
            "appears to close an account (drains its lamports to zero) without zeroing its data \
             buffer (`fill(0)`); a revival attack can reuse the stale data if the account is \
             recreated in the same transaction",
            file,
        )];
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn path() -> PathBuf {
        PathBuf::from("src/lib.rs")
    }

    #[test]
    fn missing_signer_check_flags_unchecked_accounts() {
        let src = r#"
            let account_info_iter = &mut accounts.iter();
            let payer = next_account_info(account_info_iter)?;
        "#;
        assert_eq!(missing_signer_check(src, &path()).len(), 1);
    }

    #[test]
    fn missing_signer_check_allows_checked_accounts() {
        let src = r#"
            let payer = next_account_info(account_info_iter)?;
            if !payer.is_signer { return Err(ProgramError::MissingRequiredSignature); }
        "#;
        assert!(missing_signer_check(src, &path()).is_empty());
    }

    #[test]
    fn missing_owner_check_flags_unchecked_ownership() {
        let src = "let vault = next_account_info(account_info_iter)?;";
        assert_eq!(missing_owner_check(src, &path()).len(), 1);
    }

    #[test]
    fn missing_owner_check_allows_owner_field_check() {
        let src = r#"
            let vault = next_account_info(account_info_iter)?;
            if vault.owner != program_id { return Err(ProgramError::IncorrectProgramId); }
        "#;
        assert!(missing_owner_check(src, &path()).is_empty());
    }

    #[test]
    fn arbitrary_cpi_flags_invoke_without_program_id_check() {
        let src = "invoke(&instruction, &[from.clone(), to.clone()])?;";
        assert_eq!(arbitrary_cpi(src, &path()).len(), 1);
    }

    #[test]
    fn arbitrary_cpi_allows_invoke_with_program_id_reference() {
        let src = r#"
            if target_program.key != &expected_program_id { return Err(ProgramError::IncorrectProgramId); }
            invoke_signed(&instruction, &accounts, &[&seeds])?;
            // checked against program_id above
        "#;
        assert!(arbitrary_cpi(src, &path()).is_empty());
    }

    #[test]
    fn unvalidated_writable_account_flags_missing_check() {
        let src = "**vault.lamports.borrow_mut() -= amount;";
        assert_eq!(unvalidated_writable_account(src, &path()).len(), 1);
    }

    #[test]
    fn unvalidated_writable_account_allows_checked_write() {
        let src = r#"
            if !vault.is_writable { return Err(ProgramError::InvalidAccountData); }
            **vault.lamports.borrow_mut() -= amount;
        "#;
        assert!(unvalidated_writable_account(src, &path()).is_empty());
    }

    #[test]
    fn lamports_overflow_risk_flags_plain_arithmetic() {
        let src = "**vault.lamports.borrow_mut() = vault.lamports() - amount;";
        assert_eq!(lamports_overflow_risk(src, &path()).len(), 1);
    }

    #[test]
    fn lamports_overflow_risk_allows_checked_arithmetic() {
        let src = "let new_balance = vault.lamports().checked_sub(amount).ok_or(ProgramError::InsufficientFunds)?;";
        assert!(lamports_overflow_risk(src, &path()).is_empty());
    }

    #[test]
    fn close_without_zeroing_flags_drain_without_fill() {
        let src = "**account.lamports.borrow_mut() = 0;";
        let findings = close_without_zeroing(src, &path());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn close_without_zeroing_allows_drain_with_fill() {
        let src = r#"
            **account.lamports.borrow_mut() = 0;
            account.data.borrow_mut().fill(0);
        "#;
        assert!(close_without_zeroing(src, &path()).is_empty());
    }
}
