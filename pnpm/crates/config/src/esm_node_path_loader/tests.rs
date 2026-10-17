use super::{
    add_esm_node_path_loader_option, esm_node_path_loader_import_flag,
    keep_esm_node_path_loader_option,
};
use pretty_assertions::assert_eq;
use std::{fs, process::Command};

/// The TypeScript CLI asserts its derived flag against the same file
/// (`pnpm11/exec/esm-node-path-loader/test/index.ts`), so the two stacks
/// cannot drift apart without one of the tests failing.
#[test]
fn flag_matches_the_golden_copy_shared_with_the_typescript_cli() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../pnpm11/exec/esm-node-path-loader/test/import-flag.txt",
    );
    let golden = std::fs::read_to_string(path)
        .unwrap_or_else(|err| {
            panic!("the golden flag file must be readable at {path}: {err}");
        });
    assert_eq!(esm_node_path_loader_import_flag(), golden);
}

#[test]
fn flag_never_contains_characters_the_node_options_tokenizer_splits_or_unquotes() {
    let flag = esm_node_path_loader_import_flag();
    assert!(flag.starts_with("--import=data:text/javascript,"));
    assert!(!flag.contains(|char: char| char.is_whitespace() || r#""'\"#.contains(char)));
}

#[test]
fn add_returns_just_the_flag_when_node_options_is_empty() {
    let flag = esm_node_path_loader_import_flag();
    assert_eq!(add_esm_node_path_loader_option(None), flag);
    assert_eq!(add_esm_node_path_loader_option(Some("")), flag);
}

#[test]
fn add_appends_the_flag_without_duplicating_it() {
    let flag = esm_node_path_loader_import_flag();
    let once = add_esm_node_path_loader_option(Some("--max-old-space-size=4096"));
    assert_eq!(once, format!("--max-old-space-size=4096 {flag}"));
    assert_eq!(add_esm_node_path_loader_option(Some(&once)), once);
}

#[test]
fn keep_reapplies_the_flag_only_when_the_previous_value_carried_it() {
    let flag = esm_node_path_loader_import_flag();
    let previous = add_esm_node_path_loader_option(None);
    assert_eq!(
        keep_esm_node_path_loader_option("--no-warnings", Some(&previous)),
        format!("--no-warnings {flag}"),
    );
    assert_eq!(keep_esm_node_path_loader_option("--no-warnings", None), "--no-warnings");
    assert_eq!(
        keep_esm_node_path_loader_option("--no-warnings", Some("--enable-source-maps")),
        "--no-warnings",
    );
}

/// Mirrors the TypeScript test of the same name in
/// `pnpm11/exec/esm-node-path-loader/test/index.ts`. The alias hook stands in
/// for the tsconfig paths hook in pnpm/pnpm#16979. `NODE_OPTIONS` imports
/// register before command-line ones, so it runs first and reads
/// `context.parentURL` after the fallback fails.
#[test]
fn the_fallback_leaves_the_importer_as_parent_url_for_hooks_registered_after_it() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    let node_path = root.join("store/node_modules");
    fs::create_dir_all(&node_path).expect("create NODE_PATH entry");
    fs::create_dir_all(root.join("src/generated")).expect("create src/generated");
    fs::write(root.join("src/generated/index.mjs"), "export const x = 1").expect("write index.mjs");
    fs::write(
        root.join("main.mjs"),
        r"import { x } from '@/generated/index.mjs'
console.log(import.meta.url)
console.log('resolved', x)
",
    )
    .expect("write main.mjs");
    // Like the pnpm hook, the alias hook falls back to module.register() where
    // module.registerHooks() is missing, so both hooks are in the same chain.
    fs::write(
        root.join("alias-hook.mjs"),
        r"import module from 'node:module'
import { resolveSync } from './alias-resolve.mjs'
if (module.registerHooks) {
  module.registerHooks({ resolve: resolveSync })
} else {
  module.register('./alias-resolve.mjs', import.meta.url)
}
",
    )
    .expect("write alias-hook.mjs");
    fs::write(
        root.join("alias-resolve.mjs"),
        r"import { writeFileSync } from 'node:fs'
function resolveAlias (specifier, context, error) {
  if (error?.code !== 'ERR_MODULE_NOT_FOUND' || !specifier.startsWith('@/')) throw error
  writeFileSync('parent-url.txt', context.parentURL)
  return new URL('src/' + specifier.slice(2), context.parentURL).href
}
export function resolveSync (specifier, context, nextResolve) {
  try {
    return nextResolve(specifier, context)
  } catch (error) {
    return nextResolve(resolveAlias(specifier, context, error), context)
  }
}
export async function resolve (specifier, context, nextResolve) {
  try {
    return await nextResolve(specifier, context)
  } catch (error) {
    return nextResolve(resolveAlias(specifier, context, error), context)
  }
}
",
    )
    .expect("write alias-resolve.mjs");

    let output = Command::new("node")
        .args(["--import", "./alias-hook.mjs", "main.mjs"])
        .current_dir(root)
        .env("NODE_PATH", &node_path)
        .env("NODE_OPTIONS", esm_node_path_loader_import_flag())
        .output()
        .expect("run node");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let mut lines = stdout.lines();
    let main_url = lines.next().expect("main.mjs prints its URL");
    assert_eq!(
        fs::read_to_string(root.join("parent-url.txt")).expect("read parent-url.txt"),
        main_url,
    );
    assert_eq!(lines.next(), Some("resolved 1"));
}
