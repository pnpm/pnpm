import path from 'path'
import { PnpmError } from '@pnpm/error'
import {
  type FetchFromRegistry,
  type GetAuthHeader,
  type RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import { resolveWorkspaceRange } from '@pnpm/resolve-workspace-range'
import {
  type PreferredVersions,
  type ResolveResult,
  type WantedDependency,
  type WorkspacePackages,
} from '@pnpm/resolver-base'
import { type DependencyManifest } from '@pnpm/types'
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

export function createNpmResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: ResolverFactoryOptions
) {
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
  return resolveNpm.bind(null, {
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
  })
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

  if (((workspacePackages?.[pickedPackage.name]) != null) && opts.projectDir) {
    if (workspacePackages[pickedPackage.name][pickedPackage.version]) {
      return {
        ...resolveFromLocalPackage(workspacePackages[pickedPackage.name][pickedPackage.version], spec.normalizedPref, {
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: wantedDependency.injected,
        }),
        latest: meta['dist-tags'].latest,
      }
    }
    const localVersion = pickMatchingLocalVersionOrNull(workspacePackages[pickedPackage.name], spec)
    if (localVersion && (semver.gt(localVersion, pickedPackage.version) || opts.preferWorkspacePackages)) {
      return {
        ...resolveFromLocalPackage(workspacePackages[pickedPackage.name][localVersion], spec.normalizedPref, {
          projectDir: opts.projectDir,
          lockfileDir: opts.lockfileDir,
          hardLinkLocalPackages: wantedDependency.injected,
        }),
        latest: meta['dist-tags'].latest,
      }
    }
  }

  const id = `/${pickedPackage.name}@${pickedPackage.version}`
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
) {
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
) {
  if (!workspacePackages[spec.name]) {
    throw new PnpmError(
      'WORKSPACE_PKG_NOT_FOUND',
      `In ${path.relative(process.cwd(), opts.projectDir)}: "${spec.name}@${opts.wantedDependency.pref ?? ''}" is in the dependencies but no package named "${spec.name}" is present in the workspace`,
      {
        hint: 'Packages found in the workspace: ' + Object.keys(workspacePackages).join(', '),
      }
    )
  }
  const localVersion = pickMatchingLocalVersionOrNull(workspacePackages[spec.name], spec)
  if (!localVersion) {
    throw new PnpmError(
      'NO_MATCHING_VERSION_INSIDE_WORKSPACE',
      `In ${path.relative(process.cwd(), opts.projectDir)}: No matching version found for ${opts.wantedDependency.alias ?? ''}@${opts.wantedDependency.pref ?? ''} inside the workspace`
    )
  }
  return resolveFromLocalPackage(workspacePackages[spec.name][localVersion], spec.normalizedPref, opts)
}

function pickMatchingLocalVersionOrNull (
  versions: {
    [version: string]: {
      dir: string
      manifest: DependencyManifest
    }
  },
  spec: RegistryPackageSpec
) {
  const localVersions = Object.keys(versions)
  switch (spec.type) {
  case 'tag':
    return semver.maxSatisfying(localVersions, '*', {
      includePrerelease: true,
    })
  case 'version':
    return versions[spec.fetchSpec] ? spec.fetchSpec : null
  case 'range':
    return resolveWorkspaceRange(spec.fetchSpec, localVersions)
  default:
    return null
  }
}

interface LocalPackage {
  dir: string
  manifest: DependencyManifest
}

function resolveFromLocalPackage (
  localPackage: LocalPackage,
  normalizedPref: string | undefined,
  opts: {
    hardLinkLocalPackages?: boolean
    projectDir: string
    lockfileDir?: string
  }
) {
  let id!: string
  let directory!: string
  const localPackageDir = resolveLocalPackageDir(localPackage)
  if (opts.hardLinkLocalPackages) {
    directory = normalize(path.relative(opts.lockfileDir!, localPackageDir))
    id = `file:${directory}`
  } else {
    directory = localPackageDir
    id = `link:${normalize(path.relative(opts.projectDir, localPackageDir))}`
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

function resolveLocalPackageDir (localPackage: LocalPackage) {
  if (
    localPackage.manifest.publishConfig?.directory == null ||
    localPackage.manifest.publishConfig?.linkDirectory === false
  ) return localPackage.dir
  return path.join(localPackage.dir, localPackage.manifest.publishConfig.directory)
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
}) {
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
