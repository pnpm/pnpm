//! The dotenv syntax `--env-file` reads: a port of `Dotenv::ParseContent` in
//! Node.js's `src/node_dotenv.cc`, so a file loads as it does under
//! `node --env-file`. The function is unchanged from Node.js 22 through 26.
//!
//! Like Node.js, the parser never rejects input and never expands variables.

/// The `KEY=VALUE` entries of `input`, in file order. A key may repeat, and
/// the last entry wins. A key may be empty, which no environment can hold.
pub fn parse(input: &str) -> Vec<(String, String)> {
    let lines = input.replace('\r', "");
    let mut entries = Vec::new();
    let mut content = trim_spaces(&lines);
    while !content.is_empty() {
        content = parse_entry(content, &mut entries);
    }
    entries
}

/// Read the entry or skipped line at the start of `content`, returning the
/// input after it.
fn parse_entry<'content>(
    content: &'content str,
    entries: &mut Vec<(String, String)>,
) -> &'content str {
    if content.starts_with(['\n', '#']) {
        return after_line(content);
    }
    let Some(index) = content.find(['=', '\n']) else {
        return "";
    };
    if content[index..].starts_with('\n') {
        return trim_spaces(&content[index + 1..]);
    }
    let key = trim_spaces(&content[..index]);
    let rest = &content[index + 1..];
    // Node.js stores an empty value before it checks or strips the key.
    if rest.is_empty() || rest.starts_with('\n') {
        entries.push((key.to_owned(), String::new()));
        return rest;
    }
    let rest = trim_spaces(rest);
    if key.is_empty() {
        return rest;
    }
    let key = key.strip_prefix("export ").map_or(key, trim_spaces);
    let (value, rest) = parse_value(rest);
    entries.push((key.to_owned(), value));
    rest
}

/// The value at the start of `content`, and the input after its line.
fn parse_value(content: &str) -> (String, &str) {
    let quote = content
        .chars()
        .next()
        .filter(|character| matches!(character, '"' | '\'' | '`'));
    if let Some(quote) = quote
        && let Some(offset) = content[1..].find(quote)
    {
        let inner = &content[1..=offset];
        let value = if quote == '"' { inner.replace(r"\n", "\n") } else { inner.to_owned() };
        return (value, after_line(&content[offset + 2..]));
    }
    let (line, rest) = content
        .split_once('\n')
        .unwrap_or((content, ""));
    // An unclosed quote keeps the whole line, the quote included.
    let value = if quote.is_some() {
        line
    } else {
        trim_spaces(line.split_once('#').map_or(line, |(before, _)| before))
    };
    (value.to_owned(), trim_spaces(rest))
}

fn after_line(text: &str) -> &str {
    text.split_once('\n').map_or("", |(_, after)| after)
}

fn trim_spaces(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n'])
}

#[cfg(test)]
mod tests;
