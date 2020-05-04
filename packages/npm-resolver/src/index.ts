import PnpmError from '@pnpm/error'
import resolveWorkspaceRange from '@pnpm/resolve-workspace-range'
import {
  PreferredVersions,
  ResolveResult,
  WantedDependency,
  WorkspacePackages,
} from '@pnpm/resolver-base'
import { DependencyManifest } from '@pnpm/types'
import getCredentialsByURI = require('credentials-by-uri')
import createRegFetcher from 'fetch-from-npm-registry'
import mem = require('mem')
import normalize = require('normalize-path')
import pMemoize = require('p-memoize')
import path = require('path')
import semver = require('semver')
import ssri = require('ssri')
import createPkgId from './createNpmPkgId'
import fromRegistry from './fetch'
import parsePref, {
  RegistryPackageSpec,
} from './parsePref'
import pickPackage, {
  PackageInRegistry,
  PackageMeta,
  PackageMetaCache,
  PickPackageOptions,
} from './pickPackage'
import toRaw from './toRaw'

class NoMatchingVersionError extends PnpmError {
  public readonly packageMeta: PackageMeta
  constructor (opts: { spec: RegistryPackageSpec, packageMeta: PackageMeta}) {
    super('NO_MATCHING_VERSION', `No matching version found for ${toRaw(opts.spec)}`)
    this.packageMeta = opts.packageMeta
  }
}

export {
  PackageMeta,
  PackageMetaCache,
}

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
const META_DIR = 'metadata'
const FULL_META_DIR = 'metadata-full'

export interface ResolverFactoryOptions {
  rawConfig: object,
  metaCache: PackageMetaCache,
  storeDir: string,
  cert?: string,
  fullMetadata?: boolean,
  key?: string,
  ca?: string,
  strictSsl?: boolean,
  proxy?: string,
  httpsProxy?: string,
  localAddress?: string,
  userAgent?: string,
  offline?: boolean,
  preferOffline?: boolean,
  fetchRetries?: number,
  fetchRetryFactor?: number,
  fetchRetryMintimeout?: number,
  fetchRetryMaxtimeout?: number,
}

export default function createResolver (
  opts: ResolverFactoryOptions,
) {
  if (typeof opts.rawConfig !== 'object') { // tslint:disable-line
    throw new TypeError('`opts.rawConfig` is required and needs to be an object')
  }
  if (typeof opts.rawConfig['registry'] !== 'string') { // tslint:disable-line
    throw new TypeError('`opts.rawConfig.registry` is required and needs to be a string')
  }
  if (typeof opts.metaCache !== 'object') { // tslint:disable-line
    throw new TypeError('`opts.metaCache` is required and needs to be an object')
  }
  if (typeof opts.storeDir !== 'string') { // tslint:disable-line
    throw new TypeError('`opts.storeDir` is required and needs to be a string')
  }
  const fetch = pMemoize(fromRegistry.bind(null, createRegFetcher({
    ca: opts.ca,
    cert: opts.cert,
    fullMetadata: opts.fullMetadata,
    key: opts.key,
    localAddress: opts.localAddress,
    proxy: opts.httpsProxy || opts.proxy,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    strictSSL: opts.strictSsl,
    userAgent: opts.userAgent,
  })), {
    cacheKey: (...args) => JSON.stringify(args),
    maxAge: 1000 * 20, // 20 seconds
  })
  const getCreds = getCredentialsByURI.bind(null, opts.rawConfig)
  const getAuthHeaderValueByURI = mem(
    (registry: string) => getCreds(registry).authHeaderValue,
  )
  return resolveNpm.bind(null, {
    getAuthHeaderValueByURI,
    pickPackage: pickPackage.bind(null, {
      fetch,
      metaCache: opts.metaCache,
      metaDir: opts.fullMetadata ? FULL_META_DIR : META_DIR,
      offline: opts.offline,
      preferOffline: opts.preferOffline,
      storeDir: opts.storeDir,
    }),
  })
}

export type ResolveFromNpmOptions = {
  alwaysTryWorkspacePackages?: boolean,
  defaultTag?: string,
  dryRun?: boolean,
  registry: string,
  preferredVersions?: PreferredVersions,
} & ({
  projectDir?: string,
  workspacePackages?: undefined,
} | {
  projectDir: string,
  workspacePackages: WorkspacePackages,
})

async function resolveNpm (
  ctx: {
    pickPackage: (spec: RegistryPackageSpec, opts: PickPackageOptions) => ReturnType<typeof pickPackage>,
    getAuthHeaderValueByURI: (registry: string) => string | undefined,
  },
  wantedDependency: WantedDependency,
  opts: ResolveFromNpmOptions,
): Promise<ResolveResult | null> {
  const defaultTag = opts.defaultTag || 'latest'
  const resolvedFromWorkspace = tryResolveFromWorkspace(wantedDependency, {
    defaultTag,
    projectDir: opts.projectDir,
    registry: opts.registry,
    workspacePackages: opts.workspacePackages,
  })
  if (resolvedFromWorkspace) {
    return resolvedFromWorkspace
  }
  const workspacePackages = opts.alwaysTryWorkspacePackages !== false ? opts.workspacePackages : undefined
  const spec = wantedDependency.pref
    ? parsePref(wantedDependency.pref, wantedDependency.alias, defaultTag, opts.registry)
    : defaultTagForAlias(wantedDependency.alias!, defaultTag)
  if (!spec) return null

  const authHeaderValue = ctx.getAuthHeaderValueByURI(opts.registry)
  let pickResult!: {meta: PackageMeta, pickedPackage: PackageInRegistry | null}
  try {
    pickResult = await ctx.pickPackage(spec, {
      authHeaderValue,
      dryRun: opts.dryRun === true,
      preferredVersionSelectors: opts.preferredVersions?.[spec.name],
      registry: opts.registry,
    })
  } catch (err) {
    if (workspacePackages && opts.projectDir) {
      const resolvedFromLocal = tryResolveFromWorkspacePackages(workspacePackages, spec, opts.projectDir)
      if (resolvedFromLocal) return resolvedFromLocal
    }
    throw err
  }
  const pickedPackage = pickResult.pickedPackage
  const meta = pickResult.meta
  if (!pickedPackage) {
    if (workspacePackages && opts.projectDir) {
      const resolvedFromLocal = tryResolveFromWorkspacePackages(workspacePackages, spec, opts.projectDir)
      if (resolvedFromLocal) return resolvedFromLocal
    }
    throw new NoMatchingVersionError({ spec, packageMeta: meta })
  }

  if (workspacePackages?.[pickedPackage.name] && opts.projectDir) {
    if (workspacePackages[pickedPackage.name][pickedPackage.version]) {
      return {
        ...resolveFromLocalPackage(workspacePackages[pickedPackage.name][pickedPackage.version], spec.normalizedPref, opts.projectDir),
        latest: meta['dist-tags'].latest,
      }
    }
    const localVersion = pickMatchingLocalVersionOrNull(workspacePackages[pickedPackage.name], spec)
    if (localVersion && semver.gt(localVersion, pickedPackage.version)) {
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
    defaultTag: string,
    projectDir?: string,
    registry: string,
    workspacePackages?: WorkspacePackages,
  },
) {
  if (!wantedDependency.pref?.startsWith('workspace:')) {
    return null
  }
  const pref = wantedDependency.pref.substr(10)
  const spec = parsePref(pref, wantedDependency.alias, opts.defaultTag, opts.registry)
  if (!spec) throw new Error(`Invalid workspace: spec (${wantedDependency.pref})`)
  if (!opts.workspacePackages) {
    throw new Error('Cannot resolve package from workspace because opts.workspacePackages is not defined')
  }
  if (!opts.projectDir) {
    throw new Error('Cannot resolve package from workspace because opts.projectDir is not defined')
  }
  const resolvedFromLocal = tryResolveFromWorkspacePackages(opts.workspacePackages, spec, opts.projectDir)
  if (!resolvedFromLocal) {
    throw new PnpmError(
      'NO_MATCHING_VERSION_INSIDE_WORKSPACE',
      `No matching version found for ${wantedDependency.alias}@${pref} inside the workspace`,
    )
  }
  return resolvedFromLocal
}

function tryResolveFromWorkspacePackages (
  workspacePackages: WorkspacePackages,
  spec: RegistryPackageSpec,
  projectDir: string,
) {
  if (!workspacePackages[spec.name]) return null
  const localVersion = pickMatchingLocalVersionOrNull(workspacePackages[spec.name], spec)
  if (!localVersion) return null
  return resolveFromLocalPackage(workspacePackages[spec.name][localVersion], spec.normalizedPref, projectDir)
}

function pickMatchingLocalVersionOrNull (
  versions: {
    [version: string]: {
      dir: string;
      manifest: DependencyManifest;
    },
  },
  spec: RegistryPackageSpec,
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
    dir: string,
    manifest: DependencyManifest,
  },
  normalizedPref: string | undefined,
  projectDir: string,
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
  integrity?: string,
  shasum: string,
  tarball: string,
}) {
  if (dist.integrity) {
    return dist.integrity
  }
  return ssri.fromHex(dist.shasum, 'sha1').toString()
}
