import path from 'node:path'

import { checkVersioningInvariants } from '@pnpm/releasing.versioning'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

// CI/pre-push guard: the `versioning.epics` bands and `versioning.fixed`
// lockstep are only enforced by the release engine when a package actually
// releases, so a committed version that drifted out of band — or a fixed group
// that fell out of sync — would otherwise slip through until a release happens
// to touch it. This validates the whole tree up front and fails the build.
const repoRoot = path.resolve(import.meta.dirname, '../../../../')

async function main (): Promise<void> {
  const workspace = await readWorkspaceManifest(repoRoot)
  const projects = await findWorkspaceProjectsNoCheck(repoRoot, { patterns: workspace?.packages })
  const violations = checkVersioningInvariants({
    workspaceDir: repoRoot,
    projects: projects.map(({ rootDir, manifest }) => ({ rootDir, manifest })),
    versioning: workspace?.versioning,
  })
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

await main()
