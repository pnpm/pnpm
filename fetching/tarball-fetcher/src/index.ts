import path from 'path'
import os from 'os'
import { PnpmError } from '@pnpm/error'
import {
  type FetchFunction,
  type FetchOptions,
} from '@pnpm/fetcher-base'
import type { Cafs } from '@pnpm/cafs-types'
import {
  type FetchFromRegistry,
  type GetAuthHeader,
  type RetryTimeoutOptions,
} from '@pnpm/fetching-types'
import { WorkerPool } from '@rushstack/worker-pool/lib/WorkerPool'
import {
  createDownloader,
  type DownloadFunction,
  TarballIntegrityError,
} from './remoteTarballFetcher'
import { createLocalTarballFetcher } from './localTarballFetcher'
import { createGitHostedTarballFetcher } from './gitHostedTarballFetcher'

export { BadTarballError } from './errorTypes'

export { TarballIntegrityError }

export interface TarballFetchers {
  localTarball: FetchFunction
  remoteTarball: FetchFunction
  gitHostedTarball: FetchFunction
}

export function createTarballFetcher (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  opts: {
    rawConfig: object
    unsafePerm?: boolean
    ignoreScripts?: boolean
    timeout?: number
    retry?: RetryTimeoutOptions
    offline?: boolean
  }
): TarballFetchers {
  const workerPool = new WorkerPool({
    id: 'tarball',
    maxWorkers: os.cpus().length - 1,
    workerScriptPath: path.join(__dirname, 'worker/tarballWorker.js'),
  })
  // @ts-expect-error
  if (global.finishWorkers) {
    // @ts-expect-error
    const previous = global.finishWorkers
    // @ts-expect-error
    global.finishWorkers = async () => {
      await previous()
      await workerPool.finishAsync()
    }
  } else {
    // @ts-expect-error
    global.finishWorkers = () => workerPool.finishAsync()
  }
  const download = createDownloader(workerPool, fetchFromRegistry, {
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
    gitHostedTarball: createGitHostedTarballFetcher(remoteTarballFetcher, opts),
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
    filesIndexFile: opts.filesIndexFile,
  })
}
