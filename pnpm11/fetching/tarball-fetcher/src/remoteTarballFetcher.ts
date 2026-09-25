import type { IncomingMessage } from 'node:http'
import { isIP } from 'node:net'
import util from 'node:util'

import { requestRetryLogger } from '@pnpm/core-loggers'
import { FetchError, FetchTimeoutError, isFetchTimeoutError, redactUrlForDisplay } from '@pnpm/error'
import type { FetchOptions, FetchResult } from '@pnpm/fetching.fetcher-base'
import type { FetchFromRegistry, GetAuthHeader, RetryTimeoutOptions } from '@pnpm/fetching.types'
import { globalWarn } from '@pnpm/logger'
import { isNonRetryableError } from '@pnpm/network.fetch'
import type { Cafs, FilesMap } from '@pnpm/store.cafs-types'
import { type StoreIndex, storeIndexKey } from '@pnpm/store.index'
import { addFilesFromTarball } from '@pnpm/worker'
import * as retry from '@zkochan/retry'
import throttle from 'lodash.throttle'

import { BadTarballError } from './errorTypes/index.js'
import {
  hasDirective,
  loadTarballResolution,
  storeTarballResolution,
  tarballFreshness,
} from './httpCache.js'

const BIG_TARBALL_SIZE = 1024 * 1024 * 5 // 5 MB

export interface HttpResponse {
  body: string
}

export type DownloadOptions = {
  getAuthHeaderByURI: GetAuthHeader
  cafs: Cafs
  registry?: string
  onStart?: (totalSize: number | null, attempt: number) => void
  onProgress?: (downloaded: number) => void
  integrity?: string
  redirect?: RequestRedirect
  retry?: Pick<RetryTimeoutOptions, 'retries'>
  storeIndex: StoreIndex
  pkg?: FetchOptions['pkg']
  cacheDir?: string
  pkgId?: string
} & Pick<FetchOptions, 'appendManifest' | 'readManifest' | 'filesIndexFile' | 'ignoreFilePattern'>

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
    const authHeaderValue = getSecureNodeMirrorAuthHeader(
      opts.getAuthHeaderByURI(url, { pkgName: opts.pkg?.name }),
      url,
      opts.appendManifest?.name
    )

    const downloadRetryOpts = { ...retryOpts, ...opts.retry }
    const op = retry.operation(downloadRetryOpts)

    return new Promise<FetchResult>((resolve, reject) => {
      op.attempt(async (attempt) => {
        try {
          resolve(await fetch(attempt))
        } catch (error: any) { // eslint-disable-line
          if (
            (opts.redirect === 'manual' && error.response?.status >= 300 && error.response.status < 400) ||
            error.response?.status === 401 ||
            error.response?.status === 403 ||
            error.response?.status === 404 ||
            error.code === 'ERR_PNPM_PREPARE_PKG_FAILURE' ||
            isNonRetryableError(error)
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
            maxRetries: downloadRetryOpts.retries,
            method: 'GET',
            timeout,
            url: redactUrlForDisplay(url),
          })
        }
      })
    })

    async function fetch (currentAttempt: number): Promise<FetchResult> {
      const cacheKey = opts.pkgId ?? url
      const cached = opts.cacheDir && !opts.getAuthHeaderByURI(cacheKey)
        ? loadTarballResolution(opts.cacheDir, cacheKey)
        : undefined
      const freshness = cached ? tarballFreshness(cached) : undefined
      const stored = cached ? fetchResultFromStore(opts, cached.integrity) : undefined
      if (freshness === 'fresh' && stored) {
        return stored
      }
      let data: Buffer
      let res: Response
      try {
        res = await fetchFromRegistry(url, {
          authHeaderValue,
          ifNoneMatch: freshness === 'revalidate' && stored ? cached?.etag : undefined,
          // Tarballs are already compressed; ask the server not to apply an additional
          // Content-Encoding so Content-Length matches the body we receive and we don't
          // waste CPU on round-trip re-compression. See https://github.com/pnpm/pnpm/issues/11506
          headers: { 'accept-encoding': 'identity' },
          // The fetch library can retry requests on bad HTTP responses.
          // However, it is not enough to retry on bad HTTP responses only.
          // Requests should also be retried when the tarball's integrity check fails.
          // Hence, we tell fetch to not retry,
          // and we perform the retries from this function instead.
          retry: { retries: 0 },
          redirect: opts.redirect,
          timeout: gotOpts.timeout,
        })

        if (res.status === 304 && cached && opts.cacheDir && stored) {
          storeTarballResolution(opts.cacheDir, {
            ...cached,
            etag: res.headers.get('etag') ?? cached.etag,
            cacheControl: res.headers.get('cache-control') ?? cached.cacheControl,
            fetchedAt: Date.now(),
          })
          return stored
        }
        if (res.status !== 200) {
          throw new FetchError({ url, authHeaderValue }, res)
        }

        // When Content-Encoding is present, Content-Length refers to the encoded form
        // of the data, not the decoded bytes that the fetch implementation yields.
        // See: https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Encoding
        const isEncoded = isContentEncoded(res.headers.get('content-encoding'))
        const contentLength = !isEncoded && res.headers.has('content-length') && res.headers.get('content-length')
        const parsedLength = typeof contentLength === 'string' ? parseInt(contentLength, 10) : NaN
        const size = Number.isFinite(parsedLength) && parsedLength >= 0 ? parsedLength : null
        if (opts.onStart != null) {
          opts.onStart(size, currentAttempt)
        }
        // In order to reduce the amount of logs, we only report the download progress of big tarballs
        const onProgress = (size != null && size >= BIG_TARBALL_SIZE && opts.onProgress)
          ? throttle(opts.onProgress, 500)
          : undefined
        const startTime = Date.now()
        let downloaded = 0
        if (size !== null) {
          // Known size: pre-allocate and copy directly (avoids intermediate array + second copy pass)
          data = Buffer.from(new SharedArrayBuffer(size))
          for await (const chunk of res.body!) {
            const c = chunk as Uint8Array
            const nextDownloaded = downloaded + c.byteLength
            if (nextDownloaded > size) {
              throw new BadTarballError({
                expectedSize: size,
                receivedSize: nextDownloaded,
                tarballUrl: url,
              })
            }
            data.set(c, downloaded)
            downloaded = nextDownloaded
            onProgress?.(downloaded)
          }
          if (size !== downloaded) {
            throw new BadTarballError({
              expectedSize: size,
              receivedSize: downloaded,
              tarballUrl: url,
            })
          }
        } else {
          const chunks: Uint8Array[] = []
          for await (const chunk of res.body!) {
            const c = chunk as Uint8Array
            chunks.push(c)
            downloaded += c.byteLength
            onProgress?.(downloaded)
          }
          data = Buffer.from(new SharedArrayBuffer(downloaded))
          let offset = 0
          for (const chunk of chunks) {
            data.set(chunk, offset)
            offset += chunk.byteLength
          }
        }
        const elapsedSec = (Date.now() - startTime) / 1000
        const avgKiBps = Math.floor((downloaded / elapsedSec) / 1024)
        if (downloaded > 0 && elapsedSec > 1 && avgKiBps < fetchMinSpeedKiBps) {
          const sizeKb = Math.floor(downloaded / 1024)
          globalWarn(`Tarball download average speed ${avgKiBps} KiB/s (size ${sizeKb} KiB) is below ${fetchMinSpeedKiBps} KiB/s: ${redactUrlForDisplay(url)} (GET)`)
        }
      } catch (err: unknown) {
        const error = isFetchTimeoutError(err)
          ? new FetchTimeoutError('FETCH_TIMEOUT', url, gotOpts.timeout, { cause: err })
          : util.types.isNativeError(err) ? err : new Error(String(err), { cause: err })
        Object.assign(error, {
          attempts: currentAttempt,
          resource: url,
        })
        throw error
      }
      const fetched = await addFilesFromTarball({
        buffer: data,
        storeDir: opts.cafs.storeDir,
        storeIndex: opts.storeIndex,
        readManifest: opts.readManifest,
        integrity: opts.integrity,
        filesIndexFile: opts.filesIndexFile,
        pkgId: opts.pkgId,
        url,
        pkg: opts.pkg,
        appendManifest: opts.appendManifest,
        ignoreFilePattern: opts.ignoreFilePattern,
      })
      rememberTarballResolution(opts, cacheKey, url, res, fetched.integrity)
      return fetched
    }
  }
}

function rememberTarballResolution (
  opts: DownloadOptions,
  cacheKey: string,
  requestedUrl: string,
  res: { url: string, headers: { get (name: string): string | null } },
  integrity: string | undefined
): void {
  if (opts.cacheDir && integrity && !opts.getAuthHeaderByURI(requestedUrl)) {
    const cacheControl = res.headers.get('cache-control') ?? undefined
    storeTarballResolution(opts.cacheDir, {
      url: cacheKey,
      tarball: cacheControl && hasDirective(cacheControl, 'immutable') ? res.url : requestedUrl,
      integrity,
      etag: res.headers.get('etag') ?? undefined,
      cacheControl,
      fetchedAt: Date.now(),
    })
  }
  if (!integrity) return
  opts.storeIndex.flush()
  const raw = opts.storeIndex.getRaw(opts.filesIndexFile)
  const pkgId = opts.pkgId
  if (!raw || !pkgId) return
  const key = storeIndexKey(integrity, pkgId)
  if (key !== opts.filesIndexFile) {
    opts.storeIndex.setRawMany([{ key, buffer: raw }])
  }
}

function fetchResultFromStore (opts: DownloadOptions, integrity: string): FetchResult | undefined {
  const pkgId = opts.pkgId
  const keys = pkgId ? [storeIndexKey(integrity, pkgId), opts.filesIndexFile] : [opts.filesIndexFile]
  for (const key of keys) {
    const index = opts.storeIndex.get(key) as {
      files?: Map<string, { digest: string, mode: number }>
      manifest?: FetchResult['manifest']
      requiresBuild?: boolean
      requiresPrepare?: boolean
    } | undefined
    if (!index?.files) continue
    const filesMap: FilesMap = new Map()
    for (const [name, info] of index.files) {
      filesMap.set(name, opts.cafs.getFilePathByModeInCafs(info.digest, info.mode))
    }
    return {
      filesIndexFile: key,
      filesMap,
      manifest: index.manifest,
      requiresBuild: index.requiresBuild === true,
      requiresPrepare: index.requiresPrepare,
      integrity,
    }
  }
  return undefined
}

function getSecureNodeMirrorAuthHeader (
  authHeaderValue: string | undefined,
  url: string,
  appendedPackageName: string | undefined
): string | undefined {
  if (authHeaderValue == null || appendedPackageName !== 'node') return authHeaderValue
  const parsed = new URL(url)
  if (parsed.protocol === 'https:' || isLoopbackHost(parsed.hostname)) return authHeaderValue
  return undefined
}

function isLoopbackHost (hostname: string): boolean {
  return hostname === 'localhost' || hostname === '[::1]' || (isIP(hostname) === 4 && hostname.startsWith('127.'))
}

// Per RFC 9110 §8.4, Content-Encoding is a comma-separated list of codings.
// The response is encoded unless every coding is `identity` (case-insensitive,
// surrounding whitespace ignored).
function isContentEncoded (header: string | null): boolean {
  if (header == null) return false
  return header
    .split(',')
    .map(coding => coding.trim().toLowerCase())
    .some(coding => coding !== '' && coding !== 'identity')
}
