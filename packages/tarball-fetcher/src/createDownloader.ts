import { requestRetryLogger } from '@pnpm/core-loggers'
import PnpmError, { FetchError } from '@pnpm/error'
import {
  Cafs,
  DeferredManifestPromise,
  FetchResult,
  FilesIndex,
} from '@pnpm/fetcher-base'
import { FetchFromRegistry } from '@pnpm/fetching-types'
import * as retry from '@zkochan/retry'
import { IncomingMessage } from 'http'
import ssri = require('ssri')
import urlLib = require('url')
import { BadTarballError } from './errorTypes'

class TarballIntegrityError extends PnpmError {
  public readonly found: string
  public readonly expected: string
  public readonly algorithm: string
  public readonly sri: string
  public readonly url: string

  constructor (opts: {
    found: string,
    expected: string,
    algorithm: string,
    sri: string,
    url: string,
  }) {
    super('TARBALL_INTEGRITY', `Got unexpected checksum for "${opts.url}". Wanted "${opts.expected}". Got "${opts.found}".`)
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
  auth?: {
    authHeaderValue: string | undefined,
    alwaysAuth: boolean | undefined,
  },
  cafs: Cafs,
  manifest?: DeferredManifestPromise,
  registry?: string,
  onStart?: (totalSize: number | null, attempt: number) => void,
  onProgress?: (downloaded: number) => void,
  integrity?: string,
}) => Promise<FetchResult>

export interface NpmRegistryClient {
  get: (url: string, getOpts: object, cb: (err: Error, data: object, raw: object, res: HttpResponse) => void) => void,
  fetch: (url: string, opts: {auth?: object}, cb: (err: Error, res: IncomingMessage) => void) => void,
}

export default (
  fetchFromRegistry: FetchFromRegistry,
  gotOpts: {
    // retry
    retry?: {
      retries?: number,
      factor?: number,
      minTimeout?: number,
      maxTimeout?: number,
      randomize?: boolean,
    },
  }
): DownloadFunction => {
  const retryOpts = {
    factor: 10,
    maxTimeout: 6e4, // 1 minute
    minTimeout: 1e4, // 10 seconds
    retries: 2,
    ...gotOpts.retry,
  }

  return async function download (url: string, opts: {
    auth?: {
      authHeaderValue: string | undefined,
      alwaysAuth: boolean | undefined,
    },
    cafs: Cafs,
    manifest?: DeferredManifestPromise,
    registry?: string,
    onStart?: (totalSize: number | null, attempt: number) => void,
    onProgress?: (downloaded: number) => void,
    integrity?: string,
  }): Promise<FetchResult> {
    // If a tarball is hosted on a different place than the manifest, only send
    // credentials on `alwaysAuth`
    const shouldAuth = opts.auth && (
      opts.auth.alwaysAuth ||
      !opts.registry ||
      urlLib.parse(url).host === urlLib.parse(opts.registry).host
    )

    const op = retry.operation(retryOpts)

    return new Promise<FetchResult>((resolve, reject) => {
      op.attempt(async (attempt) => {
        try {
          resolve(await fetch(attempt))
        } catch (error) {
          if (error.response?.status === 401 || error.response?.status === 403) {
            reject(error)
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
        const authHeaderValue = shouldAuth ? opts.auth?.authHeaderValue : undefined
        const res = await fetchFromRegistry(url, {
          authHeaderValue,
          // The fetch library can retry requests on bad HTTP responses.
          // However, it is not enough to retry on bad HTTP responses only.
          // Requests should also be retried when the tarball's integrity check fails.
          // Hence, we tell fetch to not retry,
          // and we perform the retries from this function instead.
          retry: { retries: 0 },
        })

        if (res.status !== 200) {
          throw new FetchError({ url, authHeaderValue }, res)
        }

        const contentLength = res.headers.has('content-length') && res.headers.get('content-length')
        const size = typeof contentLength === 'string'
          ? parseInt(contentLength, 10)
          : null
        if (opts.onStart) {
          opts.onStart(size, currentAttempt)
        }
        const onProgress = opts.onProgress
        let downloaded = 0
        res.body.on('data', (chunk: Buffer) => {
          downloaded += chunk.length
          if (onProgress) onProgress(downloaded)
        })

        return await new Promise<FetchResult>(async (resolve, reject) => {
          const stream = res.body
            .on('error', reject)

          try {
            const [integrityCheckResult, filesIndex] = await Promise.all([
              opts.integrity && safeCheckStream(res.body, opts.integrity, url) || true,
              opts.cafs.addFilesFromTarball(res.body, opts.manifest),
              waitTillClosed({ stream, size, getDownloaded: () => downloaded, url }),
            ])
            if (integrityCheckResult !== true) {
              throw integrityCheckResult
            }
            resolve({ filesIndex: filesIndex as FilesIndex })
          } catch (err) {
            reject(err)
          }
        })
      } catch (err) {
        err.attempts = currentAttempt
        err.resource = url
        throw err
      }
    }
  }
}

async function safeCheckStream (stream: any, integrity: string, url: string): Promise<true | Error> { // tslint:disable-line:no-any
  try {
    await ssri.checkStream(stream, integrity)
    return true
  } catch (err) {
    return new TarballIntegrityError({
      algorithm: err['algorithm'],
      expected: err['expected'],
      found: err['found'],
      sri: err['sri'],
      url,
    })
  }
}

function waitTillClosed (
  opts: {
    stream: NodeJS.EventEmitter,
    size: null | number,
    getDownloaded: () => number,
    url: string,
  }
) {
  return new Promise((resolve, reject) => {
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
