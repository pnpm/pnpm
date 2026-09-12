use super::PACKAGE_MAP_FILENAME;
use pnpm_config::Config;
use std::path::{Path, PathBuf};

pub fn make_node_package_map_option(package_map_path: &Path, node_options: Option<&str>) -> String {
    let node_options =
        node_options.map(str::to_string).or_else(|| std::env::var("NODE_OPTIONS").ok());
    let mut parts = remove_node_package_map_option(node_options.as_deref().unwrap_or_default());
    parts.push(format!(
        "--experimental-package-map={}",
        quote_path_if_needed(&package_map_path.to_string_lossy()),
    ));
    parts.join(" ")
}
pub fn make_node_require_option(module_path: &Path, node_options: Option<&str>) -> String {
    let node_options =
        node_options.map(str::to_string).or_else(|| std::env::var("NODE_OPTIONS").ok());
    let quoted_path = quote_path_if_needed(&module_path.to_string_lossy());
    let require_option = format!("--require={quoted_path}");
    match node_options.as_deref().map(str::trim).filter(|options| !options.is_empty()) {
        Some(node_options) => format!("{node_options} {require_option}"),
        None => require_option,
    }
}
pub fn package_map_path_for_execution(config: &Config, dir: &Path) -> Option<PathBuf> {
    if !config.node_experimental_package_map {
        return None;
    }
    // Installs write the map under the configured modules dir, so detect it
    // by that dir's basename rather than the hard-coded `node_modules`.
    let modules_dir_name =
        config.modules_dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("node_modules"));
    let workspace_path = config
        .workspace_dir
        .as_ref()
        .map(|dir| dir.join(modules_dir_name).join(PACKAGE_MAP_FILENAME));
    if let Some(path) = workspace_path
        && path.exists()
    {
        return Some(path);
    }
    let path = dir.join(modules_dir_name).join(PACKAGE_MAP_FILENAME);
    path.exists().then_some(path)
}
pub(super) fn remove_node_package_map_option(node_options: &str) -> Vec<String> {
    let tokens = split_node_options(node_options);
    let mut retained = Vec::new();
    let mut skip_next = false;
    for token in tokens {
        if skip_next {
            skip_next = false;
            continue;
        }
        if token == "--experimental-package-map" {
            skip_next = true;
            continue;
        }
        if token.starts_with("--experimental-package-map=") {
            continue;
        }
        retained.push(token);
    }
    retained
}
pub(super) fn split_node_options(node_options: &str) -> Vec<String> {
    let mut tokenizer = NodeOptionsTokenizer::default();
    for ch in node_options.chars() {
        tokenizer.push(ch);
    }
    tokenizer.finish()
}
/// Node's `NODE_OPTIONS` tokenizer: whitespace separates tokens, `'`
/// and `"` quote, and `\` escapes the next character anywhere — so an
/// escaped quote does not end a token. The literal text (backslash
/// included) is preserved so retained tokens round-trip verbatim.
#[derive(Default)]
pub(super) struct NodeOptionsTokenizer {
    tokens: Vec<String>,
    token: String,
    quote: Option<char>,
    escaped: bool,
}
impl NodeOptionsTokenizer {
    fn push(&mut self, ch: char) {
        if self.escaped {
            self.token.push(ch);
            self.escaped = false;
            return;
        }
        if ch == '\\' {
            self.token.push(ch);
            self.escaped = true;
            return;
        }
        if let Some(quote) = self.quote {
            self.token.push(ch);
            if ch == quote {
                self.quote = None;
            }
            return;
        }
        if ch == '"' || ch == '\'' {
            self.quote = Some(ch);
            self.token.push(ch);
        } else if ch.is_whitespace() {
            self.end_token();
        } else {
            self.token.push(ch);
        }
    }

    fn end_token(&mut self) {
        if !self.token.is_empty() {
            self.tokens.push(std::mem::take(&mut self.token));
        }
    }

    fn finish(mut self) -> Vec<String> {
        self.end_token();
        self.tokens
    }
}
pub(super) fn quote_path_if_needed(path: &str) -> String {
    // Node's NODE_OPTIONS tokenizer treats whitespace as a separator, `'`/`"`
    // as quote delimiters, and `\` as an escape character (so a bare Windows
    // path would lose its separators). Wrap such paths in double quotes,
    // escaping only `\` and `"`. A full JSON encode is wrong here: Node does
    // not decode `\uXXXX`, so escaping non-ASCII bytes would corrupt the path.
    if path.chars().any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '\\')) {
        let escaped = path.replace('\\', r"\\").replace('"', r#"\""#);
        format!(r#""{escaped}""#)
    } else {
        path.to_string()
    }
}
