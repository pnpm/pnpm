use std::sync::{Arc, Mutex};
use tempfile::TempDir;

use pnpm_hooks::{PnpmfileHooks, finder};

/// Helper: write `source` to a `.pnpmfile.cjs` in a fresh temp dir and return
/// the hooks bridge plus the dir (kept alive for the file's lifetime).
fn cjs_hooks(source: &str) -> (pnpm_hooks::node_runtime::NodeJsHooks, TempDir) {
    let tmp = TempDir::new().expect("temp dir");
    let path = tmp.path().join(".pnpmfile.cjs");
    std::fs::write(&path, source).expect("write pnpmfile");
    (pnpm_hooks::node_runtime::NodeJsHooks::new(path), tmp)
}

async fn read_package_err(source: &str) -> String {
    let (hooks, _tmp) = cjs_hooks(source);
    hooks
        .read_package(
            serde_json::json!({ "name": "foo", "version": "1.0.0" }),
            pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: None },
        )
        .await
        .expect_err("readPackage should fail")
        .to_string()
}

fn noop_context() -> pnpm_hooks::HookContext {
    pnpm_hooks::HookContext { log: Arc::new(|_| {}), dir: None }
}

fn write_custom_resolvers_pnpmfile(dir: &std::path::Path) -> std::path::PathBuf {
    let pnpmfile_path = dir.join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"
module.exports = {
  resolvers: [
    {
      idPrefix: 'custom',
      canResolve (wanted) {
        return typeof wanted.bareSpecifier === 'string' && wanted.bareSpecifier.startsWith('custom:');
      },
      resolve (wanted, opts) {
        return {
          id: `${this.idPrefix}/${wanted.alias}@1.0.0`,
          resolution: { tarball: `https://example.com/${wanted.alias}-1.0.0.tgz` },
          lockfileDir: opts.lockfileDir,
        };
      },
      shouldRefreshResolution (depPath, pkgSnapshot) {
        return depPath.startsWith('refresh-me@') && pkgSnapshot.resolution != null;
      },
    },
    {
      shouldRefreshResolution () {
        throw new Error('refresh check crashed');
      },
    },
  ],
}
",
    )
    .expect("write pnpmfile");
    pnpmfile_path
}

// --- Custom fetcher integration tests ---

fn write_custom_fetchers_pnpmfile(dir: &std::path::Path) -> std::path::PathBuf {
    let pnpmfile_path = dir.join(".pnpmfile.cjs");
    std::fs::write(
        &pnpmfile_path,
        r"
module.exports = {
  fetchers: [
    {
      canFetch (pkgId, resolution) {
        return resolution.type === '@custom/local';
      },
      fetch (cafs, resolution, opts, fetchers) {
        return {
          filesIndex: {
            'package.json': {
              integrity: 'sha512-abc123',
              mode: 420,
            },
            'index.js': {
              integrity: 'sha512-def456',
              mode: 420,
            },
          },
          receivedNullCafs: cafs === null,
          receivedNullFetchers: fetchers === null,
          receivedUrl: resolution.url,
          receivedOpts: opts,
        };
      },
    },
    {
      canFetch () { return false; },
    },
  ],
}
",
    )
    .expect("write pnpmfile");
    pnpmfile_path
}

#[path = "node_hooks/files.rs"]
mod files;

#[path = "node_hooks/integrity.rs"]
mod integrity;

#[path = "node_hooks/runtime.rs"]
mod runtime;

#[path = "node_hooks/lockfile.rs"]
mod lockfile;

#[path = "node_hooks/behavior.rs"]
mod behavior;

#[path = "node_hooks/dependencies.rs"]
mod dependencies;

#[path = "node_hooks/reporting.rs"]
mod reporting;

#[path = "node_hooks/configuration.rs"]
mod configuration;

#[path = "node_hooks/security.rs"]
mod security;
