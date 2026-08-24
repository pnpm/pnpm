import type { ProjectRootDir } from '@pnpm/types'
import { createProjectsGraph } from '@pnpm/workspace.projects-graph'
import { findWorkspaceProjectsNoCheck } from '@pnpm/workspace.projects-reader'
import { sortProjects } from '@pnpm/workspace.projects-sorter'

import { publishedName } from '../publishedNames.js'
import type { ApprovalItem, StageOptions } from './types.js'

/**
 * Where each workspace package sits in the order its siblings have to be
 * published in, keyed by the name the package publishes under — the only name
 * a staged version carries.
 */
export interface WorkspaceApprovalOrder {
  /**
   * Index of the topological chunk a package belongs to. A package only ever
   * depends on packages in lower-indexed chunks, so approving in ascending
   * index order publishes every dependency before its dependents.
   */
  chunkIndexByPackageName: Map<string, number>
  /** The workspace siblings a package directly depends on. */
  dependencyNamesByPackageName: Map<string, string[]>
}

/**
 * Reads the workspace the command runs in and derives the order its packages
 * have to be approved in.
 *
 * Returns `undefined` outside a workspace, where nothing is known about how
 * the staged versions relate and the selection order is kept as is.
 */
export async function readWorkspaceApprovalOrder (opts: StageOptions): Promise<WorkspaceApprovalOrder | undefined> {
  if (!opts.workspaceDir) return undefined
  const projects = await findWorkspaceProjectsNoCheck(opts.workspaceDir, {
    patterns: opts.workspacePackagePatterns,
  })
  const { graph } = createProjectsGraph(projects, {
    linkWorkspacePackages: Boolean(opts.linkWorkspacePackages),
  })
  const publishedNameByRootDir = new Map<ProjectRootDir, string>()
  for (const rootDir of Object.keys(graph) as ProjectRootDir[]) {
    const name = publishedName(graph[rootDir].package.manifest)
    if (name) publishedNameByRootDir.set(rootDir, name)
  }
  const chunkIndexByPackageName = new Map<string, number>()
  const dependencyNamesByPackageName = new Map<string, string[]>()
  sortProjects(graph).forEach((chunk, chunkIndex) => {
    for (const rootDir of chunk) {
      const name = publishedNameByRootDir.get(rootDir)
      if (!name) continue
      chunkIndexByPackageName.set(name, chunkIndex)
      dependencyNamesByPackageName.set(
        name,
        graph[rootDir].dependencies
          .map((dependencyRootDir) => publishedNameByRootDir.get(dependencyRootDir))
          .filter((dependencyName) => dependencyName != null)
      )
    }
  })
  return { chunkIndexByPackageName, dependencyNamesByPackageName }
}

/**
 * Orders staged versions so that a workspace package is approved after the
 * workspace packages it depends on. Staged versions of packages outside the
 * workspace keep their original relative order, after the workspace ones.
 */
export function sortStageItemsForApproval (items: ApprovalItem[], order?: WorkspaceApprovalOrder): ApprovalItem[] {
  return items
    .map((item, index) => ({ item, index, chunkIndex: chunkIndexOf(item, order) }))
    .sort((left, right) => left.chunkIndex - right.chunkIndex || left.index - right.index)
    .map(({ item }) => item)
}

/**
 * The packages in `unpublishedPackageNames` that `item` depends on, and that
 * therefore will not be on the registry by the time `item` would be approved.
 */
export function unavailableDependencies (
  item: ApprovalItem,
  unpublishedPackageNames: Set<string>,
  order?: WorkspaceApprovalOrder
): string[] {
  if (!order || !item.packageName) return []
  return (order.dependencyNamesByPackageName.get(item.packageName) ?? [])
    .filter((dependencyName) => unpublishedPackageNames.has(dependencyName))
}

function chunkIndexOf (item: ApprovalItem, order?: WorkspaceApprovalOrder): number {
  const chunkIndex = item.packageName ? order?.chunkIndexByPackageName.get(item.packageName) : undefined
  return chunkIndex ?? Number.MAX_SAFE_INTEGER
}
