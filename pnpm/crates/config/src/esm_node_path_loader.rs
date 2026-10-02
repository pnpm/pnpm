//! Builds the `NODE_OPTIONS` flag that restores `NODE_PATH` resolution for
//! ESM imports.
//!
//! Node.js ignores the `NODE_PATH` environment variable when resolving ESM
//! imports. With a global virtual store, hoisted (phantom) dependencies are
//! reachable only through `NODE_PATH`, because package directories live
//! outside the project, so Node's upward `node_modules` walk from their real
//! paths never reaches the project's hoisted `node_modules`. The resolve
//! hook built here restores `NODE_PATH` lookups for bare specifiers that the
//! default ESM resolution fails to find.
//!
//! Any `--import` flag makes Node.js run the main entry point through the
//! ESM loader, which rejects file extensions that only CJS require hooks
//! handle, such as the `.ts` entry point of ts-node. The hook marks such an
//! entry point with the `commonjs` format, which hands it back to the CJS
//! loader, as Node.js does without `--import`. Entry points ending in `.js`
//! or without an extension are left alone, because Node.js may detect
//! module syntax in them.
//!
//! The hook sources are identical copies of the ones the TypeScript CLI
//! embeds (`pnpm11/exec/esm-node-path-loader/src/index.ts`) — the two must
//! stay in sync so both CLIs inject the same `NODE_OPTIONS` value.

use std::{fmt::Write, sync::LazyLock};

const RESOLVE_HELPERS: &str = r"const nodePaths = (process.env.NODE_PATH ?? '').split(delimiter).filter(Boolean)
const isBareSpecifier = (specifier) => !specifier.startsWith('.') && !specifier.startsWith('/') && !specifier.startsWith('#') && !specifier.includes(':')
const isEntryPointUnknownToEsm = (resolved, context) => context.parentURL === undefined && resolved.format == null && resolved.url.startsWith('file:') && !['', '.js'].includes(extname(new URL(resolved.url).pathname))
const markUnknownEntryPointAsCommonJs = (resolved, context) => isEntryPointUnknownToEsm(resolved, context) ? { ...resolved, format: 'commonjs' } : resolved";

/// Fallback for Node.js versions that have `module.register()` but not
/// `module.registerHooks()` (>=18.19 <22.15): an off-thread hooks module
/// for `module.register()`. Runtimes with neither API (`--import` parses
/// from 18.18/19.0) get no hook at all — CJS `NODE_PATH` resolution still
/// works natively there. The retry goes through the default resolver with
/// a synthetic parent inside each `NODE_PATH` entry, so the caller's
/// export conditions (import vs require) are preserved; every entry pnpm
/// puts on `NODE_PATH` ends in `node_modules`, which is why the parent
/// walk finds the entry's packages.
const ASYNC_LOADER_TEMPLATE: &str = r"import { delimiter, extname } from 'node:path'
import { pathToFileURL } from 'node:url'
@HELPERS@

export async function resolve (specifier, context, nextResolve) {
  try {
    return markUnknownEntryPointAsCommonJs(await nextResolve(specifier, context), context)
  } catch (originalError) {
    if (originalError?.code !== 'ERR_MODULE_NOT_FOUND' || !isBareSpecifier(specifier)) throw originalError
    for (const nodePath of nodePaths) {
      try {
        return await nextResolve(specifier, { ...context, parentURL: pathToFileURL(nodePath + '/x').href })
      } catch (fallbackError) {
        if (fallbackError?.code !== 'ERR_MODULE_NOT_FOUND') throw fallbackError
      }
    }
    throw originalError
  }
}
";

const REGISTRATION_TEMPLATE: &str = r#"const { register, registerHooks } = await import('node:module')
if (registerHooks) {
  const { delimiter, extname } = await import('node:path')
  const { pathToFileURL } = await import('node:url')
  @HELPERS@
  registerHooks({
    resolve (specifier, context, nextResolve) {
      try {
        return markUnknownEntryPointAsCommonJs(nextResolve(specifier, context), context)
      } catch (originalError) {
        if (originalError?.code !== 'ERR_MODULE_NOT_FOUND' || !isBareSpecifier(specifier)) throw originalError
        for (const nodePath of nodePaths) {
          try {
            return nextResolve(specifier, { ...context, parentURL: pathToFileURL(nodePath + '/x').href })
          } catch (fallbackError) {
            if (fallbackError?.code !== 'ERR_MODULE_NOT_FOUND') throw fallbackError
          }
        }
        throw originalError
      }
    },
  })
} else if (register) {
  register("data:text/javascript,@ASYNC_LOADER@")
}
"#;

/// The hooks are inlined into the flag as data: URLs, so the flag is a
/// self-contained constant: no file has to exist on disk for the child
/// Node.js process to load it, and it stays valid no matter which project or
/// pnpm version spawned the child. The hook is installed even when
/// `NODE_PATH` is empty, because the flag alone already sends the entry
/// point through the ESM loader.
#[must_use]
pub fn esm_node_path_loader_import_flag() -> &'static str {
    static FLAG: LazyLock<String> = LazyLock::new(|| {
        let async_loader = ASYNC_LOADER_TEMPLATE.replace("@HELPERS@", RESOLVE_HELPERS);
        let registration = REGISTRATION_TEMPLATE
            .replace("@HELPERS@", &RESOLVE_HELPERS.replace('\n', "\n  "))
            .replace("@ASYNC_LOADER@", &strict_uri_encode(&async_loader));
        format!("--import=data:text/javascript,{}", strict_uri_encode(&registration))
    });
    &FLAG
}

/// Percent-encodes everything outside the RFC 3986 unreserved set, exactly
/// like the TypeScript side's `strictUriEncode`. Unlike JavaScript's
/// `encodeURIComponent`, this also encodes `!` `'` `(` `)` `*` — the single
/// quote in particular, which Node's `NODE_OPTIONS` tokenizer treats as a
/// quote delimiter and would strip from the flag.
fn strict_uri_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len() * 2);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => write!(encoded, "%{byte:02X}").expect("write hex escape to string"),
        }
    }
    encoded
}

#[must_use]
pub fn add_esm_node_path_loader_option(node_options: Option<&str>) -> String {
    let flag = esm_node_path_loader_import_flag();
    match node_options {
        None | Some("") => flag.to_string(),
        Some(node_options) if node_options.contains(flag) => node_options.to_string(),
        Some(node_options) => format!("{node_options} {flag}"),
    }
}

/// Reapplies the loader flag after `nodeOptions` from config replaces a
/// previously built `NODE_OPTIONS` value that carried it.
#[must_use]
pub fn keep_esm_node_path_loader_option(
    node_options: &str,
    previous_node_options: Option<&str>,
) -> String {
    let carried_flag = previous_node_options.is_some_and(|previous| {
        previous.contains(esm_node_path_loader_import_flag())
    });
    if carried_flag {
        add_esm_node_path_loader_option(Some(node_options))
    } else {
        node_options.to_string()
    }
}

#[cfg(test)]
mod tests;
