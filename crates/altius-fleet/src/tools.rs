use std::path::{Path, PathBuf};
use std::str::FromStr;

use altius_svm_detect::{detect, Cluster, Framework};
use altius_svm_tools::{AnchorToolchain, CargoBuildSbfToolchain, SvmToolchain};
use rust_langgraph::errors::Error as GraphError;
use rust_langgraph::prebuilt::Tool;
use serde_json::{json, Value};

use crate::agents::Role;

/// Substring that appears in a tool result message whenever the tool
/// reports failure. Every tool here returns a JSON object with an `ok`
/// boolean, and `ToolNode` serializes results with compact
/// `serde_json::to_string`, so a failed tool call always leaves
/// `"ok":false` in the transcript — that is what the pipeline scans for
/// to decide whether a stage failed.
pub const FAILURE_MARKER: &str = "\"ok\":false";

/// The tool plane for one specialist role. This is the complete set of
/// capabilities the fleet has: detect, build, unit-test, lint, and a
/// deploy *plan preview*. Deliberately absent: anything that talks to
/// `altius-signerd`, holds key material, or broadcasts a transaction.
/// Real deployment stays behind `altius deploy` + `TxGuard` + a human.
pub fn tools_for_role(role: Role, project_root: &Path) -> Vec<Tool> {
    match role {
        Role::Explorer => vec![detect_project_tool(project_root)],
        Role::Coder => vec![
            detect_project_tool(project_root),
            build_program_tool(project_root),
            run_unit_tests_tool(project_root),
        ],
        Role::Security => vec![run_security_lints_tool(project_root)],
        Role::Release => vec![plan_deploy_tool(project_root)],
    }
}

async fn blocking_tool<F>(work: F) -> rust_langgraph::errors::Result<Value>
where
    F: FnOnce() -> Value + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| GraphError::ExecutionError(format!("tool task panicked: {e}")))
}

fn toolchain_for(framework: Framework, project_root: &Path) -> Box<dyn SvmToolchain> {
    match framework {
        Framework::Anchor => Box::new(AnchorToolchain::new(project_root)),
        Framework::Pinocchio | Framework::Native => {
            Box::new(CargoBuildSbfToolchain::new(project_root))
        }
    }
}

fn detect_or_error(project_root: &Path) -> Result<altius_svm_detect::SvmProject, Value> {
    match detect(project_root) {
        Ok(Some(project)) => Ok(project),
        Ok(None) => Err(json!({
            "ok": false,
            "error": format!("{} is not an SVM project", project_root.display()),
        })),
        Err(e) => Err(json!({ "ok": false, "error": e.to_string() })),
    }
}

fn detect_project_tool(project_root: &Path) -> Tool {
    let root: PathBuf = project_root.to_path_buf();
    Tool::new(
        "detect_project",
        "Detect the SVM framework (Anchor, Pinocchio, or native Rust) of the project, \
         its programs, default cluster, and installed toolchain.",
        move |_args: Value| {
            let root = root.clone();
            blocking_tool(move || match detect_or_error(&root) {
                Err(e) => e,
                Ok(project) => json!({
                    "ok": true,
                    "framework": format!("{:?}", project.framework),
                    "default_cluster": project.default_cluster.to_string(),
                    "programs": project.programs.iter().map(|p| json!({
                        "name": p.name,
                        "program_id": p.program_id,
                    })).collect::<Vec<_>>(),
                    "toolchain": {
                        "solana_cli": project.toolchain.solana_cli_version,
                        "anchor_cli": project.toolchain.anchor_cli_version,
                        "rustc": project.toolchain.rustc_version,
                        "cargo_build_sbf": project.toolchain.cargo_build_sbf_available,
                    },
                }),
            })
        },
    )
    .with_schema(json!({ "type": "object", "properties": {} }))
}

fn build_program_tool(project_root: &Path) -> Tool {
    let root: PathBuf = project_root.to_path_buf();
    Tool::new(
        "build_program",
        "Build the project's on-chain program(s) with the framework's own build \
         command (anchor build / cargo build-sbf). Returns produced artifacts.",
        move |_args: Value| {
            let root = root.clone();
            blocking_tool(move || {
                let project = match detect_or_error(&root) {
                    Ok(p) => p,
                    Err(e) => return e,
                };
                match toolchain_for(project.framework, &root).build() {
                    Ok(artifacts) => json!({
                        "ok": true,
                        "programs": artifacts.program_paths.iter()
                            .map(|p| p.display().to_string()).collect::<Vec<_>>(),
                    }),
                    Err(e) => json!({ "ok": false, "error": e.to_string() }),
                }
            })
        },
    )
    .with_schema(json!({ "type": "object", "properties": {} }))
}

fn run_unit_tests_tool(project_root: &Path) -> Tool {
    let root: PathBuf = project_root.to_path_buf();
    Tool::new(
        "run_unit_tests",
        "Run the project's fast unit tests (cargo test --lib; LiteSVM/Mollusk style, \
         no validator). Returns per-test results.",
        move |_args: Value| {
            let root = root.clone();
            blocking_tool(move || {
                let project = match detect_or_error(&root) {
                    Ok(p) => p,
                    Err(e) => return e,
                };
                match toolchain_for(project.framework, &root).unit_test() {
                    Ok(report) => json!({
                        "ok": report.all_passed(),
                        "cases": report.cases.iter().map(|c| json!({
                            "name": c.name,
                            "passed": c.passed,
                        })).collect::<Vec<_>>(),
                    }),
                    Err(e) => json!({ "ok": false, "error": e.to_string() }),
                }
            })
        },
    )
    .with_schema(json!({ "type": "object", "properties": {} }))
}

fn run_security_lints_tool(project_root: &Path) -> Tool {
    let root: PathBuf = project_root.to_path_buf();
    Tool::new(
        "run_security_lints",
        "Run Altius's SVM security lints (missing signer/owner checks, arbitrary CPI, \
         unvalidated writable accounts, lamports overflow, close-without-zeroing) plus \
         clippy over the project. Findings are advisory unless severity is error.",
        move |_args: Value| {
            let root = root.clone();
            blocking_tool(move || {
                let project = match detect_or_error(&root) {
                    Ok(p) => p,
                    Err(e) => return e,
                };
                match toolchain_for(project.framework, &root).lint() {
                    Ok(report) => json!({
                        // Warnings don't fail the stage; hard errors do.
                        "ok": !report.has_errors(),
                        "findings": report.findings.iter().map(|f| json!({
                            "rule": f.rule_id,
                            "severity": format!("{:?}", f.severity),
                            "message": f.message,
                            "file": f.file.display().to_string(),
                        })).collect::<Vec<_>>(),
                    }),
                    Err(e) => json!({ "ok": false, "error": e.to_string() }),
                }
            })
        },
    )
    .with_schema(json!({ "type": "object", "properties": {} }))
}

/// Builds a `DeploymentPlan` PREVIEW: a throwaway payer pubkey and a
/// placeholder blockhash, so the resulting transactions could never be
/// submitted even if something tried. The point is to tell the human
/// what `altius deploy` would do — never to do it.
fn plan_deploy_tool(project_root: &Path) -> Tool {
    let root: PathBuf = project_root.to_path_buf();
    Tool::new(
        "plan_deploy",
        "Preview the deployment plan (buffer creation, write chunks, deploy/upgrade) \
         for the already-built program on a cluster. This is a plan only: it cannot \
         sign or broadcast anything; real deployment requires `altius deploy` with \
         human approval.",
        move |args: Value| {
            let root = root.clone();
            blocking_tool(move || {
                let cluster = args
                    .get("cluster")
                    .and_then(|c| c.as_str())
                    .map(Cluster::from_str)
                    .transpose();
                let cluster = match cluster {
                    Ok(c) => c.unwrap_or(Cluster::Devnet),
                    Err(e) => return json!({ "ok": false, "error": e.to_string() }),
                };

                let project = match detect_or_error(&root) {
                    Ok(p) => p,
                    Err(e) => return e,
                };
                let preview_payer = solana_pubkey::Pubkey::new_unique();
                let plan = toolchain_for(project.framework, &root).deploy(
                    cluster,
                    preview_payer,
                    solana_hash::Hash::default(),
                    false,
                );
                match plan {
                    Ok(plan) => json!({
                        "ok": true,
                        "note": "preview only — signing and broadcast require `altius deploy` with human approval",
                        "program": plan.program_pubkey.to_string(),
                        "buffer": plan.buffer_pubkey.to_string(),
                        "steps": std::iter::once(&plan.create_buffer)
                            .chain(plan.write_chunks.iter())
                            .chain(std::iter::once(&plan.finalize))
                            .map(|tx| tx.description.clone())
                            .collect::<Vec<_>>(),
                    }),
                    Err(e) => json!({ "ok": false, "error": e.to_string() }),
                }
            })
        },
    )
    .with_schema(json!({
        "type": "object",
        "properties": {
            "cluster": {
                "type": "string",
                "enum": ["localnet", "devnet", "testnet", "mainnet-beta"],
                "description": "Target cluster for the preview (default devnet)."
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn anchor_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Anchor.toml"),
            "[programs.localnet]\nmy_program = \"Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS\"\n",
        )
        .unwrap();
        dir
    }

    #[tokio::test]
    async fn detect_tool_reports_the_framework() {
        let dir = anchor_fixture();
        let tool = detect_project_tool(dir.path());
        let out = tool.invoke(json!({})).await.unwrap();
        assert_eq!(out["ok"], json!(true));
        assert_eq!(out["framework"], json!("Anchor"));
    }

    #[tokio::test]
    async fn detect_tool_fails_cleanly_outside_an_svm_project() {
        let dir = tempfile::tempdir().unwrap();
        let tool = detect_project_tool(dir.path());
        let out = tool.invoke(json!({})).await.unwrap();
        assert_eq!(out["ok"], json!(false));
        // The serialized form is exactly what FAILURE_MARKER scans for.
        assert!(serde_json::to_string(&out)
            .unwrap()
            .contains(FAILURE_MARKER));
    }

    #[tokio::test]
    async fn plan_deploy_previews_steps_without_any_signing_capability() {
        let dir = anchor_fixture();
        let deploy_dir = dir.path().join("target").join("deploy");
        fs::create_dir_all(&deploy_dir).unwrap();
        fs::write(deploy_dir.join("my_program.so"), vec![0u8; 64]).unwrap();

        let tool = plan_deploy_tool(dir.path());
        let out = tool.invoke(json!({"cluster": "devnet"})).await.unwrap();
        assert_eq!(out["ok"], json!(true));
        assert!(out["steps"].as_array().unwrap().len() >= 3);

        // The preview must carry no signature material of any kind.
        let raw = serde_json::to_string(&out).unwrap();
        assert!(!raw.contains("signature"));
        assert!(out["note"].as_str().unwrap().contains("human approval"));
    }

    /// The fleet-wide invariant: across every role, no tool name so much
    /// as suggests signing or broadcasting. This is the cheap tripwire
    /// that fails loudly if someone later wires a dangerous capability
    /// into the fleet's tool plane.
    #[test]
    fn no_role_has_signing_or_broadcast_tools() {
        let dir = anchor_fixture();
        for role in [Role::Explorer, Role::Coder, Role::Security, Role::Release] {
            for tool in tools_for_role(role, dir.path()) {
                let name = tool.name.to_lowercase();
                for forbidden in ["sign", "broadcast", "send_transaction", "keypair", "wallet"] {
                    assert!(
                        !name.contains(forbidden),
                        "fleet tool {name:?} for {role:?} looks like a {forbidden} capability"
                    );
                }
            }
        }
    }
}
