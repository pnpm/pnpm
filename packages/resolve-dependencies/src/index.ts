import { LocalPackages, Resolution } from '@pnpm/resolver-base'
import {
  PackageJson,
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import { createNodeId, nodeIdContainsSequence, ROOT_NODE_ID, WantedDependency } from '@pnpm/utils'
import { StoreController } from 'package-store'
import { Shrinkwrap } from 'pnpm-shrinkwrap'
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
export { InstallCheckLog, DeprecationLog } from './loggers'

export interface Importer {
  id: string,
  modulesDir: string,
  nonLinkedPackages: WantedDependency[],
  pkg?: PackageJson,
  prefix: string,
  shamefullyFlatten: boolean,
}

export default async function (
  opts: {
    currentShrinkwrap: Shrinkwrap,
    depth: number,
    dryRun: boolean,
    engineStrict: boolean,
    force: boolean,
    importers: Importer[],
    hooks: {
      readPackage?: ReadPackageHook,
    },
    nodeVersion: string,
    registries: Registries,
    pnpmVersion: string,
    sideEffectsCache: boolean,
    preferredVersions?: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
    skipped: Set<string>,
    storeController: StoreController,
    tag: string,
    verifyStoreIntegrity: boolean,
    virtualStoreDir: string,
    wantedShrinkwrap: Shrinkwrap,
    update: boolean,
    hasManifestInShrinkwrap: boolean,
    localPackages: LocalPackages,
  },
) {
  const directNonLinkedDepsByImporterId = {} as {[id: string]: PkgAddress[]}
  const linkedDependenciesByImporterId = {}

  const ctx = {
    childrenByParentId: {} as ChildrenByParentId,
    currentShrinkwrap: opts.currentShrinkwrap,
    defaultTag: opts.tag,
    dependenciesTree: {} as DependenciesTree,
    depth: opts.depth,
    dryRun: opts.dryRun,
    engineStrict: opts.engineStrict,
    force: opts.force,
    nodeVersion: opts.nodeVersion,
    outdatedDependencies: {} as {[pkgId: string]: string},
    pendingNodes: [] as PendingNode[],
    pnpmVersion: opts.pnpmVersion,
    registries: opts.registries,
    resolvedPackagesByPackageId: {} as ResolvedPackagesByPackageId,
    skipped: opts.skipped,
    storeController: opts.storeController,
    verifyStoreIntegrity: opts.verifyStoreIntegrity,
    virtualStoreDir: opts.virtualStoreDir,
    wantedShrinkwrap: opts.wantedShrinkwrap,
  }

  await Promise.all(opts.importers.map(async (importer) => {
    const shrImporter = opts.wantedShrinkwrap.importers[importer.id]
    const linkedDependencies = [] as LinkedDependency[]
    directNonLinkedDepsByImporterId[importer.id] = await resolveDependencies(
      {
        ...ctx,
        linkedDependencies,
        modulesDir: importer.modulesDir,
        preferredVersions: opts.preferredVersions || importer.pkg && getPreferredVersionsFromPackage(importer.pkg) || {},
        prefix: importer.prefix,
      },
      importer.nonLinkedPackages,
      {
        currentDepth: 0,
        hasManifestInShrinkwrap: opts.hasManifestInShrinkwrap,
        keypath: [],
        localPackages: opts.localPackages,
        parentNodeId: ROOT_NODE_ID,
        readPackageHook: opts.hooks.readPackage,
        resolvedDependencies: {
          ...shrImporter.dependencies,
          ...shrImporter.devDependencies,
          ...shrImporter.optionalDependencies,
        },
        shamefullyFlatten: importer.shamefullyFlatten,
        sideEffectsCache: opts.sideEffectsCache,
        update: opts.update,
      },
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
