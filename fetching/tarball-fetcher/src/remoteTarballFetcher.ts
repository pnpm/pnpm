import { type IncomingMessage } from 'http'
import { requestRetryLogger } from '@pnpm/core-loggers'
import { FetchError, PnpmError } from '@pnpm/error'
import { type FetchResult } from '@pnpm/fetcher-base'
import type { Cafs, DeferredManifestPromise } from '@pnpm/cafs-types'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import { type WorkerPool } from '@pnpm/fetching.tarball-worker'
import * as retry from '@zkochan/retry'
import throttle from 'lodash.throttle'
import { BadTarballError } from './errorTypes'

const BIG_TARBALL_SIZE = 1024 * 1024 * 5 // 5 MB

export class TarballIntegrityError extends PnpmError {
  public readonly found: string
  public readonly expected: string
  public readonly algorithm: string
  public readonly sri: string
  public readonly url: string

  constructor (opts: {
    attempts?: number
    found: string
    expected: string
    algorithm: string
    sri: string
    url: string
  }) {
    super('TARBALL_INTEGRITY',
      `Got unexpected checksum for "${opts.url}". Wanted "${opts.expected}". Got "${opts.found}".`,
      {
        attempts: opts.attempts,
        hint: `This error may happen when a package is republished to the registry with the same version.
In this case, the metadata in the local pnpm cache will contain the old integrity checksum.

If you think that this is the case, then run "pnpm store prune" and rerun the command that failed.
"pnpm store prune" will remove your local metadata cache.`,
      }
    )
    this.found = opts.found
    this.expected = opts.expected
    this.algorithm = opts.algorithm
    this.sri = opts.sri
    this.url = opts.url
  }
}

export interface HttpResponse {
  body: string
}

export type DownloadFunction = (url: string, opts: {
  getAuthHeaderByURI: (registry: string) => string | undefined
  cafs: Cafs
  manifest?: DeferredManifestPromise
  registry?: string
  onStart?: (totalSize: number | null, attempt: number) => void
  onProgress?: (downloaded: number) => void
  integrity?: string
  filesIndexFile: string
}) => Promise<FetchResult>

export interface NpmRegistryClient {
  get: (url: string, getOpts: object, cb: (err: Error, data: object, raw: object, res: HttpResponse) => void) => void
  fetch: (url: string, opts: { auth?: object }, cb: (err: Error, res: IncomingMessage) => void) => void
}

export function createDownloader (
  pool: WorkerPool,
  fetchFromRegistry: FetchFromRegistry,
  gotOpts: {
    // retry
    retry?: {
      retries?: number
      factor?: number
      minTimeout?: number
      maxTimeout?: number
      randomize?: boolean
    }
    timeout?: number
  }
): DownloadFunction {
  const retryOpts = {
    factor: 10,
    maxTimeout: 6e4, // 1 minute
    minTimeout: 1e4, // 10 seconds
    retries: 2,
    ...gotOpts.retry,
  }

  return async function download (url: string, opts: {
    getAuthHeaderByURI: (registry: string) => string | undefined
    cafs: Cafs
    manifest?: DeferredManifestPromise
    registry?: string
    onStart?: (totalSize: number | null, attempt: number) => void
    onProgress?: (downloaded: number) => void
    integrity?: string
    filesIndexFile: string
  }): Promise<FetchResult> {
    const authHeaderValue = opts.getAuthHeaderByURI(url)

    const op = retry.operation(retryOpts)

    return new Promise<FetchResult>((resolve, reject) => {
      op.attempt(async (attempt) => {
        try {
          resolve(await fetch(attempt))
        } catch (error: any) { // eslint-disable-line
          if (
            error.response?.status === 401 ||
            error.response?.status === 403 ||
            error.code === 'ERR_PNPM_PREPARE_PKG_FAILURE'
          ) {
            reject(error)
            return
          }
          const timeout = op.retry(error)
          if (timeout === false) {
            reject(op.mainError())
            return
          }
          requestRetryLogger.debug({
            attempt,
            error,
            maxRetries: retryOpts.retries,
            method: 'GET',
            timeout,
            url,
          })
        }
      })
    })

    async function fetch (currentAttempt: number): Promise<FetchResult> {
      try {
        const res = await fetchFromRegistry(url, {
          authHeaderValue,
          // The fetch library can retry requests on bad HTTP responses.
          // However, it is not enough to retry on bad HTTP responses only.
          // Requests should also be retried when the tarball's integrity check fails.
          // Hence, we tell fetch to not retry,
          // and we perform the retries from this function instead.
          retry: { retries: 0 },
          timeout: gotOpts.timeout,
        })

        if (res.status !== 200) {
          throw new FetchError({ url, authHeaderValue }, res)
        }

        const contentLength = res.headers.has('content-length') && res.headers.get('content-length')
        const size = typeof contentLength === 'string'
          ? parseInt(contentLength, 10)
          : null
        if (opts.onStart != null) {
          opts.onStart(size, currentAttempt)
        }
        // In order to reduce the amount of logs, we only report the download progress of big tarballs
        const onProgress = (size != null && size >= BIG_TARBALL_SIZE && opts.onProgress)
          ? throttle(opts.onProgress, 500)
          : undefined
        let downloaded = 0
        const chunks: Buffer[] = []
        // This will handle the 'data', 'error', and 'end' events.
        for await (const chunk of res.body!) {
          chunks.push(chunk as Buffer)
          downloaded += chunk.length
          onProgress?.(downloaded)
        }
        if (size !== null && size !== downloaded) {
          throw new BadTarballError({
            expectedSize: size,
            receivedSize: downloaded,
            tarballUrl: url,
          })
        }

        // eslint-disable-next-line no-async-promise-executor
        return await new Promise<FetchResult>(async (resolve, reject) => {
          const data: Buffer = Buffer.from(new SharedArrayBuffer(downloaded))
          let offset: number = 0
          for (const chunk of chunks) {
            chunk.copy(data, offset)
            offset += chunk.length
          }
          const localWorker = await pool.checkoutWorkerAsync(true)
          localWorker.once('message', ({ status, error, value }) => {
            pool.checkinWorker(localWorker)
            if (status === 'error') {
              if (error.type === 'integrity_validation_failed') {
                reject(new TarballIntegrityError({
                  ...error,
                  url,
                }))
                return
              }
              reject(new PnpmError('TARBALL_EXTRACT', `Failed to unpack the tarball from "${url}": ${error as string}`))
              return
            }
            opts.manifest?.resolve(value.manifest)
            resolve({ filesIndex: value.filesIndex, local: true })
          })
          localWorker.postMessage({
            type: 'extract',
            buffer: data,
            cafsDir: opts.cafs.cafsDir,
            integrity: opts.integrity,
            filesIndexFile: opts.filesIndexFile,
          })
        })
      } catch (err: any) { // eslint-disable-line
        err.attempts = currentAttempt
        err.resource = url
        throw err
      }
    }
  }
}
