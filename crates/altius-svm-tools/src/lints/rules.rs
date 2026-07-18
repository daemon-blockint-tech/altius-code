//! Native SVM heuristic rules.
//!
//! v1 rules keep stable IDs from Phase 0. Additional rules internalize public
//! Solana security knowledge (Neodyme workshop/pitfalls, solsec pattern themes)
//! as Altius-owned heuristics — no third-party source is vendored.
//!
//! Provenance notes (methodology only):
//! - https://neodyme.io/en/blog/solana_common_pitfalls/
//! - https://workshop.neodyme.io/index.html
//! - https://github.com/sannykim/solsec (pattern index themes)

use std::path::Path;

use crate::report::{LintFinding, Severity};

use super::span::{first_line_of, first_snippet};

fn finding(
    rule_id: &str,
    severity: Severity,
    message: impl Into<String>,
    file: &Path,
    contents: &str,
    needle: &str,
    recommendation: impl Into<String>,
) -> LintFinding {
    let start_line = first_line_of(contents, needle);
    LintFinding {
        rule_id: rule_id.to_string(),
        severity,
        message: message.into(),
        file: file.to_path_buf(),
        start_line,
        end_line: start_line,
        snippet: first_snippet(contents, needle),
        recommendation: Some(recommendation.into()),
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
            contents,
            "next_account_info",
            "Require `account.is_signer` (or Anchor `Signer<>` / Vipers-style assert_signer) \
             before trusting authority.",
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
            contents,
            "next_account_info",
            "Compare `account.owner` to the expected program id (or use typed Anchor constraints).",
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
        let needle = if contents.contains("invoke_signed(") {
            "invoke_signed("
        } else {
            "invoke("
        };
        return vec![finding(
            "svm-arbitrary-cpi",
            Severity::Warning,
            "performs a cross-program invocation (invoke/invoke_signed) with no visible check \
             against an expected `program_id`; confirm the CPI target isn't attacker-supplied",
            file,
            contents,
            needle,
            "Validate the CPI program account key against a constant or allowlist before invoke.",
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
            contents,
            "lamports.borrow_mut",
            "Assert `account.is_writable` (or Anchor `mut`) before mutating lamports/data.",
        )];
    }
    vec![]
}

/// Flags a file that does lamport arithmetic without `checked_*` /
/// `saturating_*` helpers.
pub(crate) fn lamports_overflow_risk(contents: &str, file: &Path) -> Vec<LintFinding> {
    let touches_lamports = contents.contains("lamports()") || contents.contains(".lamports");
    let uses_checked_math = contents.contains("checked_add")
        || contents.contains("checked_sub")
        || contents.contains("saturating_add")
        || contents.contains("saturating_sub");
    if touches_lamports && !uses_checked_math {
        let needle = if contents.contains("lamports()") {
            "lamports()"
        } else {
            ".lamports"
        };
        return vec![finding(
            "svm-lamports-overflow-risk",
            Severity::Warning,
            "performs lamport arithmetic without `checked_*`/`saturating_*`; a plain `+`/`-` can \
             panic (debug) or wrap (release) on overflow/underflow",
            file,
            contents,
            needle,
            "Use checked/saturating arithmetic (or a checked-math helper) for lamport math.",
        )];
    }
    vec![]
}

/// Flags a file that appears to close an account without zeroing data.
pub(crate) fn close_without_zeroing(contents: &str, file: &Path) -> Vec<LintFinding> {
    let drains_lamports = contents.contains("lamports.borrow_mut() = 0")
        || contents.contains("borrow_mut_lamports()? = 0");
    if drains_lamports && !contents.contains("fill(0)") {
        let needle = if contents.contains("borrow_mut_lamports()? = 0") {
            "borrow_mut_lamports()? = 0"
        } else {
            "lamports.borrow_mut() = 0"
        };
        return vec![finding(
            "svm-close-without-zeroing",
            Severity::Error,
            "appears to close an account (drains its lamports to zero) without zeroing its data \
             buffer (`fill(0)`); a revival attack can reuse the stale data if the account is \
             recreated in the same transaction",
            file,
            contents,
            needle,
            "Zero account data before/while closing, or use a close helper that clears data.",
        )];
    }
    vec![]
}

/// Canonical bump: `find_program_address` used without capturing/storing the bump.
///
/// Theme: Neodyme PDA / bump seed canonicalization pitfalls.
pub(crate) fn pda_bump_canonicalization(contents: &str, file: &Path) -> Vec<LintFinding> {
    let finds_pda = contents.contains("find_program_address")
        || contents.contains("Pubkey::create_program_address");
    let mentions_bump = contents.contains("bump") || contents.contains("Bump");
    if finds_pda && !mentions_bump {
        return vec![finding(
            "svm-pda-bump-canonicalization",
            Severity::Warning,
            "derives a PDA via find/create_program_address without any visible bump handling; \
             non-canonical bumps can bypass PDA authority checks",
            file,
            contents,
            if contents.contains("find_program_address") {
                "find_program_address"
            } else {
                "create_program_address"
            },
            "Always use the canonical bump from `find_program_address` and persist/verify it.",
        )];
    }
    vec![]
}

/// Sysvar / instruction introspection without address validation.
pub(crate) fn sysvar_address_validation(contents: &str, file: &Path) -> Vec<LintFinding> {
    let uses_sysvar = contents.contains("Sysvar")
        || contents.contains("instructions::load_instruction_at")
        || contents.contains("load_current_index");
    let checks_key = contents.contains("sysvar::")
        || contents.contains("ID")
        || contents.contains("check_id")
        || contents.contains("key ==");
    if uses_sysvar && contents.contains("next_account_info") && !checks_key {
        return vec![finding(
            "svm-sysvar-address-validation",
            Severity::Warning,
            "uses sysvar/instruction introspection APIs while iterating accounts, but no visible \
             sysvar address/`check_id` validation was found",
            file,
            contents,
            "next_account_info",
            "Validate sysvar account keys (e.g. clock/instructions sysvar id) before trusting them.",
        )];
    }
    vec![]
}

/// Account confusion: multiple `AccountInfo` bindings without key equality checks.
pub(crate) fn account_confusion(contents: &str, file: &Path) -> Vec<LintFinding> {
    let next_count = contents.matches("next_account_info").count();
    let key_checks = contents.contains(".key")
        && (contents.contains("==") || contents.contains("eq(") || contents.contains("key()"));
    if next_count >= 3 && !key_checks && !contents.contains("Account<'info") {
        return vec![finding(
            "svm-account-confusion",
            Severity::Warning,
            "reads several accounts via next_account_info without visible key-equality checks; \
             attackers may swap similarly shaped accounts",
            file,
            contents,
            "next_account_info",
            "Compare account keys to expected PDAs/mints/authorities before use.",
        )];
    }
    vec![]
}

/// Integer / rounding risk beyond lamports (mul/div without checked helpers).
pub(crate) fn unchecked_arithmetic(contents: &str, file: &Path) -> Vec<LintFinding> {
    let has_mul_div = contents.contains(" * ")
        || contents.contains(" / ")
        || contents.contains("*= ")
        || contents.contains("/= ");
    let has_checked = contents.contains("checked_mul")
        || contents.contains("checked_div")
        || contents.contains("saturating_mul")
        || contents.contains("checked_add");
    let looks_financial = contents.contains("amount")
        || contents.contains("shares")
        || contents.contains("price")
        || contents.contains("liquidity");
    if has_mul_div && looks_financial && !has_checked {
        return vec![finding(
            "svm-unchecked-arithmetic",
            Severity::Warning,
            "performs financial mul/div arithmetic without checked helpers; overflow or \
             truncation can mint/steal value",
            file,
            contents,
            if contents.contains(" * ") {
                " * "
            } else {
                " / "
            },
            "Use checked arithmetic (or a checked-math crate pattern) for amounts/prices/shares.",
        )];
    }
    vec![]
}

/// Missing remaining-accounts / unchecked account constraint themes (Anchor).
pub(crate) fn remaining_accounts_risk(contents: &str, file: &Path) -> Vec<LintFinding> {
    if contents.contains("remaining_accounts")
        && !contents.contains("owner")
        && !contents.contains("is_signer")
    {
        return vec![finding(
            "svm-remaining-accounts-risk",
            Severity::Warning,
            "uses `remaining_accounts` without visible signer/owner validation in this file",
            file,
            contents,
            "remaining_accounts",
            "Validate every remaining account (owner, signer, PDA seeds) before trusting it.",
        )];
    }
    vec![]
}

/// Oracle / price feed trust without staleness or confidence checks.
pub(crate) fn oracle_trust_risk(contents: &str, file: &Path) -> Vec<LintFinding> {
    let mentions_oracle = contents.contains("oracle")
        || contents.contains("Pyth")
        || contents.contains("pyth")
        || contents.contains("price_feed")
        || contents.contains("Aggregator");
    let has_staleness = contents.contains("publish_time")
        || contents.contains("slot")
        || contents.contains("stale")
        || contents.contains("confidence")
        || contents.contains("max_age");
    if mentions_oracle && !has_staleness {
        return vec![finding(
            "svm-oracle-trust-risk",
            Severity::Warning,
            "references an oracle/price feed without visible staleness or confidence checks",
            file,
            contents,
            if contents.contains("oracle") {
                "oracle"
            } else if contents.contains("Pyth") {
                "Pyth"
            } else {
                "price"
            },
            "Enforce max price age, confidence intervals, and feed ownership before acting.",
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
        let findings = missing_signer_check(src, &path());
        assert_eq!(findings.len(), 1);
        assert!(findings[0].start_line.is_some());
        assert!(findings[0].snippet.is_some());
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

    #[test]
    fn pda_bump_flags_missing_bump() {
        let src = "let (pda, _) = Pubkey::find_program_address(&[b\"vault\"], program_id);";
        // mentions bump via `_` discard — still no "bump" token
        assert_eq!(pda_bump_canonicalization(src, &path()).len(), 1);
    }

    #[test]
    fn pda_bump_allows_named_bump() {
        let src = "let (pda, bump) = Pubkey::find_program_address(&[b\"vault\"], program_id);";
        assert!(pda_bump_canonicalization(src, &path()).is_empty());
    }

    #[test]
    fn remaining_accounts_flags_unvalidated() {
        let src = "for acc in ctx.remaining_accounts { /* use */ }";
        assert_eq!(remaining_accounts_risk(src, &path()).len(), 1);
    }

    #[test]
    fn oracle_flags_missing_staleness() {
        let src = "let px = load_pyth_price(oracle_account)?;";
        assert_eq!(oracle_trust_risk(src, &path()).len(), 1);
    }
}
