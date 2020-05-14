import { Config } from '@pnpm/config'
import createResolver from '@pnpm/default-resolver'
import LRU = require('lru-cache')

export type CreateResolverOptions = Pick<Config,
  | 'ca'
  | 'cert'
  | 'fetchRetries'
  | 'fetchRetryFactor'
  | 'fetchRetryMaxtimeout'
  | 'fetchRetryMintimeout'
  | 'fetchRetryMintimeout'
  | 'httpsProxy'
  | 'key'
  | 'localAddress'
  | 'offline'
  | 'proxy'
  | 'rawConfig'
  | 'strictSsl'
  | 'userAgent'
  | 'verifyStoreIntegrity'
> & Required<Pick<Config, 'storeDir'>>

export default function (
  opts: CreateResolverOptions
) {
  return createResolver(Object.assign(opts, {
    fullMetadata: false,
    metaCache: new LRU({
      max: 10000,
      maxAge: 120 * 1000, // 2 minutes
    }) as any, // tslint:disable-line:no-any
  }))
}
