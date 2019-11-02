import PnpmError from '@pnpm/error'
import {
  LocalPackages,
  ResolveResult,
  WantedDependency,
} from '@pnpm/resolver-base'
import { DependencyManifest } from '@pnpm/types'
import getCredentialsByURI = require('credentials-by-uri')
import createRegFetcher from 'fetch-from-npm-registry'
import mem = require('mem')
import normalize = require('normalize-path')
import path = require('path')
import semver = require('semver')
import ssri = require('ssri')
import createPkgId from './createNpmPkgId'
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
const META_FILENAME = 'index.json'
const FULL_META_FILENAME = 'index-full.json'

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
  const fetch = createRegFetcher({
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
  }) as (url: string, opts: {auth?: object}) => Promise<object>
  return resolveNpm.bind(null, {
    getCredentialsByURI: mem((registry: string) => getCredentialsByURI(registry, opts.rawConfig)),
    pickPackage: pickPackage.bind(null, {
      fetch,
      metaCache: opts.metaCache,
      metaFileName: opts.fullMetadata ? FULL_META_FILENAME : META_FILENAME,
      offline: opts.offline,
      preferOffline: opts.preferOffline,
      storeDir: opts.storeDir,
    }),
  })
}

export type ResolveFromNpmOptions = {
  defaultTag?: string,
  dryRun?: boolean,
  registry: string,
  preferredVersions?: {
    [packageName: string]: {
      selector: string,
      type: 'version' | 'range' | 'tag',
    },
  },
} & ({
  prefix?: string,
  localPackages?: undefined,
} | {
  prefix: string,
  localPackages: LocalPackages,
})

async function resolveNpm (
  ctx: {
    pickPackage: (spec: RegistryPackageSpec, opts: PickPackageOptions) => ReturnType<typeof pickPackage>,
    getCredentialsByURI: (registry: string) => object,
  },
  wantedDependency: WantedDependency,
  opts: ResolveFromNpmOptions,
): Promise<ResolveResult | null> {
  const defaultTag = opts.defaultTag || 'latest'
  const resolvedFromWorkspace = tryResolveFromWorkspace(wantedDependency, {
    defaultTag,
    localPackages: opts.localPackages,
    prefix: opts.prefix,
    registry: opts.registry,
  })
  if (resolvedFromWorkspace) {
    return resolvedFromWorkspace
  }
  const spec = wantedDependency.pref
    ? parsePref(wantedDependency.pref, wantedDependency.alias, defaultTag, opts.registry)
    : defaultTagForAlias(wantedDependency.alias!, defaultTag)
  if (!spec) return null

  const auth = ctx.getCredentialsByURI(opts.registry)
  let pickResult!: {meta: PackageMeta, pickedPackage: PackageInRegistry | null}
  try {
    pickResult = await ctx.pickPackage(spec, {
      auth,
      dryRun: opts.dryRun === true,
      preferredVersionSelector: opts.preferredVersions?.[spec.name],
      registry: opts.registry,
    })
  } catch (err) {
    if (opts.localPackages) {
      const resolvedFromLocal = tryResolveFromLocalPackages(opts.localPackages, spec, opts.prefix)
      if (resolvedFromLocal) return resolvedFromLocal
    }
    throw err
  }
  const pickedPackage = pickResult.pickedPackage
  const meta = pickResult.meta
  if (!pickedPackage) {
    if (opts.localPackages) {
      const resolvedFromLocal = tryResolveFromLocalPackages(opts.localPackages, spec, opts.prefix)
      if (resolvedFromLocal) return resolvedFromLocal
    }
    throw new NoMatchingVersionError({ spec, packageMeta: meta })
  }

  if (opts.localPackages?.[pickedPackage.name]) {
    if (opts.localPackages[pickedPackage.name][pickedPackage.version]) {
      return {
        ...resolveFromLocalPackage(opts.localPackages[pickedPackage.name][pickedPackage.version], spec.normalizedPref, opts.prefix),
        latest: meta['dist-tags'].latest,
      }
    }
    const localVersion = pickMatchingLocalVersionOrNull(opts.localPackages[pickedPackage.name], spec)
    if (localVersion && semver.gt(localVersion, pickedPackage.version)) {
      return {
        ...resolveFromLocalPackage(opts.localPackages[pickedPackage.name][localVersion], spec.normalizedPref, opts.prefix),
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
    localPackages?: LocalPackages,
    prefix?: string,
    registry: string,
  }
) {
  if (!wantedDependency.pref?.startsWith('workspace:')) {
    return null
  }
  const pref = wantedDependency.pref.substr(10)
  const spec = parsePref(pref, wantedDependency.alias, opts.defaultTag, opts.registry)
  if (!spec) throw new Error(`Invalid workspace: spec (${wantedDependency.pref})`)
  if (!opts.localPackages) {
    throw new Error('Cannot resolve package from workspace because opts.localPackages is not defined')
  }
  if (!opts.prefix) {
    throw new Error('Cannot resolve package from workspace because opts.prefix is not defined')
  }
  const resolvedFromLocal = tryResolveFromLocalPackages(opts.localPackages, spec, opts.prefix)
  if (!resolvedFromLocal) {
    throw new PnpmError(
      'NO_MATCHING_VERSION_INSIDE_WORKSPACE',
      `No matching version found for ${wantedDependency.alias}@${pref} inside the workspace`,
    )
  }
  return resolvedFromLocal
}

function tryResolveFromLocalPackages (
  localPackages: LocalPackages,
  spec: RegistryPackageSpec,
  prefix: string,
) {
  if (!localPackages[spec.name]) return null
  const localVersion = pickMatchingLocalVersionOrNull(localPackages[spec.name], spec)
  if (!localVersion) return null
  return resolveFromLocalPackage(localPackages[spec.name][localVersion], spec.normalizedPref, prefix)
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
      return semver.maxSatisfying(localVersions, spec.fetchSpec, true)
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
  prefix: string,
) {
  return {
    id: `link:${normalize(path.relative(prefix, localPackage.dir))}`,
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
