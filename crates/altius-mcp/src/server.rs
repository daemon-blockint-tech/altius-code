use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use altius_core::redact_secrets;
use altius_svm_detect::{detect, Framework, SvmProject};
use altius_svm_tools::{AnchorToolchain, CargoBuildSbfToolchain, SvmToolchain};
use axum::Router;
use rmcp::{
    handler::server::wrapper::Parameters,
    model::CallToolResult,
    schemars, tool, tool_router,
    transport::{
        stdio,
        streamable_http_server::{
            session::local::LocalSessionManager, tower::StreamableHttpService,
        },
        StreamableHttpServerConfig,
    },
    ServiceExt,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_PATH_LEN: usize = 4096;
const MAX_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Error)]
pub enum McpServerError {
    #[error("workspace root is invalid: {0}")]
    InvalidRoot(String),
    #[error("MCP transport failed: {0}")]
    Transport(String),
    #[error("MCP HTTP server failed: {0}")]
    Http(String),
}

#[derive(Debug, Clone)]
pub struct AltiusMcpServer {
    workspace_root: PathBuf,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ProjectRequest {
    /// Project path relative to the configured workspace root.
    #[serde(default = "default_project")]
    pub project: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TestRequest {
    /// Project path relative to the configured workspace root.
    #[serde(default = "default_project")]
    pub project: String,
    /// Run integration tests instead of unit tests.
    #[serde(default)]
    pub integration: bool,
}

#[derive(Debug, Serialize)]
struct ToolEnvelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct Detection {
    framework: String,
    default_cluster: String,
    toolchain: String,
    programs: Vec<DetectedProgram>,
}

#[derive(Debug, Serialize)]
struct DetectedProgram {
    name: String,
    path: String,
    program_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct BuildOutput {
    program_paths: Vec<String>,
    idl_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct TestOutput {
    all_passed: bool,
    cases: Vec<TestCase>,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Serialize)]
struct TestCase {
    name: String,
    passed: bool,
    compute_units_consumed: Option<u64>,
}

#[derive(Debug, Serialize)]
struct LintOutput {
    has_errors: bool,
    findings: Vec<LintFindingDto>,
}

/// Wire DTO for lint/scan findings (compatible with prior MCP shape).
#[derive(Debug, Serialize)]
struct LintFindingDto {
    rule_id: String,
    severity: String,
    message: String,
    file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chain: Option<String>,
}

impl AltiusMcpServer {
    pub fn new(workspace_root: impl AsRef<Path>) -> Result<Self, McpServerError> {
        let workspace_root = workspace_root
            .as_ref()
            .canonicalize()
            .map_err(|error| McpServerError::InvalidRoot(error.to_string()))?;
        if !workspace_root.is_dir() {
            return Err(McpServerError::InvalidRoot(format!(
                "{} is not a directory",
                workspace_root.display()
            )));
        }
        Ok(Self { workspace_root })
    }

    fn resolve_project(&self, requested: &str) -> Result<PathBuf, String> {
        if requested.len() > MAX_PATH_LEN {
            return Err("project path is too long".into());
        }
        let requested = Path::new(requested);
        if requested.is_absolute() {
            return Err("project path must be relative to the workspace root".into());
        }
        let resolved = self
            .workspace_root
            .join(requested)
            .canonicalize()
            .map_err(|error| format!("cannot resolve project path: {error}"))?;
        if !resolved.starts_with(&self.workspace_root) {
            return Err("project path escapes the workspace root".into());
        }
        if !resolved.is_dir() {
            return Err("project path is not a directory".into());
        }
        Ok(resolved)
    }
}

#[tool_router(server_handler)]
impl AltiusMcpServer {
    #[tool(description = "Detect an SVM project's framework, programs, toolchain, and cluster")]
    async fn detect_project(
        &self,
        Parameters(request): Parameters<ProjectRequest>,
    ) -> CallToolResult {
        let root = self.workspace_root.clone();
        let result = self
            .resolve_project(&request.project)
            .and_then(|project| detect_required(&project))
            .map(|project| detection_output(project, &root));
        tool_result(result)
    }

    #[tool(description = "Build an SVM project without deploying or submitting transactions")]
    async fn build_project(
        &self,
        Parameters(request): Parameters<ProjectRequest>,
    ) -> CallToolResult {
        let root = self.workspace_root.clone();
        let project = match self.resolve_project(&request.project) {
            Ok(project) => project,
            Err(error) => return tool_result::<BuildOutput>(Err(error)),
        };
        let result = tokio::task::spawn_blocking(move || {
            let detected = detect_required(&project)?;
            let artifacts = toolchain_for(&detected, &project)
                .build()
                .map_err(|error| error.to_string())?;
            Ok(BuildOutput {
                program_paths: artifacts
                    .program_paths
                    .iter()
                    .map(|path| display_path(path, &root))
                    .collect(),
                idl_path: artifacts.idl_path.map(|path| display_path(&path, &root)),
            })
        })
        .await
        .map_err(|error| format!("build worker failed: {error}"))
        .and_then(|result| result);
        tool_result(result)
    }

    #[tool(description = "Run unit or integration tests for an SVM project")]
    async fn test_project(&self, Parameters(request): Parameters<TestRequest>) -> CallToolResult {
        let project = match self.resolve_project(&request.project) {
            Ok(project) => project,
            Err(error) => return tool_result::<TestOutput>(Err(error)),
        };
        let integration = request.integration;
        let result = tokio::task::spawn_blocking(move || {
            let detected = detect_required(&project)?;
            let toolchain = toolchain_for(&detected, &project);
            let report = if integration {
                toolchain.integration_test()
            } else {
                toolchain.unit_test()
            }
            .map_err(|error| error.to_string())?;
            Ok(TestOutput {
                all_passed: report.all_passed(),
                cases: report
                    .cases
                    .into_iter()
                    .map(|case| TestCase {
                        name: case.name,
                        passed: case.passed,
                        compute_units_consumed: case.compute_units_consumed,
                    })
                    .collect(),
                stdout: bounded_redacted(&report.raw_stdout),
                stderr: bounded_redacted(&report.raw_stderr),
            })
        })
        .await
        .map_err(|error| format!("test worker failed: {error}"))
        .and_then(|result| result);
        tool_result(result)
    }

    #[tool(description = "Run Altius security lints against an SVM project")]
    async fn lint_project(
        &self,
        Parameters(request): Parameters<ProjectRequest>,
    ) -> CallToolResult {
        let root = self.workspace_root.clone();
        let project = match self.resolve_project(&request.project) {
            Ok(project) => project,
            Err(error) => return tool_result::<LintOutput>(Err(error)),
        };
        let result = tokio::task::spawn_blocking(move || {
            let detected = detect_required(&project)?;
            let report = toolchain_for(&detected, &project)
                .lint()
                .map_err(|error| error.to_string())?;
            let scan = report.to_scan_report(display_path(&project, &root));
            Ok(LintOutput {
                has_errors: scan.has_errors(),
                findings: scan
                    .findings
                    .into_iter()
                    .map(|finding| {
                        let legacy = match finding.severity {
                            altius_findings::Severity::High
                            | altius_findings::Severity::Critical => "error",
                            _ => "warning",
                        };
                        LintFindingDto {
                            rule_id: finding.pattern_id,
                            severity: legacy.into(),
                            message: bounded_redacted(&finding.description),
                            file: finding.location.file,
                            id: Some(finding.id),
                            fingerprint: Some(finding.fingerprint),
                            confidence: Some(finding.confidence.as_str().into()),
                            chain: Some(finding.chain.as_str().into()),
                        }
                    })
                    .collect(),
            })
        })
        .await
        .map_err(|error| format!("lint worker failed: {error}"))
        .and_then(|result| result);
        tool_result(result)
    }
}

pub async fn serve_stdio(workspace_root: impl AsRef<Path>) -> Result<(), McpServerError> {
    let server = AltiusMcpServer::new(workspace_root)?;
    let service = server
        .serve(stdio())
        .await
        .map_err(|error| McpServerError::Transport(error.to_string()))?;
    service
        .waiting()
        .await
        .map_err(|error| McpServerError::Transport(error.to_string()))?;
    Ok(())
}

pub async fn serve_http(
    workspace_root: impl AsRef<Path>,
    bind: SocketAddr,
) -> Result<(), McpServerError> {
    let server = AltiusMcpServer::new(workspace_root)?;
    let service: StreamableHttpService<AltiusMcpServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(server.clone()),
            LocalSessionManager::default().into(),
            StreamableHttpServerConfig::default(),
        );
    let app = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|error| McpServerError::Http(error.to_string()))?;
    axum::serve(listener, app)
        .await
        .map_err(|error| McpServerError::Http(error.to_string()))
}

fn toolchain_for(project: &SvmProject, root: &Path) -> Box<dyn SvmToolchain> {
    match project.framework {
        Framework::Anchor => Box::new(AnchorToolchain::new(root)),
        Framework::Pinocchio | Framework::Native => Box::new(CargoBuildSbfToolchain::new(root)),
    }
}

fn detect_required(project: &Path) -> Result<SvmProject, String> {
    detect(project)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no supported SVM project found at the requested path".into())
}

fn detection_output(project: SvmProject, root: &Path) -> Detection {
    Detection {
        framework: project.framework.to_string(),
        default_cluster: project.default_cluster.to_string(),
        toolchain: format!("{:?}", project.toolchain),
        programs: project
            .programs
            .into_iter()
            .map(|program| DetectedProgram {
                name: program.name,
                path: display_path(&program.path, root),
                program_id: program.program_id,
            })
            .collect(),
    }
}

fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn bounded_redacted(value: &str) -> String {
    let redacted = redact_secrets(value);
    if redacted.len() <= MAX_TEXT_BYTES {
        return redacted;
    }
    let mut boundary = MAX_TEXT_BYTES;
    while !redacted.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…[truncated]", &redacted[..boundary])
}

fn tool_result<T: Serialize>(result: Result<T, String>) -> CallToolResult {
    let envelope = match result {
        Ok(data) => ToolEnvelope {
            ok: true,
            data: Some(data),
            error: None,
        },
        Err(error) => ToolEnvelope {
            ok: false,
            data: None,
            error: Some(bounded_redacted(&error)),
        },
    };
    let value = serde_json::to_value(&envelope).unwrap_or_else(
        |error| serde_json::json!({"ok": false, "error": format!("serialization failed: {error}")}),
    );
    if envelope.ok {
        CallToolResult::structured(value)
    } else {
        CallToolResult::structured_error(value)
    }
}

fn default_project() -> String {
    ".".into()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn project_paths_cannot_escape_workspace() {
        let workspace = tempfile::tempdir().unwrap();
        let server = AltiusMcpServer::new(workspace.path()).unwrap();
        assert!(server.resolve_project("..").is_err());
        assert!(server.resolve_project("/tmp").is_err());
    }

    #[test]
    fn detects_native_project_as_structured_json() {
        let workspace = tempfile::tempdir().unwrap();
        fs::write(
            workspace.path().join("Cargo.toml"),
            r#"[package]
name = "fixture"
version = "0.1.0"
[lib]
crate-type = ["cdylib", "lib"]
[dependencies]
solana-program = "2"
"#,
        )
        .unwrap();
        fs::create_dir(workspace.path().join("src")).unwrap();
        fs::write(
            workspace.path().join("src/lib.rs"),
            "solana_program::declare_id!(\"11111111111111111111111111111111\");",
        )
        .unwrap();

        let detected = detect_required(workspace.path()).unwrap();
        let json = serde_json::to_value(ToolEnvelope {
            ok: true,
            data: Some(detection_output(detected, workspace.path())),
            error: None,
        })
        .unwrap();
        assert_eq!(json["data"]["framework"], "native");
    }

    #[test]
    fn truncation_preserves_utf8_boundaries() {
        let value = "é".repeat(MAX_TEXT_BYTES);
        let bounded = bounded_redacted(&value);
        assert!(bounded.ends_with("…[truncated]"));
        assert!(bounded.is_char_boundary(bounded.len()));
    }
}
