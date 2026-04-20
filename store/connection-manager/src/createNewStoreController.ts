import { promises as fs } from 'node:fs'

import { packageManager } from '@pnpm/cli.meta'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { type ClientOptions, createClient } from '@pnpm/installing.client'
import { type CafsLocker, createPackageStore, type StoreController } from '@pnpm/store.controller'
import { StoreIndex } from '@pnpm/store.index'

type CreateResolverOptions = Pick<Config,
| 'fetchRetries'
| 'fetchRetryFactor'
| 'fetchRetryMaxtimeout'
| 'fetchRetryMintimeout'
| 'offline'
| 'configByUri'
| 'verifyStoreIntegrity'
> & Required<Pick<Config, 'cacheDir' | 'storeDir'>>

export type CreateNewStoreControllerOptions = CreateResolverOptions & Pick<Config,
| 'ca'
| 'cert'
| 'engineStrict'
| 'force'
| 'nodeDownloadMirrors'
| 'nodeVersion'
| 'fetchTimeout'
| 'fetchWarnTimeoutMs'
| 'fetchMinSpeedKiBps'
| 'gitShallowHosts'
| 'ignoreScripts'
| 'httpProxy'
| 'httpsProxy'
| 'key'
| 'localAddress'
| 'maxSockets'
| 'minimumReleaseAge'
| 'minimumReleaseAgeIgnoreMissingTime'
| 'minimumReleaseAgeStrict'
| 'networkConcurrency'
| 'noProxy'
| 'offline'
| 'packageImportMethod'
| 'preferOffline'
| 'preserveAbsolutePaths'
| 'registries'
| 'registrySupportsTimeField'
| 'resolutionMode'
| 'saveWorkspaceProtocol'
| 'strictSsl'
| 'trustPolicy'
| 'unsafePerm'
| 'userAgent'
| 'verifyStoreIntegrity'
| 'virtualStoreDirMaxLength'
> & Pick<ConfigContext, 'hooks'> & {
  cafsLocker?: CafsLocker
  ignoreFile?: (filename: string) => boolean
  fetchFullMetadata?: boolean
} & Partial<Pick<Config, 'deployAllFiles' | 'strictStorePkgContentCheck'>> & Pick<ClientOptions, 'resolveSymlinksInInjectedDirs'>

export async function createNewStoreController (
  opts: CreateNewStoreControllerOptions
): Promise<{ ctrl: StoreController, dir: string }> {
  const fullMetadata = opts.fetchFullMetadata ?? (
    (
      opts.resolutionMode === 'time-based' ||
      opts.trustPolicy === 'no-downgrade'
    ) && !opts.registrySupportsTimeField
  )
  await fs.mkdir(opts.storeDir, { recursive: true })
  const storeIndex = new StoreIndex(opts.storeDir)
  const { resolve, fetchers, clearResolutionCache } = createClient({
    customResolvers: opts.hooks?.customResolvers,
    customFetchers: opts.hooks?.customFetchers,
    unsafePerm: opts.unsafePerm,
    ca: opts.ca,
    cacheDir: opts.cacheDir,
    storeDir: opts.storeDir,
    cert: opts.cert,
    fetchWarnTimeoutMs: opts.fetchWarnTimeoutMs,
    fetchMinSpeedKiBps: opts.fetchMinSpeedKiBps,
    fullMetadata,
    filterMetadata: fullMetadata,
    httpProxy: opts.httpProxy,
    httpsProxy: opts.httpsProxy,
    ignoreScripts: opts.ignoreScripts,
    key: opts.key,
    localAddress: opts.localAddress,
    nodeDownloadMirrors: opts.nodeDownloadMirrors,
    noProxy: opts.noProxy,
    offline: opts.offline,
    preferOffline: opts.preferOffline,
    configByUri: opts.configByUri,
    registries: opts.registries,
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
    saveWorkspaceProtocol: opts.saveWorkspaceProtocol,
    preserveAbsolutePaths: opts.preserveAbsolutePaths,
    strictPublishedByCheck: Boolean(opts.minimumReleaseAge) && opts.minimumReleaseAgeStrict === true,
    ignoreMissingTimeField: opts.minimumReleaseAgeIgnoreMissingTime,
    storeIndex,
  })
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
      customFetchers: opts.hooks?.customFetchers,
      storeIndex,
    }),
    dir: opts.storeDir,
  }
}
