import { skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { getPatchInfo } from '@pnpm/patching.config'
import type { PkgResolutionId, PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { OnFetchError, PackageResponse, RequestPackageOptions } from '@pnpm/store.controller-types'
import semver from 'semver'

import { getExactSinglePreferredVersions } from './getExactSinglePreferredVersions.js'
import { hasAlias, type WantedDependency } from './getWantedDependencies.js'
import { replaceVersionInBareSpecifier } from './replaceVersionInBareSpecifier.js'
import type { ResolutionContext, ResolveDependencyOptions, ResolvedPkgsById } from './resolutionTypes.js'
import { unwrapPackageName } from './unwrapPackageName.js'

export type CurrentPkg = NonNullable<ResolveDependencyOptions['currentPkg']>

/**
 * One dependency edge being resolved by `resolveDependency`, shared by the
 * steps that request, read, and record its package.
 */
export interface DependencyRequest {
  ctx: ResolutionContext
  currentPkg: CurrentPkg
  depIsLinked: boolean
  options: ResolveDependencyOptions
  wantedDependency: WantedDependency
}

type PackageRequestError = Error & {
  code?: string
  hint?: string
  package?: unknown
  pkgsStack?: unknown
  prefix?: string
}

export function getPkgsInfoFromIds (
  ids: PkgResolutionId[],
  resolvedPkgsById: ResolvedPkgsById
): Array<{ id: PkgResolutionId, name: string, version: string }> {
  return ids
    .slice(1)
    .map((id) => {
      const { name, version } = resolvedPkgsById[id]
      return { id, name, version }
    })
}

/**
 * Requests the package of the wanted dependency from the store. Returns
 * `null` when the request failed and the dependency is an optional one that
 * may be skipped.
 */
export async function requestDependencyPackage (request: DependencyRequest): Promise<PackageResponse | null> {
  const { ctx, options, wantedDependency } = request
  // Normalize the `preferredVersion` (singular) and `preferredVersions`
  // (plural) options. If the singular option is passed through, it'll be used
  // instead of the plural option.
  const preferredVersions = !options.updateRequested && options.preferredVersion != null && hasAlias(wantedDependency)
    ? getExactSinglePreferredVersions(wantedDependency, options.preferredVersion)
    : options.preferredVersions

  try {
    pinBareSpecifierToCurrentVersion(request)
    return await ctx.storeController.requestPackage(wantedDependency, getRequestPackageOptions(request, preferredVersions))
  } catch (err: unknown) {
    return handlePackageRequestFailure(request, err as PackageRequestError)
  }
}

function pinBareSpecifierToCurrentVersion ({ ctx, currentPkg, options, wantedDependency }: DependencyRequest): void {
  if (!currentPkg.version) return
  if (options.updatePatches && !hasRegistryRevisionSpecifier(wantedDependency.bareSpecifier)) {
    wantedDependency.bareSpecifier = replaceVersionInBareSpecifier(
      wantedDependency.bareSpecifier,
      currentPkg.version,
      ctx.namedRegistryPrefixes
    )
  }
  if (!options.update && pkgIdPinsVersion(currentPkg.pkgId, currentPkg.version) && options.currentDepth !== 0) {
    wantedDependency.bareSpecifier = replaceVersionInBareSpecifier(wantedDependency.bareSpecifier, currentPkg.version, ctx.namedRegistryPrefixes)
  }
}

function getRequestPackageOptions (request: DependencyRequest, preferredVersions: PreferredVersions): RequestPackageOptions {
  const { ctx, currentPkg, options, wantedDependency } = request
  return {
    ...getReleasePolicyOptions(request),
    allowBuild: ctx.allowBuild,
    alwaysTryWorkspacePackages: ctx.linkWorkspacePackagesDepth >= options.currentDepth,
    currentPkg: {
      id: currentPkg.pkgId,
      name: currentPkg.name,
      resolution: currentPkg.resolution,
      version: currentPkg.version,
      publishedAt: currentPkg.pkgId ? ctx.wantedLockfile.time?.[currentPkg.pkgId] : undefined,
    },
    expectedPkg: currentPkg,
    defaultTag: ctx.defaultTag,
    ignoreScripts: ctx.ignoreScripts,
    pickLowestVersion: options.pickLowestVersion,
    downloadPriority: -options.currentDepth,
    lockfileDir: ctx.lockfileDir,
    nodeVersion: ctx.nodeVersion,
    deferEnginesCheck: (manifest) => manifest.name != null &&
      manifest.version != null &&
      getPatchInfo(ctx.patchedDependencies, manifest.name, manifest.version) != null,
    preferredVersions,
    preferWorkspacePackages: ctx.preferWorkspacePackages,
    projectDir: (options.currentDepth > 0 && !wantedDependency.bareSpecifier.startsWith('file:'))
      ? ctx.lockfileDir
      : options.parentPkg.rootDir,
    skipFetch: ctx.dryRun,
    update: options.update,
    updatePatches: options.updatePatches,
    updateRequested: options.updateRequested,
    updateChecksums: options.updateChecksums,
    workspacePackages: ctx.workspacePackages,
    supportedArchitectures: options.supportedArchitectures,
    onFetchError: createFetchErrorDecorator(request),
    injectWorkspacePackages: ctx.injectWorkspacePackages,
    calcSpecifier: options.currentDepth === 0,
    rangeSpecStyle: options.rangeSpecStyle,
    readPackageHook: ctx.readPackageHook,
  }
}

type ReleasePolicyOptions = Pick<RequestPackageOptions,
'publishedBy' | 'fallbackPublishedBy' | 'publishedByExclude' | 'trustPolicy' | 'trustPolicyExclude' | 'trustPolicyIgnoreAfter'>

function getReleasePolicyOptions ({ ctx, options }: DependencyRequest): ReleasePolicyOptions {
  return {
    publishedBy: options.publishedBy,
    fallbackPublishedBy: ctx.maximumPublishedBy,
    publishedByExclude: ctx.publishedByExclude,
    trustPolicy: ctx.trustPolicy,
    trustPolicyExclude: ctx.trustPolicyExclude,
    trustPolicyIgnoreAfter: ctx.trustPolicyIgnoreAfter,
  }
}

function createFetchErrorDecorator ({ ctx, options }: DependencyRequest): OnFetchError {
  return (err) => {
    const fetchError = err as PackageRequestError
    fetchError.prefix = options.prefix
    fetchError.pkgsStack = getPkgsInfoFromIds(options.parentIds, ctx.resolvedPkgsById)
    return fetchError
  }
}

function handlePackageRequestFailure ({ ctx, options, wantedDependency }: DependencyRequest, err: PackageRequestError): null {
  const wantedDependencyDetails = {
    name: wantedDependency.alias,
    bareSpecifier: wantedDependency.bareSpecifier,
    version: wantedDependency.alias ? wantedDependency.bareSpecifier : undefined,
  }
  if (wantedDependency.optional && err.code !== 'ERR_PNPM_TRUST_DOWNGRADE') {
    if (!wantedLockfileContainsSatisfyingEntry(ctx.wantedLockfile, wantedDependency)) {
      skippedOptionalDependencyLogger.debug({
        details: err.toString(),
        package: wantedDependencyDetails,
        parents: getPkgsInfoFromIds(options.parentIds, ctx.resolvedPkgsById),
        prefix: options.prefix,
        reason: 'resolution_failure',
      })
      return null
    }
    err.hint ??= 'This optional dependency is not skipped, because the lockfile contains a resolution for it. ' +
      'Skipping it would remove the locked entries, making the lockfile differ depending on which machine ran the install. ' +
      'If the version was intentionally removed from the registry, update the dependent package or remove the entries from the lockfile.'
  }
  err.package = wantedDependencyDetails
  err.prefix = options.prefix
  err.pkgsStack = getPkgsInfoFromIds(options.parentIds, ctx.resolvedPkgsById)
  throw err
}

function hasRegistryRevisionSpecifier (specifier: string): boolean {
  const selectorStart = Math.max(specifier.lastIndexOf(':'), specifier.lastIndexOf('@')) + 1
  const selector = specifier.slice(selectorStart)
  if (semver.valid(selector) == null) return false
  const marker = selector.lastIndexOf('+r')
  if (marker === -1) return false
  const revision = selector.slice(marker + 2)
  return revision.length > 0 && Array.from(revision).every((character) => character >= '0' && character <= '9')
}

/**
 * Whether the wanted lockfile already holds a package entry that satisfies the
 * wanted dependency. An optional dependency that fails to resolve is normally
 * skipped, but when a locked resolution exists the failure is environmental
 * (e.g. a registry mirror that hasn't synced the release yet) rather than a
 * genuinely uninstallable package. Silently skipping in that case would erase
 * the locked entries, making the lockfile differ across machines from
 * identical inputs and leaving frozen installs on other hosts with nothing to
 * link (https://github.com/pnpm/pnpm/issues/12853).
 *
 * Only plain semver specifiers are checked; exotic specifiers (git, catalogs,
 * tags, URLs) keep the skip-on-failure behavior.
 *
 * The check is deliberately by package name and range rather than by the
 * current edge's locked dep path. `pnpm dedupe` — the flow where the erasure
 * bites — clears every per-snapshot dependency map before resolving
 * (`forgetResolutionsOfAllPrevWantedDeps`), so on this code path no edge-level
 * lockfile linkage exists to key on; only the package entries survive. A
 * satisfying entry locked via any edge also means the registry served this
 * package in-range before, so failing loudly instead of skipping is the right
 * outcome even when the failing edge itself was never locked.
 */
function wantedLockfileContainsSatisfyingEntry (lockfile: LockfileObject, wantedDependency: WantedDependency): boolean {
  if (!wantedDependency.alias) return false
  const { pkgName, bareSpecifier } = unwrapPackageName(wantedDependency.alias, wantedDependency.bareSpecifier)
  if (semver.validRange(bareSpecifier) == null) return false
  return Object.keys(lockfile.packages ?? {}).some((depPath) => {
    const parsed = dp.parse(depPath)
    return parsed.name === pkgName && parsed.version != null && semver.satisfies(parsed.version, bareSpecifier)
  })
}

// A pkgId pins the lockfile-resolved version either as plain `name@version`
// or as the registry-qualified `name@<registryName>:<version>` form.
function pkgIdPinsVersion (pkgId: PkgResolutionId | undefined, version: string): boolean {
  if (pkgId == null) return false
  if (pkgId.endsWith(`@${version}`)) return true
  const parsed = dp.parse(pkgId)
  return parsed.registryName != null && parsed.version === version
}
