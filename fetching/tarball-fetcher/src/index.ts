import { PnpmError } from '@pnpm/error'
import type {
  FetchFunction,
  FetchOptions,
  FetchResult,
} from '@pnpm/fetching.fetcher-base'
import type {
  FetchFromRegistry,
  GetAuthHeader,
  RetryTimeoutOptions,
} from '@pnpm/fetching.types'
import type { Cafs } from '@pnpm/store.cafs-types'
import type { StoreIndex } from '@pnpm/store.index'
import { TarballIntegrityError } from '@pnpm/worker'

import { createGitHostedTarballFetcher } from './gitHostedTarballFetcher.js'
import { createLocalTarballFetcher } from './localTarballFetcher.js'
import {
  createDownloader,
  type CreateDownloaderOptions,
  type DownloadFunction,
} from './remoteTarballFetcher.js'

export { BadTarballError } from './errorTypes/index.js'

export { TarballIntegrityError }

// Export individual fetcher factories for custom fetcher authors
export { createGitHostedTarballFetcher } from './gitHostedTarballFetcher.js'
export { createLocalTarballFetcher } from './localTarballFetcher.js'
export { createDownloader, type CreateDownloaderOptions, type DownloadFunction } from './remoteTarballFetcher.js'

export interface TarballFetchers {
  localTarball: FetchFunction
  remoteTarball: FetchFunction
  gitHostedTarball: FetchFunction
}

export function createTarballFetcher (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: {
    unsafePerm?: boolean
    ignoreScripts?: boolean
    storeIndex: StoreIndex
    timeout?: number
    retry?: RetryTimeoutOptions
    offline?: boolean
  } & Pick<CreateDownloaderOptions, 'fetchMinSpeedKiBps'>
): TarballFetchers {
  const download = createDownloader(fetchFromRegistry, {
    retry: opts.retry,
    timeout: opts.timeout,
    fetchMinSpeedKiBps: opts.fetchMinSpeedKiBps,
  })

  const remoteTarballFetcher = fetchFromTarball.bind(null, {
    download,
    getAuthHeaderByURI: getAuthHeader,
    offline: opts.offline,
    storeIndex: opts.storeIndex,
  }) as FetchFunction

  return {
    localTarball: createLocalTarballFetcher(opts.storeIndex),
    remoteTarball: remoteTarballFetcher,
    gitHostedTarball: createGitHostedTarballFetcher(remoteTarballFetcher, opts),
  }
}

async function fetchFromTarball (
  ctx: {
    download: DownloadFunction
    getAuthHeaderByURI: (registry: string) => string | undefined
    offline?: boolean
    storeIndex: StoreIndex
  },
  cafs: Cafs,
  resolution: {
    integrity?: string
    registry?: string
    tarball: string
  },
  opts: FetchOptions
): Promise<FetchResult> {
  if (ctx.offline) {
    throw new PnpmError('NO_OFFLINE_TARBALL',
      `A package is missing from the store but cannot download it in offline mode. The missing package may be downloaded from ${resolution.tarball}.`)
  }
  return ctx.download(resolution.tarball, {
    getAuthHeaderByURI: ctx.getAuthHeaderByURI,
    cafs,
    storeIndex: ctx.storeIndex,
    integrity: resolution.integrity,
    readManifest: opts.readManifest,
    onProgress: opts.onProgress,
    onStart: opts.onStart,
    registry: resolution.registry,
    filesIndexFile: opts.filesIndexFile,
    pkg: opts.pkg,
    appendManifest: opts.appendManifest,
    ignoreFilePattern: opts.ignoreFilePattern,
  })
}
