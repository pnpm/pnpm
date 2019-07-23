import { ResolveFunction } from '@pnpm/default-resolver'
import { Registries } from '@pnpm/types'
import { pickRegistryForPackage } from '@pnpm/utils'
import mem = require('mem')
import createResolver from './createResolver'

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
): (packageName: string) => Promise<string | null> {
  const resolve = createResolver(opts)
  return mem(getLatestVersion.bind(null, resolve, opts))
}

export async function getLatestVersion (
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
  return resolution && resolution.latest || null
}
