import type { SupportedArchitectures } from '@pnpm/types'
import { findWorkspaceProjects } from '@pnpm/workspace.projects-reader'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

import { filterProjectsBySelectorObjects, type ProjectSelector, type ReadProjectsResult } from './index.js'

export async function filterProjectsBySelectorObjectsFromDir (
  workspaceDir: string,
  projectSelectors: ProjectSelector[],
  opts?: {
    engineStrict?: boolean
    linkWorkspacePackages?: boolean
    changedFilesIgnorePattern?: string[]
    supportedArchitectures?: SupportedArchitectures
  }
): Promise<ReadProjectsResult> {
  const workspaceManifest = await readWorkspaceManifest(workspaceDir)
  const allProjects = await findWorkspaceProjects(workspaceDir, {
    patterns: workspaceManifest?.packages,
    engineStrict: opts?.engineStrict,
    supportedArchitectures: opts?.supportedArchitectures ?? {
      os: ['current'],
      cpu: ['current'],
      libc: ['current'],
    },
  })
  const { allProjectsGraph, selectedProjectsGraph } = await filterProjectsBySelectorObjects(
    allProjects,
    projectSelectors,
    {
      linkWorkspacePackages: opts?.linkWorkspacePackages,
      workspaceDir,
      changedFilesIgnorePattern: opts?.changedFilesIgnorePattern,
    }
  )
  return { allProjects, allProjectsGraph, selectedProjectsGraph }
}
