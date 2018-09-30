import { LocalPackages } from '@pnpm/resolver-base'
import { PackageJson, ReadPackageHook } from '@pnpm/types'
import { createNodeId, nodeIdContainsSequence, ROOT_NODE_ID, WantedDependency } from '@pnpm/utils'
import { StoreController } from 'package-store'
import { Shrinkwrap } from 'pnpm-shrinkwrap'
import getPreferredVersionsFromPackage from './getPreferredVersions'
import resolveDependencies, { ResolutionContext } from './resolveDependencies'

export { ResolvedPackage, DependenciesGraph, DependenciesGraphNode } from './resolveDependencies'
export { InstallCheckLog, DeprecationLog } from './loggers'

export default async function (
  opts: {
    currentShrinkwrap: Shrinkwrap,
    depth: number,
    dryRun: boolean,
    engineStrict: boolean,
    force: boolean,
    importerPath: string,
    hooks: {
      readPackage?: ReadPackageHook,
    },
    nodeVersion: string,
    nonLinkedPackages: WantedDependency[],
    rawNpmConfig: object,
    pkg?: PackageJson,
    pnpmVersion: string,
    sideEffectsCache: boolean,
    shamefullyFlatten: boolean,
    preferredVersions?: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
    prefix: string,
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
  const preferredVersions = opts.preferredVersions || opts.pkg && getPreferredVersionsFromPackage(opts.pkg) || {}

  const ctx: ResolutionContext = {
    childrenByParentId: {},
    currentShrinkwrap: opts.currentShrinkwrap,
    defaultTag: opts.tag,
    dependenciesGraph: {},
    depth: opts.depth,
    dryRun: opts.dryRun,
    engineStrict: opts.engineStrict,
    force: opts.force,
    nodeVersion: opts.nodeVersion,
    outdatedDependencies: {},
    pendingNodes: [],
    pnpmVersion: opts.pnpmVersion,
    preferredVersions,
    prefix: opts.prefix,
    rawNpmConfig: opts.rawNpmConfig,
    registry: opts.wantedShrinkwrap.registry,
    resolvedFromLocalPackages: [],
    resolvedPackagesByPackageId: {},
    skipped: opts.skipped,
    storeController: opts.storeController,
    verifyStoreIntegrity: opts.verifyStoreIntegrity,
    virtualStoreDir: opts.virtualStoreDir,
    wantedShrinkwrap: opts.wantedShrinkwrap,
  }

  const shrImporter = opts.wantedShrinkwrap.importers[opts.importerPath]
  const rootPkgs = await resolveDependencies(
    ctx,
    opts.nonLinkedPackages,
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
      shamefullyFlatten: opts.shamefullyFlatten,
      sideEffectsCache: opts.sideEffectsCache,
      update: opts.update,
    },
  )

  ctx.pendingNodes.forEach((pendingNode) => {
    ctx.dependenciesGraph[pendingNode.nodeId] = {
      children: () => buildTree(ctx, pendingNode.nodeId, pendingNode.resolvedPackage.id,
        ctx.childrenByParentId[pendingNode.resolvedPackage.id], pendingNode.depth + 1, pendingNode.installable),
      depth: pendingNode.depth,
      installable: pendingNode.installable,
      resolvedPackage: pendingNode.resolvedPackage,
    }
  })

  return {
    dependenciesGraph: ctx.dependenciesGraph,
    outdatedDependencies: ctx.outdatedDependencies,
    resolvedFromLocalPackages: ctx.resolvedFromLocalPackages,
    resolvedPackagesByPackageId: ctx.resolvedPackagesByPackageId,
    rootPkgs,
  }
}

function buildTree (
  ctx: ResolutionContext,
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
    ctx.dependenciesGraph[childNodeId] = {
      children: () => buildTree(ctx, childNodeId, child.pkgId, ctx.childrenByParentId[child.pkgId], depth + 1, installable),
      depth,
      installable,
      resolvedPackage: ctx.resolvedPackagesByPackageId[child.pkgId],
    }
  }
  return childrenNodeIds
}
