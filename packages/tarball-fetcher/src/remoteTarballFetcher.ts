import { IncomingMessage } from 'http'
import { requestRetryLogger } from '@pnpm/core-loggers'
import { FetchError, PnpmError } from '@pnpm/error'
import { FetchResult } from '@pnpm/fetcher-base'
import type { Cafs, DeferredManifestPromise } from '@pnpm/cafs-types'
import { FetchFromRegistry } from '@pnpm/fetching-types'
import * as retry from '@zkochan/retry'
import ssri from 'ssri'
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
      { attempts: opts.attempts }
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
}) => Promise<FetchResult>

export interface NpmRegistryClient {
  get: (url: string, getOpts: object, cb: (err: Error, data: object, raw: object, res: HttpResponse) => void) => void
  fetch: (url: string, opts: { auth?: object }, cb: (err: Error, res: IncomingMessage) => void) => void
}

export function createDownloader (
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
        const onProgress = size != null && size >= BIG_TARBALL_SIZE
          ? opts.onProgress
          : undefined
        let downloaded = 0
        res.body!.on('data', (chunk: Buffer) => {
          downloaded += chunk.length
          if (onProgress != null) onProgress(downloaded)
        })

        // eslint-disable-next-line no-async-promise-executor
        return await new Promise<FetchResult>(async (resolve, reject) => {
          const stream = res.body!
            .on('error', reject)

          try {
            const [integrityCheckResult, filesIndex] = await Promise.all([
              opts.integrity ? safeCheckStream(res.body, opts.integrity, url) : true,
              opts.cafs.addFilesFromTarball(res.body!, opts.manifest),
              waitTillClosed({ stream, size, getDownloaded: () => downloaded, url }),
            ])
            if (integrityCheckResult !== true) {
              // eslint-disable-next-line
              throw integrityCheckResult
            }

            resolve({ filesIndex })
          } catch (err: any) { // eslint-disable-line
            reject(err)
          }
        })
      } catch (err: any) { // eslint-disable-line
        err.attempts = currentAttempt
        err.resource = url
        throw err
      }
    }
  }
}

async function safeCheckStream (stream: any, integrity: string, url: string): Promise<true | Error> { // eslint-disable-line @typescript-eslint/no-explicit-any
  try {
    await ssri.checkStream(stream, integrity)
    return true
  } catch (err: any) { // eslint-disable-line
    return new TarballIntegrityError({
      algorithm: err['algorithm'],
      expected: err['expected'],
      found: err['found'],
      sri: err['sri'],
      url,
    })
  }
}

async function waitTillClosed (
  opts: {
    stream: NodeJS.EventEmitter
    size: null | number
    getDownloaded: () => number
    url: string
  }
) {
  return new Promise<void>((resolve, reject) => {
    opts.stream.on('end', () => {
      const downloaded = opts.getDownloaded()
      if (opts.size !== null && opts.size !== downloaded) {
        const err = new BadTarballError({
          expectedSize: opts.size,
          receivedSize: downloaded,
          tarballUrl: opts.url,
        })
        reject(err)
        return
      }
      resolve()
    })
  })
}
