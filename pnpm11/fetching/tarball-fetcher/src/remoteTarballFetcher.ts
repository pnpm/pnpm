import { promises as fs } from 'node:fs'
import type { IncomingMessage } from 'node:http'
import { isIP } from 'node:net'
import path from 'node:path'

import { requestRetryLogger } from '@pnpm/core-loggers'
import { FetchError, FetchTimeoutError, isError, isFetchTimeoutError, redactUrlForDisplay } from '@pnpm/error'
import type { FetchOptions, FetchResult } from '@pnpm/fetching.fetcher-base'
import type { FetchFromRegistry, GetAuthHeader, RetryTimeoutOptions } from '@pnpm/fetching.types'
import { globalWarn } from '@pnpm/logger'
import { isNonRetryableError } from '@pnpm/network.fetch'
import type { Cafs } from '@pnpm/store.cafs-types'
import type { StoreIndex } from '@pnpm/store.index'
import { addFilesFromTarball } from '@pnpm/worker'
import * as retry from '@zkochan/retry'
import throttle from 'lodash.throttle'

import { BadTarballError } from './errorTypes/index.js'

export const MAX_BUFFERED_DOWNLOAD_SIZE = 64 * 1024 * 1024

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
  pkgId?: string
} & Pick<FetchOptions, 'appendManifest' | 'readManifest' | 'filesIndexFile' | 'ignoreFilePattern'>

export type DownloadFunction = (url: string, opts: DownloadOptions) => Promise<FetchResult>

export interface NpmRegistryClient {
  get: (url: string, getOpts: object, cb: (err: Error, data: object, raw: object, res: HttpResponse) => void) => void
  fetch: (url: string, opts: { auth?: object }, cb: (err: Error, res: IncomingMessage) => void) => void
}

export interface CreateDownloaderOptions {
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
  const defaultRetryOpts = {
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

    const downloadRetryOpts = { ...defaultRetryOpts, ...opts.retry }
    const op = retry.operation(downloadRetryOpts)

    return new Promise<FetchResult>((resolve, reject) => {
      op.attempt((attempt) => {
        void executeDownloadAttempt({
          attempt,
          authHeaderValue,
          downloadRetryOpts,
          fetchFromRegistry,
          fetchMinSpeedKiBps,
          gotOpts,
          op,
          opts,
          reject,
          resolve,
          url,
        })
      })
    })
  }
}

interface DownloadAttemptContext {
  attempt: number
  authHeaderValue: string | undefined
  downloadRetryOpts: { retries?: number }
  fetchFromRegistry: FetchFromRegistry
  fetchMinSpeedKiBps: number
  gotOpts: CreateDownloaderOptions
  op: ReturnType<typeof retry.operation>
  opts: DownloadOptions
  reject: (err: any) => void // eslint-disable-line
  resolve: (res: FetchResult) => void
  url: string
}

async function executeDownloadAttempt (ctx: DownloadAttemptContext): Promise<void> {
  try {
    const result = await fetchTarball({
      attempt: ctx.attempt,
      authHeaderValue: ctx.authHeaderValue,
      fetchFromRegistry: ctx.fetchFromRegistry,
      fetchMinSpeedKiBps: ctx.fetchMinSpeedKiBps,
      gotOpts: ctx.gotOpts,
      opts: ctx.opts,
      url: ctx.url,
    })
    ctx.resolve(result)
  } catch (error: any) { // eslint-disable-line
    handleDownloadRetry({
      attempt: ctx.attempt,
      downloadRetryOpts: ctx.downloadRetryOpts,
      error,
      op: ctx.op,
      opts: ctx.opts,
      reject: ctx.reject,
      url: ctx.url,
    })
  }
}

interface FetchTarballContext {
  attempt: number
  authHeaderValue: string | undefined
  fetchFromRegistry: FetchFromRegistry
  fetchMinSpeedKiBps: number
  gotOpts: CreateDownloaderOptions
  opts: DownloadOptions
  url: string
}

async function fetchTarball (ctx: FetchTarballContext): Promise<FetchResult> {
  let data: DownloadedTarball
  try {
    const res = await ctx.fetchFromRegistry(ctx.url, {
      authHeaderValue: ctx.authHeaderValue,
      headers: { 'accept-encoding': 'identity' },
      retry: { retries: 0 },
      redirect: ctx.opts.redirect,
      timeout: ctx.gotOpts.timeout,
    })

    if (res.status !== 200) {
      throw new FetchError({ url: ctx.url, authHeaderValue: ctx.authHeaderValue }, res)
    }

    const size = parseTarballContentLength(res)
    ctx.opts.onStart?.(size, ctx.attempt)
    const onProgress = (size != null && size >= BIG_TARBALL_SIZE && ctx.opts.onProgress)
      ? throttle(ctx.opts.onProgress, 500)
      : undefined

    data = await readResponseBody({
      body: res.body!,
      fetchMinSpeedKiBps: ctx.fetchMinSpeedKiBps,
      onProgress,
      size,
      storeDir: ctx.opts.cafs.storeDir,
      url: ctx.url,
    })
  } catch (err: unknown) {
    throw wrapFetchError(err, ctx.url, ctx.attempt, ctx.gotOpts.timeout)
  }

  return importDownloadedTarball(ctx, data)
}

async function importDownloadedTarball (ctx: FetchTarballContext, data: DownloadedTarball): Promise<FetchResult> {
  try {
    return await addFilesFromTarball({
      buffer: data.buffer,
      tarballFile: data.tarballFile,
      storeDir: ctx.opts.cafs.storeDir,
      storeIndex: ctx.opts.storeIndex,
      readManifest: ctx.opts.readManifest,
      integrity: ctx.opts.integrity,
      filesIndexFile: ctx.opts.filesIndexFile,
      pkgId: ctx.opts.pkgId,
      url: ctx.url,
      pkg: ctx.opts.pkg,
      appendManifest: ctx.opts.appendManifest,
      ignoreFilePattern: ctx.opts.ignoreFilePattern,
    })
  } finally {
    await data.cleanup?.()
  }
}

function parseTarballContentLength (res: { headers: { get: (name: string) => string | null, has: (name: string) => boolean } }): number | null {
  const isEncoded = isContentEncoded(res.headers.get('content-encoding'))
  const contentLength = !isEncoded && res.headers.has('content-length') && res.headers.get('content-length')
  const parsedLength = typeof contentLength === 'string' ? parseInt(contentLength, 10) : NaN
  return Number.isFinite(parsedLength) && parsedLength >= 0 ? parsedLength : null
}

interface DownloadedTarball {
  buffer?: Buffer
  tarballFile?: string
  cleanup?: () => Promise<void>
}

interface DownloadBuffer {
  chunks: Uint8Array[]
  downloaded: number
  directory?: string
  file?: Awaited<ReturnType<typeof fs.open>>
}

type ReadResponseBodyOptions = {
  body: AsyncIterable<unknown>
  fetchMinSpeedKiBps: number
  onProgress: ((downloaded: number) => void) | undefined
  size: number | null
  storeDir: string
  url: string
}

async function readResponseBody (opts: ReadResponseBodyOptions): Promise<DownloadedTarball> {
  const startTime = Date.now()
  if (opts.size != null && opts.size <= MAX_BUFFERED_DOWNLOAD_SIZE) {
    const buffer = await readKnownSizeBody(opts)
    checkDownloadSpeed(opts.size, startTime, opts.fetchMinSpeedKiBps, opts.url)
    return { buffer }
  }
  const state: DownloadBuffer = { chunks: [], downloaded: 0 }
  try {
    for await (const chunk of opts.body) {
      const bytes = chunk as Uint8Array
      state.downloaded += bytes.byteLength
      checkDownloadedSize(opts, state.downloaded, false)
      await bufferDownloadChunk(state, bytes, opts.storeDir)
      opts.onProgress?.(state.downloaded)
    }
    checkDownloadedSize(opts, state.downloaded, true)
    checkDownloadSpeed(state.downloaded, startTime, opts.fetchMinSpeedKiBps, opts.url)
    await state.file?.close()
    return finishDownloadBuffer(state)
  } catch (err: unknown) {
    await state.file?.close().catch(() => {})
    if (state.directory) await fs.rm(state.directory, { recursive: true, force: true })
    throw err
  }
}

async function readKnownSizeBody (opts: ReadResponseBodyOptions): Promise<Buffer> {
  const buffer = Buffer.from(new SharedArrayBuffer(opts.size!))
  let downloaded = 0
  for await (const chunk of opts.body) {
    const bytes = chunk as Uint8Array
    checkDownloadedSize(opts, downloaded + bytes.byteLength, false)
    buffer.set(bytes, downloaded)
    downloaded += bytes.byteLength
    opts.onProgress?.(downloaded)
  }
  checkDownloadedSize(opts, downloaded, true)
  return buffer
}

function checkDownloadedSize (opts: ReadResponseBodyOptions, downloaded: number, finished: boolean): void {
  if (opts.size == null) return
  if (downloaded > opts.size || (finished && downloaded !== opts.size)) {
    throw new BadTarballError({ expectedSize: opts.size, receivedSize: downloaded, tarballUrl: opts.url })
  }
}

async function bufferDownloadChunk (state: DownloadBuffer, bytes: Uint8Array, storeDir: string): Promise<void> {
  if (!state.file && state.downloaded > MAX_BUFFERED_DOWNLOAD_SIZE) {
    await fs.mkdir(storeDir, { recursive: true })
    state.directory = await fs.mkdtemp(path.join(storeDir, 'download-'))
    state.file = await fs.open(path.join(state.directory, 'archive'), 'wx', 0o600)
    for (const buffered of state.chunks) {
      // eslint-disable-next-line no-await-in-loop -- writes preserve tarball byte order
      await state.file.writeFile(buffered)
    }
    state.chunks.length = 0
  }
  if (state.file) {
    await state.file.writeFile(bytes)
  } else {
    state.chunks.push(bytes)
  }
}

function finishDownloadBuffer (state: DownloadBuffer): DownloadedTarball {
  if (state.directory) {
    const tempDirectory = state.directory
    return {
      tarballFile: path.join(tempDirectory, 'archive'),
      cleanup: async () => fs.rm(tempDirectory, { recursive: true, force: true }),
    }
  }
  const buffer = Buffer.from(new SharedArrayBuffer(state.downloaded))
  let offset = 0
  for (const chunk of state.chunks) {
    buffer.set(chunk, offset)
    offset += chunk.byteLength
  }
  return { buffer }
}

function checkDownloadSpeed (downloaded: number, startTime: number, fetchMinSpeedKiBps: number, url: string): void {
  const elapsedSec = (Date.now() - startTime) / 1000
  const avgKiBps = Math.floor((downloaded / elapsedSec) / 1024)
  if (downloaded > 0 && elapsedSec > 1 && avgKiBps < fetchMinSpeedKiBps) {
    const sizeKb = Math.floor(downloaded / 1024)
    globalWarn(`Tarball download average speed ${avgKiBps} KiB/s (size ${sizeKb} KiB) is below ${fetchMinSpeedKiBps} KiB/s: ${redactUrlForDisplay(url)} (GET)`)
  }
}

function wrapFetchError (err: unknown, url: string, attempt: number, timeout?: number): Error {
  const error = isFetchTimeoutError(err)
    ? new FetchTimeoutError('FETCH_TIMEOUT', url, timeout, { cause: err })
    : isError(err) ? err : new Error(String(err), { cause: err })
  Object.assign(error, {
    attempts: attempt,
    resource: url,
  })
  return error
}

function handleDownloadRetry (opts: {
  attempt: number
  downloadRetryOpts: { retries?: number }
  error: any // eslint-disable-line
  op: ReturnType<typeof retry.operation>
  opts: DownloadOptions
  reject: (err: any) => void // eslint-disable-line
  url: string
}): void {
  const status = opts.error.response?.status
  const isManualRedirect = opts.opts.redirect === 'manual' && status >= 300 && status < 400
  const isFatal = isManualRedirect ||
    status === 401 ||
    status === 403 ||
    status === 404 ||
    opts.error.code === 'ERR_PNPM_PREPARE_PKG_FAILURE' ||
    opts.error.code === 'ERR_PNPM_TARBALL_ENTRY_TOO_LARGE' ||
    isNonRetryableError(opts.error)

  if (isFatal) {
    opts.reject(opts.error)
    return
  }

  const timeout = opts.op.retry(opts.error)
  if (timeout === false) {
    opts.reject(opts.op.mainError())
    return
  }

  logRetryAttempt(opts.attempt, opts.error, opts.downloadRetryOpts.retries ?? 0, timeout, opts.url)
}

function logRetryAttempt (
  attempt: number,
  error: any, // eslint-disable-line
  maxRetries: number,
  timeout: number,
  url: string
): void {
  const errorInfo = {
    name: error.name,
    message: error.message,
    code: error.code,
    errno: error.errno,
    status: error.status,
    statusCode: error.statusCode,
    cause: error.cause ? {
      code: error.cause.code,
      errno: error.cause.errno,
    } : undefined,
  }
  requestRetryLogger.debug({
    attempt,
    error: errorInfo,
    maxRetries,
    method: 'GET',
    timeout,
    url: redactUrlForDisplay(url),
  })
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
