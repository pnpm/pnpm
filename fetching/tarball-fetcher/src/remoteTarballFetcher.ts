import assert from 'assert'
import { type IncomingMessage } from 'http'
import util from 'util'
import { requestRetryLogger } from '@pnpm/core-loggers'
import { FetchError } from '@pnpm/error'
import { type FetchResult, type FetchOptions } from '@pnpm/fetcher-base'
import { type Cafs } from '@pnpm/cafs-types'
import { type FetchFromRegistry } from '@pnpm/fetching-types'
import { globalWarn } from '@pnpm/logger'
import { addFilesFromTarball } from '@pnpm/worker'
import * as retry from '@zkochan/retry'
import throttle from 'lodash.throttle'
import { BadTarballError } from './errorTypes/index.js'

const BIG_TARBALL_SIZE = 1024 * 1024 * 5 // 5 MB

export interface HttpResponse {
  body: string
}

export type DownloadOptions = {
  getAuthHeaderByURI: (registry: string) => string | undefined
  cafs: Cafs
  registry?: string
  onStart?: (totalSize: number | null, attempt: number) => void
  onProgress?: (downloaded: number) => void
  integrity?: string
} & Pick<FetchOptions, 'pkg' | 'appendManifest' | 'readManifest' | 'filesIndexFile'>

export type DownloadFunction = (url: string, opts: DownloadOptions) => Promise<FetchResult>

export interface NpmRegistryClient {
  get: (url: string, getOpts: object, cb: (err: Error, data: object, raw: object, res: HttpResponse) => void) => void
  fetch: (url: string, opts: { auth?: object }, cb: (err: Error, res: IncomingMessage) => void) => void
}

export interface CreateDownloaderOptions {
  // retry
  retry?: {
    retries?: number
    factor?: number
    minTimeout?: number
    maxTimeout?: number
    randomize?: boolean
  }
  timeout?: number
  fetchMinSpeedKiBps?: number
}

export function createDownloader (
  fetchFromRegistry: FetchFromRegistry,
  gotOpts: CreateDownloaderOptions
): DownloadFunction {
  const retryOpts = {
    factor: 10,
    maxTimeout: 6e4, // 1 minute
    minTimeout: 1e4, // 10 seconds
    retries: 2,
    ...gotOpts.retry,
  }
  const fetchMinSpeedKiBps = gotOpts.fetchMinSpeedKiBps ?? 50 // 50 KiB/s

  return async function download (url: string, opts: DownloadOptions): Promise<FetchResult> {
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
            error.response?.status === 404 ||
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
          // Extract error properties into a plain object because Error properties
          // are non-enumerable and don't serialize well through the logging system
          const errorInfo = {
            name: error.name,
            message: error.message,
            code: error.code,
            errno: error.errno,
            // For HTTP errors from our ResponseError class
            status: error.status,
            statusCode: error.statusCode,
            // undici wraps the actual network error in a cause property
            cause: error.cause ? {
              code: error.cause.code,
              errno: error.cause.errno,
            } : undefined,
          }
          requestRetryLogger.debug({
            attempt,
            error: errorInfo,
            maxRetries: retryOpts.retries,
            method: 'GET',
            timeout,
            url,
          })
        }
      })
    })

    async function fetch (currentAttempt: number): Promise<FetchResult> {
      let data: Buffer
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
        const startTime = Date.now()
        let downloaded = 0
        const chunks: Buffer[] = []
        // This will handle the 'data', 'error', and 'end' events.
        // Note: undici returns Uint8Array chunks, not Buffer chunks, so we convert them
        for await (const chunk of res.body!) {
          const bufferChunk = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk as Uint8Array)
          chunks.push(bufferChunk)
          downloaded += bufferChunk.length
          onProgress?.(downloaded)
        }
        if (size !== null && size !== downloaded) {
          throw new BadTarballError({
            expectedSize: size,
            receivedSize: downloaded,
            tarballUrl: url,
          })
        }
        const elapsedSec = (Date.now() - startTime) / 1000
        const avgKiBps = Math.floor((downloaded / elapsedSec) / 1024)
        if (downloaded > 0 && elapsedSec > 1 && avgKiBps < fetchMinSpeedKiBps) {
          const sizeKb = Math.floor(downloaded / 1024)
          globalWarn(`Tarball download average speed ${avgKiBps} KiB/s (size ${sizeKb} KiB) is below ${fetchMinSpeedKiBps} KiB/s: ${url} (GET)`)
        }

        data = Buffer.from(new SharedArrayBuffer(downloaded))
        let offset: number = 0
        for (const chunk of chunks) {
          chunk.copy(data, offset)
          offset += chunk.length
        }
      } catch (err: unknown) {
        assert(util.types.isNativeError(err))
        Object.assign(err, {
          attempts: currentAttempt,
          resource: url,
        })
        throw err
      }
      return addFilesFromTarball({
        buffer: data,
        storeDir: opts.cafs.storeDir,
        readManifest: opts.readManifest,
        integrity: opts.integrity,
        filesIndexFile: opts.filesIndexFile,
        url,
        pkg: opts.pkg,
        appendManifest: opts.appendManifest,
      })
    }
  }
}
