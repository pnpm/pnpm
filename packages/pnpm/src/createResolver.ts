import createResolver from '@pnpm/default-resolver'
import LRU = require('lru-cache')

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
    offline?: boolean,
    proxy?: string,
    rawConfig: object,
    store: string,
    strictSsl?: boolean,
    userAgent?: string,
    verifyStoreIntegrity?: boolean,
  },
) {
  return createResolver(Object.assign(opts, {
    fullMetadata: false,
    metaCache: new LRU({
      max: 10000,
      maxAge: 120 * 1000, // 2 minutes
    }) as any, // tslint:disable-line:no-any
  }))
}
