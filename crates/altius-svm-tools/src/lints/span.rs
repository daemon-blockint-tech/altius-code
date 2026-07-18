/// Locate the 1-based line of the first occurrence of `needle` in `contents`.
pub(crate) fn first_line_of(contents: &str, needle: &str) -> Option<u32> {
    let mut line = 1u32;
    for row in contents.lines() {
        if row.contains(needle) {
            return Some(line);
        }
        line = line.saturating_add(1);
    }
    None
}

/// Return a trimmed snippet for the first matching line, if any.
pub(crate) fn first_snippet(contents: &str, needle: &str) -> Option<String> {
    contents
        .lines()
        .find(|row| row.contains(needle))
        .map(|row| {
            let trimmed = row.trim();
            if trimmed.len() > 200 {
                format!("{}…", &trimmed[..200])
            } else {
                trimmed.to_owned()
            }
        })
}
