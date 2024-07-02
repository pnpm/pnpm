import path from 'path'
import { PnpmError } from '@pnpm/error'
import {
  type FetchFromRegistry,
  type GetAuthHeader,
  type RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import { resolveWorkspaceRange } from '@pnpm/resolve-workspace-range'
import {
  type PkgResolutionId,
  type PreferredVersions,
  type ResolveResult,
  type WantedDependency,
  type WorkspacePackage,
  type WorkspacePackages,
  type WorkspacePackagesByVersion,
} from '@pnpm/resolver-base'
import { LRUCache } from 'lru-cache'
import normalize from 'normalize-path'
import pMemoize from 'p-memoize'
import clone from 'ramda/src/clone'
import semver from 'semver'
import ssri from 'ssri'
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
  constructor (opts: { wantedDependency: WantedDependency, packageMeta: PackageMeta }) {
    const dep = opts.wantedDependency.alias
      ? `${opts.wantedDependency.alias}@${opts.wantedDependency.pref ?? ''}`
      : opts.wantedDependency.pref!
    super('NO_MATCHING_VERSION', `No matching version found for ${dep}`)
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

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
const META_DIR = 'metadata'
const FULL_META_DIR = 'metadata-full'
const FULL_FILTERED_META_DIR = 'metadata-v1.1'

export interface ResolverFactoryOptions {
  cacheDir: string
  fullMetadata?: boolean
  filterMetadata?: boolean
  offline?: boolean
  preferOffline?: boolean
  retry?: RetryTimeoutOptions
  timeout?: number
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
        metaDir: opts.fullMetadata ? (opts.filterMetadata ? FULL_FILTERED_META_DIR : FULL_META_DIR) : META_DIR,
        offline: opts.offline,
        preferOffline: opts.preferOffline,
        cacheDir: opts.cacheDir,
      }),
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
  registry: string
  preferredVersions?: PreferredVersions
  preferWorkspacePackages?: boolean
  updateToLatest?: boolean
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
  },
  wantedDependency: WantedDependency,
  opts: ResolveFromNpmOptions
): Promise<ResolveResult | null> {
  const defaultTag = opts.defaultTag ?? 'latest'
  if (wantedDependency.pref?.startsWith('workspace:')) {
    if (wantedDependency.pref.startsWith('workspace:.')) return null
    const resolvedFromWorkspace = tryResolveFromWorkspace(wantedDependency, {
      defaultTag,
      lockfileDir: opts.lockfileDir,
      projectDir: opts.projectDir,
      registry: opts.registry,
      workspacePackages: opts.workspacePackages,
    })
    if (resolvedFromWorkspace != null) {
      return resolvedFromWorkspace
    }
  }
  const workspacePackages = opts.alwaysTryWorkspacePackages !== false ? opts.workspacePackages : undefined
  const spec = wantedDependency.pref
    ? parsePref(wantedDependency.pref, wantedDependency.alias, defaultTag, opts.registry)
    : defaultTagForAlias(wantedDependency.alias!, defaultTag)
  if (spec == null) return null

  const authHeaderValue = ctx.getAuthHeaderValueByURI(opts.registry)
  let pickResult!: { meta: PackageMeta, pickedPackage: PackageInRegistry | null }
  try {
    pickResult = await ctx.pickPackage(spec, {
      pickLowestVersion: opts.pickLowestVersion,
      publishedBy: opts.publishedBy,
      authHeaderValue,
      dryRun: opts.dryRun === true,
      preferredVersionSelectors: opts.preferredVersions?.[spec.name],
      registry: opts.registry,
      updateToLatest: opts.updateToLatest,
    })
  } catch (err: any) { // eslint-disable-line
    if ((workspacePackages != null) && opts.projectDir) {
      try {
        return tryResolveFromWorkspacePackages(workspacePackages, spec, {
          wantedDependency,
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: wantedDependency.injected,
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
          hardLinkLocalPackages: wantedDependency.injected,
        })
      } catch {
        // ignore
      }
    }
    throw new NoMatchingVersionError({ wantedDependency, packageMeta: meta })
  }

  const workspacePkgsMatchingName = workspacePackages?.get(pickedPackage.name)
  if (workspacePkgsMatchingName && opts.projectDir) {
    const matchedPkg = workspacePkgsMatchingName.get(pickedPackage.version)
    if (matchedPkg) {
      return {
        ...resolveFromLocalPackage(matchedPkg, spec.normalizedPref, {
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: wantedDependency.injected,
        }),
        latest: meta['dist-tags'].latest,
      }
    }
    const localVersion = pickMatchingLocalVersionOrNull(workspacePkgsMatchingName, spec)
    if (localVersion && (semver.gt(localVersion, pickedPackage.version) || opts.preferWorkspacePackages)) {
      return {
        ...resolveFromLocalPackage(workspacePkgsMatchingName.get(localVersion)!, spec.normalizedPref, {
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: wantedDependency.injected,
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
  return {
    id,
    latest: meta['dist-tags'].latest,
    manifest: pickedPackage,
    normalizedPref: spec.normalizedPref,
    resolution,
    resolvedVia: 'npm-registry',
    publishedAt: meta.time?.[pickedPackage.version],
  }
}

function tryResolveFromWorkspace (
  wantedDependency: WantedDependency,
  opts: {
    defaultTag: string
    lockfileDir?: string
    projectDir?: string
    registry: string
    workspacePackages?: WorkspacePackages
  }
): ResolveResult | null {
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
  return tryResolveFromWorkspacePackages(opts.workspacePackages, spec, {
    wantedDependency,
    projectDir: opts.projectDir,
    hardLinkLocalPackages: wantedDependency.injected,
    lockfileDir: opts.lockfileDir,
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
  }
): ResolveResult {
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
  const localVersion = pickMatchingLocalVersionOrNull(workspacePkgsMatchingName, spec)
  if (!localVersion) {
    throw new PnpmError(
      'NO_MATCHING_VERSION_INSIDE_WORKSPACE',
      `In ${path.relative(process.cwd(), opts.projectDir)}: No matching version found for ${opts.wantedDependency.alias ?? ''}@${opts.wantedDependency.pref ?? ''} inside the workspace`
    )
  }
  return resolveFromLocalPackage(workspacePkgsMatchingName.get(localVersion)!, spec.normalizedPref, opts)
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
  normalizedPref: string | undefined,
  opts: {
    hardLinkLocalPackages?: boolean
    projectDir: string
    lockfileDir?: string
  }
): ResolveResult {
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
  return {
    id,
    manifest: clone(localPackage.manifest),
    normalizedPref,
    resolution: {
      directory,
      type: 'directory',
    },
    resolvedVia: 'local-filesystem',
  }
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
