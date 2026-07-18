use std::path::Path;
use std::str::FromStr;

use altius_detect::{detect_best, DetectionRegistry};
use altius_findings::{to_sarif, ChainFamily};
use altius_scanners::{default_registry, ScannerRegistry};

use crate::cli::{ScanArgs, ScanFormat};
use crate::error::CliError;

pub fn run_scan(args: &ScanArgs) -> Result<(), CliError> {
    let root = args
        .path
        .canonicalize()
        .map_err(|e| CliError::message(format!("cannot resolve path: {e}")))?;
    if !root.is_dir() {
        return Err(CliError::message("scan path is not a directory"));
    }

    let chain = resolve_chain(&args.chain, &root)?;
    let registry: ScannerRegistry = default_registry();
    let report = if chain == ChainFamily::Unknown {
        registry
            .scan_all(&root)
            .map_err(|e| CliError::message(e.to_string()))?
    } else {
        registry
            .scan_chain(&root, chain)
            .map_err(|e| CliError::message(e.to_string()))?
    };

    match args.format {
        ScanFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&report)
                    .map_err(|e| CliError::message(e.to_string()))?
            );
        }
        ScanFormat::Markdown => {
            println!("{}", to_markdown(&report));
        }
        ScanFormat::Sarif => {
            println!(
                "{}",
                serde_json::to_string_pretty(&to_sarif(&report))
                    .map_err(|e| CliError::message(e.to_string()))?
            );
        }
    }

    if args.fail_on_findings && report.has_errors() {
        return Err(CliError::message(
            "scan found High/Critical findings (fail-on-findings)",
        ));
    }
    Ok(())
}

fn resolve_chain(raw: &str, root: &Path) -> Result<ChainFamily, CliError> {
    let lower = raw.to_ascii_lowercase();
    if lower == "auto" {
        let registry = DetectionRegistry::with_defaults();
        return Ok(detect_best(&registry, root)
            .map_err(|e| CliError::message(e.to_string()))?
            .map(|d| d.chain)
            .unwrap_or(ChainFamily::Unknown));
    }
    ChainFamily::from_str(&lower).map_err(|_| CliError::message(format!("unknown chain `{raw}`")))
}

fn to_markdown(report: &altius_findings::ScanReport) -> String {
    let mut out = format!(
        "# Altius scan\n\nTarget: `{}`\nFindings: {}\n\n",
        report.target,
        report.findings.len()
    );
    for finding in &report.findings {
        out.push_str(&format!(
            "## [{}] {}\n\n- pattern: `{}`\n- severity: {}\n- confidence: {}\n- file: `{}`\n\n{}\n\n",
            finding.severity,
            finding.title,
            finding.pattern_id,
            finding.severity,
            finding.confidence,
            finding.location.file,
            finding.description
        ));
    }
    out
}
