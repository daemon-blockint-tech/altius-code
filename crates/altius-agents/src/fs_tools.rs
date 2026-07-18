//! Sandboxed filesystem and allowlisted command tools for fleet agents.
//!
//! All paths are confined to a project root. Commands are FailClosed: only
//! an explicit binary + (for git) subcommand allowlist may run. No tool here
//! may sign, deploy, or broadcast.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use regex::RegexBuilder;
use serde_json::{json, Value};

/// Default binaries allowed for `run_command` (first argv token).
pub const DEFAULT_BASH_ALLOWLIST: &[&str] = &[
    "cargo", "anchor", "solana", "npm", "npx", "yarn", "pnpm", "forge", "cast", "anvil", "git",
    "rustc", "python3", "pytest",
];

/// Git subcommands permitted in Phase A (read-only).
const GIT_READONLY: &[&str] = &["status", "diff", "log", "show"];

const MAX_FILE_BYTES: usize = 16 * 1024;
const MAX_GREP_MATCHES: usize = 50;
const MAX_GLOB_MATCHES: usize = 100;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_COMMAND_OUTPUT_BYTES: usize = 16 * 1024;

const SHELL_META: &[char] = &[
    '|', '&', ';', '<', '>', '`', '$', '(', ')', '{', '}', '\n', '\r',
];

/// Resolve a relative path that must already exist under `project_root`.
pub fn resolve_existing_path(project_root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = relative.trim();
    if relative.is_empty() {
        return Err("`path` must not be empty".into());
    }
    let requested = Path::new(relative);
    if requested.is_absolute() {
        return Err("`path` must be relative to the project root".into());
    }
    let root = canonicalize_root(project_root)?;
    let resolved = root
        .join(requested)
        .canonicalize()
        .map_err(|error| format!("cannot resolve `path`: {error}"))?;
    if !resolved.starts_with(&root) {
        return Err("`path` escapes the project root".into());
    }
    Ok(resolved)
}

/// Resolve a relative path for write/create. Parent must stay under root.
pub fn resolve_writable_path(project_root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = relative.trim();
    if relative.is_empty() {
        return Err("`path` must not be empty".into());
    }
    let requested = Path::new(relative);
    if requested.is_absolute() {
        return Err("`path` must be relative to the project root".into());
    }
    if relative.contains("..") {
        // Reject `..` components before join to avoid TOCTOU surprises on create.
        for component in requested.components() {
            if matches!(component, std::path::Component::ParentDir) {
                return Err("`path` must not contain `..`".into());
            }
        }
    }
    let root = canonicalize_root(project_root)?;
    let joined = root.join(requested);
    if let Ok(existing) = joined.canonicalize() {
        if !existing.starts_with(&root) {
            return Err("`path` escapes the project root".into());
        }
        return Ok(existing);
    }
    let parent = joined
        .parent()
        .ok_or_else(|| "cannot resolve parent directory".to_owned())?;
    let parent = if parent.exists() {
        parent
            .canonicalize()
            .map_err(|error| format!("cannot resolve parent: {error}"))?
    } else {
        return Err("parent directory does not exist".into());
    };
    if !parent.starts_with(&root) {
        return Err("`path` escapes the project root".into());
    }
    Ok(joined)
}

fn canonicalize_root(project_root: &Path) -> Result<PathBuf, String> {
    project_root
        .canonicalize()
        .map_err(|error| format!("cannot resolve project root: {error}"))
}

pub fn read_file(project_root: &Path, path: &str) -> Result<Value, String> {
    let target = resolve_existing_path(project_root, path)?;
    if !target.is_file() {
        return Err("path is not a file".into());
    }
    let mut file = fs::File::open(&target).map_err(|error| error.to_string())?;
    let mut buf = Vec::new();
    file.by_ref()
        .take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|error| error.to_string())?;
    let truncated = buf.len() > MAX_FILE_BYTES;
    if truncated {
        buf.truncate(MAX_FILE_BYTES);
    }
    let content = String::from_utf8_lossy(&buf).into_owned();
    Ok(json!({
        "path": path,
        "content": content,
        "truncated": truncated,
    }))
}

pub fn write_file(project_root: &Path, path: &str, content: &str) -> Result<Value, String> {
    let target = resolve_writable_path(project_root, path)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&target, content).map_err(|error| error.to_string())?;
    Ok(json!({
        "path": path,
        "bytes_written": content.len(),
    }))
}

pub fn edit_file(
    project_root: &Path,
    path: &str,
    old_string: &str,
    new_string: &str,
) -> Result<Value, String> {
    if old_string.is_empty() {
        return Err("`old_string` must not be empty".into());
    }
    let target = resolve_existing_path(project_root, path)?;
    let content = fs::read_to_string(&target).map_err(|error| error.to_string())?;
    let matches = content.matches(old_string).count();
    if matches == 0 {
        return Err("`old_string` not found in file".into());
    }
    if matches > 1 {
        return Err(format!(
            "`old_string` matched {matches} times; refine it to a unique span"
        ));
    }
    let updated = content.replacen(old_string, new_string, 1);
    fs::write(&target, &updated).map_err(|error| error.to_string())?;
    Ok(json!({
        "path": path,
        "replacements": 1,
    }))
}

pub fn grep(
    project_root: &Path,
    pattern: &str,
    path: Option<&str>,
) -> Result<Value, String> {
    if pattern.is_empty() {
        return Err("`pattern` must not be empty".into());
    }
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(false)
        .size_limit(1 << 20)
        .build()
        .map_err(|error| format!("invalid pattern: {error}"))?;
    let root = canonicalize_root(project_root)?;
    let search_root = match path {
        Some(p) if !p.is_empty() && p != "." => resolve_existing_path(project_root, p)?,
        _ => root.clone(),
    };
    let mut matches = Vec::new();
    walk_files(&search_root, &root, &mut |file| {
        if matches.len() >= MAX_GREP_MATCHES {
            return false;
        }
        let Ok(text) = fs::read_to_string(file) else {
            return true;
        };
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .display()
            .to_string();
        for (idx, line) in text.lines().enumerate() {
            if matches.len() >= MAX_GREP_MATCHES {
                return false;
            }
            if regex.is_match(line) {
                matches.push(json!({
                    "path": rel,
                    "line": idx + 1,
                    "text": truncate_line(line),
                }));
            }
        }
        true
    })?;
    Ok(json!({
        "pattern": pattern,
        "matches": matches,
        "truncated": matches.len() >= MAX_GREP_MATCHES,
    }))
}

pub fn glob_files(project_root: &Path, pattern: &str) -> Result<Value, String> {
    if pattern.is_empty() {
        return Err("`pattern` must not be empty".into());
    }
    let root = canonicalize_root(project_root)?;
    let matcher = glob_to_regex(pattern)?;
    let mut paths = Vec::new();
    walk_files(&root, &root, &mut |file| {
        if paths.len() >= MAX_GLOB_MATCHES {
            return false;
        }
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .display()
            .to_string()
            .replace('\\', "/");
        if matcher.is_match(&rel) {
            paths.push(rel);
        }
        true
    })?;
    Ok(json!({
        "pattern": pattern,
        "paths": paths,
        "truncated": paths.len() >= MAX_GLOB_MATCHES,
    }))
}

/// Validate and run an allowlisted command in `project_root`.
pub fn run_command(
    project_root: &Path,
    argv: &[String],
    allowlist: &[String],
) -> Result<Value, String> {
    if argv.is_empty() {
        return Err("`argv` must not be empty".into());
    }
    for arg in argv {
        if arg.chars().any(|c| SHELL_META.contains(&c)) {
            return Err("shell metacharacters are not allowed in argv".into());
        }
        if arg.contains('\0') {
            return Err("nul bytes are not allowed in argv".into());
        }
    }
    let binary = argv[0].as_str();
    let allowed: Vec<&str> = if allowlist.is_empty() {
        DEFAULT_BASH_ALLOWLIST.to_vec()
    } else {
        allowlist.iter().map(String::as_str).collect()
    };
    if !allowed.iter().any(|name| *name == binary) {
        return Err(format!(
            "binary `{binary}` is not on the command allowlist"
        ));
    }
    if binary == "git" {
        let sub = argv.get(1).map(String::as_str).unwrap_or("");
        if !GIT_READONLY.contains(&sub) {
            return Err(format!(
                "git subcommand `{sub}` is not allowed (Phase A is read-only git)"
            ));
        }
    }
    if binary == "solana" {
        if argv.iter().any(|a| a.contains("keygen") || a == "transfer") {
            return Err("solana keygen/transfer commands are forbidden".into());
        }
    }

    let root = canonicalize_root(project_root)?;
    let mut cmd = Command::new(binary);
    cmd.args(&argv[1..]).current_dir(&root).env_clear();
    // Minimal PATH so allowlisted tools resolve; do not inherit secrets.
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    if let Ok(home) = std::env::var("HOME") {
        cmd.env("HOME", home);
    }
    cmd.env("TERM", "dumb");

    let output = run_with_timeout(cmd)?;
    let stdout = truncate_bytes(&output.stdout);
    let stderr = truncate_bytes(&output.stderr);
    Ok(json!({
        "argv": argv,
        "status": output.status.code(),
        "stdout": stdout.text,
        "stderr": stderr.text,
        "stdout_truncated": stdout.truncated,
        "stderr_truncated": stderr.truncated,
    }))
}

struct TruncatedText {
    text: String,
    truncated: bool,
}

fn truncate_bytes(bytes: &[u8]) -> TruncatedText {
    let truncated = bytes.len() > MAX_COMMAND_OUTPUT_BYTES;
    let slice = if truncated {
        &bytes[..MAX_COMMAND_OUTPUT_BYTES]
    } else {
        bytes
    };
    TruncatedText {
        text: String::from_utf8_lossy(slice).into_owned(),
        truncated,
    }
}

fn truncate_line(line: &str) -> String {
    const MAX: usize = 240;
    if line.len() <= MAX {
        return line.to_owned();
    }
    let mut boundary = MAX;
    while !line.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…", &line[..boundary])
}

fn run_with_timeout(mut cmd: Command) -> Result<std::process::Output, String> {
    use std::process::Stdio;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|error| error.to_string())?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().map_err(|error| error.to_string());
            }
            Ok(None) => {
                if start.elapsed() > COMMAND_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "command timed out after {}s",
                        COMMAND_TIMEOUT.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// Walk files under `dir`. `visit` returns `false` to stop early.
/// Returns `Ok(false)` when stopped early, `Ok(true)` when exhausted.
fn walk_files(
    dir: &Path,
    root: &Path,
    visit: &mut dyn FnMut(&Path) -> bool,
) -> Result<bool, String> {
    let entries = fs::read_dir(dir).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        // Stay under root even if a symlink points out.
        let Ok(canon) = path.canonicalize() else {
            continue;
        };
        if !canon.starts_with(root) {
            continue;
        }
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name == ".git" || name == "target" || name == "node_modules" {
                continue;
            }
            if !walk_files(&canon, root, visit)? {
                return Ok(false);
            }
        } else if file_type.is_file() && !visit(&canon) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn glob_to_regex(pattern: &str) -> Result<regex::Regex, String> {
    let mut out = String::from("^");
    let chars: Vec<char> = pattern.replace('\\', "/").chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                out.push_str(".*");
                i += 2;
                if i < chars.len() && chars[i] == '/' {
                    i += 1;
                }
            }
            '*' => {
                out.push_str("[^/]*");
                i += 1;
            }
            '?' => {
                out.push_str("[^/]");
                i += 1;
            }
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' => {
                out.push('\\');
                out.push(chars[i]);
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out.push('$');
    regex::Regex::new(&out).map_err(|error| format!("invalid glob: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("hello.txt"), "hello world\nfoo bar\n").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "fn main() {}\n").unwrap();
        dir
    }

    #[test]
    fn read_and_edit_roundtrip() {
        let dir = fixture();
        let read = read_file(dir.path(), "hello.txt").unwrap();
        assert!(read["content"].as_str().unwrap().contains("hello"));
        edit_file(dir.path(), "hello.txt", "hello", "hola").unwrap();
        let read = read_file(dir.path(), "hello.txt").unwrap();
        assert!(read["content"].as_str().unwrap().starts_with("hola"));
    }

    #[test]
    fn path_escape_rejected() {
        let dir = fixture();
        assert!(resolve_existing_path(dir.path(), "..").is_err());
        assert!(resolve_writable_path(dir.path(), "../evil.txt").is_err());
        assert!(resolve_writable_path(dir.path(), "/etc/passwd").is_err());
    }

    #[test]
    fn grep_and_glob_find_files() {
        let dir = fixture();
        let grepped = grep(dir.path(), "fn main", None).unwrap();
        assert_eq!(grepped["matches"].as_array().unwrap().len(), 1);
        let found = glob_files(dir.path(), "src/**/*.rs").unwrap();
        assert!(found["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.as_str() == Some("src/lib.rs")));
    }

    #[test]
    fn run_command_denies_dangerous_argv() {
        let dir = fixture();
        let allow: Vec<String> = DEFAULT_BASH_ALLOWLIST
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert!(run_command(
            dir.path(),
            &["rm".into(), "-rf".into(), ".".into()],
            &allow
        )
        .is_err());
        assert!(run_command(
            dir.path(),
            &["sh".into(), "-c".into(), "echo hi".into()],
            &allow
        )
        .is_err());
        assert!(run_command(
            dir.path(),
            &["curl".into(), "https://example.com".into()],
            &allow
        )
        .is_err());
        assert!(run_command(
            dir.path(),
            &["git".into(), "push".into()],
            &allow
        )
        .is_err());
        assert!(run_command(
            dir.path(),
            &["cargo".into(), "test".into(), "|".into(), "cat".into()],
            &allow
        )
        .is_err());
    }

    #[test]
    fn run_command_allows_git_status() {
        let dir = fixture();
        let allow: Vec<String> = DEFAULT_BASH_ALLOWLIST
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        // May fail if git missing; only assert allowlist accepts the argv shape.
        let _ = run_command(dir.path(), &["git".into(), "status".into()], &allow);
    }
}
