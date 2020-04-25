import PnpmError from '@pnpm/error'
import { Cafs, FetchResult, FilesIndex } from '@pnpm/fetcher-base'
import createFetcher from 'fetch-from-npm-registry'
import fs = require('graceful-fs')
import { IncomingMessage } from 'http'
import makeDir = require('make-dir')
import path = require('path')
import pathTemp = require('path-temp')
import retry = require('retry')
import rimraf = require('rimraf')
import ssri = require('ssri')
import urlLib = require('url')
import { BadTarballError } from './errorTypes'

class TarballFetchError extends PnpmError {
  public readonly httpStatusCode: number
  public readonly uri: string
  public readonly response: unknown & { status: number, statusText: string }

  constructor (uri: string, response: { status: number, statusText: string }) {
    super('TARBALL_FETCH', `${response.status} ${response.statusText}: ${uri}`)
    this.httpStatusCode = response.status
    this.uri = uri
    this.response = response
  }
}

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

export type DownloadFunction = (url: string, saveto: string, opts: {
  auth?: {
    authHeaderValue: string | undefined,
    alwaysAuth: boolean | undefined,
  },
  cafs: Cafs,
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
  gotOpts: {
    alwaysAuth: boolean,
    registry: string,
    // proxy
    proxy?: string,
    localAddress?: string,
    // ssl
    ca?: string,
    cert?: string,
    key?: string,
    strictSSL?: boolean,
    // retry
    retry?: {
      retries?: number,
      factor?: number,
      minTimeout?: number,
      maxTimeout?: number,
      randomize?: boolean,
    },
    userAgent?: string,
  },
): DownloadFunction => {
  const fetchFromNpmRegistry = createFetcher(gotOpts)

  const retryOpts = {
    factor: 10,
    maxTimeout: 6e4, // 1 minute
    minTimeout: 1e4, // 10 seconds
    retries: 2,
    ...gotOpts.retry,
  }

  return async function download (url: string, saveto: string, opts: {
    auth?: {
      authHeaderValue: string | undefined,
      alwaysAuth: boolean | undefined,
    },
    cafs: Cafs,
    registry?: string,
    onStart?: (totalSize: number | null, attempt: number) => void,
    onProgress?: (downloaded: number) => void,
    integrity?: string,
  }): Promise<FetchResult> {
    const saveToDir = path.dirname(saveto)
    await makeDir(saveToDir)

    // If a tarball is hosted on a different place than the manifest, only send
    // credentials on `alwaysAuth`
    const shouldAuth = opts.auth && (
      opts.auth.alwaysAuth ||
      !opts.registry ||
      urlLib.parse(url).host === urlLib.parse(opts.registry).host
    )

    const op = retry.operation(retryOpts)

    return new Promise<FetchResult>((resolve, reject) => {
      op.attempt((currentAttempt) => {
        fetch(currentAttempt)
          .then(resolve)
          .catch((err) => {
            if (err.httpStatusCode === 403) {
              reject(err)
              return
            }
            if (op.retry(err)) {
              return
            }
            reject(op.mainError())
          })
      })
    })

    async function fetch (currentAttempt: number): Promise<FetchResult> {
      try {
        const res = await fetchFromNpmRegistry(url, {
          authHeaderValue: shouldAuth ? opts.auth?.authHeaderValue : undefined,
        })

        if (res.status !== 200) {
          throw new TarballFetchError(url, res)
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

        const tempTarballLocation = pathTemp(saveToDir)
        const writeStream = fs.createWriteStream(tempTarballLocation)

        return await new Promise<FetchResult>(async (resolve, reject) => {
          const stream = res.body
            .on('error', reject)
            .pipe(writeStream)
            .on('error', reject)

          try {
            const [integrityCheckResult, filesIndex] = await Promise.all([
              opts.integrity && safeCheckStream(res.body, opts.integrity, url) || true,
              opts.cafs.addFilesFromTarball(res.body),
              waitTillClosed({ stream, size, getDownloaded: () => downloaded, url }),
            ])
            if (integrityCheckResult !== true) {
              throw integrityCheckResult
            }
            fs.rename(tempTarballLocation, saveto, () => {
              // ignore errors
            })
            resolve({ filesIndex: filesIndex as FilesIndex })
          } catch (err) {
            rimraf(tempTarballLocation, () => {
              // ignore errors
            })
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
  },
) {
  return new Promise((resolve, reject) => {
    opts.stream.on('close', () => {
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
