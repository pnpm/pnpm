// Applies the pending release plan, then runs the meta-updater to mirror the
// bumped Rust wrapper versions into the Rust sources the release builds from.
//
// `pnpm version -r` (native workspace release management) consumes the pending
// `.changeset/*.md` intents: it bumps versions across the workspace, writes
// changelogs, and records consumed intents in the committed `.changeset/
// ledger.yaml`. The ledger keeps cherry-picks and merge-backs between release
// branches safe, and the Rust products' `alpha` release lanes (configured under
// `versioning` in pnpm-workspace.yaml) advance their `X.Y.Z-alpha.N` prerelease
// lines. `pnpm version -r` bumps only the npm wrapper manifests, so the
// meta-updater then copies those versions into the Rust sources that embed
// them (see the Rust-source handlers in `.meta-updater/src/index.ts`);
// `meta-updater --test` in pre-push and CI enforces the same sync.

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
  execSync('pnpm update-manifests', { cwd: repoRoot, stdio: 'inherit' })
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
