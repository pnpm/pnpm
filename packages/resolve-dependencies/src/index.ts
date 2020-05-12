import { Lockfile } from '@pnpm/lockfile-types'
import { PreferredVersions, Resolution, WorkspacePackages } from '@pnpm/resolver-base'
import { StoreController } from '@pnpm/store-controller-types'
import {
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import R = require('ramda')
import { WantedDependency } from './getNonDevWantedDependencies'
import {
  createNodeId,
  nodeIdContainsSequence,
} from './nodeIdUtils'
import resolveDependencies, {
  ChildrenByParentId,
  DependenciesTree,
  LinkedDependency,
  PendingNode,
  PkgAddress,
  ResolvedPackagesByPackageId,
} from './resolveDependencies'

export * from './nodeIdUtils'
export { LinkedDependency, ResolvedPackage, DependenciesTree, DependenciesTreeNode } from './resolveDependencies'

export type ResolvedDirectDependency = {
  alias: string,
  optional: boolean,
  dev: boolean,
  resolution: Resolution,
  id: string,
  version: string,
  name: string,
  normalizedPref?: string,
}

export interface Importer {
  id: string,
  modulesDir: string,
  preferredVersions?: PreferredVersions,
  rootDir: string,
  wantedDependencies: Array<WantedDependency & { updateDepth: number }>,
}

export default async function (
  importers: Importer[],
  opts: {
    alwaysTryWorkspacePackages?: boolean,
    currentLockfile: Lockfile,
    dryRun: boolean,
    engineStrict: boolean,
    force: boolean,
    hooks: {
      readPackage?: ReadPackageHook,
    },
    nodeVersion: string,
    registries: Registries,
    resolutionStrategy?: 'fast' | 'fewer-dependencies',
    pnpmVersion: string,
    sideEffectsCache: boolean,
    lockfileDir: string,
    storeController: StoreController,
    tag: string,
    virtualStoreDir: string,
    wantedLockfile: Lockfile,
    updateLockfile: boolean,
    workspacePackages: WorkspacePackages,
  }
) {
  const directDepsByImporterId = {} as {[id: string]: Array<PkgAddress | LinkedDependency>}

  const wantedToBeSkippedPackageIds = new Set<string>()
  const ctx = {
    childrenByParentId: {} as ChildrenByParentId,
    currentLockfile: opts.currentLockfile,
    defaultTag: opts.tag,
    dependenciesTree: {} as DependenciesTree,
    dryRun: opts.dryRun,
    engineStrict: opts.engineStrict,
    force: opts.force,
    lockfileDir: opts.lockfileDir,
    nodeVersion: opts.nodeVersion,
    outdatedDependencies: {} as {[pkgId: string]: string},
    pendingNodes: [] as PendingNode[],
    pnpmVersion: opts.pnpmVersion,
    readPackageHook: opts.hooks.readPackage,
    registries: opts.registries,
    resolvedPackagesByPackageId: {} as ResolvedPackagesByPackageId,
    sideEffectsCache: opts.sideEffectsCache,
    skipped: wantedToBeSkippedPackageIds,
    storeController: opts.storeController,
    updateLockfile: opts.updateLockfile,
    virtualStoreDir: opts.virtualStoreDir,
    wantedLockfile: opts.wantedLockfile,
  }

  await Promise.all(importers.map(async (importer) => {
    const projectSnapshot = opts.wantedLockfile.importers[importer.id]
    // This array will only contain the dependencies that should be linked in.
    // The already linked-in dependencies will not be added.
    const linkedDependencies = [] as LinkedDependency[]
    const resolveCtx = {
      ...ctx,
      linkedDependencies,
      modulesDir: importer.modulesDir,
      prefix: importer.rootDir,
      resolutionStrategy: opts.resolutionStrategy || 'fast',
    }
    const resolveOpts = {
      alwaysTryWorkspacePackages: opts.alwaysTryWorkspacePackages,
      currentDepth: 0,
      parentDependsOnPeers: true,
      parentNodeId: `>${importer.id}>`,
      preferredVersions: importer.preferredVersions || {},
      proceed: true,
      resolvedDependencies: {
        ...projectSnapshot.dependencies,
        ...projectSnapshot.devDependencies,
        ...projectSnapshot.optionalDependencies,
      },
      updateDepth: -1,
      workspacePackages: opts.workspacePackages,
    }
    directDepsByImporterId[importer.id] = await resolveDependencies(
      resolveCtx,
      importer.wantedDependencies,
      resolveOpts
    )
  }))

  ctx.pendingNodes.forEach((pendingNode) => {
    ctx.dependenciesTree[pendingNode.nodeId] = {
      children: () => buildTree(ctx, pendingNode.nodeId, pendingNode.resolvedPackage.id,
        ctx.childrenByParentId[pendingNode.resolvedPackage.id], pendingNode.depth + 1, pendingNode.installable),
      depth: pendingNode.depth,
      installable: pendingNode.installable,
      resolvedPackage: pendingNode.resolvedPackage,
    }
  })

  const resolvedImporters = {} as {
    [id: string]: {
      directDependencies: ResolvedDirectDependency[],
      directNodeIdsByAlias: {
        [alias: string]: string,
      },
      linkedDependencies: LinkedDependency[],
    },
  }

  for (const { id } of importers) {
    const directDeps = directDepsByImporterId[id]
    const [linkedDependencies, directNonLinkedDeps] = R.partition((dep) => dep.isLinkedDependency === true, directDeps) as [LinkedDependency[], PkgAddress[]]

    resolvedImporters[id] = {
      directDependencies: directDeps
        .map((dep) => {
          if (dep.isLinkedDependency === true) {
            return dep
          }
          return {
            ...ctx.dependenciesTree[dep.nodeId].resolvedPackage,
            alias: dep.alias,
            normalizedPref: dep.normalizedPref,
          }
        }) as ResolvedDirectDependency[],
      directNodeIdsByAlias: directNonLinkedDeps
        .reduce((acc, dependency) => {
          acc[dependency.alias] = dependency.nodeId
          return acc
        }, {}),
      linkedDependencies,
    }
  }

  return {
    dependenciesTree: ctx.dependenciesTree,
    outdatedDependencies: ctx.outdatedDependencies,
    resolvedImporters,
    resolvedPackagesByPackageId: ctx.resolvedPackagesByPackageId,
    wantedToBeSkippedPackageIds,
  }
}

function buildTree (
  ctx: {
    childrenByParentId: ChildrenByParentId,
    dependenciesTree: DependenciesTree,
    resolvedPackagesByPackageId: ResolvedPackagesByPackageId,
    skipped: Set<string>,
  },
  parentNodeId: string,
  parentId: string,
  children: Array<{alias: string, pkgId: string}>,
  depth: number,
  installable: boolean
) {
  const childrenNodeIds = {}
  for (const child of children) {
    if (nodeIdContainsSequence(parentNodeId, parentId, child.pkgId)) {
      continue
    }
    const childNodeId = createNodeId(parentNodeId, child.pkgId)
    childrenNodeIds[child.alias] = childNodeId
    installable = installable && !ctx.skipped.has(child.pkgId)
    ctx.dependenciesTree[childNodeId] = {
      children: () => buildTree(ctx, childNodeId, child.pkgId, ctx.childrenByParentId[child.pkgId], depth + 1, installable),
      depth,
      installable,
      resolvedPackage: ctx.resolvedPackagesByPackageId[child.pkgId],
    }
  }
  return childrenNodeIds
}
