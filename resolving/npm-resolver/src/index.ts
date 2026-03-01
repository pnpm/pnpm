import path from 'path'
import { PnpmError } from '@pnpm/error'
import {
  type FetchFromRegistry,
  type GetAuthHeader,
  type RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { type PackageMeta, type PackageInRegistry } from '@pnpm/registry.types'
import { resolveWorkspaceRange } from '@pnpm/resolve-workspace-range'
import {
  type DirectoryResolution,
  type PkgResolutionId,
  type PreferredVersions,
  type ResolveResult,
  type TarballResolution,
  type WantedDependency,
  type WorkspacePackage,
  type WorkspacePackages,
  type WorkspacePackagesByVersion,
} from '@pnpm/resolver-base'
import { getIndexFilePathInCafs } from '@pnpm/store.cafs'
import {
  readPkgFromCafs,
} from '@pnpm/worker'
import {
  type DependencyManifest,
  type PackageVersionPolicy,
  type PinnedVersion,
  type Registries,
  type TrustPolicy,
} from '@pnpm/types'
import { LRUCache } from 'lru-cache'
import normalize from 'normalize-path'
import pMemoize from 'p-memoize'
import { clone } from 'ramda'
import semver from 'semver'
import ssri from 'ssri'
import versionSelectorType from 'version-selector-type'
import {
  type PackageMetaCache,
  type PickPackageOptions,
  pickPackage,
} from './pickPackage.js'
import {
  parseJsrSpecifierToRegistryPackageSpec,
  parseBareSpecifier,
  type JsrRegistryPackageSpec,
  type RegistryPackageSpec,
} from './parseBareSpecifier.js'
import { fetchMetadataFromFromRegistry, type FetchMetadataFromFromRegistryOptions, RegistryResponseError } from './fetch.js'
import { workspacePrefToNpm } from './workspacePrefToNpm.js'
import { whichVersionIsPinned } from './whichVersionIsPinned.js'
import { pickVersionByVersionRange } from './pickPackageFromMeta.js'
import { failIfTrustDowngraded } from './trustChecks.js'
import { normalizeRegistryUrl } from './normalizeRegistryUrl.js'

export interface NoMatchingVersionErrorOptions {
  wantedDependency: WantedDependency
  packageMeta: PackageMeta
  registry: string
  immatureVersion?: string
  publishedBy?: Date
}

export class NoMatchingVersionError extends PnpmError {
  public readonly packageMeta: PackageMeta
  public readonly immatureVersion?: string
  constructor (opts: NoMatchingVersionErrorOptions) {
    const dep = opts.wantedDependency.alias
      ? `${opts.wantedDependency.alias}@${opts.wantedDependency.bareSpecifier ?? ''}`
      : opts.wantedDependency.bareSpecifier!
    let errorMessage: string
    if (opts.publishedBy && opts.immatureVersion && opts.packageMeta.time) {
      const time = new Date(opts.packageMeta.time[opts.immatureVersion])
      const releaseAgeText = formatTimeAgo(time)
      const pkgName = opts.wantedDependency.alias ?? opts.packageMeta.name
      errorMessage = `Version ${opts.immatureVersion} (released ${releaseAgeText}) of ${pkgName} does not meet the minimumReleaseAge constraint`
    } else {
      errorMessage = `No matching version found for ${dep} while fetching it from ${opts.registry}`
    }
    super(opts.publishedBy ? 'NO_MATURE_MATCHING_VERSION' : 'NO_MATCHING_VERSION', errorMessage)
    this.packageMeta = opts.packageMeta
    this.immatureVersion = opts.immatureVersion
  }
}

function formatTimeAgo (date: Date): string {
  const now = Date.now()
  const diffMs = now - date.getTime()

  // Handle clock skew (future dates) and very recent releases (< 1 minute)
  if (diffMs < 60 * 1000) {
    return 'just now'
  }

  const diffMinutes = Math.floor(diffMs / (60 * 1000))
  const diffHours = Math.floor(diffMs / (60 * 60 * 1000))
  const diffDays = Math.floor(diffMs / (24 * 60 * 60 * 1000))

  if (diffHours >= 48) {
    return `${diffDays} day${diffDays === 1 ? '' : 's'} ago`
  }
  if (diffMinutes >= 90) {
    return `${diffHours} hour${diffHours === 1 ? '' : 's'} ago`
  }
  return `${diffMinutes} minute${diffMinutes === 1 ? '' : 's'} ago`
}

export {
  parseBareSpecifier,
  workspacePrefToNpm,
  type PackageMeta,
  type PackageMetaCache,
  type RegistryPackageSpec,
  RegistryResponseError,
}

export interface ResolverFactoryOptions {
  cacheDir: string
  storeDir?: string
  fullMetadata?: boolean
  filterMetadata?: boolean
  offline?: boolean
  preferOffline?: boolean
  retry?: RetryTimeoutOptions
  timeout?: number
  registries: Registries
  saveWorkspaceProtocol?: boolean | 'rolling'
  preserveAbsolutePaths?: boolean
  strictPublishedByCheck?: boolean
  fetchWarnTimeoutMs?: number
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

export interface WorkspaceResolveResult extends ResolveResult {
  manifest: DependencyManifest
  resolution: DirectoryResolution
  resolvedVia: 'workspace'
}

export type NpmResolver = (
  wantedDependency: WantedDependency & { optional?: boolean },
  opts: ResolveFromNpmOptions
) => Promise<NpmResolveResult | JsrResolveResult | WorkspaceResolveResult | null>

export function createNpmResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: ResolverFactoryOptions
): { resolveFromNpm: NpmResolver, resolveFromJsr: NpmResolver, clearCache: () => void } {
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
  const metaCache = new LRUCache<string, PackageMeta>({
    max: 10000,
    ttl: 120 * 1000, // 2 minutes
  })
  // Create peek function if storeDir is provided
  const storeDir = opts.storeDir
  const peekLockerForPeek = new Map<string, Promise<DependencyManifest | undefined>>()
  let peekManifestFromStore: ResolveFromNpmContext['peekManifestFromStore'] | undefined
  if (storeDir) {
    peekManifestFromStore = async (peekOpts) => {
      const filesIndexFile = getIndexFilePathInCafs(storeDir, peekOpts.integrity, peekOpts.id)
      const existingRequest = peekLockerForPeek.get(filesIndexFile)
      if (existingRequest != null) {
        return existingRequest
      }
      const request = readPkgFromCafs(
        {
          storeDir,
          verifyStoreIntegrity: false,
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
      strictPublishedByCheck: opts.strictPublishedByCheck,
    }),
    registries: opts.registries,
    saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
    peekManifestFromStore,
  }
  return {
    resolveFromNpm: resolveNpm.bind(null, ctx),
    resolveFromJsr: resolveJsr.bind(null, ctx),
    clearCache: () => {
      metaCache.clear()
    },
  }
}

export interface ResolveFromNpmContext {
  pickPackage: (spec: RegistryPackageSpec, opts: PickPackageOptions) => ReturnType<typeof pickPackage>
  getAuthHeaderValueByURI: (registry: string) => string | undefined
  registries: Registries
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
  if (ctx.peekManifestFromStore && opts.currentPkg?.resolution && !opts.update) {
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
            publishedAt: undefined, // Don't have this without metadata
          }
        }
      }
    }
  }

  const authHeaderValue = ctx.getAuthHeaderValueByURI(registry)
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
      updateToLatest: opts.update === 'latest',
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

    if (opts.publishedBy) {
      const immatureVersion = pickVersionByVersionRange({
        meta,
        versionRange: spec.fetchSpec,
        preferredVersionSelectors: opts.preferredVersions?.[spec.name],
      })
      if (immatureVersion) {
        throw new NoMatchingVersionError({
          wantedDependency,
          packageMeta: meta,
          registry,
          immatureVersion,
          publishedBy: opts.publishedBy,
        })
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
  return {
    id,
    latest: meta['dist-tags'].latest,
    manifest: pickedPackage,
    resolution,
    resolvedVia: 'npm-registry',
    publishedAt: meta.time?.[pickedPackage.version],
    normalizedBareSpecifier,
  }
}

async function resolveJsr (
  ctx: ResolveFromNpmContext,
  wantedDependency: WantedDependency,
  opts: Omit<ResolveFromNpmOptions, 'registry'>
): Promise<JsrResolveResult | null> {
  if (!wantedDependency.bareSpecifier) return null
  const defaultTag = opts.defaultTag ?? 'latest'

  const registry = ctx.registries['@jsr']! // '@jsr' is always defined
  const spec = parseJsrSpecifierToRegistryPackageSpec(wantedDependency.bareSpecifier, wantedDependency.alias, defaultTag)
  if (spec == null) return null

  const authHeaderValue = ctx.getAuthHeaderValueByURI(registry)
  const { meta, pickedPackage } = await ctx.pickPackage(spec, {
    pickLowestVersion: opts.pickLowestVersion,
    publishedBy: opts.publishedBy,
    authHeaderValue,
    dryRun: opts.dryRun === true,
    preferredVersionSelectors: opts.preferredVersions?.[spec.name],
    registry,
    updateToLatest: opts.update === 'latest',
  })

  if (pickedPackage == null) {
    throw new NoMatchingVersionError({ wantedDependency, packageMeta: meta, registry })
  }

  const id = `${pickedPackage.name}@${pickedPackage.version}` as PkgResolutionId
  const resolution = {
    integrity: getIntegrity(pickedPackage.dist),
    tarball: normalizeRegistryUrl(pickedPackage.dist.tarball),
  }
  return {
    id,
    latest: meta['dist-tags'].latest,
    manifest: pickedPackage,
    normalizedBareSpecifier: opts.calcSpecifier
      ? calcJsrSpecifier({
        wantedDependency,
        spec,
        version: pickedPackage.version,
        defaultPinnedVersion: opts.pinnedVersion,
      })
      : undefined,
    resolution,
    resolvedVia: 'jsr-registry',
    publishedAt: meta.time?.[pickedPackage.version],
    alias: spec.jsrPkgName,
  }
}

function calcJsrSpecifier ({
  wantedDependency,
  spec,
  version,
  defaultPinnedVersion,
}: {
  wantedDependency: WantedDependency
  spec: JsrRegistryPackageSpec
  version: string
  defaultPinnedVersion?: PinnedVersion
}): string {
  const range = calcRange(version, wantedDependency, defaultPinnedVersion)
  if (!wantedDependency.alias || spec.jsrPkgName === wantedDependency.alias) return `jsr:${range}`
  return `jsr:${spec.jsrPkgName}@${range}`
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
