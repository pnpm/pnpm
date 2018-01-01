import {
  PackageManifest,
  PnpmOptions,
} from '@pnpm/types'
import getCredentialsByURI = require('credentials-by-uri')
import createRegFetcher from 'fetch-from-npm-registry'
import mem = require('mem')
import path = require('path')
import semver = require('semver')
import ssri = require('ssri')
import createPkgId from './createNpmPkgId'
import loadPkgMeta, {
  PackageInRegistry,
  PackageMeta,
} from './loadPackageMeta'
import parsePref from './parsePref'
import toRaw from './toRaw'

export {
  PackageManifest,
  PackageMeta,
}

export default function createResolver (
  opts: PnpmOptions & {
    rawNpmConfig: object,
    metaCache: Map<string, object>,
    store: string,
  },
) {
  if (typeof opts.rawNpmConfig !== 'object') {
    throw new TypeError('`opts.rawNpmConfig` is required and needs to be an object')
  }
  if (typeof opts.rawNpmConfig['registry'] !== 'string') { // tslint:disable-line
    throw new TypeError('`opts.rawNpmConfig.registry` is required and needs to be a string')
  }
  if (typeof opts.metaCache !== 'object') {
    throw new TypeError('`opts.metaCache` is required and needs to be an object')
  }
  if (typeof opts.store !== 'string') {
    throw new TypeError('`opts.store` is required and needs to be a string')
  }
  const fetch = createRegFetcher({
    ca: opts.ca,
    cert: opts.cert,
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
  })
  return resolveNpm.bind(null, {
    getCredentialsByURI: mem((registry: string) => getCredentialsByURI(registry, opts.rawNpmConfig)),
    loadPkgMeta: loadPkgMeta.bind(null, fetch, opts.metaCache),
    offline: opts.offline,
    store: opts.store,
  })
}

async function resolveNpm (
  ctx: {
    loadPkgMeta: Function, //tslint:disable-line
    offline?: boolean,
    store: string,
    getCredentialsByURI: (registry: string) => object,
  },
  wantedDependency: {
    alias?: string,
    pref: string,
  },
  opts: {
    dryRun?: boolean,
    registry: string,
    preferredVersions?: {
      [packageName: string]: {
        selector: string,
        type: 'version' | 'range' | 'tag',
      },
    },
  },
) {
  const spec = parsePref(wantedDependency.pref, wantedDependency.alias)
  if (!spec) return null
  const auth = ctx.getCredentialsByURI(opts.registry)
  const meta = await ctx.loadPkgMeta(spec, {
    auth,
    dryRun: opts.dryRun === true,
    offline: ctx.offline,
    registry: opts.registry,
    storePath: ctx.store,
  })
  let version: string | undefined
  switch (spec.type) {
    case 'version':
      version = spec.fetchSpec
      break
    case 'tag':
      version = meta['dist-tags'][spec.fetchSpec]
      break
    case 'range':
      version = pickVersionByVersionRange(meta, spec.fetchSpec, opts.preferredVersions && opts.preferredVersions[spec.name])
      break
  }
  const correctPkg = meta.versions[version as string]
  if (!correctPkg) {
    const versions = Object.keys(meta.versions)
    const message = versions.length
      ? 'Versions in registry:\n' + versions.join(', ') + '\n'
      : 'No valid version found.'
    const err = new Error('No compatible version found: ' +
      toRaw(spec) + '\n' + message)
    throw err
  }
  const id = createPkgId(correctPkg.dist.tarball, correctPkg.name, correctPkg.version)

  const resolution = {
    integrity: getIntegrity(correctPkg.dist),
    registry: opts.registry,
    tarball: correctPkg.dist.tarball,
  }
  return {
    id,
    latest: meta['dist-tags'].latest,
    package: correctPkg,
    resolution,
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

function pickVersionByVersionRange (
  meta: PackageMeta,
  versionRange: string,
  preferredVerSel?: {
    type: 'version' | 'range' | 'tag',
    selector: string,
  },
) {
  let versions: string[] | undefined
  const latest = meta['dist-tags'].latest

  if (preferredVerSel && preferredVerSel.selector !== versionRange) {
    const preferredVersions: string[] = []
    switch (preferredVerSel.type) {
      case 'tag': {
        preferredVersions.push(meta['dist-tags'][preferredVerSel.selector])
        break
      }
      case 'range': {
        // This might be slow if there are many versions
        // and the package is an indirect dependency many times in the project.
        // If it will create noticable slowdown, then might be a good idea to add some caching
        versions = Object.keys(meta.versions)
        for (const version of versions) {
          if (semver.satisfies(version, preferredVerSel.selector, true)) {
            preferredVersions.push(version)
          }
        }
        break
      }
      case 'version': {
        if (meta.versions[preferredVerSel.selector]) {
          preferredVersions.push(preferredVerSel.selector)
        }
        break
      }
    }

    if (preferredVersions.indexOf(latest) !== -1 && semver.satisfies(latest, versionRange, true)) {
      return latest
    }
    const preferredVersion = semver.maxSatisfying(preferredVersions, versionRange, true)
    if (preferredVersion) {
      return preferredVersion
    }
  }

  // Not using semver.satisfies in case of * because it does not select beta versions.
  // E.g.: 1.0.0-beta.1. See issue: https://github.com/pnpm/pnpm/issues/865
  if (versionRange === '*' || semver.satisfies(latest, versionRange, true)) {
    return latest
  }
  const maxVersion = semver.maxSatisfying(versions || Object.keys(meta.versions), versionRange, true)
  return maxVersion
}
