import { promises as fs } from 'fs'
import { createClient, type ClientOptions } from '@pnpm/client'
import { type Config } from '@pnpm/config'
import { createPackageStore, type CafsLocker, type StoreController } from '@pnpm/package-store'
import { packageManager } from '@pnpm/cli-meta'

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
| 'ignoreScripts'
| 'hooks'
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
| 'registrySupportsTimeField'
| 'resolutionMode'
| 'strictSsl'
| 'unsafePerm'
| 'userAgent'
| 'verifyStoreIntegrity'
| 'virtualStoreDirMaxLength'
> & {
  cafsLocker?: CafsLocker
  ignoreFile?: (filename: string) => boolean
} & Partial<Pick<Config, 'userConfig' | 'deployAllFiles' | 'sslConfigs' | 'strictStorePkgContentCheck'>> & Pick<ClientOptions, 'resolveSymlinksInInjectedDirs'>

export async function createNewStoreController (
  opts: CreateNewStoreControllerOptions
): Promise<{ ctrl: StoreController, dir: string }> {
  const fullMetadata = opts.resolutionMode === 'time-based' && !opts.registrySupportsTimeField
  const { resolve, fetchers, clearResolutionCache } = createClient({
    customFetchers: opts.hooks?.fetchers,
    userConfig: opts.userConfig,
    unsafePerm: opts.unsafePerm,
    authConfig: opts.rawConfig,
    ca: opts.ca,
    cacheDir: opts.cacheDir,
    cert: opts.cert,
    fullMetadata,
    filterMetadata: fullMetadata,
    httpProxy: opts.httpProxy,
    httpsProxy: opts.httpsProxy,
    ignoreScripts: opts.ignoreScripts,
    key: opts.key,
    localAddress: opts.localAddress,
    noProxy: opts.noProxy,
    offline: opts.offline,
    preferOffline: opts.preferOffline,
    rawConfig: opts.rawConfig,
    sslConfigs: opts.sslConfigs,
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
    resolveSymlinksInInjectedDirs: opts.resolveSymlinksInInjectedDirs,
    includeOnlyPackageFiles: !opts.deployAllFiles,
  })
  await fs.mkdir(opts.storeDir, { recursive: true })
  return {
    ctrl: createPackageStore(resolve, fetchers, {
      cafsLocker: opts.cafsLocker,
      engineStrict: opts.engineStrict,
      force: opts.force,
      nodeVersion: opts.nodeVersion,
      pnpmVersion: packageManager.version,
      ignoreFile: opts.ignoreFile,
      importPackage: opts.hooks?.importPackage,
      networkConcurrency: opts.networkConcurrency,
      packageImportMethod: opts.packageImportMethod,
      cacheDir: opts.cacheDir,
      storeDir: opts.storeDir,
      verifyStoreIntegrity: typeof opts.verifyStoreIntegrity === 'boolean'
        ? opts.verifyStoreIntegrity
        : true,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      strictStorePkgContentCheck: opts.strictStorePkgContentCheck,
      clearResolutionCache,
    }),
    dir: opts.storeDir,
  }
}
