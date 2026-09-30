import path from 'node:path'

import { deprecationLogger, progressLogger } from '@pnpm/core-loggers'
import type { PatchInfo } from '@pnpm/patching.types'
import type { DirectoryResolution } from '@pnpm/resolving.resolver-base'
import type { PackageResponse } from '@pnpm/store.controller-types'
import type { PackageManifest, PkgIdWithPatchHash } from '@pnpm/types'
import semver from 'semver'

import { claimChildrenResolution, type ClaimedChildrenResolution } from './childrenResolution.js'
import { getMissingPeers } from './missingPeers.js'
import { nextNodeId, type NodeId } from './nextNodeId.js'
import type { DependencyRequest } from './requestDependencyPackage.js'
import type { PkgAddress } from './resolutionTypes.js'
import {
  detectNamedRegistryCollision,
  detectRegistryRevisionConflict,
  getResolvedPackage,
  pkgIsLeaf,
} from './resolvedPackage.js'

export interface ResolvedManifest {
  hasBin: boolean
  pkg: PackageManifest
  prepare?: boolean
}

export interface ResolvedPackageSource extends ResolvedManifest {
  patch?: PatchInfo
  pkgIdWithPatchHash: PkgIdWithPatchHash
  pkgResponse: PackageResponse
}

interface PackageNode {
  childrenResolution: ClaimedChildrenResolution
  installable: boolean
  nodeId: NodeId
}

/**
 * Records the resolved package in `ctx.resolvedPkgsById` (or merges this
 * occurrence into the existing record) and returns the address of the
 * package at this position of the dependency tree.
 */
export function addResolvedPackageToGraph (request: DependencyRequest, resolved: ResolvedPackageSource): PkgAddress {
  const { ctx, options } = request
  const { pkg, pkgResponse } = resolved
  // In case of leaf dependencies (dependencies that have no prod deps or peer deps),
  // we only ever need to analyze one leaf dep in a graph, so the nodeId can be short and stateless.
  const nodeId = pkgIsLeaf(pkg) ? pkgResponse.body.id as unknown as NodeId : nextNodeId()

  const parentIsInstallable = options.parentPkg.installable === undefined || options.parentPkg.installable
  const installable = parentIsInstallable && pkgResponse.body.isInstallable !== false
  const packageIsNew = !ctx.resolvedPkgsById[pkgResponse.body.id]
  const childrenResolution = claimChildrenResolution(ctx, {
    currentDepth: options.currentDepth,
    parentIds: options.parentIds,
    pkgId: pkgResponse.body.id,
  })
  const node: PackageNode = { childrenResolution, installable, nodeId }

  if (packageIsNew) {
    addNewResolvedPackage(request, resolved, parentIsInstallable)
  } else {
    mergeIntoResolvedPackage(request, pkgResponse)
    if (!childrenResolution.isOwner) {
      placeNodeOfResolvedPackage(request, resolved, node)
    }
  }
  return toPkgAddress(request, resolved, node)
}

function addNewResolvedPackage (
  request: DependencyRequest,
  resolved: ResolvedPackageSource,
  parentIsInstallable: boolean
): void {
  const { ctx, currentPkg, options, wantedDependency } = request
  const { pkgResponse } = resolved
  reportDeprecatedPackage(request, resolved)
  if (pkgResponse.body.isInstallable === false || !parentIsInstallable) {
    ctx.skipped.add(pkgResponse.body.id)
  }
  progressLogger.debug({
    packageId: pkgResponse.body.id,
    requester: ctx.lockfileDir,
    status: 'resolved',
  })

  // WARN: It is very important to keep this sync
  // Otherwise, deprecation messages for the same package might get written several times
  ctx.resolvedPkgsById[pkgResponse.body.id] = getResolvedPackage({
    dependencyLockfile: currentPkg.dependencyLockfile,
    pkgIdWithPatchHash: resolved.pkgIdWithPatchHash,
    force: ctx.force,
    hasBin: resolved.hasBin,
    patch: resolved.patch,
    pkg: resolved.pkg,
    pkgResponse,
    prepare: resolved.prepare,
    wantedDependency,
    parentImporterId: options.parentIds[0],
    optional: wantedDependency.optional || options.parentPkg.optional,
  })
}

// Report deprecated packages only on first occurrence.
function reportDeprecatedPackage ({ ctx, options }: DependencyRequest, { pkg, pkgResponse }: ResolvedPackageSource): void {
  if (
    !pkg.deprecated ||
    (ctx.allowedDeprecatedVersions[pkg.name] && semver.satisfies(pkg.version, ctx.allowedDeprecatedVersions[pkg.name]))
  ) return
  deprecationLogger.debug({
    depth: options.currentDepth,
    nonDeprecatedAlternative: pkgResponse.body.nonDeprecatedAlternative,
    pkgId: pkgResponse.body.id,
    pkgName: pkg.name,
    pkgVersion: pkg.version,
    prefix: options.prefix,
  })
}

function mergeIntoResolvedPackage ({ ctx, options, wantedDependency }: DependencyRequest, pkgResponse: PackageResponse): void {
  const resolvedPkg = ctx.resolvedPkgsById[pkgResponse.body.id]
  detectNamedRegistryCollision(resolvedPkg, pkgResponse)
  detectRegistryRevisionConflict(resolvedPkg, pkgResponse)
  resolvedPkg.prod = resolvedPkg.prod || !wantedDependency.dev && !wantedDependency.optional
  resolvedPkg.dev = resolvedPkg.dev || wantedDependency.dev
  resolvedPkg.optional = resolvedPkg.optional && (wantedDependency.optional || options.parentPkg.optional)
  if (resolvedPkg.fetching == null && pkgResponse.fetching != null) {
    resolvedPkg.fetching = pkgResponse.fetching
    resolvedPkg.filesIndexFile = pkgResponse.filesIndexFile!
  }
}

function placeNodeOfResolvedPackage (
  { ctx, currentPkg, options, wantedDependency }: DependencyRequest,
  { pkg, pkgResponse }: ResolvedPackageSource,
  { installable, nodeId }: PackageNode
): void {
  const existingNode = ctx.dependenciesTree.get(nodeId)
  if (existingNode != null) {
    existingNode.depth = Math.min(existingNode.depth, options.currentDepth)
    return
  }
  ctx.pendingNodes.push({
    alias: wantedDependency.alias ?? pkgResponse.body.alias ?? pkg.name,
    depth: options.currentDepth,
    parentIds: options.parentIds,
    installable,
    lockedPeerContext: currentPkg.lockedPeerContext,
    previousDepPath: currentPkg.depPath,
    nodeId,
    resolvedPackage: ctx.resolvedPkgsById[pkgResponse.body.id],
  })
}

function toPkgAddress (
  { ctx, depIsLinked, options, wantedDependency }: DependencyRequest,
  { pkg, pkgResponse }: ResolvedPackageSource,
  { childrenResolution, installable, nodeId }: PackageNode
): PkgAddress {
  const rootDir = pkgResponse.body.resolution.type === 'directory'
    ? path.resolve(ctx.lockfileDir, (pkgResponse.body.resolution as DirectoryResolution).directory)
    : options.prefix
  const resolvedPkg = ctx.resolvedPkgsById[pkgResponse.body.id]

  return {
    alias: wantedDependency.alias ?? pkgResponse.body.alias ?? pkg.name,
    depIsLinked,
    resolvedVia: pkgResponse.body.resolvedVia,
    isNew: childrenResolution.isOwner,
    nodeId,
    wantedDependency,
    normalizedBareSpecifier: pkgResponse.body.normalizedBareSpecifier,
    missingPeersOfChildren: childrenResolution.missingPeersOfChildren,
    childrenResolutionId: childrenResolution.id,
    pkgId: pkgResponse.body.id,
    rootDir,
    missingPeers: getMissingPeers(resolvedPkg.peerDependencies),
    optional: resolvedPkg.optional,
    version: resolvedPkg.version,
    saveCatalogName: wantedDependency.saveCatalogName,

    // Next fields are actually only needed when isNew = true
    installable,
    isLinkedDependency: undefined,
    pkg,
    updated: pkgResponse.body.updated,
    publishedAt: pkgResponse.body.publishedAt,
  }
}
