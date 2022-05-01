import { promises as fs } from 'fs'
import createClient from '@pnpm/client'
import { Config } from '@pnpm/config'
import createStore from '@pnpm/package-store'
import pnpm from '@pnpm/cli-meta'

type CreateResolverOptions = Pick<Config,
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'offline'
| 'rawConfig'
| 'verifyStoreIntegrity'
> & Required<Pick<Config, 'cacheDir' | 'storeDir'>>

export type CreateNewStoreControllerOptions = CreateResolverOptions & Pick<Config,
| 'ca'
| 'cert'
| 'engineStrict'
| 'force'
| 'nodeVersion'
| 'fetchTimeout'
| 'gitShallowHosts'
| 'httpProxy'
| 'httpsProxy'
| 'key'
| 'localAddress'
| 'maxSockets'
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
} & Partial<Pick<Config, 'userConfig'>>

export default async (
  opts: CreateNewStoreControllerOptions
) => {
  const { resolve, fetchers } = createClient({
    userConfig: opts.userConfig,
    authConfig: opts.rawConfig,
    ca: opts.ca,
    cacheDir: opts.cacheDir,
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
    strictSsl: opts.strictSsl ?? true,
    timeout: opts.fetchTimeout,
    userAgent: opts.userAgent,
    maxSockets: opts.maxSockets ?? (
      opts.networkConcurrency != null
        ? (opts.networkConcurrency * 3)
        : undefined
    ),
    gitShallowHosts: opts.gitShallowHosts,
  })
  await fs.mkdir(opts.storeDir, { recursive: true })
  return {
    ctrl: await createStore(resolve, fetchers, {
      engineStrict: opts.engineStrict,
      force: opts.force,
      nodeVersion: opts.nodeVersion,
      pnpmVersion: pnpm.version,
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
