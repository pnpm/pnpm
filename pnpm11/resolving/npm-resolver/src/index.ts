import path from 'node:path'

import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import type {
  FetchFromRegistry,
  GetAuthHeader,
  RetryTimeoutOptions,
} from '@pnpm/fetching.types'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import type {
  DirectoryResolution,
  LatestInfo,
  LatestQuery,
  PkgResolutionId,
  PreferredVersions,
  Resolution,
  ResolutionPolicyViolation,
  ResolveOptions,
  ResolveResult,
  TarballResolution,
  WantedDependency,
  WorkspacePackage,
  WorkspacePackages,
  WorkspacePackagesByVersion,
} from '@pnpm/resolving.resolver-base'
import { storeIndexKey } from '@pnpm/store.index'
import type {
  DependencyManifest,
  PackageVersionPolicy,
  PinnedVersion,
  Registries,
  TrustPolicy,
} from '@pnpm/types'
import {
  readPkgFromCafs,
} from '@pnpm/worker'
import { resolveWorkspaceRange } from '@pnpm/workspace.range-resolver'
import { LRUCache } from 'lru-cache'
import normalize from 'normalize-path'
import pMemoize, { pMemoizeClear } from 'p-memoize'
import { clone } from 'ramda'
import semver from 'semver'
import ssri from 'ssri'
import versionSelectorType from 'version-selector-type'

import { fetchMetadataFromFromRegistry, type FetchMetadataFromFromRegistryOptions, RegistryResponseError } from './fetch.js'
import { normalizeRegistryUrl } from './normalizeRegistryUrl.js'
import {
  BUILTIN_NAMED_REGISTRIES,
  parseBareSpecifier,
  parseJsrSpecifierToRegistryPackageSpec,
  parseNamedRegistrySpecifierToRegistryPackageSpec,
  type RegistryPackageSpec,
} from './parseBareSpecifier.js'
import {
  type PackageMetaCache,
  pickPackage,
  type PickPackageOptions,
} from './pickPackage.js'
import { pickPackageFromMeta, pickVersionByVersionRange } from './pickPackageFromMeta.js'
import { failIfTrustDowngraded } from './trustChecks.js'
import { MINIMUM_RELEASE_AGE_VIOLATION_CODE } from './violationCodes.js'
import { whichVersionIsPinned } from './whichVersionIsPinned.js'
import { workspacePrefToNpm } from './workspacePrefToNpm.js'

export interface NoMatchingVersionErrorOptions {
  wantedDependency: WantedDependency
  packageMeta: PackageMeta
  registry: string
}

export class NoMatchingVersionError extends PnpmError {
  public readonly packageMeta: PackageMeta
  constructor (opts: NoMatchingVersionErrorOptions) {
    const dep = opts.wantedDependency.alias
      ? `${opts.wantedDependency.alias}@${opts.wantedDependency.bareSpecifier ?? ''}`
      : opts.wantedDependency.bareSpecifier!
    super('NO_MATCHING_VERSION', `No matching version found for ${dep} while fetching it from ${opts.registry}`)
    this.packageMeta = opts.packageMeta
  }
}

export function formatTimeAgo (date: Date): string | null {
  const ts = date.getTime()
  if (isNaN(ts)) {
    return null
  }
  const now = Date.now()
  const diffMs = now - ts

  // Handle clock skew (future dates)
  if (diffMs < 0) {
    return null
  }

  const diffSec = Math.floor(diffMs / 1000)
  const diffMin = Math.floor(diffSec / 60)
  const diffHour = Math.floor(diffMin / 60)
  const diffDay = Math.floor(diffHour / 24)
  const diffMonth = Math.floor(diffDay / 30)
  const diffYear = Math.floor(diffDay / 365)

  if (diffYear > 0) return `${diffYear} year${diffYear === 1 ? '' : 's'} ago`
  if (diffMonth > 0) return `${diffMonth} month${diffMonth === 1 ? '' : 's'} ago`
  if (diffDay > 0) return `${diffDay} day${diffDay === 1 ? '' : 's'} ago`
  if (diffHour > 0) return `${diffHour} hour${diffHour === 1 ? '' : 's'} ago`
  if (diffMin > 0) return `${diffMin} minute${diffMin === 1 ? '' : 's'} ago`
  return 'a few seconds ago'
}

export {
  BUILTIN_NAMED_REGISTRIES,
  fetchMetadataFromFromRegistry,
  type FetchMetadataFromFromRegistryOptions,
  type PackageMeta,
  type PackageMetaCache,
  parseBareSpecifier,
  pickPackageFromMeta,
  pickVersionByVersionRange,
  type RegistryPackageSpec,
  RegistryResponseError,
  workspacePrefToNpm,
}
export { createNpmResolutionVerifier, type CreateNpmResolutionVerifierOptions } from './createNpmResolutionVerifier.js'
export {
  MINIMUM_RELEASE_AGE_VIOLATION_CODE,
  TRUST_DOWNGRADE_VIOLATION_CODE,
} from './violationCodes.js'
export { whichVersionIsPinned } from './whichVersionIsPinned.js'

export interface ResolverFactoryOptions {
  cacheDir: string
  storeDir?: string
  frozenStore?: boolean
  fullMetadata?: boolean
  filterMetadata?: boolean
  offline?: boolean
  preferOffline?: boolean
  retry?: RetryTimeoutOptions
  timeout?: number
  registries: Registries
  namedRegistries?: Record<string, string>
  saveWorkspaceProtocol?: boolean | 'rolling'
  preserveAbsolutePaths?: boolean
  ignoreMissingTimeField?: boolean
  fetchWarnTimeoutMs?: number
  /** Pre-populated metadata cache. When provided, the resolver uses this
   *  instead of creating a new LRU cache. Useful for servers that keep
   *  metadata in SQLite or persist it across requests. */
  metaCache?: PackageMetaCache
}

export interface NpmResolveResult extends ResolveResult {
  latest?: string
  manifest: DependencyManifest
  resolution: TarballResolution
  resolvedVia: 'npm-registry'
}

export interface JsrResolveResult extends ResolveResult {
  alias: string
  manifest: DependencyManifest
  resolution: TarballResolution
  resolvedVia: 'jsr-registry'
}

export interface NamedRegistryResolveResult extends ResolveResult {
  alias: string
  /** The named-registry alias that was matched, e.g. `gh` or a user-defined name. */
  registryName: string
  manifest: DependencyManifest
  resolution: TarballResolution
  resolvedVia: 'named-registry'
}

export interface WorkspaceResolveResult extends ResolveResult {
  manifest: DependencyManifest
  resolution: DirectoryResolution
  resolvedVia: 'workspace'
}

export type NpmResolver = (
  wantedDependency: WantedDependency & { optional?: boolean },
  opts: ResolveFromNpmOptions
) => Promise<NpmResolveResult | JsrResolveResult | NamedRegistryResolveResult | WorkspaceResolveResult | null>

export type ResolveLatestFromNpmStyle = (
  query: LatestQuery,
  opts: ResolveOptions
) => Promise<LatestInfo | undefined>

export function createNpmResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: ResolverFactoryOptions
): {
  resolveFromNpm: NpmResolver
  resolveFromJsr: NpmResolver
  resolveFromNamedRegistry: NpmResolver
  resolveLatestFromNpm: ResolveLatestFromNpmStyle
  resolveLatestFromJsr: ResolveLatestFromNpmStyle
  resolveLatestFromNamedRegistry: ResolveLatestFromNpmStyle
  clearCache: () => void
} {
  if (typeof opts.cacheDir !== 'string') {
    throw new TypeError('`opts.cacheDir` is required and needs to be a string')
  }
  const fetchOpts: FetchMetadataFromFromRegistryOptions = {
    fetch: fetchFromRegistry,
    retry: opts.retry ?? {},
    timeout: opts.timeout ?? 60000,
    fetchWarnTimeoutMs: opts.fetchWarnTimeoutMs ?? 10 * 1000, // 10 sec
  }
  const fetch = pMemoize(fetchMetadataFromFromRegistry.bind(null, fetchOpts), {
    cacheKey: (...args) => JSON.stringify(args),
  })
  // Track ownership so `clearCache()` below only wipes the in-memory
  // cache when this factory created it. A caller-supplied
  // `opts.metaCache` may be shared with another resolver instance (or
  // outlive this resolver entirely — e.g. a long-lived agent process
  // that keeps one cache across many install requests); clearing it
  // here would silently evict entries that other consumers are still
  // using.
  const ownsMetaCache = opts.metaCache == null
  const metaCache: PackageMetaCache = opts.metaCache ?? createDefaultPackageMetaCache()
  // Create peek function if storeDir is provided
  const storeDir = opts.storeDir
  const peekLockerForPeek = new Map<string, Promise<DependencyManifest | undefined>>()
  let peekManifestFromStore: ResolveFromNpmContext['peekManifestFromStore'] | undefined
  if (storeDir) {
    peekManifestFromStore = async (peekOpts) => {
      const filesIndexFile = storeIndexKey(peekOpts.integrity, peekOpts.id)
      const existingRequest = peekLockerForPeek.get(filesIndexFile)
      if (existingRequest != null) {
        return existingRequest
      }
      const request = readPkgFromCafs(
        {
          storeDir,
          verifyStoreIntegrity: false,
          frozenStore: opts.frozenStore,
        },
        filesIndexFile,
        {
          expectedPkg: { name: peekOpts.name, version: peekOpts.version },
        }
      ).then(({ bundledManifest }) => {
        if (!bundledManifest) return undefined
        return bundledManifest as DependencyManifest
      }).catch(() => undefined)
      peekLockerForPeek.set(filesIndexFile, request)
      return request
    }
  }
  const namedRegistries = mergeNamedRegistries(opts.namedRegistries)
  const namedRegistryNames: ReadonlySet<string> = new Set(Object.keys(namedRegistries))
  const ctx: ResolveFromNpmContext = {
    getAuthHeaderValueByURI: getAuthHeader,
    pickPackage: pickPackage.bind(null, {
      fetch,
      fullMetadata: opts.fullMetadata,
      filterMetadata: opts.filterMetadata,
      metaCache,
      offline: opts.offline,
      preferOffline: opts.preferOffline,
      cacheDir: opts.cacheDir,
      ignoreMissingTimeField: opts.ignoreMissingTimeField,
    }),
    registries: opts.registries,
    namedRegistries,
    namedRegistryNames,
    saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
    peekManifestFromStore,
  }
  const boundResolveFromNpm = resolveNpm.bind(null, ctx)
  const boundResolveFromJsr = resolveJsr.bind(null, ctx)
  const boundResolveFromNamedRegistry = resolveFromNamedRegistry.bind(null, ctx)
  const defaultRegistry = opts.registries.default
  return {
    resolveFromNpm: boundResolveFromNpm,
    resolveFromJsr: boundResolveFromJsr,
    resolveFromNamedRegistry: boundResolveFromNamedRegistry,
    resolveLatestFromNpm: createResolveLatest(boundResolveFromNpm,
      (query) => isNpmSpec(query, defaultRegistry)),
    resolveLatestFromJsr: createResolveLatest(boundResolveFromJsr, isJsrSpec),
    resolveLatestFromNamedRegistry: createResolveLatest(boundResolveFromNamedRegistry,
      (query) => isNamedRegistrySpec(query, ctx.namedRegistryNames)),
    clearCache: () => {
      if (ownsMetaCache && 'clear' in metaCache && typeof metaCache.clear === 'function') {
        metaCache.clear()
      }
      pMemoizeClear(fetch)
    },
  }
}

function isNpmSpec (query: LatestQuery, defaultRegistry: string): boolean {
  const { alias, bareSpecifier } = query.wantedDependency
  if (!bareSpecifier) return alias != null
  return parseBareSpecifier(bareSpecifier, alias, 'latest', defaultRegistry) != null
}

function isJsrSpec (query: LatestQuery): boolean {
  if (!query.wantedDependency.bareSpecifier?.startsWith('jsr:')) return false
  return parseJsrSpecifierToRegistryPackageSpec(
    query.wantedDependency.bareSpecifier,
    query.wantedDependency.alias,
    'latest'
  ) != null
}

function isNamedRegistrySpec (
  query: LatestQuery,
  knownRegistryNames: ReadonlySet<string>
): boolean {
  if (!query.wantedDependency.bareSpecifier) return false
  try {
    return parseNamedRegistrySpecifierToRegistryPackageSpec(
      query.wantedDependency.bareSpecifier,
      knownRegistryNames,
      query.wantedDependency.alias,
      'latest'
    ) != null
  } catch {
    return false
  }
}

function createResolveLatest (
  resolve: NpmResolver,
  matches: (query: LatestQuery) => boolean
) {
  return async (query: LatestQuery, opts: ResolveOptions): Promise<LatestInfo | undefined> => {
    if (!matches(query)) return undefined
    // Always pass the manifest's bareSpecifier so protocol-prefixed specs
    // (`jsr:@scope/pkg@^1.0.0`, `gh:owner/repo@^1.0.0`) still match their
    // resolver. In --compatible mode that range drives the pick; otherwise
    // `update: 'latest'` tells the resolver to ignore the range and take
    // the absolute newest.
    const bareSpecifier = query.wantedDependency.bareSpecifier ?? 'latest'
    const resolveOpts = query.compatible ? opts : { ...opts, update: 'latest' as const }
    try {
      const result = await resolve(
        { alias: query.wantedDependency.alias, bareSpecifier },
        resolveOpts
      )
      // Policy-blocked: handled but no latest to surface.
      if (result?.policyViolation?.code === MINIMUM_RELEASE_AGE_VIOLATION_CODE) {
        return {}
      }
      return { latestManifest: result?.manifest }
    } catch (err) {
      if (opts.publishedBy && (err as { code?: string }).code === 'ERR_PNPM_NO_MATCHING_VERSION') {
        return {}
      }
      throw err
    }
  }
}

export interface ResolveFromNpmContext {
  pickPackage: (spec: RegistryPackageSpec, opts: PickPackageOptions) => ReturnType<typeof pickPackage>
  getAuthHeaderValueByURI: GetAuthHeader
  registries: Registries
  namedRegistries: Record<string, string>
  namedRegistryNames: ReadonlySet<string>
  saveWorkspaceProtocol?: boolean | 'rolling'
  peekManifestFromStore?: (opts: {
    id: PkgResolutionId
    integrity: string
    name?: string
    version?: string
  }) => Promise<DependencyManifest | undefined>
}

export type ResolveFromNpmOptions = {
  alwaysTryWorkspacePackages?: boolean
  defaultTag?: string
  publishedBy?: Date
  publishedByExclude?: PackageVersionPolicy
  pickLowestVersion?: boolean
  trustPolicy?: TrustPolicy
  trustPolicyExclude?: PackageVersionPolicy
  trustPolicyIgnoreAfter?: number
  dryRun?: boolean
  lockfileDir?: string
  preferredVersions?: PreferredVersions
  preferWorkspacePackages?: boolean
  update?: false | 'compatible' | 'latest'
  updateChecksums?: boolean
  injectWorkspacePackages?: boolean
  calcSpecifier?: boolean
  pinnedVersion?: PinnedVersion
} & ({
  projectDir?: string
  workspacePackages?: undefined
} | {
  projectDir: string
  workspacePackages: WorkspacePackages
})

async function resolveNpm (
  ctx: ResolveFromNpmContext,
  wantedDependency: WantedDependency & { optional?: boolean },
  opts: ResolveFromNpmOptions & {
    currentPkg?: {
      id: PkgResolutionId
      name?: string
      version?: string
      resolution: TarballResolution
      publishedAt?: string
    }
  }
): Promise<NpmResolveResult | WorkspaceResolveResult | null> {
  const defaultTag = opts.defaultTag ?? 'latest'
  const registry = wantedDependency.alias
    ? pickRegistryForPackage(ctx.registries, wantedDependency.alias, wantedDependency.bareSpecifier)
    : ctx.registries.default
  if (wantedDependency.bareSpecifier?.startsWith('workspace:')) {
    if (wantedDependency.bareSpecifier.startsWith('workspace:.')) return null
    const resolvedFromWorkspace = tryResolveFromWorkspace(wantedDependency, {
      defaultTag,
      lockfileDir: opts.lockfileDir,
      projectDir: opts.projectDir,
      registry,
      workspacePackages: opts.workspacePackages,
      injectWorkspacePackages: opts.injectWorkspacePackages,
      update: Boolean(opts.update),
      saveWorkspaceProtocol: ctx.saveWorkspaceProtocol !== false ? ctx.saveWorkspaceProtocol : true,
      calcSpecifier: opts.calcSpecifier,
      pinnedVersion: opts.pinnedVersion,
    })
    if (resolvedFromWorkspace != null) {
      return resolvedFromWorkspace
    }
  }
  const workspacePackages = opts.alwaysTryWorkspacePackages !== false ? opts.workspacePackages : undefined
  const spec = wantedDependency.bareSpecifier
    ? parseBareSpecifier(wantedDependency.bareSpecifier, wantedDependency.alias, defaultTag, registry)
    : defaultTagForAlias(wantedDependency.alias!, defaultTag)
  if (spec == null) return null

  // Fast path: if we have a current resolution with integrity, try to peek the manifest from the store.
  // This avoids the expensive metadata fetch from the registry.
  // We do this AFTER ensuring the spec is valid for this resolver to avoids hijacking other resolvers.
  // If publishedBy is set (resolutionMode=time-based or minimumReleaseAge is configured), we only take
  // the fast path when publishedAt is already known from the lockfile's `time:` block; otherwise we
  // fall through to a registry fetch so the cutoff isn't computed from missing data.
  if (
    ctx.peekManifestFromStore &&
    opts.currentPkg?.resolution &&
    !opts.update &&
    (opts.publishedBy == null || opts.currentPkg.publishedAt != null)
  ) {
    const currentResolution = opts.currentPkg.resolution
    // Only use this optimization for tarball resolutions with integrity (npm packages)
    if ('tarball' in currentResolution && currentResolution.integrity) {
      const manifest = await ctx.peekManifestFromStore({
        id: opts.currentPkg.id,
        integrity: currentResolution.integrity,
        name: opts.currentPkg.name,
        version: opts.currentPkg.version,
      })
      // Verify the manifest matches what we expect
      if (manifest?.name && manifest?.version) {
        const id = `${manifest.name}@${manifest.version}` as PkgResolutionId
        // Only return if the ID matches what we have in currentPkg
        if (id === opts.currentPkg.id) {
          return {
            id,
            manifest,
            resolution: currentResolution as TarballResolution,
            resolvedVia: 'npm-registry',
            publishedAt: opts.currentPkg.publishedAt,
            // Loose-mode bypass: a lockfile entry whose publishedAt sits
            // after the maturity cutoff would have been rejected at
            // resolver time, but the peek path skips the maturity check.
            // Report inline so the deps-resolver aggregator surfaces it
            // to the install command.
            policyViolation: detectMinReleaseAgeViolation({
              name: manifest.name,
              version: manifest.version,
              publishedAt: opts.currentPkg.publishedAt,
              resolution: currentResolution,
              publishedBy: opts.publishedBy,
              publishedByExclude: opts.publishedByExclude,
            }),
          }
        }
      }
    }
  }

  const authHeaderValue = ctx.getAuthHeaderValueByURI(registry, { pkgName: spec.name })
  let pickResult!: { meta: PackageMeta, pickedPackage: PackageInRegistry | null }
  try {
    pickResult = await ctx.pickPackage(spec, {
      pickLowestVersion: opts.pickLowestVersion,
      publishedBy: opts.publishedBy,
      publishedByExclude: opts.publishedByExclude,
      authHeaderValue,
      dryRun: opts.dryRun === true,
      preferredVersionSelectors: opts.preferredVersions?.[spec.name],
      registry,
      includeLatestTag: opts.update === 'latest',
      updateChecksums: opts.updateChecksums,
      optional: wantedDependency.optional,
    })
  } catch (err: any) { // eslint-disable-line
    if ((workspacePackages != null) && opts.projectDir) {
      try {
        return tryResolveFromWorkspacePackages(workspacePackages, spec, {
          wantedDependency,
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: opts.injectWorkspacePackages === true || wantedDependency.injected,
          update: false,
          saveWorkspaceProtocol: ctx.saveWorkspaceProtocol,
          calcSpecifier: opts.calcSpecifier,
          pinnedVersion: opts.pinnedVersion,
        })
      } catch {
        // ignore
      }
    }
    throw err
  }
  const pickedPackage = pickResult.pickedPackage
  const meta = pickResult.meta
  if (pickedPackage == null) {
    if ((workspacePackages != null) && opts.projectDir) {
      try {
        return tryResolveFromWorkspacePackages(workspacePackages, spec, {
          wantedDependency,
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: opts.injectWorkspacePackages === true || wantedDependency.injected,
          update: false,
          saveWorkspaceProtocol: ctx.saveWorkspaceProtocol,
          calcSpecifier: opts.calcSpecifier,
          pinnedVersion: opts.pinnedVersion,
        })
      } catch {
        // ignore
      }
    }

    throw new NoMatchingVersionError({ wantedDependency, packageMeta: meta, registry })
  } else if (opts.trustPolicy === 'no-downgrade') {
    failIfTrustDowngraded(meta, pickedPackage.version, opts)
  }

  const workspacePkgsMatchingName = workspacePackages?.get(pickedPackage.name)
  if (workspacePkgsMatchingName && opts.projectDir) {
    const matchedPkg = workspacePkgsMatchingName.get(pickedPackage.version)
    if (matchedPkg) {
      return {
        ...resolveFromLocalPackage(matchedPkg, spec, {
          wantedDependency,
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: opts.injectWorkspacePackages === true || wantedDependency.injected,
          saveWorkspaceProtocol: ctx.saveWorkspaceProtocol,
          calcSpecifier: opts.calcSpecifier,
          pinnedVersion: opts.pinnedVersion,
        }),
        latest: meta['dist-tags'].latest,
      }
    }
    const localVersion = pickMatchingLocalVersionOrNull(workspacePkgsMatchingName, spec)
    if (localVersion && (semver.gt(localVersion, pickedPackage.version) || opts.preferWorkspacePackages)) {
      return {
        ...resolveFromLocalPackage(workspacePkgsMatchingName.get(localVersion)!, spec, {
          wantedDependency,
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: opts.injectWorkspacePackages === true || wantedDependency.injected,
          saveWorkspaceProtocol: ctx.saveWorkspaceProtocol,
          calcSpecifier: opts.calcSpecifier,
          pinnedVersion: opts.pinnedVersion,
        }),
        latest: meta['dist-tags'].latest,
      }
    }
  }

  const id = `${pickedPackage.name}@${pickedPackage.version}` as PkgResolutionId
  const resolution = {
    integrity: getIntegrity(pickedPackage.dist),
    tarball: normalizeRegistryUrl(pickedPackage.dist.tarball),
  }
  let normalizedBareSpecifier: string | undefined
  if (opts.calcSpecifier) {
    normalizedBareSpecifier = spec.normalizedBareSpecifier ?? calcSpecifier({
      wantedDependency,
      spec,
      version: pickedPackage.version,
      defaultPinnedVersion: opts.pinnedVersion,
    })
  }
  const publishedAt = meta.time?.[pickedPackage.version]
  return {
    id,
    latest: meta['dist-tags'].latest,
    manifest: pickedPackage,
    resolution,
    resolvedVia: 'npm-registry',
    publishedAt,
    normalizedBareSpecifier,
    policyViolation: detectMinReleaseAgeViolation({
      name: pickedPackage.name,
      version: pickedPackage.version,
      publishedAt,
      resolution,
      publishedBy: opts.publishedBy,
      publishedByExclude: opts.publishedByExclude,
    }),
  }
}

async function resolveJsr (
  ctx: ResolveFromNpmContext,
  wantedDependency: WantedDependency & { optional?: boolean },
  opts: Omit<ResolveFromNpmOptions, 'registry'>
): Promise<JsrResolveResult | null> {
  if (!wantedDependency.bareSpecifier) return null

  const spec = parseJsrSpecifierToRegistryPackageSpec(wantedDependency.bareSpecifier, wantedDependency.alias, opts.defaultTag ?? 'latest')
  if (spec == null) return null

  const picked = await pickFromSimpleRegistry(ctx, wantedDependency, opts, spec, ctx.registries['@jsr']!) // '@jsr' is always defined
  return {
    ...picked,
    normalizedBareSpecifier: opts.calcSpecifier
      ? calcPrefixedSpecifier('jsr:', spec.jsrPkgName, wantedDependency, picked.manifest.version, opts.pinnedVersion)
      : undefined,
    resolvedVia: 'jsr-registry',
    alias: spec.jsrPkgName,
  }
}

// Merges user-supplied named-registry aliases (from config) on top of pnpm's
// built-in defaults (e.g. `gh` → GitHub Packages). User entries take precedence
// so GHES users can point `gh` at their enterprise host. URLs are validated
// here so typos like `npm.work.example.com` (no scheme) surface at startup
// rather than as a confusing 404 during resolution. The named-registry
// resolver runs last in the resolution chain, so an alias that collides with
// another specifier scheme (e.g. `git`, `github`, `jsr`) is silently shadowed
// by that scheme's dedicated resolver — no cross-resolver knowledge needed.
function mergeNamedRegistries (userDefined?: Record<string, string>): Record<string, string> {
  const merged: Record<string, string> = { ...BUILTIN_NAMED_REGISTRIES }
  if (!userDefined) return merged
  for (const [alias, url] of Object.entries(userDefined)) {
    if (typeof url !== 'string' || !isValidHttpUrl(url)) {
      throw new PnpmError(
        'INVALID_NAMED_REGISTRY_URL',
        `The named registry alias '${alias}' is mapped to '${String(url)}', which is not a valid http(s) URL.`,
        { hint: 'Provide a URL that starts with http:// or https://, e.g. https://npm.pkg.example.com/' }
      )
    }
    merged[alias] = url
  }
  return merged
}

function isValidHttpUrl (url: string): boolean {
  try {
    const parsed = new URL(url)
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}

// Resolves a `<alias>:` specifier from one of the configured named registries.
// The `gh:` alias ships as a built-in default pointing at the GitHub Packages
// npm registry; additional aliases come from pnpm-workspace.yaml's
// `namedRegistries` field. Auth tokens are looked up by the resolved registry
// URL, so a `//npm.pkg.github.com/:_authToken=...` entry in `.npmrc` is
// picked up automatically for `gh:` specifiers (and analogously for any user-
// configured alias).
async function resolveFromNamedRegistry (
  ctx: ResolveFromNpmContext,
  wantedDependency: WantedDependency & { optional?: boolean },
  opts: Omit<ResolveFromNpmOptions, 'registry'>
): Promise<NamedRegistryResolveResult | null> {
  if (!wantedDependency.bareSpecifier) return null

  const spec = parseNamedRegistrySpecifierToRegistryPackageSpec(
    wantedDependency.bareSpecifier,
    ctx.namedRegistryNames,
    wantedDependency.alias,
    opts.defaultTag ?? 'latest'
  )
  if (spec == null) return null

  const registry = ctx.namedRegistries[spec.registryName]
  if (!registry) return null // defensive: should never trigger because parse checks the alias set

  const picked = await pickFromSimpleRegistry(ctx, wantedDependency, opts, spec, registry)
  return {
    ...picked,
    normalizedBareSpecifier: opts.calcSpecifier
      ? calcPrefixedSpecifier(`${spec.registryName}:`, spec.name, wantedDependency, picked.manifest.version, opts.pinnedVersion)
      : undefined,
    resolvedVia: 'named-registry',
    registryName: spec.registryName,
    // Exposes the scoped package name so callers that omit an explicit alias
    // (e.g. `pnpm add gh:@acme/foo`) record the dependency under `@acme/foo`.
    alias: spec.name,
  }
}

// Shared inner shell for resolvers that pull from a single registry URL with
// an already-parsed RegistryPackageSpec (jsr, named-registry). Returns the
// fields common to their result envelopes; each caller adds its own
// resolvedVia, alias, and normalizedBareSpecifier.
async function pickFromSimpleRegistry (
  ctx: ResolveFromNpmContext,
  wantedDependency: WantedDependency & { optional?: boolean },
  opts: Omit<ResolveFromNpmOptions, 'registry'>,
  spec: RegistryPackageSpec,
  registry: string
): Promise<{
  id: PkgResolutionId
  latest?: string
  manifest: DependencyManifest
  resolution: TarballResolution
  publishedAt?: string
  policyViolation?: ResolutionPolicyViolation
}> {
  const authHeaderValue = ctx.getAuthHeaderValueByURI(registry, { pkgName: spec.name })
  const { meta, pickedPackage } = await ctx.pickPackage(spec, {
    pickLowestVersion: opts.pickLowestVersion,
    publishedBy: opts.publishedBy,
    publishedByExclude: opts.publishedByExclude,
    authHeaderValue,
    dryRun: opts.dryRun === true,
    preferredVersionSelectors: opts.preferredVersions?.[spec.name],
    registry,
    includeLatestTag: opts.update === 'latest',
    updateChecksums: opts.updateChecksums,
    optional: wantedDependency.optional,
  })
  if (pickedPackage == null) {
    throw new NoMatchingVersionError({ wantedDependency, packageMeta: meta, registry })
  }
  const resolution = {
    integrity: getIntegrity(pickedPackage.dist),
    tarball: normalizeRegistryUrl(pickedPackage.dist.tarball),
  }
  const publishedAt = meta.time?.[pickedPackage.version]
  return {
    id: `${pickedPackage.name}@${pickedPackage.version}` as PkgResolutionId,
    latest: meta['dist-tags'].latest,
    manifest: pickedPackage,
    resolution,
    publishedAt,
    policyViolation: detectMinReleaseAgeViolation({
      name: pickedPackage.name,
      version: pickedPackage.version,
      publishedAt,
      resolution,
      publishedBy: opts.publishedBy,
      publishedByExclude: opts.publishedByExclude,
    }),
  }
}

// Builds a `<prefix><pkgName>@<range>` specifier (or a bare `<prefix><range>`
// when the dependency alias matches the package name). Shared between the
// jsr and named-registry resolvers since they only differ in `prefix` and
// which spec field holds the package name.
function calcPrefixedSpecifier (
  prefix: string,
  pkgName: string,
  wantedDependency: WantedDependency,
  version: string,
  defaultPinnedVersion?: PinnedVersion
): string {
  const range = calcRange(version, wantedDependency, defaultPinnedVersion)
  if (!wantedDependency.alias || pkgName === wantedDependency.alias) return `${prefix}${range}`
  return `${prefix}${pkgName}@${range}`
}

function calcSpecifier ({
  wantedDependency,
  spec,
  version,
  defaultPinnedVersion,
}: {
  wantedDependency: WantedDependency
  spec: RegistryPackageSpec
  version: string
  defaultPinnedVersion?: PinnedVersion
}): string {
  if (wantedDependency.prevSpecifier === wantedDependency.bareSpecifier && wantedDependency.prevSpecifier && versionSelectorType(wantedDependency.prevSpecifier)?.type === 'tag') {
    return wantedDependency.prevSpecifier
  }
  const range = calcRange(version, wantedDependency, defaultPinnedVersion)
  if (!wantedDependency.alias || spec.name === wantedDependency.alias) return range
  return `npm:${spec.name}@${range}`
}

function calcRange (version: string, wantedDependency: WantedDependency, defaultPinnedVersion?: PinnedVersion): string {
  if (semver.parse(version)?.prerelease.length) {
    return version
  }
  const pinnedVersion = (wantedDependency.prevSpecifier ? whichVersionIsPinned(wantedDependency.prevSpecifier) : undefined) ??
    (wantedDependency.bareSpecifier ? whichVersionIsPinned(wantedDependency.bareSpecifier) : undefined) ??
    defaultPinnedVersion
  return createVersionSpec(version, pinnedVersion)
}

function tryResolveFromWorkspace (
  wantedDependency: WantedDependency,
  opts: {
    defaultTag: string
    lockfileDir?: string
    projectDir?: string
    registry: string
    workspacePackages?: WorkspacePackages
    injectWorkspacePackages?: boolean
    update?: boolean
    saveWorkspaceProtocol?: boolean | 'rolling'
    calcSpecifier?: boolean
    pinnedVersion?: PinnedVersion
  }
): WorkspaceResolveResult | null {
  if (!wantedDependency.bareSpecifier?.startsWith('workspace:')) {
    return null
  }
  const bareSpecifier = workspacePrefToNpm(wantedDependency.bareSpecifier)

  const spec = parseBareSpecifier(bareSpecifier, wantedDependency.alias, opts.defaultTag, opts.registry)
  if (spec == null) throw new Error(`Invalid workspace: spec (${wantedDependency.bareSpecifier})`)
  if (opts.workspacePackages == null) {
    throw new Error('Cannot resolve package from workspace because opts.workspacePackages is not defined')
  }
  if (!opts.projectDir) {
    throw new Error('Cannot resolve package from workspace because opts.projectDir is not defined')
  }
  return tryResolveFromWorkspacePackages(opts.workspacePackages, spec, {
    wantedDependency,
    projectDir: opts.projectDir,
    hardLinkLocalPackages: opts.injectWorkspacePackages === true || wantedDependency.injected,
    lockfileDir: opts.lockfileDir,
    update: opts.update,
    saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
    calcSpecifier: opts.calcSpecifier,
    pinnedVersion: opts.pinnedVersion,
  })
}

function tryResolveFromWorkspacePackages (
  workspacePackages: WorkspacePackages,
  spec: RegistryPackageSpec,
  opts: {
    wantedDependency: WantedDependency
    hardLinkLocalPackages?: boolean
    projectDir: string
    lockfileDir?: string
    update?: boolean
    saveWorkspaceProtocol?: boolean | 'rolling'
    calcSpecifier?: boolean
    pinnedVersion?: PinnedVersion
  }
): WorkspaceResolveResult {
  const workspacePkgsMatchingName = workspacePackages.get(spec.name)
  if (!workspacePkgsMatchingName) {
    throw new PnpmError(
      'WORKSPACE_PKG_NOT_FOUND',
      `In ${path.relative(process.cwd(), opts.projectDir)}: "${spec.name}@${opts.wantedDependency.bareSpecifier ?? ''}" is in the dependencies but no package named "${spec.name}" is present in the workspace`,
      {
        hint: 'Packages found in the workspace: ' + Array.from(workspacePackages.keys()).join(', '),
      }
    )
  }
  const localVersion = pickMatchingLocalVersionOrNull(
    workspacePkgsMatchingName,
    opts.update ? { name: spec.name, fetchSpec: '*', type: 'range' } : spec
  )
  if (!localVersion) {
    const availableVersions = Array.from(workspacePkgsMatchingName.keys()).sort((a, b) => semver.rcompare(a, b))
    throw new PnpmError(
      'NO_MATCHING_VERSION_INSIDE_WORKSPACE',
      `In ${path.relative(process.cwd(), opts.projectDir)}: No matching version found for ${opts.wantedDependency.alias ?? ''}@${opts.wantedDependency.bareSpecifier ?? ''} inside the workspace` +
      (availableVersions.length ? `. Available versions: ${availableVersions.join(', ')}` : ''),
      availableVersions.length
        ? {
          hint: `Available workspace versions for "${spec.name}": ${availableVersions.join(', ')}`,
        }
        : undefined
    )
  }
  return resolveFromLocalPackage(workspacePkgsMatchingName.get(localVersion)!, spec, opts)
}

function pickMatchingLocalVersionOrNull (
  versions: WorkspacePackagesByVersion,
  spec: RegistryPackageSpec
): string | null {
  switch (spec.type) {
    case 'tag':
      return semver.maxSatisfying(Array.from(versions.keys()), '*', {
        includePrerelease: true,
      })
    case 'version':
      return versions.has(spec.fetchSpec) ? spec.fetchSpec : null
    case 'range':
      return resolveWorkspaceRange(spec.fetchSpec, Array.from(versions.keys()))
    default:
      return null
  }
}

function resolveFromLocalPackage (
  localPackage: WorkspacePackage,
  spec: RegistryPackageSpec,
  opts: {
    wantedDependency: WantedDependency
    hardLinkLocalPackages?: boolean
    projectDir: string
    lockfileDir?: string
    saveWorkspaceProtocol?: boolean | 'rolling'
    calcSpecifier?: boolean
    pinnedVersion?: PinnedVersion
  }
): WorkspaceResolveResult {
  let id!: PkgResolutionId
  let directory!: string
  const localPackageDir = resolveLocalPackageDir(localPackage)
  if (opts.hardLinkLocalPackages) {
    directory = normalize(path.relative(opts.lockfileDir!, localPackageDir))
    id = `file:${directory}` as PkgResolutionId
  } else {
    directory = localPackageDir
    id = `link:${normalize(path.relative(opts.projectDir, localPackageDir))}` as PkgResolutionId
  }
  let normalizedBareSpecifier: string | undefined
  if (opts.calcSpecifier) {
    normalizedBareSpecifier = spec.normalizedBareSpecifier ?? calcSpecifierForWorkspaceDep({
      wantedDependency: opts.wantedDependency,
      spec,
      saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
      version: localPackage.manifest.version,
      defaultPinnedVersion: opts.pinnedVersion,
    })
  }
  return {
    id,
    manifest: clone(localPackage.manifest),
    resolution: {
      directory,
      type: 'directory',
    },
    resolvedVia: 'workspace',
    normalizedBareSpecifier,
  }
}

function calcSpecifierForWorkspaceDep ({
  wantedDependency,
  spec,
  saveWorkspaceProtocol,
  version,
  defaultPinnedVersion,
}: {
  wantedDependency: WantedDependency
  spec: RegistryPackageSpec
  saveWorkspaceProtocol: boolean | 'rolling' | undefined
  version: string
  defaultPinnedVersion?: PinnedVersion
}): string {
  if (!saveWorkspaceProtocol && !wantedDependency.bareSpecifier?.startsWith('workspace:')) {
    return calcSpecifier({ wantedDependency, spec, version, defaultPinnedVersion })
  }
  const prefix = (!wantedDependency.alias || spec.name === wantedDependency.alias) ? 'workspace:' : `workspace:${spec.name}@`
  if (saveWorkspaceProtocol === 'rolling') {
    const specifier = wantedDependency.prevSpecifier ?? wantedDependency.bareSpecifier
    if (specifier) {
      if ([`${prefix}*`, `${prefix}^`, `${prefix}~`].includes(specifier)) return specifier
      const pinnedVersion = whichVersionIsPinned(specifier)
      switch (pinnedVersion) {
        case 'major': return `${prefix}^`
        case 'minor': return `${prefix}~`
        case 'patch':
        case 'none': return `${prefix}*`
      }
    }
    return `${prefix}^`
  }
  if (semver.parse(version)?.prerelease.length) {
    return `${prefix}${version}`
  }
  const pinnedVersion = (wantedDependency.prevSpecifier ? whichVersionIsPinned(wantedDependency.prevSpecifier) : undefined) ?? defaultPinnedVersion
  const range = createVersionSpec(version, pinnedVersion)
  return `${prefix}${range}`
}

function resolveLocalPackageDir (localPackage: WorkspacePackage): string {
  if (
    localPackage.manifest.publishConfig?.directory == null ||
    localPackage.manifest.publishConfig?.linkDirectory === false
  ) return localPackage.rootDir
  return path.join(localPackage.rootDir, localPackage.manifest.publishConfig.directory)
}

function defaultTagForAlias (alias: string, defaultTag: string): RegistryPackageSpec {
  return {
    fetchSpec: defaultTag,
    name: alias,
    type: 'tag',
  }
}

/**
 * Inline minimumReleaseAge detection: returns a violation entry when the
 * picked version's publish timestamp is past the policy cutoff (and
 * isn't covered by `publishedByExclude`). The resolver already has the
 * timestamp in hand, so reporting inline saves the install layer from
 * re-walking the resolved tree and re-fetching the same metadata. The
 * deps-resolver aggregates the per-resolve `policyViolation` fields into
 * a single set the install command reacts to.
 *
 * Returns `undefined` for resolutions outside the policy — no policy
 * active, version excluded by pattern, timestamp missing or malformed,
 * or version mature. Specific-version exclusions (`pkg@1.0.0`) and
 * full-name exclusions (`pkg`) are both honored so an entry already on
 * the user's exclude list isn't re-announced every install.
 */
function detectMinReleaseAgeViolation (args: {
  name: string
  version: string
  publishedAt: string | undefined
  resolution: Resolution
  publishedBy: Date | undefined
  publishedByExclude: PackageVersionPolicy | undefined
}): ResolutionPolicyViolation | undefined {
  if (!args.publishedBy || !args.publishedAt) return undefined
  const excludeResult = args.publishedByExclude?.(args.name)
  if (excludeResult === true) return undefined
  if (Array.isArray(excludeResult) && excludeResult.includes(args.version)) return undefined
  const ts = new Date(args.publishedAt).getTime()
  if (Number.isNaN(ts) || ts <= args.publishedBy.getTime()) return undefined
  return {
    name: args.name,
    version: args.version,
    resolution: args.resolution,
    code: MINIMUM_RELEASE_AGE_VIOLATION_CODE,
    reason: `was published at ${new Date(ts).toISOString()}, within the minimumReleaseAge cutoff (${args.publishedBy.toISOString()})`,
  }
}

function getIntegrity (dist: {
  integrity?: string
  shasum: string
  tarball: string
}): string | undefined {
  if (dist.integrity) {
    return dist.integrity
  }
  if (!dist.shasum) {
    return undefined
  }
  const integrity = ssri.fromHex(dist.shasum, 'sha1')
  if (!integrity) {
    throw new PnpmError('INVALID_TARBALL_INTEGRITY', `Tarball "${dist.tarball}" has invalid shasum specified in its metadata: ${dist.shasum}`)
  }
  return integrity.toString()
}

function createVersionSpec (version: string, pinnedVersion?: PinnedVersion): string {
  switch (pinnedVersion ?? 'major') {
    case 'none':
    case 'major':
      return `^${version}`
    case 'minor':
      return `~${version}`
    case 'patch':
      return version
    default:
      throw new PnpmError('BAD_PINNED_VERSION', `Cannot pin '${pinnedVersion ?? 'undefined'}'`)
  }
}

/**
 * Construct the LRU `PackageMetaCache` instance the resolver uses by
 * default. Exported so the install layer can build one cache and hand
 * the same reference to both the resolver and the verifier — the
 * verifier's fast path reads from it when the resolver has already
 * fetched a packument during the same install.
 */
export function createDefaultPackageMetaCache (): PackageMetaCache {
  return new LRUCache<string, PackageMeta>({
    max: 10000,
    ttl: 120 * 1000, // 2 minutes
  })
}
