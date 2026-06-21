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
  // A registry tarball is only verifiable against an integrity checksum, so one without
  // an `integrity` can't be completed without downloading the bytes to compute it. This
  // tells the package requester to fetch even under `--lockfile-only` and never to serve
  // such an entry from the store. Git-hosted and local (`file:`) tarballs are anchored
  // otherwise and keep the default (no forced fetch).
  // A usable integrity is a non-empty string; anything else (missing, `""`, or a non-string
  // value from a tampered lockfile) counts as missing so the bytes are fetched to compute one.
  remoteTarballFetcher.resolutionNeedsFetch = (resolution) => {
    const integrity = (resolution as { integrity?: unknown }).integrity
    return typeof integrity !== 'string' || integrity.length === 0
  }

  return {
    localTarball: createLocalTarballFetcher(opts.storeIndex),
    remoteTarball: remoteTarballFetcher,
    gitHostedTarball: createGitHostedTarballFetcher(remoteTarballFetcher, opts),
  }
}

async function fetchFromTarball (
  ctx: {
    download: DownloadFunction
    getAuthHeaderByURI: GetAuthHeader
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
