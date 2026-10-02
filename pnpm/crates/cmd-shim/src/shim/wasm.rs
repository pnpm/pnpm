use super::{ScriptRuntime, relative_target, relocatable::is_within_root};
use serde_json::json;
use std::path::Path;

pub fn generate_wasm_shim(
    target: &Path,
    shim: &Path,
    runtime: Option<&ScriptRuntime>,
    node_path: &[String],
    root: Option<&Path>,
) -> String {
    let directory = shim.parent().unwrap_or_else(|| Path::new(""));
    let target_relative = relative_target(target, shim);
    let paths = launcher_node_paths(node_path, directory, root);
    let arguments = shell_words::split(runtime.map_or("", |runtime| runtime.args.as_str()))
        .map_err(|error| error.to_string());
    let config = json!({
        "target": target_relative,
        "program": runtime.and_then(|runtime| runtime.prog.as_deref()),
        "arguments": arguments,
        "nodePath": paths,
    });
    let permission_helper = include_str!("../../../../wasm/host/executable-mode.mjs")
        .strip_prefix("export ")
        .expect("the standalone executable permission helper exports one function");
    let launcher = include_str!("wasm_shim.js").replace("PNPM_SHIM_CONFIG", &config.to_string());
    let body = format!("{permission_helper}\n{launcher}");
    let marker = if target_relative.contains(['\n', '\r']) || target_relative.contains("*/") {
        String::new()
    } else {
        format!("/*\n# cmd-shim-target={target_relative}\n*/\n")
    };
    format!("#!/usr/bin/env node\n{body}// pnpm-wasm-shim={config}\n{marker}")
}

fn launcher_node_paths(entries: &[String], directory: &Path, root: Option<&Path>) -> Vec<String> {
    entries
        .iter()
        .map(|entry| {
            if is_within_root(root, directory, Path::new(entry)) {
                super::relative_path_from(directory, Path::new(entry))
                    .to_string_lossy()
                    .into_owned()
            } else {
                entry.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
