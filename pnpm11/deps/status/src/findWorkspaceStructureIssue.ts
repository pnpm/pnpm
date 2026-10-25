import { importerIdOf, projectBinsDigest } from '@pnpm/workspace.state'
import { equals, filter } from 'ramda'

import type { WorkspaceDepsStatusContext } from './checkWorkspaceDepsStatus.js'

export function findWorkspaceStructureIssue ({ allProjects, opts, workspaceState }: WorkspaceDepsStatusContext): string | undefined {
  if (!equals(
    filter(value => value != null, workspaceState.settings.catalogs ?? {}),
    filter(value => value != null, opts.catalogs ?? {})
  )) {
    return 'Catalogs cache outdated'
  }
  if (allProjects.length !== Object.keys(workspaceState.projects).length ||
    !allProjects.every((currentProject) => {
      const prevProject = workspaceState.projects[currentProject.rootDir]
      if (!prevProject) return false
      return prevProject.name === currentProject.manifest.name && (prevProject.version ?? '0.0.0') === (currentProject.manifest.version ?? '0.0.0')
    })
  ) {
    return 'The workspace structure has changed since last install'
  }
  return undefined
}

/**
 * Whether a project the install covers links a workspace bin declaration
 * that differs from what it was proven installed with. `workspaceBins` is the
 * digest of the current declarations across the workspace. A state without a
 * validation record proves nothing about any project, so it raises no issue
 * and leaves the verdict to the checks that handle such a state.
 */
export function findWorkspaceBinsIssue (
  ctx: WorkspaceDepsStatusContext,
  installCovers: (rootDir: string) => boolean,
  workspaceBins: string | undefined
): string | undefined {
  const { allProjects, workspaceState: { validation } } = ctx
  if (validation == null) return undefined
  const lockfileDir = ctx.opts.lockfileDir ?? ctx.workspaceDir
  if (allProjects.some(({ rootDir, manifest }) => {
    if (!installCovers(rootDir)) return false
    const importerId = importerIdOf(lockfileDir, rootDir)
    return validation.workspaceBins?.[importerId] !== projectBinsDigest(importerId, manifest, workspaceBins)
  })) {
    return 'workspace package bins changed since the last install'
  }
  return undefined
}
