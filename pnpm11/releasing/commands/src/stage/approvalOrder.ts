import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import npa from '@pnpm/npm-package-arg'
import type { BaseManifest, ProjectRootDir } from '@pnpm/types'
import { createProjectsGraph } from '@pnpm/workspace.projects-graph'

import { readTarballManifest, type TarballManifest } from '../tarball/summarizeTarball.js'
import type { StageContext } from './context.js'
import { fetchStageTarball } from './tarball.js'
import type { ApprovalItem } from './types.js'

type DependencyField = 'dependencies' | 'devDependencies' | 'optionalDependencies' | 'peerDependencies'

const DEPENDENCY_FIELDS: DependencyField[] = [
  'peerDependencies',
  'devDependencies',
  'optionalDependencies',
  'dependencies',
]

/** The dependency order derived from the exact tarballs being approved. */
export interface StageApprovalOrder {
  dependencyStageIdsByStageId: Map<string, string[]>
  orderIndexByStageId: Map<string, number>
  packageNameByStageId: Map<string, string>
}

/**
 * Downloads every selected staged package before approval and derives their
 * dependency graph from the package.json files that will reach the registry.
 */
export async function readStageApprovalOrder (
  context: StageContext,
  items: ApprovalItem[]
): Promise<StageApprovalOrder> {
  const projects: Array<{ manifest: BaseManifest, rootDir: ProjectRootDir }> = []
  for (const item of items) {
    // eslint-disable-next-line no-await-in-loop
    const tarball = await fetchStageTarball(context, item.id)
    // eslint-disable-next-line no-await-in-loop
    const manifest = await readTarballManifest(tarball)
    projects.push({
      manifest: manifestForGraph(manifest),
      rootDir: item.id as ProjectRootDir,
    })
  }
  const { graph } = createProjectsGraph(projects, { linkWorkspacePackages: true })
  const orderIndexByStageId = new Map<string, number>()
  const dependencyStageIdsByStageId = new Map<string, string[]>()
  const packageNameByStageId = new Map<string, string>()
  const dependencies = new Map<ProjectRootDir, ProjectRootDir[]>(
    Object.entries(graph).map(([rootDir, node]) => [rootDir as ProjectRootDir, node.dependencies])
  )
  graphSequencer(dependencies).order.forEach((rootDir, orderIndex) => {
    orderIndexByStageId.set(rootDir, orderIndex)
    dependencyStageIdsByStageId.set(rootDir, graph[rootDir].dependencies)
    const packageName = graph[rootDir].package.manifest.name
    if (packageName) packageNameByStageId.set(rootDir, packageName)
  })
  return { dependencyStageIdsByStageId, orderIndexByStageId, packageNameByStageId }
}

/** Approve staged dependencies before the selected packages that need them. */
export function sortStageItemsForApproval (items: ApprovalItem[], order: StageApprovalOrder): ApprovalItem[] {
  return items
    .map((item, index) => ({ item, index, orderIndex: order.orderIndexByStageId.get(item.id) ?? Number.MAX_SAFE_INTEGER }))
    .sort((left, right) => left.orderIndex - right.orderIndex || left.index - right.index)
    .map(({ item }) => item)
}

/** Selected staged dependencies of `item` whose approval failed or was skipped. */
export function unavailableDependencies (
  item: ApprovalItem,
  unpublishedStageIds: Set<string>,
  order: StageApprovalOrder
): string[] {
  return (order.dependencyStageIdsByStageId.get(item.id) ?? [])
    .filter((stageId) => unpublishedStageIds.has(stageId))
    .map((stageId) => order.packageNameByStageId.get(stageId) ?? stageId)
}

function manifestForGraph (manifest: TarballManifest): BaseManifest {
  const graphManifest: BaseManifest = {
    name: manifest.name,
    version: manifest.version,
  }
  for (const field of DEPENDENCY_FIELDS) {
    const dependencies = manifest[field]
    if (!dependencies || typeof dependencies !== 'object') continue
    const normalized: Record<string, string> = {}
    for (const [name, spec] of Object.entries(dependencies)) {
      if (typeof spec !== 'string') continue
      const parsed = parseRegistryDependency(name, spec)
      normalized[parsed.name] = parsed.spec
    }
    graphManifest[field] = normalized
  }
  return graphManifest
}

function parseRegistryDependency (name: string, spec: string): { name: string, spec: string } {
  try {
    const parsed = npa.resolve(name, spec, process.cwd())
    const registrySpec = parsed.type === 'alias' ? parsed.subSpec : parsed
    if (
      registrySpec?.name &&
      (registrySpec.type === 'version' || registrySpec.type === 'range') &&
      typeof registrySpec.fetchSpec === 'string'
    ) {
      return { name: registrySpec.name, spec: registrySpec.fetchSpec }
    }
  } catch {}
  return { name, spec }
}
