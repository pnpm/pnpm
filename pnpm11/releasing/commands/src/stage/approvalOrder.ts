import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import { PnpmError } from '@pnpm/error'
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
 * Returns dependency, order, and display-name mappings keyed by the supplied
 * stage IDs. Every selected tarball is fetched and validated before the
 * mappings are returned; request and manifest errors abort the preflight. An
 * empty selection returns empty mappings without making a request.
 */
export async function readStageApprovalOrder (
  context: StageContext,
  items: ApprovalItem[]
): Promise<StageApprovalOrder> {
  const projects: Array<{ manifest: BaseManifest, rootDir: ProjectRootDir }> = []
  const stageIdByVersionByPackageName = new Map<string, Map<string, string>>()
  for (const item of items) {
    // eslint-disable-next-line no-await-in-loop
    const tarball = await fetchStageTarball(context, item.id)
    // eslint-disable-next-line no-await-in-loop
    const manifest = await readTarballManifest(tarball)
    const stageIdByVersion = stageIdByVersionByPackageName.get(manifest.name) ?? new Map<string, string>()
    const duplicateStageId = stageIdByVersion.get(manifest.version)
    if (duplicateStageId != null) {
      throw new PnpmError(
        'STAGE_DUPLICATE_PACKAGE',
        `Cannot approve stages ${duplicateStageId} and ${item.id} together because both publish ${manifest.name}@${manifest.version}`
      )
    }
    stageIdByVersion.set(manifest.version, item.id)
    stageIdByVersionByPackageName.set(manifest.name, stageIdByVersion)
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
  if (!spec.startsWith('npm:')) return { name, spec }
  const parsed = npa.resolve(name, spec, process.cwd())
  return (parsed.type === 'version' || parsed.type === 'range') &&
    typeof parsed.name === 'string' &&
    typeof parsed.fetchSpec === 'string'
    ? { name: parsed.name, spec: parsed.fetchSpec }
    : { name, spec }
}
