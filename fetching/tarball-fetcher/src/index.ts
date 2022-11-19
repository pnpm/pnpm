import { PnpmError } from '@pnpm/error'
import {
  FetchFunction,
  FetchOptions,
} from '@pnpm/fetcher-base'
import type { Cafs } from '@pnpm/cafs-types'
import {
  FetchFromRegistry,
  GetAuthHeader,
  RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import {
  createDownloader,
  DownloadFunction,
  TarballIntegrityError,
} from './remoteTarballFetcher'
import { createLocalTarballFetcher } from './localTarballFetcher'
import { createGitHostedTarballFetcher, waitForFilesIndex } from './gitHostedTarballFetcher'

export { BadTarballError } from './errorTypes'

export { TarballIntegrityError, waitForFilesIndex }

export interface TarballFetchers {
  localTarball: FetchFunction
  remoteTarball: FetchFunction
  gitHostedTarball: FetchFunction
}

export function createTarballFetcher (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: {
    timeout?: number
    retry?: RetryTimeoutOptions
    offline?: boolean
  }
): TarballFetchers {
  const download = createDownloader(fetchFromRegistry, {
    retry: opts.retry,
    timeout: opts.timeout,
  })

  const remoteTarballFetcher = fetchFromTarball.bind(null, {
    download,
    getAuthHeaderByURI: getAuthHeader,
    offline: opts.offline,
  }) as FetchFunction

  return {
    localTarball: createLocalTarballFetcher(),
    remoteTarball: remoteTarballFetcher,
    gitHostedTarball: createGitHostedTarballFetcher(remoteTarballFetcher),
  }
}

async function fetchFromTarball (
  ctx: {
    download: DownloadFunction
    getAuthHeaderByURI: (registry: string) => string | undefined
    offline?: boolean
  },
  cafs: Cafs,
  resolution: {
    integrity?: string
    registry?: string
    tarball: string
  },
  opts: FetchOptions
) {
  if (ctx.offline) {
    throw new PnpmError('NO_OFFLINE_TARBALL',
      `A package is missing from the store but cannot download it in offline mode. The missing package may be downloaded from ${resolution.tarball}.`)
  }
  return ctx.download(resolution.tarball, {
    getAuthHeaderByURI: ctx.getAuthHeaderByURI,
    cafs,
    integrity: resolution.integrity,
    manifest: opts.manifest,
    onProgress: opts.onProgress,
    onStart: opts.onStart,
    registry: resolution.registry,
  })
}
