import createResolver, { ResolveFunction } from '@pnpm/default-resolver'
import { DependencyManifest, Registries } from '@pnpm/types'
import { pickRegistryForPackage } from '@pnpm/utils'
import LRU = require('lru-cache')
import mem = require('mem')

export default function (
  opts: {
    ca?: string,
    cert?: string,
    fetchRetries?: number,
    fetchRetryFactor?: number,
    fetchRetryMaxtimeout?: number,
    fetchRetryMintimeout?: number,
    httpsProxy?: string,
    key?: string,
    localAddress?: string,
    lockfileDirectory: string,
    offline?: boolean,
    prefix: string,
    proxy?: string,
    rawNpmConfig: object,
    registries: Registries,
    store: string,
    strictSsl?: boolean,
    userAgent?: string,
    verifyStoreIntegrity?: boolean,
  },
): (packageName: string) => Promise<DependencyManifest | null> {
  const resolve = createResolver(Object.assign(opts, {
    fullMetadata: true,
    metaCache: new LRU({
      max: 10000,
      maxAge: 120 * 1000, // 2 minutes
    }) as any, // tslint:disable-line:no-any
  }))
  return mem(getLatestManifest.bind(null, resolve, opts))
}

export async function getLatestManifest (
  resolve: ResolveFunction,
  opts: {
    lockfileDirectory: string,
    prefix: string,
    registries: Registries,
  },
  packageName: string,
) {
  const resolution = await resolve({ alias: packageName, pref: 'latest' }, {
    lockfileDirectory: opts.lockfileDirectory,
    preferredVersions: {},
    prefix: opts.prefix,
    registry: pickRegistryForPackage(opts.registries, packageName),
  })
  return resolution && resolution.package || null
}
