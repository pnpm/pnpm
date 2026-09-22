use std::collections::HashMap;

#[cfg(test)]
mod tests;

const RELOCATION_MARKER: &str = "/* [comment possibly relocated by pnpm] */";

struct Comment<'a> {
    text: &'a str,
    line: usize,
}

/// Restores comments from validated JSON5 beside matching serialized JSON lines.
/// Unmatched comments are retained, with relocation marked unless already noted.
pub(crate) fn restore_comments(original: &str, serialized: &str) -> String {
    let (stripped, comments) = extract_comments(original);
    if comments.is_empty() {
        return serialized.to_owned();
    }
    let already_marked = comments
        .iter()
        .any(|comment| comment.text == RELOCATION_MARKER);
    let source_lines: Vec<_> = stripped
        .split(is_line_break)
        .map(canonicalize)
        .collect();
    let lines: Vec<_> = serialized.split('\n').collect();
    let index = index_lines(&lines);
    let mut prefixes: Vec<Vec<(&str, bool)>> = vec![Vec::new(); lines.len()];
    for comment in comments {
        let (location, relocated) = locate_comment(&comment, &source_lines, &index, lines.len());
        prefixes[location].push((comment.text, relocated && !already_marked));
    }
    render_comments(&lines, prefixes, original.len().max(serialized.len()))
}

fn render_comments(lines: &[&str], prefixes: Vec<Vec<(&str, bool)>>, capacity: usize) -> String {
    let mut output = String::with_capacity(capacity);
    for (line, comments) in lines.iter().zip(prefixes) {
        for (text, relocated) in comments {
            output.push_str(text);
            output.push('\n');
            if relocated {
                output.push_str(RELOCATION_MARKER);
                output.push('\n');
            }
        }
        output.push_str(line);
        output.push('\n');
    }
    output.pop();
    output
}

fn extract_comments(source: &str) -> (String, Vec<Comment<'_>>) {
    let mut stripped = String::with_capacity(source.len());
    let mut comments = Vec::new();
    let mut cursor = 0;
    let mut line = 0;
    while cursor < source.len() {
        let remaining = &source[cursor..];
        let length = token_length(remaining);
        let token = &remaining[..length];
        if token.starts_with("//") || token.starts_with("/*") {
            comments.push(Comment { text: token, line });
            stripped.extend(token.chars().filter(|&character| is_line_break(character)));
        } else {
            stripped.push_str(token);
        }
        line += token
            .chars()
            .filter(|&character| is_line_break(character))
            .count();
        cursor += length;
    }
    (stripped, comments)
}

fn token_length(source: &str) -> usize {
    if source.starts_with("//") {
        return source.find(is_line_break).unwrap_or(source.len());
    }
    if source.starts_with("/*") {
        return source
            .find("*/")
            .map_or(source.len(), |end| end + 2);
    }
    let mut characters = source.char_indices();
    let Some((_, first)) = characters.next() else { return 0 };
    if first != '\'' && first != '"' {
        return first.len_utf8();
    }
    while let Some((position, character)) = characters.next() {
        if character == '\\' {
            characters.next();
        } else if character == first {
            return position + character.len_utf8();
        }
    }
    source.len()
}

fn is_line_break(character: char) -> bool {
    matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn canonicalize(line: &str) -> String {
    line.chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '\'' | '"'))
        .collect()
}

fn index_lines(lines: &[&str]) -> HashMap<String, Option<usize>> {
    let mut index = HashMap::new();
    for (position, line) in lines.iter().enumerate() {
        let key = canonicalize(line);
        if !key.is_empty() {
            index
                .entry(key)
                .and_modify(|value| *value = None)
                .or_insert(Some(position));
        }
    }
    index
}

fn locate_comment(
    comment: &Comment<'_>,
    source: &[String],
    index: &HashMap<String, Option<usize>>,
    line_count: usize,
) -> (usize, bool) {
    if let Some(location) = find_line(source.get(comment.line), index) {
        return (location, false);
    }
    if comment.line == 0 {
        return (0, false);
    }
    if let Some(location) = find_line(source.get(comment.line + 1), index) {
        return (location, false);
    }
    if let Some(location) = find_line(source.get(comment.line - 1), index) {
        return ((location + 1).min(line_count - 1), false);
    }
    (comment.line.min(line_count - 1), true)
}

fn find_line(line: Option<&String>, index: &HashMap<String, Option<usize>>) -> Option<usize> {
    line.and_then(|line| index.get(line))
        .copied()
        .flatten()
}
