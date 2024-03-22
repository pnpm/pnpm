import throttle from 'lodash.throttle'
import * as retry from '@zkochan/retry'

import { FetchError } from '@pnpm/error'
import { addFilesFromTarball } from '@pnpm/worker'
import { requestRetryLogger } from '@pnpm/core-loggers'
import type { Cafs, FetchFromRegistry, FetchResult, FetchOptions, DownloadFunction } from '@pnpm/types'

import { BadTarballError } from './errorTypes/index.js'

const BIG_TARBALL_SIZE = 1024 * 1024 * 5 // 5 MB

export function createDownloader(
  fetchFromRegistry: FetchFromRegistry,
  gotOpts: {
    // retry
    retry?: {
      retries: number
      factor: number
      minTimeout: number
      maxTimeout: number
      randomize: boolean
    } | undefined
    timeout?: number | undefined
  }
): DownloadFunction {
  const retryOpts: retry.RetryTimeoutOptions & {
    maxRetryTime: number;
  } = {
    factor: 10,
    maxTimeout: 6e4, // 1 minute
    minTimeout: 1e4, // 10 seconds
    retries: 2,
    maxRetryTime: 6e4 * 2, // 2 minutes
    ...gotOpts.retry,
  }

  return async function download(
    url: string,
    opts: {
      getAuthHeaderByURI: (registry: string) => string | undefined
      cafs: Cafs
      readManifest?: boolean | undefined
      registry?: string | undefined
      onStart?: ((totalSize: number | null, attempt: number) => void) | undefined
      onProgress?: ((downloaded: number) => void) | undefined
      integrity?: string | undefined
      filesIndexFile: string
    } & Pick<FetchOptions, 'pkg'>
  ): Promise<FetchResult> {
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
          requestRetryLogger.debug({
            attempt,
            error,
            maxRetries: retryOpts.retries ?? 0,
            method: 'GET',
            timeout,
            url,
          })
        }
      })
    })

    async function fetch(currentAttempt: number): Promise<FetchResult> {
      let data: Buffer
      try {
        const res = await fetchFromRegistry(url, {
          authHeaderValue,
          // The fetch library can retry requests on bad HTTP responses.
          // However, it is not enough to retry on bad HTTP responses only.
          // Requests should also be retried when the tarball's integrity check fails.
          // Hence, we tell fetch to not retry,
          // and we perform the retries from this function instead.
          retry: { retries: 0, factor: 0, minTimeout: 0, maxTimeout: 0, randomize: false },
          timeout: gotOpts.timeout,
        })

        if (res.status !== 200) {
          throw new FetchError({ url, authHeaderValue }, res)
        }

        const contentLength =
          res.headers.has('content-length') && res.headers.get('content-length')
        const size =
          typeof contentLength === 'string' ? parseInt(contentLength, 10) : null
        if (opts.onStart != null) {
          opts.onStart(size, currentAttempt)
        }
        // In order to reduce the amount of logs, we only report the download progress of big tarballs
        const onProgress =
          size != null && size >= BIG_TARBALL_SIZE && opts.onProgress
            ? throttle(opts.onProgress, 500)
            : undefined
        let downloaded = 0
        const chunks: Buffer[] = []
        // This will handle the 'data', 'error', and 'end' events.
        for await (const chunk of (res.body ?? '')) {
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

        data = Buffer.from(new SharedArrayBuffer(downloaded))
        let offset: number = 0
        for (const chunk of chunks) {
          chunk.copy(data, offset)
          offset += chunk.length
        }
      } catch (err: any) { // eslint-disable-line
        err.attempts = currentAttempt
        err.resource = url
        throw err
      }
      return addFilesFromTarball({
        buffer: data,
        cafsDir: opts.cafs.cafsDir,
        readManifest: opts.readManifest,
        integrity: opts.integrity,
        filesIndexFile: opts.filesIndexFile,
        url,
        pkg: opts.pkg,
      })
    }
  }
}
