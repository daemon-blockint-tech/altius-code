use std::fs;
use std::path::{Path, PathBuf};

use crate::error::ScannerError;

const SKIPPED_DIRS: [&str; 6] = ["target", ".git", "node_modules", "cache", "out", "dist"];

pub(crate) fn collect_files(
    root: &Path,
    extensions: &[&str],
    max_depth: usize,
) -> Result<Vec<PathBuf>, ScannerError> {
    let mut out = Vec::new();
    walk(root, 0, max_depth, extensions, &mut out)?;
    Ok(out)
}

fn walk(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    extensions: &[&str],
    out: &mut Vec<PathBuf>,
) -> Result<(), ScannerError> {
    if depth > max_depth || !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| ScannerError::Io(e.to_string()))? {
        let path = entry.map_err(|e| ScannerError::Io(e.to_string()))?.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if SKIPPED_DIRS.contains(&name) {
                continue;
            }
            walk(&path, depth + 1, max_depth, extensions, out)?;
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if extensions.contains(&ext) {
                out.push(path);
            }
        }
    }
    Ok(())
}

pub(crate) fn first_line(contents: &str, needle: &str) -> Option<u32> {
    contents
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(needle))
        .map(|(idx, _)| (idx + 1) as u32)
}
