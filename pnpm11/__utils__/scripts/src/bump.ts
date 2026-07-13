// Applies the pending release plan and mirrors the bumped Rust wrapper
// versions into the Rust sources the release builds from.
//
// `pnpm version -r` (native workspace release management) consumes the pending
// `.changeset/*.md` intents: it bumps versions across the workspace, writes
// changelogs, and records consumed intents in the committed `.changeset/
// ledger.yaml`. The ledger keeps cherry-picks and merge-backs between release
// branches safe, and the Rust products' `alpha` release lanes (configured under
// `versioning` in pnpm-workspace.yaml) advance their `X.Y.Z-alpha.N` prerelease
// lines. The one job left to this script is `syncRustVersions`: the Rust
// sources embed the versions their release builds report, so the bumped npm
// wrapper versions have to be copied into them.

import { execSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import url from 'node:url'

// The module-level consts are still in their temporal dead zone while this
// file's statements run, so the actual `main()` call sits at the bottom.
function main (): void {
  const repoRoot = findRepoRoot(import.meta.dirname)
  // The release PR branch is dirty here (refreshed trust roots, synthesized
  // changesets), so skip the clean-tree check.
  execSync('pnpm version -r --no-git-checks', { cwd: repoRoot, stdio: 'inherit' })
  syncRustVersions(repoRoot)
}

// The npm wrapper manifests of the Rust products, versioned by the release
// plan; their versions are mirrored into Rust sources by syncRustVersions.
export const RUST_CLI_WRAPPER_MANIFEST = 'pnpm/npm/pnpm/package.json'
export const PNPR_WRAPPER_MANIFEST = 'pnpr/npm/pnpr/package.json'

// The Rust sources embed the versions their release builds report; the npm
// wrapper manifests bumped by `pnpm version -r` are the source of truth. The
// release workflow verifies the copies match, so a missed sync fails the
// release instead of shipping a binary with a stale --version.
export function syncRustVersions (repoRoot: string): void {
  const pnpmVersion = readManifestVersion(path.join(repoRoot, RUST_CLI_WRAPPER_MANIFEST))
  replaceInFile(
    path.join(repoRoot, 'pnpm/crates/config/src/defaults.rs'),
    /(pub const PNPM_VERSION: &str = )"[^"]*"/,
    `$1"${pnpmVersion}"`
  )
  const pnprVersion = readManifestVersion(path.join(repoRoot, PNPR_WRAPPER_MANIFEST))
  replaceInFile(
    path.join(repoRoot, 'pnpr/crates/pnpr/Cargo.toml'),
    /^(version\s*=\s*)"[^"]*"/m,
    `$1"${pnprVersion}"`
  )
  replaceInFile(
    path.join(repoRoot, 'Cargo.lock'),
    /(\[\[package\]\]\nname = "pnpr"\nversion = )"[^"]*"/,
    `$1"${pnprVersion}"`
  )
}

function readManifestVersion (absManifestPath: string): string {
  const manifest = JSON.parse(fs.readFileSync(absManifestPath, 'utf8'))
  if (typeof manifest.version !== 'string') {
    throw new Error(`No version field in ${absManifestPath}`)
  }
  return manifest.version
}

function replaceInFile (file: string, pattern: RegExp, replacement: string): void {
  const content = fs.readFileSync(file, 'utf8')
  if (!pattern.test(content)) {
    throw new Error(`Pattern ${pattern} not found in ${file}`)
  }
  fs.writeFileSync(file, content.replace(pattern, replacement))
}

export function findRepoRoot (startDir: string): string {
  let dir = startDir
  while (!fs.existsSync(path.join(dir, '.changeset'))) {
    const parent = path.dirname(dir)
    if (parent === dir) {
      throw new Error(`No .changeset directory found above ${startDir}`)
    }
    dir = parent
  }
  return dir
}

function isDirectInvocation (): boolean {
  if (process.argv[1] === undefined) return false
  try {
    return import.meta.url === url.pathToFileURL(fs.realpathSync(process.argv[1])).href
  } catch {
    return false
  }
}

if (isDirectInvocation()) {
  main()
}
