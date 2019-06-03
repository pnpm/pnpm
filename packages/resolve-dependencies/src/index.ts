import { Lockfile } from '@pnpm/lockfile-types'
import { LocalPackages, Resolution } from '@pnpm/resolver-base'
import { StoreController } from '@pnpm/store-controller-types'
import {
  ImporterManifest,
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import {
  createNodeId,
  getWantedDependencies,
  nodeIdContainsSequence,
  ROOT_NODE_ID,
  WantedDependency,
} from '@pnpm/utils'
import getPreferredVersionsFromPackage from './getPreferredVersions'
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
  nonLinkedPackages: WantedDependency[],
  manifest?: ImporterManifest,
  prefix: string,
  shamefullyFlatten: boolean,
}

export default async function (
  opts: {
    currentLockfile: Lockfile,
    dryRun: boolean,
    engineStrict: boolean,
    force: boolean,
    importers: Importer[],
    hooks: {
      readPackage?: ReadPackageHook,
    },
    nodeVersion: string,
    registries: Registries,
    resolutionStrategy?: 'fast' | 'fewer-dependencies',
    pnpmVersion: string,
    sideEffectsCache: boolean,
    lockfileDirectory: string,
    preferredVersions?: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
    storeController: StoreController,
    tag: string,
    virtualStoreDir: string,
    wantedLockfile: Lockfile,
    updateDepth?: number,
    hasManifestInLockfile: boolean,
    localPackages: LocalPackages,
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
    hasManifestInLockfile: opts.hasManifestInLockfile,
    lockfileDirectory: opts.lockfileDirectory,
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
    virtualStoreDir: opts.virtualStoreDir,
    wantedLockfile: opts.wantedLockfile,
  }

  await Promise.all(opts.importers.map(async (importer) => {
    const lockfileImporter = opts.wantedLockfile.importers[importer.id]
    const linkedDependencies = [] as LinkedDependency[]
    const resolveCtx = {
      ...ctx,
      linkedDependencies,
      modulesDir: importer.modulesDir,
      prefix: importer.prefix,
      resolutionStrategy: opts.resolutionStrategy || 'fast',
    }
    const resolveOpts = {
      currentDepth: 0,
      localPackages: opts.localPackages,
      parentDependsOnPeers: true,
      parentNodeId: ROOT_NODE_ID,
      preferredVersions: opts.preferredVersions || importer.manifest && getPreferredVersionsFromPackage(importer.manifest) || {},
      resolvedDependencies: {
        ...lockfileImporter.dependencies,
        ...lockfileImporter.devDependencies,
        ...lockfileImporter.optionalDependencies,
      },
      updateDepth: importer.shamefullyFlatten ? Infinity : (typeof opts.updateDepth === 'number' ? opts.updateDepth : -1),
    }
    // TODO: only new dependencies should have big depth.
    // We know which are new. Those that are not in the manifest
    directNonLinkedDepsByImporterId[importer.id] = await resolveDependencies(
      resolveCtx,
      (!importer.manifest
        ? importer.nonLinkedPackages
        : [
          ...importer.nonLinkedPackages.map((wantedDep) => !wantedDep['isNew'] ? { ...wantedDep, updateDepth: -1 } : wantedDep),
          ...getWantedDependencies(importer.manifest)
            .filter((wantedDep) => {
              return importer.nonLinkedPackages.every((nonLinked) => nonLinked.alias !== wantedDep.alias)
            })
            .map((wantedDep) => ({ ...wantedDep, updateDepth: -1 }))
        ]),
      resolveOpts,
    )
    // This if should be removed.
    // if (!importer.manifest) {
    //   directNonLinkedDepsByImporterId[importer.id] = newDirectDeps
    // } else {
    //   directNonLinkedDepsByImporterId[importer.id] = [
    //     ...newDirectDeps,
    //     ...await resolveDependencies(
    //       {
    //         ...resolveCtx,
    //         updateDepth: -1,
    //       },
    //       getWantedDependencies(importer.manifest)
    //         .filter((wantedDep) => {
    //           return newDirectDeps.every((newDep) => newDep.alias !== wantedDep.alias)
    //             && importer.nonLinkedPackages.some((nonLinked) => nonLinked.alias === wantedDep.alias)
    //         }),
    //       {
    //         ...resolveOpts,
    //       },
    //     ),
    //   ]
    // }
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

  for (const importer of opts.importers) {
    const directNonLinkedDeps = directNonLinkedDepsByImporterId[importer.id]
    const linkedDependencies = linkedDependenciesByImporterId[importer.id]

    resolvedImporters[importer.id] = {
      directDependencies: [
        ...directNonLinkedDeps
          .map((dependency) => ({
            ...ctx.dependenciesTree[dependency.nodeId].resolvedPackage,
            alias: dependency.alias,
            normalizedPref: dependency.normalizedPref,
            specRaw: dependency.specRaw,
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
