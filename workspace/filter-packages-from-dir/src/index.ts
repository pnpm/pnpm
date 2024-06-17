import { type SupportedArchitectures } from '@pnpm/types'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { filterPkgsBySelectorObjects, type PackageSelector, type ReadProjectsResult } from '@pnpm/filter-workspace-packages'

export async function filterPackagesFromDir (
  workspaceDir: string,
  pkgSelectors: PackageSelector[],
  opts?: {
    engineStrict?: boolean
    linkWorkspacePackages?: boolean
    changedFilesIgnorePattern?: string[]
    supportedArchitectures?: SupportedArchitectures
  }
): Promise<ReadProjectsResult> {
  const workspaceManifest = await readWorkspaceManifest(workspaceDir)
  const allProjects = await findWorkspacePackages(workspaceDir, {
    patterns: workspaceManifest?.packages,
    engineStrict: opts?.engineStrict,
    supportedArchitectures: opts?.supportedArchitectures ?? {
      os: ['current'],
      cpu: ['current'],
      libc: ['current'],
    },
  })
  const { allProjectsGraph, selectedProjectsGraph } = await filterPkgsBySelectorObjects(
    allProjects,
    pkgSelectors,
    {
      linkWorkspacePackages: opts?.linkWorkspacePackages,
      workspaceDir,
      changedFilesIgnorePattern: opts?.changedFilesIgnorePattern,
    }
  )
  return { allProjects, allProjectsGraph, selectedProjectsGraph }
}
