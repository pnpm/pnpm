import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import { StrictPnpmOptions } from '@pnpm/types'
import createStore from 'package-store'
import path = require('path')

export default async (
  opts: {
    registry?: string,
    rawNpmConfig: object,
    lock?: boolean,
    store: string,
    alwaysAuth?: boolean,
    strictSsl?: boolean,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    fetchRetries?: number,
    fetchRetryFactor?: number,
    fetchRetryMintimeout?: number,
    fetchRetryMaxtimeout?: number,
    userAgent?: string,
    ignoreFile?: (filename: string) => boolean,
    offline?: boolean,
    lockStaleDuration?: number,
    networkConcurrency?: number,
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'reflink',
  },
) => {
  const sopts = Object.assign(opts, {
    locks: opts.lock ? path.join(opts.store, '_locks') : undefined,
    registry: opts.registry || 'https://registry.npmjs.org/',
  })
  const resolve = createResolver(Object.assign(sopts, {
    fullMetadata: false,
    metaCache: new Map(),
  }))
  const fetchers = createFetcher(sopts)
  return {
    ctrl: await createStore(resolve, fetchers as {}, {
      lockStaleDuration: sopts.lockStaleDuration,
      locks: sopts.locks,
      networkConcurrency: sopts.networkConcurrency,
      packageImportMethod: sopts.packageImportMethod,
      store: sopts.store,
    }),
    path: sopts.store,
  }
}
