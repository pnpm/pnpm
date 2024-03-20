import { promises as fs } from 'node:fs'

import { createClient } from '@pnpm/client'
import { packageManager } from '@pnpm/cli-meta'
import { createPackageStore } from '@pnpm/package-store'
import type { CreateNewStoreControllerOptions, StoreController } from '@pnpm/types'

export async function createNewStoreController(
  opts: CreateNewStoreControllerOptions
): Promise<{
    ctrl: StoreController;
    dir: string;
  }> {
  const fullMetadata =
    opts.resolutionMode === 'time-based' && !opts.registrySupportsTimeField
  const { resolve, fetchers } = createClient({
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
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    strictSsl: opts.strictSsl ?? true,
    timeout: opts.fetchTimeout,
    userAgent: opts.userAgent,
    maxSockets:
      opts.maxSockets ??
      (opts.networkConcurrency != null
        ? opts.networkConcurrency * 3
        : undefined),
    gitShallowHosts: opts.gitShallowHosts,
    resolveSymlinksInInjectedDirs: opts.resolveSymlinksInInjectedDirs,
    includeOnlyPackageFiles: !opts.deployAllFiles,
  })
  await fs.mkdir(opts.storeDir, { recursive: true })
  return {
    ctrl: await createPackageStore(resolve, fetchers, {
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
      verifyStoreIntegrity:
        typeof opts.verifyStoreIntegrity === 'boolean'
          ? opts.verifyStoreIntegrity
          : true,
    }),
    dir: opts.storeDir,
  }
}
