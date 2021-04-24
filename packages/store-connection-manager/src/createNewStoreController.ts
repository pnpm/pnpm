import { promises as fs } from 'fs'
import createClient from '@pnpm/client'
import { Config } from '@pnpm/config'
import createStore from '@pnpm/package-store'

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
| 'fetchTimeout'
| 'httpProxy'
| 'httpsProxy'
| 'key'
| 'localAddress'
| 'networkConcurrency'
| 'noProxy'
| 'offline'
| 'packageImportMethod'
| 'preferOffline'
| 'registry'
| 'strictSsl'
| 'userAgent'
| 'verifyStoreIntegrity'
> & {
  ignoreFile?: (filename: string) => boolean
}

export default async (
  opts: CreateNewStoreControllerOptions
) => {
  const { resolve, fetchers } = createClient({
    authConfig: opts.rawConfig,
    ca: opts.ca,
    cert: opts.cert,
    fullMetadata: false,
    httpProxy: opts.httpProxy,
    httpsProxy: opts.httpsProxy,
    key: opts.key,
    localAddress: opts.localAddress,
    noProxy: opts.noProxy,
    offline: opts.offline,
    preferOffline: opts.preferOffline,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    storeDir: opts.storeDir,
    strictSSL: opts.strictSsl ?? true,
    timeout: opts.fetchTimeout,
    userAgent: opts.userAgent,
    maxSockets: opts.networkConcurrency != null
      ? (opts.networkConcurrency * 3)
      : undefined,
  })
  await fs.mkdir(opts.storeDir, { recursive: true })
  return {
    ctrl: await createStore(resolve, fetchers, {
      ignoreFile: opts.ignoreFile,
      networkConcurrency: opts.networkConcurrency,
      packageImportMethod: opts.packageImportMethod,
      storeDir: opts.storeDir,
      verifyStoreIntegrity: typeof opts.verifyStoreIntegrity === 'boolean'
        ? opts.verifyStoreIntegrity
        : true,
    }),
    dir: opts.storeDir,
  }
}
