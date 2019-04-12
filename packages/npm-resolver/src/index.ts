import {
  LocalPackages,
  ResolveResult,
  WantedDependency,
} from '@pnpm/resolver-base'
import { PackageJson } from '@pnpm/types'
import getCredentialsByURI = require('credentials-by-uri')
import createRegFetcher from 'fetch-from-npm-registry'
import mem from 'mem'
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
} from './pickPackage'
import toRaw from './toRaw'

export {
  PackageMeta,
  PackageMetaCache,
}

// This file contains meta information
// about all the packages published by the same name, not just the manifest
// of one package/version
const META_FILENAME = 'index.json'
const FULL_META_FILENAME = 'index-full.json'

export default function createResolver (
  opts: {
    cert?: string,
    fullMetadata?: boolean,
    key?: string,
    ca?: string,
    strictSsl?: boolean,
    rawNpmConfig: object,
    metaCache: PackageMetaCache,
    store: string,
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
  },
) {
  if (typeof opts.rawNpmConfig !== 'object') { // tslint:disable-line
    throw new TypeError('`opts.rawNpmConfig` is required and needs to be an object')
  }
  if (typeof opts.rawNpmConfig['registry'] !== 'string') { // tslint:disable-line
    throw new TypeError('`opts.rawNpmConfig.registry` is required and needs to be a string')
  }
  if (typeof opts.metaCache !== 'object') { // tslint:disable-line
    throw new TypeError('`opts.metaCache` is required and needs to be an object')
  }
  if (typeof opts.store !== 'string') { // tslint:disable-line
    throw new TypeError('`opts.store` is required and needs to be a string')
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
    getCredentialsByURI: mem((registry: string) => getCredentialsByURI(registry, opts.rawNpmConfig)),
    pickPackage: pickPackage.bind(null, {
      fetch,
      metaCache: opts.metaCache,
      metaFileName: opts.fullMetadata ? FULL_META_FILENAME : META_FILENAME,
      offline: opts.offline,
      preferOffline: opts.preferOffline,
      storePath: opts.store,
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
    pickPackage: (spec: RegistryPackageSpec, opts: object) => ReturnType<typeof pickPackage>,
    getCredentialsByURI: (registry: string) => object,
  },
  wantedDependency: WantedDependency,
  opts: ResolveFromNpmOptions,
): Promise<ResolveResult | null> {
  const spec = wantedDependency.pref
    ? parsePref(wantedDependency.pref, wantedDependency.alias, opts.defaultTag || 'latest', opts.registry)
    : defaultTagForAlias(wantedDependency.alias!, opts.defaultTag || 'latest')
  if (!spec) return null
  const auth = ctx.getCredentialsByURI(opts.registry)
  let pickResult!: {meta: PackageMeta, pickedPackage: PackageInRegistry | null}
  try {
    pickResult = await ctx.pickPackage(spec, {
      auth,
      dryRun: opts.dryRun === true,
      preferredVersionSelector: opts.preferredVersions && opts.preferredVersions[spec.name],
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
    const err = new Error(`No matching version found for ${toRaw(spec)}`)
    // tslint:disable:no-string-literal
    err['code'] = 'ERR_PNPM_NO_MATCHING_VERSION'
    err['packageMeta'] = meta
    // tslint:enable:no-string-literal
    throw err
  }

  if (opts.localPackages && opts.localPackages[pickedPackage.name]) {
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
    normalizedPref: spec.normalizedPref,
    package: pickedPackage,
    resolution,
    resolvedVia: 'npm-registry',
  }
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
      directory: string;
      package: PackageJson;
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
    directory: string,
    package: PackageJson,
  },
  normalizedPref: string | undefined,
  prefix: string,
) {
  return {
    id: `link:${normalize(path.relative(prefix, localPackage.directory))}`,
    normalizedPref,
    package: localPackage.package,
    resolution: {
      directory: localPackage.directory,
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
