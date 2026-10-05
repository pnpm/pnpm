import path from 'node:path'

import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import { getPatchInfo } from '@pnpm/patching.config'
import { convertEnginesRuntimeToDependencies } from '@pnpm/pkg-manifest.utils'
import { detectMinReleaseAgeViolation, MINIMUM_RELEASE_AGE_VIOLATION_CODE } from '@pnpm/resolving.npm-resolver'
import type {
  DirectoryResolution,
  PkgResolutionId,
  ResolutionPolicyViolation,
  VersionSelectors,
} from '@pnpm/resolving.resolver-base'
import type { PackageResponse } from '@pnpm/store.controller-types'
import type { PackageManifest, PkgIdWithPatchHash } from '@pnpm/types'
import { pathExists } from 'path-exists'
import { omit } from 'ramda'

import { addResolvedPackageToGraph, type ResolvedManifest } from './addResolvedPackageToGraph.js'
import type { WantedDependency } from './getWantedDependencies.js'
import { startPackageResolution, waitForPackageResolutionTurn } from './packageResolutionBarrier.js'
import { parentIdsContainSequence } from './parentIdsContainSequence.js'
import {
  type CurrentPkg,
  type DependencyRequest,
  getPkgsInfoFromIds,
  requestDependencyPackage,
} from './requestDependencyPackage.js'
import type {
  LinkedDependency,
  ParentPkgAliases,
  ResolutionContext,
  ResolveDependencyOptions,
  ResolveDependencyResult,
} from './resolutionTypes.js'
import { copyResolvedManifest, getManifestFromResponse } from './resolvedPackage.js'

const dependencyResolvedLogger = logger('_dependency_resolved')

const omitDepsFields = omit(['dependencies', 'optionalDependencies', 'peerDependencies', 'peerDependenciesMeta'])

/**
 * Resolvers whose package id pins the package contents. A local directory can
 * change under the same id, so its manifest stays the source of truth.
 */
const IMMUTABLE_CONTENT_RESOLVERS = new Set(['git-repository', 'jsr-registry', 'named-registry', 'npm-registry'])

export async function resolveDependency (
  wantedDependency: WantedDependency,
  ctx: ResolutionContext,
  options: ResolveDependencyOptions
): Promise<ResolveDependencyResult> {
  const packageRootLinkTarget = dp.packageRootLinkTarget(wantedDependency.bareSpecifier)
  if (packageRootLinkTarget != null) {
    return linkIntoDeclaringPackage(wantedDependency, packageRootLinkTarget)
  }
  const currentPkg = options.currentPkg ?? {}
  const depIsLinked = await isDepLinkedInVirtualStore(ctx, currentPkg)
  if (isLinkedAndNotToBeUpdated(options, currentPkg, depIsLinked)) {
    return null
  }

  const request: DependencyRequest = {
    ctx,
    currentPkg,
    depIsLinked,
    options,
    wantedDependency: options.parentPkg.installable
      ? wantedDependency
      : { ...wantedDependency, optional: true },
  }
  const finishPackageResolution = startPackageResolution(ctx, options.currentDepth)
  try {
    await waitForPackageResolutionTurn(ctx, options.currentDepth)
    const pkgResponse = await requestDependencyPackage(request)
    if (pkgResponse == null) return null
    return await resolvePackageFromResponse(request, pkgResponse)
  } finally {
    finishPackageResolution()
  }
}

async function isDepLinkedInVirtualStore (ctx: ResolutionContext, currentPkg: CurrentPkg): Promise<boolean> {
  const currentLockfileContainsTheDep = currentPkg.depPath
    ? Boolean(ctx.currentLockfile.packages?.[currentPkg.depPath])
    : undefined
  return Boolean(
    // if package is not in `node_modules/.pnpm-lock.yaml`
    // we can safely assume that it doesn't exist in `node_modules`
    currentLockfileContainsTheDep &&
    currentPkg.depPath &&
    currentPkg.dependencyLockfile &&
    currentPkg.name &&
    await pathExists(
      path.join(
        ctx.virtualStoreDir,
        dp.depPathToFilename(currentPkg.depPath, ctx.virtualStoreDirMaxLength),
        'node_modules',
        currentPkg.name,
        'package.json'
      )
    )
  )
}

function isLinkedAndNotToBeUpdated (options: ResolveDependencyOptions, currentPkg: CurrentPkg, depIsLinked: boolean): boolean {
  return !options.update && !options.proceed &&
    options.currentDepth === Math.max(0, options.updateDepth) &&
    (currentPkg.resolution != null) && depIsLinked
}

async function resolvePackageFromResponse (
  request: DependencyRequest,
  pkgResponse: PackageResponse
): Promise<ResolveDependencyResult> {
  const { ctx, options, wantedDependency } = request
  dependencyResolvedLogger.debug({
    resolution: pkgResponse.body.id,
    wanted: {
      dependentId: options.parentPkg.pkgId,
      name: wantedDependency.alias,
      rawSpec: wantedDependency.bareSpecifier,
    },
  })

  const policyViolation = recheckAgainstMinimumReleaseAge(ctx, pkgResponse.body)
  if (policyViolation) {
    ctx.resolutionPolicyViolations.push(policyViolation)
  }

  assertExoticSubdepAllowed(request, pkgResponse)
  recordPreferredVersionOfResolvedPackage(ctx, pkgResponse)

  if (
    !pkgResponse.body.updated &&
    options.currentDepth === Math.max(0, options.updateDepth) &&
    request.depIsLinked && !ctx.force && !options.proceed
  ) {
    return null
  }

  if (pkgResponse.body.isLocal) {
    return toLinkedDependency(wantedDependency, pkgResponse.body)
  }
  return resolveNonLocalPackage(request, pkgResponse)
}

// Check if exotic dependencies are disallowed in subdependencies
function assertExoticSubdepAllowed ({ ctx, options, wantedDependency }: DependencyRequest, pkgResponse: PackageResponse): void {
  if (
    !ctx.blockExoticSubdeps ||
    options.currentDepth <= 0 ||
    options.parentPkg.resolvedVia === 'workspace' ||
    pkgResponse.body.resolvedVia == null || // This is already coming from the lockfile, we skip the check in this case for now. Should be fixed later.
    !isExoticDep(pkgResponse.body.resolvedVia)
  ) return
  const error = new PnpmError(
    'EXOTIC_SUBDEP',
    `Exotic dependency "${wantedDependency.alias ?? wantedDependency.bareSpecifier}" (resolved via ${pkgResponse.body.resolvedVia}) is not allowed in subdependencies when blockExoticSubdeps is enabled`
  )
  error.prefix = options.prefix
  error.pkgsStack = getPkgsInfoFromIds(options.parentIds, ctx.resolvedPkgsById)
  throw error
}

function recordPreferredVersionOfResolvedPackage (ctx: ResolutionContext, pkgResponse: PackageResponse): void {
  const { manifest } = pkgResponse.body
  if (!ctx.allPreferredVersions || !manifest?.version) return
  if (!ctx.allPreferredVersions[manifest.name]) {
    // Null-prototype: keyed by versions from the resolved manifest.
    ctx.allPreferredVersions[manifest.name] = Object.create(null) as VersionSelectors
  }
  ctx.allPreferredVersions[manifest.name][manifest.version] = 'version'
}

type LocalPackageBody = PackageResponse['body'] & { isLocal: true, resolution: DirectoryResolution }

function toLinkedDependency (wantedDependency: WantedDependency, body: LocalPackageBody): LinkedDependency {
  if (!body.manifest) {
    // This should actually never happen because the local-resolver returns a manifest
    // even if no real manifest exists in the filesystem.
    throw new PnpmError('MISSING_PACKAGE_JSON', `Can't install ${wantedDependency.bareSpecifier}: Missing package.json file`)
  }
  return {
    alias: wantedDependency.alias ?? body.alias ?? body.manifest.name ?? path.basename(body.resolution.directory),
    dev: wantedDependency.dev,
    isLinkedDependency: true,
    name: body.manifest.name,
    optional: wantedDependency.optional,
    pkgId: body.id,
    resolution: body.resolution,
    version: body.manifest.version || '0.0.0',
    normalizedBareSpecifier: body.normalizedBareSpecifier,
    pkg: body.manifest,
    wantedDependency,
  }
}

async function resolveNonLocalPackage (
  request: DependencyRequest,
  pkgResponse: PackageResponse
): Promise<ResolveDependencyResult> {
  const { ctx, options } = request
  const manifest = await readResolvedManifest(request, pkgResponse)
  let pkgIdWithPatchHash = (pkgResponse.body.id.startsWith(`${manifest.name}@`) ? pkgResponse.body.id : `${manifest.name}@${pkgResponse.body.id}`) as PkgIdWithPatchHash
  const patch = getPatchInfo(ctx.patchedDependencies, manifest.name, manifest.version)
  if (patch) {
    pkgIdWithPatchHash = `${pkgIdWithPatchHash}(patch_hash=${patch.hash})` as PkgIdWithPatchHash
  }

  // We are building the dependency tree only until there are new packages
  // or the packages repeat in a unique order.
  // This is needed later during peer dependencies resolution.
  //
  // So we resolve foo > bar > qar > foo
  // But we stop on foo > bar > qar > foo > qar
  // In the second example, there's no reason to walk qar again
  // when qar is included the first time, the dependencies of foo
  // are already resolved and included as parent dependencies of qar.
  // So during peers resolution, qar cannot possibly get any new or different
  // peers resolved, after the first occurrence.
  //
  // However, in the next example we would analyze the second qar as well,
  // because zoo is a new parent package:
  // foo > bar > qar > zoo > qar
  if (isRepeatedInParentPath(options, pkgResponse.body.id)) {
    return null
  }

  const resolvedManifest = applyLockedPackageSnapshot(request, pkgResponse, manifest)
  if (options.currentDepth === 0 && pkgResponse.body.latest && pkgResponse.body.latest !== resolvedManifest.pkg.version) {
    ctx.outdatedDependencies[pkgResponse.body.id] = pkgResponse.body.latest
  }
  collectPeerDependencyNames(ctx.allPeerDepNames, resolvedManifest.pkg)
  return addResolvedPackageToGraph(request, { ...resolvedManifest, patch, pkgIdWithPatchHash, pkgResponse })
}

function isRepeatedInParentPath (options: ResolveDependencyOptions, pkgId: PkgResolutionId): boolean {
  return parentIdsContainSequence(
    options.parentIds,
    options.parentPkg.pkgId,
    pkgId
  ) || pkgId === options.parentPkg.pkgId
}

async function readResolvedManifest (
  { ctx, currentPkg, options, wantedDependency }: DependencyRequest,
  pkgResponse: PackageResponse
): Promise<PackageManifest> {
  let pkg: PackageManifest = copyResolvedManifest(getManifestFromResponse(pkgResponse, wantedDependency, currentPkg))
  if (ctx.readPackageHook != null && !pkgResponse.body.hooked) {
    pkg = await ctx.readPackageHook(pkg)
  }
  keepLockedPeerDependencies({ ctx, currentPkg, options }, pkgResponse, pkg)
  if (!pkg.version) {
    pkg.version = '0.0.0'
  }
  pkg = omitPeersFromDependencies(pkg, { autoInstallPeers: ctx.autoInstallPeers, parentPkgAliases: options.parentPkgAliases })
  if (pkg.engines?.runtime != null) {
    convertEnginesRuntimeToDependencies(pkg, 'engines', 'dependencies')
  }
  if (!pkg.name) { // TODO: don't fail on optional dependencies
    throw new PnpmError('MISSING_PACKAGE_NAME', `Can't install ${wantedDependency.bareSpecifier}: Missing package name`)
  }
  return pkg
}

/**
 * Gives a package reused from the lockfile the peer dependencies its lockfile
 * entry records. The registry metadata and the package.json in the store may
 * disagree about them, and reading the other source would rewrite the entry
 * of a package nobody updated.
 */
function keepLockedPeerDependencies (
  { ctx, currentPkg, options }: Pick<DependencyRequest, 'ctx' | 'currentPkg' | 'options'>,
  pkgResponse: PackageResponse,
  pkg: PackageManifest
): void {
  const snapshot = currentPkg.dependencyLockfile
  if (
    !ctx.lockedPeersAreCurrent ||
    options.update ||
    snapshot == null ||
    pkgResponse.body.updated ||
    pkgResponse.body.resolvedVia == null ||
    !IMMUTABLE_CONTENT_RESOLVERS.has(pkgResponse.body.resolvedVia)
  ) return
  delete pkg.peerDependencies
  delete pkg.peerDependenciesMeta
  if (snapshot.peerDependencies != null) {
    pkg.peerDependencies = { ...snapshot.peerDependencies }
  }
  if (snapshot.peerDependenciesMeta != null) {
    pkg.peerDependenciesMeta = Object.fromEntries(
      Object.entries(snapshot.peerDependenciesMeta).map(([peerName, peerMeta]) => [peerName, { ...peerMeta }])
    )
  }
}

function omitPeersFromDependencies (
  pkg: PackageManifest,
  { autoInstallPeers, parentPkgAliases }: { autoInstallPeers: boolean, parentPkgAliases: ParentPkgAliases }
): PackageManifest {
  const { dependencies, peerDependencies, peerDependenciesMeta } = pkg
  if (!peerDependencies || !dependencies) return pkg
  const isAutoInstalledPeer = (peerDep: string): boolean =>
    autoInstallPeers && peerDependenciesMeta?.[peerDep]?.optional !== true
  return {
    ...pkg,
    dependencies: omit(
      Object.keys(peerDependencies).filter((peerDep) => isAutoInstalledPeer(peerDep) || parentPkgAliases[peerDep]),
      dependencies
    ),
  }
}

function applyLockedPackageSnapshot (
  { currentPkg, options }: DependencyRequest,
  pkgResponse: PackageResponse,
  pkg: PackageManifest
): ResolvedManifest {
  if (
    !options.update && (currentPkg.dependencyLockfile != null) && currentPkg.depPath &&
    !pkgResponse.body.updated &&
    // peerDependencies field is also used for transitive peer dependencies which should not be linked
    // That's why we cannot omit reading package.json of such dependencies.
    // This can be removed if we implement something like peerDependenciesMeta.transitive: true
    (currentPkg.dependencyLockfile.peerDependencies == null)
  ) {
    return {
      hasBin: currentPkg.dependencyLockfile.hasBin === true,
      pkg: {
        ...nameVerFromPkgSnapshot(currentPkg.depPath, currentPkg.dependencyLockfile),
        ...omitDepsFields(currentPkg.dependencyLockfile),
        ...pkg,
      },
    }
  }
  const prepare = Boolean(
    pkgResponse.body.resolvedVia === 'git-repository' &&
    typeof pkg.scripts?.prepare === 'string'
  )

  if (
    currentPkg.dependencyLockfile?.deprecated &&
    !pkgResponse.body.updated && !pkg.deprecated
  ) {
    pkg.deprecated = currentPkg.dependencyLockfile.deprecated
  }
  const hasBin = (currentPkg.dependencyLockfile?.hasBin != null && !pkg.bin)
    ? currentPkg.dependencyLockfile.hasBin
    : Boolean((pkg.bin && !(pkg.bin === '' || Object.keys(pkg.bin).length === 0)) ?? pkg.directories?.bin)
  return { hasBin, pkg, prepare }
}

function collectPeerDependencyNames (allPeerDepNames: Set<string>, pkg: PackageManifest): void {
  for (const name in pkg.peerDependencies) {
    allPeerDepNames.add(name)
  }
  for (const name in pkg.peerDependenciesMeta) {
    allPeerDepNames.add(name)
  }
}

/**
 * A `link:<root>/...` dependency points inside the package that declares it,
 * whose files are only on disk once that package is placed. It is recorded
 * as a link without reading the target, and each linker resolves `<root>`
 * against the declaring package's directory.
 */
function linkIntoDeclaringPackage (wantedDependency: WantedDependency, target: string): LinkedDependency {
  const alias = wantedDependency.alias ?? path.posix.basename(target)
  return {
    alias,
    dev: wantedDependency.dev,
    isLinkedDependency: true,
    name: alias,
    optional: wantedDependency.optional,
    pkg: { name: alias, version: '0.0.0' },
    pkgId: wantedDependency.bareSpecifier as PkgResolutionId,
    resolution: { type: 'directory', directory: target },
    version: '0.0.0',
    wantedDependency,
  }
}

/**
 * Returns the policy violation to record for a resolved package. A violation
 * with any other code is returned unchanged. A `minimumReleaseAge` violation
 * is returned only if the package was published after `ctx.maximumPublishedBy`
 * and `ctx.publishedByExclude` does not cover it.
 *
 * The resolver flags a pick against the cutoff it picked with, which
 * `resolutionMode: time-based` tightens below the `minimumReleaseAge` cutoff
 * for subdependencies. The picking cutoff is never later than the
 * `minimumReleaseAge` one, so every real violation is flagged first.
 */
function recheckAgainstMinimumReleaseAge (
  ctx: Pick<ResolutionContext, 'maximumPublishedBy' | 'publishedByExclude'>,
  { policyViolation: violation, publishedAt }: { policyViolation?: ResolutionPolicyViolation, publishedAt?: string }
): ResolutionPolicyViolation | undefined {
  if (violation?.code !== MINIMUM_RELEASE_AGE_VIOLATION_CODE) return violation
  return detectMinReleaseAgeViolation({
    name: violation.name,
    version: violation.version,
    publishedAt,
    resolution: violation.resolution,
    publishedBy: ctx.maximumPublishedBy,
    publishedByExclude: ctx.publishedByExclude,
  })
}

const NON_EXOTIC_RESOLVED_VIA = new Set([
  'custom-resolver',
  'github.com/denoland/deno',
  'github.com/oven-sh/bun',
  'jsr-registry',
  'local-filesystem',
  'named-registry',
  'nodejs.org',
  'npm-registry',
  'workspace',
])

function isExoticDep (resolvedVia: string): boolean {
  return !NON_EXOTIC_RESOLVED_VIA.has(resolvedVia)
}
