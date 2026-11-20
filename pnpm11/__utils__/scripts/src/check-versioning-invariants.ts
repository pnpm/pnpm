import path from 'node:path'
import util from 'node:util'

import { checkPendingRelease, describeCheckedIntents } from '@pnpm/releasing.versioning'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

// CI/pre-push guard, running what `pnpm change check` runs, from this
// repository's sources instead of the released CLI: a pending intent that
// names a package no longer in the workspace, and a committed version that
// drifted out of its epic band or out of lockstep with its fixed group, would
// otherwise surface only when a release consumes them. This validates the
// whole tree up front and fails the build.
const repoRoot = path.resolve(import.meta.dirname, '../../../../')

async function main (): Promise<void> {
  const workspace = await readWorkspaceManifest(repoRoot)
  const projects = await findWorkspaceProjectsNoCheck(repoRoot, { patterns: workspace?.packages })
  const { intentCount, violations } = await checkPendingRelease({
    workspaceDir: repoRoot,
    projects: projects.map(({ rootDir, manifest }) => ({ rootDir, manifest })),
    versioning: workspace?.versioning,
  })
  console.log(describeCheckedIntents(intentCount))
  if (violations.length === 0) {
    console.log('All package versions satisfy the configured versioning invariants.')
    return
  }
  console.error(`Found ${violations.length} versioning invariant violation${violations.length === 1 ? '' : 's'}:`)
  for (const violation of violations) {
    console.error(`  - ${violation.message}`)
  }
  process.exitCode = 1
}

// The stack trace of a malformed intent or versioning setting points into the
// release engine, never at the file that has to change, so print the message
// alone.
await main().catch((err: unknown) => {
  console.error(util.types.isNativeError(err) ? err.message : err)
  process.exitCode = 1
})
