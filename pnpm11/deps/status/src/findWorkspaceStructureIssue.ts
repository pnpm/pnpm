import { importerIdOf, projectBinsDigest, workspaceBinsDigest } from '@pnpm/workspace.state'
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

export function findWorkspaceBinsIssue (ctx: WorkspaceDepsStatusContext, installCovers: (rootDir: string) => boolean): string | undefined {
  const { allProjects, workspaceState } = ctx
  const lockfileDir = ctx.opts.lockfileDir ?? ctx.workspaceDir
  const digest = workspaceBinsDigest(lockfileDir, allProjects)
  if (allProjects.some(({ rootDir, manifest }) => {
    if (!installCovers(rootDir)) return false
    const importerId = importerIdOf(lockfileDir, rootDir)
    return workspaceState.validation?.workspaceBins?.[importerId] !== projectBinsDigest(importerId, manifest, digest)
  })) {
    return 'workspace package bins changed since the last install'
  }
  return undefined
}
