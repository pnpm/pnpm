import path from 'path'
import { FULL_META_DIR, FULL_FILTERED_META_DIR, ABBREVIATED_META_DIR } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import {
  type FetchFromRegistry,
  type GetAuthHeader,
  type RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import { resolveWorkspaceRange } from '@pnpm/resolve-workspace-range'
import {
  type PkgResolutionId,
  type PreferredVersions,
  type ResolveResult,
  type WantedDependency,
  type WorkspacePackage,
  type WorkspacePackages,
  type WorkspacePackagesByVersion,
  type WorkspaceResolveResult,
} from '@pnpm/resolver-base'
import { whichVersionIsPinned } from '@pnpm/which-version-is-pinned'
import { type Registries, type PinnedVersion } from '@pnpm/types'
import { LRUCache } from 'lru-cache'
import normalize from 'normalize-path'
import pMemoize from 'p-memoize'
import clone from 'ramda/src/clone'
import semver from 'semver'
import ssri from 'ssri'
import versionSelectorType from 'version-selector-type'
import {
  type PackageInRegistry,
  type PackageMeta,
  type PackageMetaCache,
  type PickPackageOptions,
  pickPackage,
} from './pickPackage'
import {
  parsePref,
  type RegistryPackageSpec,
} from './parsePref'
import { fromRegistry, RegistryResponseError } from './fetch'
import { workspacePrefToNpm } from './workspacePrefToNpm'

export class NoMatchingVersionError extends PnpmError {
  public readonly packageMeta: PackageMeta
  constructor (opts: { wantedDependency: WantedDependency, packageMeta: PackageMeta, registry: string }) {
    const dep = opts.wantedDependency.alias
      ? `${opts.wantedDependency.alias}@${opts.wantedDependency.pref ?? ''}`
      : opts.wantedDependency.pref!
    super('NO_MATCHING_VERSION', `No matching version found for ${dep} while fetching it from ${opts.registry}`)
    this.packageMeta = opts.packageMeta
  }
}

export {
  parsePref,
  workspacePrefToNpm,
  type PackageMeta,
  type PackageMetaCache,
  type RegistryPackageSpec,
  RegistryResponseError,
}

export interface ResolverFactoryOptions {
  cacheDir: string
  fullMetadata?: boolean
  filterMetadata?: boolean
  offline?: boolean
  preferOffline?: boolean
  retry?: RetryTimeoutOptions
  timeout?: number
  registries: Registries
  saveWorkspaceProtocol?: boolean | 'rolling'
}

export type NpmResolver = (wantedDependency: WantedDependency, opts: ResolveFromNpmOptions) => Promise<ResolveResult | null>

export function createNpmResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: ResolverFactoryOptions
): { resolveFromNpm: NpmResolver, clearCache: () => void } {
  if (typeof opts.cacheDir !== 'string') {
    throw new TypeError('`opts.cacheDir` is required and needs to be a string')
  }
  const fetchOpts = {
    retry: opts.retry ?? {},
    timeout: opts.timeout ?? 60000,
  }
  const fetch = pMemoize(fromRegistry.bind(null, fetchFromRegistry, fetchOpts), {
    cacheKey: (...args) => JSON.stringify(args),
    maxAge: 1000 * 20, // 20 seconds
  })
  const metaCache = new LRUCache<string, PackageMeta>({
    max: 10000,
    ttl: 120 * 1000, // 2 minutes
  })
  return {
    resolveFromNpm: resolveNpm.bind(null, {
      getAuthHeaderValueByURI: getAuthHeader,
      pickPackage: pickPackage.bind(null, {
        fetch,
        filterMetadata: opts.filterMetadata,
        metaCache,
        metaDir: opts.fullMetadata ? (opts.filterMetadata ? FULL_FILTERED_META_DIR : FULL_META_DIR) : ABBREVIATED_META_DIR,
        offline: opts.offline,
        preferOffline: opts.preferOffline,
        cacheDir: opts.cacheDir,
      }),
      registries: opts.registries,
      saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
    }),
    clearCache: () => {
      metaCache.clear()
    },
  }
}

export type ResolveFromNpmOptions = {
  alwaysTryWorkspacePackages?: boolean
  defaultTag?: string
  publishedBy?: Date
  pickLowestVersion?: boolean
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
  ctx: {
    pickPackage: (spec: RegistryPackageSpec, opts: PickPackageOptions) => ReturnType<typeof pickPackage>
    getAuthHeaderValueByURI: (registry: string) => string | undefined
    registries: Registries
    saveWorkspaceProtocol?: boolean | 'rolling'
  },
  wantedDependency: WantedDependency,
  opts: ResolveFromNpmOptions
): Promise<ResolveResult | null> {
  const defaultTag = opts.defaultTag ?? 'latest'
  const registry = wantedDependency.alias
    ? pickRegistryForPackage(ctx.registries, wantedDependency.alias, wantedDependency.pref)
    : ctx.registries.default
  if (wantedDependency.pref?.startsWith('workspace:')) {
    if (wantedDependency.pref.startsWith('workspace:.')) return null
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
  const spec = wantedDependency.pref
    ? parsePref(wantedDependency.pref, wantedDependency.alias, defaultTag, registry)
    : defaultTagForAlias(wantedDependency.alias!, defaultTag)
  if (spec == null) return null

  const authHeaderValue = ctx.getAuthHeaderValueByURI(registry)
  let pickResult!: { meta: PackageMeta, pickedPackage: PackageInRegistry | null }
  try {
    pickResult = await ctx.pickPackage(spec, {
      pickLowestVersion: opts.pickLowestVersion,
      publishedBy: opts.publishedBy,
      authHeaderValue,
      dryRun: opts.dryRun === true,
      preferredVersionSelectors: opts.preferredVersions?.[spec.name],
      registry,
      updateToLatest: opts.update === 'latest',
    })
  } catch (err: any) { // eslint-disable-line
    if ((workspacePackages != null) && opts.projectDir) {
      try {
        return tryResolveFromWorkspacePackages(workspacePackages, wantedDependency, spec, {
          wantedDependency,
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: opts.injectWorkspacePackages === true || wantedDependency.injected,
          update: Boolean(opts.update),
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
        return tryResolveFromWorkspacePackages(workspacePackages, wantedDependency, spec, {
          wantedDependency,
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: opts.injectWorkspacePackages === true || wantedDependency.injected,
          update: Boolean(opts.update),
          saveWorkspaceProtocol: ctx.saveWorkspaceProtocol,
          calcSpecifier: opts.calcSpecifier,
          pinnedVersion: opts.pinnedVersion,
        })
      } catch {
        // ignore
      }
    }
    throw new NoMatchingVersionError({ wantedDependency, packageMeta: meta, registry })
  }

  const workspacePkgsMatchingName = workspacePackages?.get(pickedPackage.name)
  if (workspacePkgsMatchingName && opts.projectDir) {
    const matchedPkg = workspacePkgsMatchingName.get(pickedPackage.version)
    if (matchedPkg) {
      return {
        ...resolveFromLocalPackage(matchedPkg, wantedDependency, spec, {
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
        ...resolveFromLocalPackage(workspacePkgsMatchingName.get(localVersion)!, wantedDependency, spec, {
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
    tarball: pickedPackage.dist.tarball,
  }
  let specifier: string | undefined
  if (opts.calcSpecifier) {
    specifier = spec.normalizedPref ?? calcSpecifier({
      wantedDependency,
      spec,
      version: pickedPackage.version,
      pinnedVersion: opts.pinnedVersion,
    })
  }
  return {
    id,
    latest: meta['dist-tags'].latest,
    manifest: pickedPackage,
    resolution,
    resolvedVia: 'npm-registry',
    publishedAt: meta.time?.[pickedPackage.version],
    specifier,
  }
}

function calcSpecifier ({
  wantedDependency,
  spec,
  version,
  pinnedVersion,
}: {
  wantedDependency: WantedDependency
  spec: RegistryPackageSpec
  version: string
  pinnedVersion?: PinnedVersion
}): string {
  if (wantedDependency.prevSpecifier === wantedDependency.pref && wantedDependency.prevSpecifier && versionSelectorType(wantedDependency.prevSpecifier)?.type === 'tag') {
    return wantedDependency.prevSpecifier
  }
  const range = calcRange(version, wantedDependency, pinnedVersion)
  if (!wantedDependency.alias || spec.name === wantedDependency.alias) return range
  return `npm:${spec.name}@${range}`
}

function calcRange (version: string, wantedDependency: WantedDependency, pinnedVersion?: PinnedVersion): string {
  if (semver.parse(version)?.prerelease.length) {
    return version
  }
  return createVersionSpec(version, {
    pinnedVersion: (wantedDependency.prevSpecifier ? whichVersionIsPinned(wantedDependency.prevSpecifier) : undefined) ??
      (wantedDependency.pref ? whichVersionIsPinned(wantedDependency.pref) : undefined) ??
      pinnedVersion,
  })
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
  if (!wantedDependency.pref?.startsWith('workspace:')) {
    return null
  }
  const pref = workspacePrefToNpm(wantedDependency.pref)

  const spec = parsePref(pref, wantedDependency.alias, opts.defaultTag, opts.registry)
  if (spec == null) throw new Error(`Invalid workspace: spec (${wantedDependency.pref})`)
  if (opts.workspacePackages == null) {
    throw new Error('Cannot resolve package from workspace because opts.workspacePackages is not defined')
  }
  if (!opts.projectDir) {
    throw new Error('Cannot resolve package from workspace because opts.projectDir is not defined')
  }
  return tryResolveFromWorkspacePackages(opts.workspacePackages, wantedDependency, spec, {
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
  wantedDependency: WantedDependency,
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
      `In ${path.relative(process.cwd(), opts.projectDir)}: "${spec.name}@${opts.wantedDependency.pref ?? ''}" is in the dependencies but no package named "${spec.name}" is present in the workspace`,
      {
        hint: 'Packages found in the workspace: ' + Object.keys(workspacePackages).join(', '),
      }
    )
  }
  const localVersion = pickMatchingLocalVersionOrNull(
    workspacePkgsMatchingName,
    opts.update ? { name: spec.name, fetchSpec: '*', type: 'range' } : spec
  )
  if (!localVersion) {
    throw new PnpmError(
      'NO_MATCHING_VERSION_INSIDE_WORKSPACE',
      `In ${path.relative(process.cwd(), opts.projectDir)}: No matching version found for ${opts.wantedDependency.alias ?? ''}@${opts.wantedDependency.pref ?? ''} inside the workspace`
    )
  }
  return resolveFromLocalPackage(workspacePkgsMatchingName.get(localVersion)!, wantedDependency, spec, opts)
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
  wantedDependency: WantedDependency,
  spec: RegistryPackageSpec,
  opts: {
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
  let specifier: string | undefined
  if (opts.calcSpecifier) {
    specifier = spec.normalizedPref ?? calcSpecifierForWorkspaceDep({
      wantedDependency,
      spec,
      saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
      version: localPackage.manifest.version,
      pinnedVersion: opts.pinnedVersion,
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
    specifier,
  }
}

function calcSpecifierForWorkspaceDep ({
  wantedDependency,
  spec,
  saveWorkspaceProtocol,
  version,
  pinnedVersion,
}: {
  wantedDependency: WantedDependency
  spec: RegistryPackageSpec
  saveWorkspaceProtocol: boolean | 'rolling' | undefined
  version: string
  pinnedVersion?: PinnedVersion
}): string {
  if (!saveWorkspaceProtocol && !wantedDependency.pref?.startsWith('workspace:')) {
    return calcSpecifier({ wantedDependency, spec, version, pinnedVersion })
  }
  const prefix = (!wantedDependency.alias || spec.name === wantedDependency.alias) ? 'workspace:' : `workspace:${spec.name}@`
  if (saveWorkspaceProtocol === 'rolling') {
    const pref = wantedDependency.prevSpecifier ?? wantedDependency.pref
    if (pref) {
      if ([`${prefix}*`, `${prefix}^`, `${prefix}~`].includes(pref)) return pref
      const pinnedVersion = whichVersionIsPinned(pref)
      switch (pinnedVersion) {
      case 'major': return `${prefix}^`
      case 'minor': return `${prefix}~`
      case 'patch': return `${prefix}*`
      }
    }
    return `${prefix}^`
  }
  const range = semver.parse(version)?.prerelease.length
    ? version
    : createVersionSpec(version, {
      pinnedVersion: (wantedDependency.prevSpecifier ? whichVersionIsPinned(wantedDependency.prevSpecifier) : undefined) ?? pinnedVersion,
    })
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

function createVersionSpec (version: string | undefined, opts: { pinnedVersion?: PinnedVersion, rolling?: boolean }): string {
  switch (opts.pinnedVersion ?? 'major') {
  case 'none':
  case 'major':
    if (opts.rolling) return '^'
    return !version ? '*' : `^${version}`
  case 'minor':
    if (opts.rolling) return '~'
    return !version ? '*' : `~${version}`
  case 'patch':
    if (opts.rolling) return '*'
    return !version ? '*' : `${version}`
  default:
    throw new PnpmError('BAD_PINNED_VERSION', `Cannot pin '${opts.pinnedVersion ?? 'undefined'}'`)
  }
}
