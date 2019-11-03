import { Lockfile } from '@pnpm/lockfile-types'
import { LocalPackages, Resolution } from '@pnpm/resolver-base'
import { StoreController } from '@pnpm/store-controller-types'
import {
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import {
  createNodeId,
  nodeIdContainsSequence,
  WantedDependency,
} from '@pnpm/utils'
import resolveDependencies, {
  ChildrenByParentId,
  DependenciesTree,
  LinkedDependency,
  PendingNode,
  PkgAddress,
  ResolvedPackagesByPackageId,
} from './resolveDependencies'

export { LinkedDependency, ResolvedPackage, DependenciesTree, DependenciesTreeNode } from './resolveDependencies'

export interface Importer {
  id: string,
  modulesDir: string,
  preferredVersions?: {
    [packageName: string]: {
      selector: string,
      type: 'version' | 'range' | 'tag',
    },
  },
  rootDir: string,
  wantedDependencies: Array<WantedDependency & { updateDepth: number }>,
}

export default async function (
  importers: Importer[],
  opts: {
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
    localPackages: LocalPackages,
    updateLockfile: boolean,
  },
) {
  const directNonLinkedDepsByImporterId = {} as {[id: string]: PkgAddress[]}
  const linkedDependenciesByImporterId = {}

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
    const lockfileImporter = opts.wantedLockfile.importers[importer.id]
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
      currentDepth: 0,
      localPackages: opts.localPackages,
      parentDependsOnPeers: true,
      parentNodeId: `>${importer.id}>`,
      preferredVersions: importer.preferredVersions || {},
      proceed: true,
      resolvedDependencies: {
        ...lockfileImporter.dependencies,
        ...lockfileImporter.devDependencies,
        ...lockfileImporter.optionalDependencies,
      },
      updateDepth: -1,
    }
    directNonLinkedDepsByImporterId[importer.id] = await resolveDependencies(
      resolveCtx,
      importer.wantedDependencies,
      resolveOpts,
    )
    linkedDependenciesByImporterId[importer.id] = linkedDependencies
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
      directDependencies: Array<{
        alias: string,
        optional: boolean,
        dev: boolean,
        resolution: Resolution,
        id: string,
        version: string,
        name: string,
        specRaw: string,
        normalizedPref?: string,
      }>,
      directNodeIdsByAlias: {
        [alias: string]: string,
      },
      linkedDependencies: LinkedDependency[],
    },
  }

  for (const { id } of importers) {
    const directNonLinkedDeps = directNonLinkedDepsByImporterId[id]
    const linkedDependencies = linkedDependenciesByImporterId[id]

    resolvedImporters[id] = {
      directDependencies: [
        ...directNonLinkedDeps
          .map(({ alias, nodeId, normalizedPref, specRaw }) => ({
            ...ctx.dependenciesTree[nodeId].resolvedPackage,
            alias,
            normalizedPref,
            specRaw,
          })) as Array<{
            alias: string,
            optional: boolean,
            dev: boolean,
            resolution: Resolution,
            id: string,
            version: string,
            name: string,
            specRaw: string,
            normalizedPref?: string,
          }>,
        ...linkedDependencies,
      ],
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
  installable: boolean,
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
