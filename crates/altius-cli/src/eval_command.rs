use std::path::PathBuf;

use altius_eval::{score_suite, GoldSuite};

use crate::cli::EvalArgs;
use crate::error::CliError;

pub fn run_eval(args: &EvalArgs) -> Result<(), CliError> {
    let suite = if let Some(path) = &args.suite {
        let text = std::fs::read_to_string(path)
            .map_err(|e| CliError::message(format!("cannot read suite: {e}")))?;
        serde_json::from_str(&text).map_err(|e| CliError::message(format!("invalid suite: {e}")))?
    } else {
        GoldSuite::builtin_smoke()
    };
    let fixtures_root = args
        .fixtures
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    let report = score_suite(&suite, &fixtures_root)
        .map_err(|e| CliError::message(e.to_string()))?;

    if args.markdown {
        println!("{}", report.to_markdown());
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&report)
                .map_err(|e| CliError::message(e.to_string()))?
        );
    }
    Ok(())
}
