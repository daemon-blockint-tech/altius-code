//! First-class GitHub MCP connector.
//!
//! The connector accepts only an MCP URL and the *name* of an environment
//! variable holding a bearer token. Token values never enter clap arguments,
//! serialized config, model context, or logs.

use std::sync::Arc;

use altius_agents::{GitHubAccess, GitHubTooling, McpTools};
use altius_mcp::{McpAttachments, McpRemoteConfig};

use crate::cli::{GitHubAccessArg, GitHubMcpArgs};
use crate::error::CliError;

pub async fn attach(
    args: &GitHubMcpArgs,
    attachments: &McpAttachments,
) -> Result<Option<GitHubTooling>, CliError> {
    let Some(url) = args
        .github_mcp_url
        .as_ref()
        .map(|url| url.trim())
        .filter(|url| !url.is_empty())
    else {
        return Ok(None);
    };

    let config = McpRemoteConfig {
        name: "github".into(),
        url: url.to_owned(),
        authorization_token_env: args.github_token_env.clone(),
    };
    let attached = attachments
        .attach_remote(config)
        .await
        .map_err(|error| CliError::message(format!("GitHub MCP attach failed: {error}")))?;
    let access = match args.github_access {
        GitHubAccessArg::ReadOnly => GitHubAccess::ReadOnly,
        GitHubAccessArg::PullRequests => GitHubAccess::PullRequests,
    };
    let tools = McpTools::github(Arc::clone(&attached), access);
    let specs = tools.tool_specs();
    eprintln!(
        "altius: GitHub MCP attached ({} tool(s), access={})",
        specs.len(),
        match access {
            GitHubAccess::ReadOnly => "read-only",
            GitHubAccess::PullRequests => "pull-requests",
        }
    );
    Ok(Some(GitHubTooling {
        tools: specs,
        dispatcher: Arc::new(tools),
    }))
}
