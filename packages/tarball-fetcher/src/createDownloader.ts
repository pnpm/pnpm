import { IncomingMessage } from 'http'
import urlLib from 'url'
import { requestRetryLogger } from '@pnpm/core-loggers'
import PnpmError, { FetchError } from '@pnpm/error'
import {
  Cafs,
  DeferredManifestPromise,
  FetchResult,
  FilesIndex,
  PackageFileInfo,
} from '@pnpm/fetcher-base'
import { FetchFromRegistry } from '@pnpm/fetching-types'
import preparePackage from '@pnpm/prepare-package'
import * as retry from '@zkochan/retry'
import fromPairs from 'ramda/src/fromPairs'
import omit from 'ramda/src/omit'
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
  auth?: {
    authHeaderValue: string | undefined
    alwaysAuth: boolean | undefined
  }
  cafs: Cafs
  manifest?: DeferredManifestPromise
  registry?: string
  onStart?: (totalSize: number | null, attempt: number) => void
  onProgress?: (downloaded: number) => void
  integrity?: string
}) => Promise<FetchResult>

export interface NpmRegistryClient {
  get: (url: string, getOpts: object, cb: (err: Error, data: object, raw: object, res: HttpResponse) => void) => void
  fetch: (url: string, opts: {auth?: object}, cb: (err: Error, res: IncomingMessage) => void) => void
}

export default (
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
      authHeaderValue: string | undefined
      alwaysAuth: boolean | undefined
    }
    cafs: Cafs
    manifest?: DeferredManifestPromise
    registry?: string
    onStart?: (totalSize: number | null, attempt: number) => void
    onProgress?: (downloaded: number) => void
    integrity?: string
  }): Promise<FetchResult> {
    // If a tarball is hosted on a different place than the manifest, only send
    // credentials on `alwaysAuth`
    const shouldAuth = (opts.auth != null) && (
      opts.auth.alwaysAuth === true ||
      !opts.registry ||
      new urlLib.URL(url).host === new urlLib.URL(opts.registry).host
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
              throw integrityCheckResult
            }
            if (!isGitHostedPkgUrl(url)) {
              resolve({ filesIndex })
              return
            }
            resolve({ filesIndex: await prepareGitHostedPkg(filesIndex, opts.cafs) })
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

function isGitHostedPkgUrl (url: string) {
  return (
    url.startsWith('https://codeload.github.com/') ||
    url.startsWith('https://bitbucket.org/') ||
    url.startsWith('https://gitlab.com/')
  ) && url.includes('tar.gz')
}

export async function waitForFilesIndex (filesIndex: FilesIndex): Promise<Record<string, PackageFileInfo>> {
  return fromPairs(
    await Promise.all(
      Object.entries(filesIndex).map(async ([fileName, fileInfo]): Promise<[string, PackageFileInfo]> => {
        const { integrity, checkedAt } = await fileInfo.writeResult
        return [
          fileName,
          {
            ...omit(['writeResult'], fileInfo),
            checkedAt,
            integrity: integrity.toString(),
          },
        ]
      })
    )
  )
}

async function prepareGitHostedPkg (filesIndex: FilesIndex, cafs: Cafs) {
  const tempLocation = await cafs.tempDir()
  await cafs.importPackage(tempLocation, {
    filesResponse: {
      filesIndex: await waitForFilesIndex(filesIndex),
      fromStore: false,
    },
    force: true,
  })
  await preparePackage(tempLocation)
  const newFilesIndex = await cafs.addFilesFromDir(tempLocation)
  return newFilesIndex
}

async function safeCheckStream (stream: any, integrity: string, url: string): Promise<true | Error> { // eslint-disable-line @typescript-eslint/no-explicit-any
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
