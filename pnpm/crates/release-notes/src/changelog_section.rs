/// The section of `changelog` under the heading whose text is `version`, as
/// changesets (`## 1.4.0`) and Keep a Changelog (`## [1.4.0] - 2026-10-28`)
/// write it, up to the next heading of the same or a higher level. The
/// heading itself and the blank lines around the body are left out, and
/// headings inside fenced code blocks are not headings. `None` when no
/// heading names the version or its section is empty.
#[must_use]
pub fn find_changelog_section(changelog: &str, version: &str) -> Option<String> {
    let lines: Vec<&str> = changelog.lines().collect();
    let headings = markdown_headings(&lines);
    let (position, (start, level)) = headings
        .iter()
        .enumerate()
        .find(|(_, (line, _))| heading_names_version(lines[*line], version))?;
    let end = headings[position + 1..]
        .iter()
        .find(|(_, next_level)| next_level <= level)
        .map_or(lines.len(), |(line, _)| *line);
    let body = lines[start + 1..end].join("\n");
    let body = body.trim_matches('\n').trim_end();
    (!body.trim().is_empty()).then(|| format!("{body}\n"))
}

/// `(line index, level)` of every ATX heading outside fenced code blocks.
fn markdown_headings(lines: &[&str]) -> Vec<(usize, usize)> {
    let mut headings = Vec::new();
    let mut fence: Option<&str> = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if let Some(marker) = fence_marker(trimmed) {
            fence = match fence {
                Some(open) if closes_fence(open, marker, trimmed) => None,
                Some(open) => Some(open),
                None => Some(marker),
            };
            continue;
        }
        if fence.is_none()
            && let Some(level) = heading_level(trimmed)
        {
            headings.push((index, level));
        }
    }
    headings
}

/// The run of backticks or tildes a fence line starts with: three or more.
fn fence_marker(line: &str) -> Option<&str> {
    let character = line
        .chars()
        .next()
        .filter(|character| matches!(character, '`' | '~'))?;
    let length = line
        .chars()
        .take_while(|&next| next == character)
        .count();
    (length >= 3).then(|| &line[..length])
}

/// Whether `line`, starting with fence `marker`, closes the fence `open`
/// opened: the same character, at least as many of it, and nothing after
/// but whitespace.
fn closes_fence(open: &str, marker: &str, line: &str) -> bool {
    marker.starts_with(open) && line[marker.len()..].trim().is_empty()
}

fn heading_level(line: &str) -> Option<usize> {
    let level = line
        .chars()
        .take_while(|&character| character == '#')
        .count();
    let after = &line[level..];
    let is_heading =
        (1..=6).contains(&level) && (after.is_empty() || after.starts_with([' ', '\t']));
    is_heading.then_some(level)
}

/// Whether a heading's text, after its `#` marks, is `version`, optionally
/// in brackets, prefixed with `v`, or followed by a date.
fn heading_names_version(line: &str, version: &str) -> bool {
    let text = line
        .trim_start()
        .trim_start_matches('#')
        .trim();
    let token = match text.strip_prefix('[') {
        Some(bracketed) => bracketed
            .split(']')
            .next()
            .unwrap_or_default(),
        None => text
            .split_whitespace()
            .next()
            .unwrap_or_default(),
    };
    token.strip_prefix('v').unwrap_or(token) == version
}

#[cfg(test)]
mod tests;
