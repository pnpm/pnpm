import { promises as fs } from 'node:fs'

import { packageManager } from '@pnpm/cli.meta'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { type ClientOptions, createClient } from '@pnpm/installing.client'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import { type CafsLocker, createPackageStore, type StoreController } from '@pnpm/store.controller'
import { ReadOnlyStoreIndex, StoreIndex } from '@pnpm/store.index'

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
| 'frozenStore'
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
| 'minimumReleaseAgeExclude'
| 'minimumReleaseAgeIgnoreMissingTime'
| 'minimumReleaseAgeStrict'
| 'networkConcurrency'
| 'noProxy'
| 'offline'
| 'packageImportMethod'
| 'preferOffline'
| 'preserveAbsolutePaths'
| 'registries'
| 'namedRegistries'
| 'registrySupportsTimeField'
| 'resolutionMode'
| 'saveWorkspaceProtocol'
| 'strictSsl'
| 'supportedArchitectures'
| 'trustPolicy'
| 'trustPolicyExclude'
| 'trustPolicyIgnoreAfter'
| 'unsafePerm'
| 'userAgent'
| 'verifyStoreIntegrity'
| 'virtualStoreDirMaxLength'
> & Pick<ConfigContext, 'hooks'> & {
  cafsLocker?: CafsLocker
  ignoreFile?: (filename: string) => boolean
  fetchFullMetadata?: boolean
} & Partial<Pick<Config, 'deployAllFiles' | 'strictStorePkgContentCheck'>> & Pick<ClientOptions, 'resolveSymlinksInInjectedDirs'>

/**
 * Whether the resolver should request full registry metadata instead of the
 * abbreviated document.
 *
 * Full metadata is needed when:
 * - `supportedArchitectures.libc` is set, because the npm registry's
 *   abbreviated metadata currently does not contain `libc`
 *   (see <https://github.com/pnpm/pnpm/issues/7362#issuecomment-1971964689>);
 * - the trust policy is `no-downgrade`, because the trust checks read trust
 *   evidence (`_npmUser`) that abbreviated metadata never carries, regardless
 *   of `registrySupportsTimeField`;
 * - the resolution mode is time-based and the registry does not include the
 *   `time` field in abbreviated metadata.
 */
export function shouldFetchFullMetadata (
  opts: Pick<CreateNewStoreControllerOptions,
  | 'fetchFullMetadata'
  | 'registrySupportsTimeField'
  | 'resolutionMode'
  | 'supportedArchitectures'
  | 'trustPolicy'
  >
): boolean {
  return opts.fetchFullMetadata ?? (
    opts.supportedArchitectures?.libc != null ||
    opts.trustPolicy === 'no-downgrade' ||
    (opts.resolutionMode === 'time-based' && !opts.registrySupportsTimeField)
  )
}

export async function createNewStoreController (
  opts: CreateNewStoreControllerOptions
): Promise<{ ctrl: StoreController, dir: string, resolutionVerifiers: ResolutionVerifier[] }> {
  const fullMetadata = shouldFetchFullMetadata(opts)
  if (!opts.frozenStore) {
    await fs.mkdir(opts.storeDir, { recursive: true })
  }
  const storeIndex = opts.frozenStore ? new ReadOnlyStoreIndex(opts.storeDir) : new StoreIndex(opts.storeDir)
  const { resolve, fetchers, clearResolutionCache, resolutionVerifiers } = createClient({
    customResolvers: opts.hooks?.customResolvers,
    customFetchers: opts.hooks?.customFetchers,
    unsafePerm: opts.unsafePerm,
    ca: opts.ca,
    cacheDir: opts.cacheDir,
    storeDir: opts.storeDir,
    cert: opts.cert,
    frozenStore: opts.frozenStore,
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
    namedRegistries: opts.namedRegistries,
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
    ignoreMissingTimeField: opts.minimumReleaseAgeIgnoreMissingTime,
    minimumReleaseAge: opts.minimumReleaseAge,
    minimumReleaseAgeStrict: opts.minimumReleaseAgeStrict,
    minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
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
      frozenStore: opts.frozenStore,
      storeIndex,
    }),
    dir: opts.storeDir,
    resolutionVerifiers,
  }
}
