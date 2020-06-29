import createClient from '@pnpm/client'
import { Config } from '@pnpm/config'
import createStore from '@pnpm/package-store'
import LRU = require('lru-cache')
import fs = require('mz/fs')
import path = require('path')

type CreateResolverOptions = Pick<Config,
  | 'fetchRetries'
  | 'fetchRetryFactor'
  | 'fetchRetryMaxtimeout'
  | 'fetchRetryMintimeout'
  | 'fetchRetryMintimeout'
  | 'offline'
  | 'rawConfig'
  | 'verifyStoreIntegrity'
> & Required<Pick<Config, 'storeDir'>>

export type CreateNewStoreControllerOptions = CreateResolverOptions & Pick<Config,
    | 'ca'
    | 'cert'
    | 'key'
    | 'localAddress'
    | 'networkConcurrency'
    | 'offline'
    | 'packageImportMethod'
    | 'preferOffline'
    | 'proxy'
    | 'registry'
    | 'strictSsl'
    | 'userAgent'
    | 'verifyStoreIntegrity'
  > & {
    ignoreFile?: (filename: string) => boolean,
  }

export default async (
  opts: CreateNewStoreControllerOptions
) => {
  const { resolve, fetchers } = createClient({
    authConfig: opts.rawConfig,
    ca: opts.ca,
    cert: opts.cert,
    fullMetadata: false,
    key: opts.key,
    localAddress: opts.localAddress,
    metaCache: new LRU({
      max: 10000,
      maxAge: 120 * 1000, // 2 minutes
    }) as any, // tslint:disable-line:no-any
    offline: opts.offline,
    preferOffline: opts.preferOffline,
    proxy: opts.proxy,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    storeDir: opts.storeDir,
    strictSSL: opts.strictSsl ?? true,
    userAgent: opts.userAgent,
  })
  await fs.mkdir(opts.storeDir, { recursive: true })
  return {
    ctrl: await createStore(resolve, fetchers, {
      ignoreFile: opts.ignoreFile,
      networkConcurrency: opts.networkConcurrency,
      packageImportMethod: opts.packageImportMethod,
      storeDir: opts.storeDir,
      verifyStoreIntegrity: typeof opts.verifyStoreIntegrity === 'boolean' ?
        opts.verifyStoreIntegrity : true,
    }),
    dir: opts.storeDir,
  }
}
