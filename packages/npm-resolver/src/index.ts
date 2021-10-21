import path from 'path'
import PnpmError from '@pnpm/error'
import {
  FetchFromRegistry,
  GetCredentials,
  RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import resolveWorkspaceRange from '@pnpm/resolve-workspace-range'
import {
  PreferredVersions,
  ResolveResult,
  WantedDependency,
  WorkspacePackages,
} from '@pnpm/resolver-base'
import { DependencyManifest } from '@pnpm/types'
import LRU from 'lru-cache'
import normalize from 'normalize-path'
import pMemoize from 'p-memoize'
import semver from 'semver'
import ssri from 'ssri'
import pickPackage, {
  PackageInRegistry,
  PackageMeta,
  PackageMetaCache,
  PickPackageOptions,
} from './pickPackage'
import parsePref, {
  RegistryPackageSpec,
} from './parsePref'
import fromRegistry, { RegistryResponseError } from './fetch'
import createPkgId from './createNpmPkgId'
import workspacePrefToNpm from './workspacePrefToNpm'

export class NoMatchingVersionError extends PnpmError {
  public readonly packageMeta: PackageMeta
  constructor (opts: { wantedDependency: WantedDependency, packageMeta: PackageMeta}) {
    const dep = opts.wantedDependency.alias
      ? `${opts.wantedDependency.alias}@${opts.wantedDependency.pref ?? ''}`
      : opts.wantedDependency.pref!
    super('NO_MATCHING_VERSION', `No matching version found for ${dep}`)
    this.packageMeta = opts.packageMeta
  }
}

export {
  PackageMeta,
  PackageMetaCache,
  RegistryResponseError,
}

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
const META_DIR = 'metadata'
const FULL_META_DIR = 'metadata-full'

export interface ResolverFactoryOptions {
  cacheDir: string
  fullMetadata?: boolean
  offline?: boolean
  preferOffline?: boolean
  retry?: RetryTimeoutOptions
  timeout?: number
}

export default function createResolver (
  fetchFromRegistry: FetchFromRegistry,
  getCredentials: GetCredentials,
  opts: ResolverFactoryOptions
) {
  if (typeof opts.cacheDir !== 'string') { // eslint-disable-line
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
  const getAuthHeaderValueByURI = (registry: string) => getCredentials(registry).authHeaderValue
  const metaCache = new LRU({
    max: 10000,
    maxAge: 120 * 1000, // 2 minutes
  }) as any // eslint-disable-line @typescript-eslint/no-explicit-any
  return resolveNpm.bind(null, {
    getAuthHeaderValueByURI,
    pickPackage: pickPackage.bind(null, {
      fetch,
      metaCache,
      metaDir: opts.fullMetadata ? FULL_META_DIR : META_DIR,
      offline: opts.offline,
      preferOffline: opts.preferOffline,
      cacheDir: opts.cacheDir,
    }),
  })
}

export type ResolveFromNpmOptions = {
  alwaysTryWorkspacePackages?: boolean
  defaultTag?: string
  dryRun?: boolean
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
  let pickResult!: {meta: PackageMeta, pickedPackage: PackageInRegistry | null}
  try {
    pickResult = await ctx.pickPackage(spec, {
      authHeaderValue,
      dryRun: opts.dryRun === true,
      preferredVersionSelectors: opts.preferredVersions?.[spec.name],
      registry: opts.registry,
    })
  } catch (err: any) { // eslint-disable-line
    if ((workspacePackages != null) && opts.projectDir) {
      const resolvedFromLocal = tryResolveFromWorkspacePackages(workspacePackages, spec, opts.projectDir)
      if (resolvedFromLocal != null) return resolvedFromLocal
    }
    throw err
  }
  const pickedPackage = pickResult.pickedPackage
  const meta = pickResult.meta
  if (pickedPackage == null) {
    if ((workspacePackages != null) && opts.projectDir) {
      const resolvedFromLocal = tryResolveFromWorkspacePackages(workspacePackages, spec, opts.projectDir)
      if (resolvedFromLocal != null) return resolvedFromLocal
    }
    throw new NoMatchingVersionError({ wantedDependency, packageMeta: meta })
  }

  if (((workspacePackages?.[pickedPackage.name]) != null) && opts.projectDir) {
    if (workspacePackages[pickedPackage.name][pickedPackage.version]) {
      return {
        ...resolveFromLocalPackage(workspacePackages[pickedPackage.name][pickedPackage.version], spec.normalizedPref, opts.projectDir),
        latest: meta['dist-tags'].latest,
      }
    }
    const localVersion = pickMatchingLocalVersionOrNull(workspacePackages[pickedPackage.name], spec)
    if (localVersion && (semver.gt(localVersion, pickedPackage.version) || opts.preferWorkspacePackages)) {
      return {
        ...resolveFromLocalPackage(workspacePackages[pickedPackage.name][localVersion], spec.normalizedPref, opts.projectDir),
        latest: meta['dist-tags'].latest,
      }
    }
  }

  const id = createPkgId(pickedPackage.dist.tarball, pickedPackage.name, pickedPackage.version)
  const resolution = {
    integrity: getIntegrity(pickedPackage.dist),
    registry: opts.registry,
    tarball: pickedPackage.dist.tarball,
  }
  return {
    id,
    latest: meta['dist-tags'].latest,
    manifest: pickedPackage,
    normalizedPref: spec.normalizedPref,
    resolution,
    resolvedVia: 'npm-registry',
  }
}

function tryResolveFromWorkspace (
  wantedDependency: WantedDependency,
  opts: {
    defaultTag: string
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
  const resolvedFromLocal = tryResolveFromWorkspacePackages(opts.workspacePackages, spec, opts.projectDir)
  if (resolvedFromLocal == null) {
    throw new PnpmError(
      'NO_MATCHING_VERSION_INSIDE_WORKSPACE',
      `In ${path.relative(process.cwd(), opts.projectDir)}: No matching version found for ${wantedDependency.alias ?? ''}@${pref} inside the workspace`
    )
  }
  return resolvedFromLocal
}

function tryResolveFromWorkspacePackages (
  workspacePackages: WorkspacePackages,
  spec: RegistryPackageSpec,
  projectDir: string
) {
  if (!workspacePackages[spec.name]) return null
  const localVersion = pickMatchingLocalVersionOrNull(workspacePackages[spec.name], spec)
  if (!localVersion) return null
  return resolveFromLocalPackage(workspacePackages[spec.name][localVersion], spec.normalizedPref, projectDir)
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
    return semver.maxSatisfying(localVersions, '*')
  case 'version':
    return versions[spec.fetchSpec] ? spec.fetchSpec : null
  case 'range':
    return resolveWorkspaceRange(spec.fetchSpec, localVersions)
  default:
    return null
  }
}

function resolveFromLocalPackage (
  localPackage: {
    dir: string
    manifest: DependencyManifest
  },
  normalizedPref: string | undefined,
  projectDir: string
) {
  return {
    id: `link:${normalize(path.relative(projectDir, localPackage.dir))}`,
    manifest: localPackage.manifest,
    normalizedPref,
    resolution: {
      directory: localPackage.dir,
      type: 'directory',
    },
    resolvedVia: 'local-filesystem',
  }
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
