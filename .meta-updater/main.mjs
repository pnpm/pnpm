import fs from 'node:fs'
import path from 'node:path'

import { createFormat, createUpdateOptions } from '@pnpm/meta-updater'

import { NPM_SIGNING_KEYS_FILE, renderNpmSigningKeysModule, replaceRustNpmSigningKeys } from './npmSigningKeys.mjs'

export default async (workspaceDir) => {
  // The Rust products embed the versions their release builds report and
  // verify. `pnpm version -r` bumps only the npm wrapper manifests, so those
  // are the source of truth; the Rust-source handlers below mirror them, and
  // `meta-updater --test` (run in pre-push and CI) fails a missed sync before
  // it can reach the release workflow.
  const rustCliVersion = readJson(workspaceDir, 'pnpm/npm/pnpm/package.json').version
  const pnprVersion = readJson(workspaceDir, 'pnpr/npm/pnpr/package.json').version
  const npmSigningKeys = readJson(workspaceDir, NPM_SIGNING_KEYS_FILE)
  return createUpdateOptions({
    formats: {
      '.rs': textFormat,
      '.toml': textFormat,
      '.lock': textFormat,
      '.ts': textFormat,
    },
    files: {
      '.config/cspell.json': (cspell) => {
        if (cspell && typeof cspell === 'object' && Array.isArray(cspell.words)) {
          cspell.words = cspell.words.sort((w1, w2) => w1.localeCompare(w2))
        }
        return cspell
      },
      'pnpm/crates/config/src/defaults.rs': (content) =>
        replaceVersionLiteral(content, /(pub const PNPM_VERSION: &str = )"[^"]*"/, `$1"${rustCliVersion}"`),
      'pnpr/crates/pnpr/Cargo.toml': (content) =>
        replaceVersionLiteral(content, /^(version\s*=\s*)"[^"]*"/m, `$1"${pnprVersion}"`),
      'Cargo.lock': (content) =>
        replaceVersionLiteral(content, /(\[\[package\]\]\nname = "pnpr"\nversion = )"[^"]*"/, `$1"${pnprVersion}"`),
      // The npm signing keys each product pins, from the one committed list.
      // pnpm keeps trusting the keys npm withdrew; get-pnpm, an installer of
      // current releases only, does not.
      'pnpm/npm/get-pnpm/src/npmSigningKeys.ts': (content) =>
        content == null ? content : renderNpmSigningKeysModule(npmSigningKeys, { currentOnly: true }),
      'pnpm/crates/cli/src/cli_args/self_update/verify_engine/signatures.rs': (content) =>
        replaceRustNpmSigningKeys(content, npmSigningKeys),
    },
  })
}

function readJson (workspaceDir, relativePath) {
  return JSON.parse(fs.readFileSync(path.join(workspaceDir, relativePath), 'utf8'))
}

// Rewrites a version literal in a source file. `meta-updater` passes `null` for
// the packages that don't have the file, which are left untouched; a present
// file that has lost the expected pattern is a hard error so a renamed constant
// can't silently stop being synced.
function replaceVersionLiteral (content, pattern, replacement) {
  if (content == null) return content
  if (!pattern.test(content)) {
    throw new Error(`Pattern ${pattern} not found`)
  }
  return content.replace(pattern, replacement)
}

const textFormat = createFormat({
  read: ({ resolvedPath }) => fs.readFileSync(resolvedPath, 'utf8'),
  update: (actual, updater, options) => updater(actual, options),
  equal: (expected, actual) => expected === actual,
  write: (expected, { resolvedPath }) => {
    fs.writeFileSync(resolvedPath, expected)
  },
})
