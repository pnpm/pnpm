import createFetcher from '@pnpm/default-fetcher'
import createResolver from '@pnpm/default-resolver'
import logger from '@pnpm/logger'
import createStore from '@pnpm/package-store'
import dirIsCaseSensitive from 'dir-is-case-sensitive'
import LRU = require('lru-cache')
import makeDir = require('make-dir')
import path = require('path')

export default async (
  opts: {
    alwaysAuth?: boolean,
    ca?: string,
    cert?: string,
    fetchRetries?: number,
    fetchRetryFactor?: number,
    fetchRetryMaxtimeout?: number,
    fetchRetryMintimeout?: number,
    httpsProxy?: string,
    ignoreFile?: (filename: string) => boolean,
    key?: string,
    localAddress?: string,
    lock: boolean,
    lockStaleDuration?: number,
    networkConcurrency?: number,
    offline?: boolean,
    packageImportMethod?: 'auto' | 'hardlink' | 'copy' | 'reflink',
    proxy?: string,
    rawNpmConfig: object,
    registry?: string,
    store: string,
    strictSsl?: boolean,
    userAgent?: string,
    verifyStoreIntegrity?: boolean,
  },
) => {
  // TODO: either print a warning or just log if --no-lock is used
  const sopts = Object.assign(opts, {
    locks: opts.lock ? path.join(opts.store, '_locks') : undefined,
    registry: opts.registry || 'https://registry.npmjs.org/',
  })
  const resolve = createResolver(Object.assign(sopts, {
    fullMetadata: false,
    metaCache: new LRU({
      max: 10000,
      maxAge: 120 * 1000, // 2 minutes
    }) as any, // tslint:disable-line:no-any
  }))
  await makeDir(sopts.store)
  const fsIsCaseSensitive = await dirIsCaseSensitive(sopts.store)
  logger.debug({
    // An undefined field would cause a crash of the logger
    // so converting it to null
    isCaseSensitive: typeof fsIsCaseSensitive === 'boolean'
      ? fsIsCaseSensitive : null,
    store: sopts.store,
  })
  const fetchers = createFetcher({ ...sopts, fsIsCaseSensitive })
  return {
    ctrl: await createStore(resolve, fetchers as {}, {
      locks: sopts.locks,
      lockStaleDuration: sopts.lockStaleDuration,
      networkConcurrency: sopts.networkConcurrency,
      packageImportMethod: sopts.packageImportMethod,
      store: sopts.store,
      verifyStoreIntegrity: typeof sopts.verifyStoreIntegrity === 'boolean' ?
        sopts.verifyStoreIntegrity : true,
    }),
    path: sopts.store,
  }
}
