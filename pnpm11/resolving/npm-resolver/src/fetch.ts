import url from 'node:url'

import { requestRetryLogger } from '@pnpm/core-loggers'
import {
  FetchError,
  type FetchErrorRequest,
  type FetchErrorResponse,
  FetchTimeoutError,
  isError,
  isFetchTimeoutError,
  PnpmError,
  redactUrlCredentials,
  redactUrlForDisplay,
} from '@pnpm/error'
import type { FetchFromRegistry, RetryTimeoutOptions } from '@pnpm/fetching.types'
import { globalWarn } from '@pnpm/logger'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import * as retry from '@zkochan/retry'
import semver from 'semver'

import { clearMeta } from './clearMeta.js'
import { dropIncompletePublishTimes } from './publishTimes.js'

/**
 * Content type of an abbreviated (install-oriented) package metadata document.
 * A spec-compliant registry echoes this in the response `Content-Type` when it
 * honors the abbreviated `Accept` header. Its absence signals that the registry
 * ignored the header and served the full document instead.
 * https://github.com/npm/registry/blob/ae49abf1bac0/docs/responses/package-metadata.md
 */
const ABBREVIATED_META_CONTENT_TYPE = 'application/vnd.npm.install-v1+json'

interface RegistryResponse {
  status: number
  statusText: string
  headers: {
    get: (name: string) => string | null
  }
  json: () => Promise<PackageMeta>
  text: () => Promise<string>
}

export interface FetchMetadataResult {
  meta: PackageMeta
  etag?: string
  /** The request asked for the full document, not the abbreviated one. */
  fullMetadata?: boolean
  /**
   * The response `Cache-Control` said this document is already stale
   * (`max-age=0`, `no-cache`, or `no-store`).
   */
  uncacheable?: boolean
  notModified?: false
}

export interface FetchMetadataNotModifiedResult {
  notModified: true
}

export class RegistryResponseError extends FetchError {
  public readonly pkgName: string

  constructor (
    request: FetchErrorRequest,
    response: FetchErrorResponse,
    pkgName: string
  ) {
    let hint: string | undefined
    if (response.status === 404) {
      hint = `${pkgName} is not in the npm registry, or you have no permission to fetch it.`
      const nameWithoutVersion = stripTrailingSemverSuffix(pkgName)
      if (nameWithoutVersion != null) {
        hint += ` Did you mean ${nameWithoutVersion}?`
      }
    }
    super(request, response, hint)
    this.pkgName = pkgName
  }
}

/**
 * Detect when a package name accidentally includes a `<version>` suffix
 * (e.g. `lodash@4.17.21` or `lodash4.17.21`) and return the part before the
 * version. Returns `undefined` when no semver suffix is present.
 *
 * Implemented as an O(n) scan to avoid polynomial backtracking on adversarial
 * input (CodeQL: js/polynomial-redos).
 */
function stripTrailingSemverSuffix (pkgName: string): string | undefined {
  // Common case: "name@version" – split on the rightmost '@'.
  // `atIdx > 0` rules out the leading '@' of scoped names like '@scope/foo'.
  const atIdx = pkgName.lastIndexOf('@')
  if (atIdx > 0 && semver.valid(pkgName.slice(atIdx + 1)) != null) {
    return pkgName.slice(0, atIdx)
  }
  return stripUnseparatedSemverSuffix(pkgName)
}

/**
 * Detects a trailing "<digits>.<digits>.<digits>" appended to a name with no
 * separator (e.g. "foo1.0.0"). Walks backwards through three digit-blocks
 * separated by dots; this is O(n) and free of regex backtracking.
 */
function stripUnseparatedSemverSuffix (pkgName: string): string | undefined {
  let versionStart = pkgName.length
  versionStart = consumeTrailingDigits(pkgName, versionStart)
  if (versionStart === pkgName.length || versionStart === 0 || pkgName.charCodeAt(versionStart - 1) !== 46 /* '.' */) return undefined
  versionStart--
  const beforePatch = versionStart
  versionStart = consumeTrailingDigits(pkgName, versionStart)
  if (versionStart === beforePatch || versionStart === 0 || pkgName.charCodeAt(versionStart - 1) !== 46) return undefined
  versionStart--
  const beforeMinor = versionStart
  versionStart = consumeTrailingDigits(pkgName, versionStart)
  if (versionStart === beforeMinor || versionStart === 0) return undefined
  if (semver.valid(pkgName.slice(versionStart)) == null) return undefined
  let prefix = pkgName.slice(0, versionStart)
  if (prefix.endsWith('@')) prefix = prefix.slice(0, -1)
  return prefix.length > 0 ? prefix : undefined
}

function consumeTrailingDigits (text: string, end: number): number {
  let digitsStart = end
  while (digitsStart > 0) {
    const charCode = text.charCodeAt(digitsStart - 1)
    if (charCode < 48 || charCode > 57) break
    digitsStart--
  }
  return digitsStart
}

export interface FetchMetadataFromFromRegistryOptions {
  fetch: FetchFromRegistry
  retry: RetryTimeoutOptions
  timeout: number
  fetchWarnTimeoutMs: number
}

export interface FetchMetadataOptions {
  registry: string
  authHeaderValue?: string
  cacheBypass?: boolean
  fullMetadata?: boolean
  etag?: string
  modified?: string
}

export async function fetchMetadataFromFromRegistry (
  fetchOpts: FetchMetadataFromFromRegistryOptions,
  pkgName: string,
  opts: FetchMetadataOptions
): Promise<FetchMetadataResult | FetchMetadataNotModifiedResult> {
  const request = createMetadataRequest(fetchOpts, pkgName, opts)
  const op = retry.operation(fetchOpts.retry)
  return new Promise((resolve, reject) => {
    op.attempt(async (attempt) => {
      await attemptMetadataFetch({ attempt, op, reject, request, resolve })
    })
  })
}

interface MetadataRequest {
  fetchOpts: FetchMetadataFromFromRegistryOptions
  pkgName: string
  uri: string
  authHeaderValue?: string
  cacheBypass: boolean
  fullMetadata?: boolean
  ifNoneMatch?: string
  ifModifiedSince?: string
  hasValidator: boolean
}

interface MetadataFetchAttempt {
  request: MetadataRequest
  op: ReturnType<typeof retry.operation>
  attempt: number
  resolve: (result: FetchMetadataResult | FetchMetadataNotModifiedResult) => void
  reject: (error: unknown) => void
}

function createMetadataRequest (
  fetchOpts: FetchMetadataFromFromRegistryOptions,
  pkgName: string,
  {
    authHeaderValue,
    cacheBypass = false,
    etag: cachedEtag,
    fullMetadata,
    modified: cachedModified,
    registry,
  }: FetchMetadataOptions
): MetadataRequest {
  const uri = toUri(pkgName, registry)
  const ifNoneMatch = cachedEtag
  const ifModifiedSince = cachedModified
    ? new Date(cachedModified).toUTCString()
    : undefined
  return {
    fetchOpts,
    pkgName,
    uri,
    authHeaderValue,
    cacheBypass,
    fullMetadata,
    ifNoneMatch,
    ifModifiedSince,
    hasValidator: Boolean(ifNoneMatch || ifModifiedSince),
  }
}

async function attemptMetadataFetch ({ attempt, op, reject, request, resolve }: MetadataFetchAttempt): Promise<void> {
  let response: RegistryResponse
  const startTime = Date.now()
  try {
    response = await requestMetadata(request)
  } catch (error: any) { // eslint-disable-line
    reject(toMetaFetchError(error, request, attempt))
    return
  }
  if (response.status === 304) {
    if (request.hasValidator) {
      resolve({ notModified: true })
    } else {
      reject(notModifiedWithoutCacheError(request.pkgName))
    }
    return
  }
  if (response.status >= 400) {
    reject(new RegistryResponseError({ authHeaderValue: request.authHeaderValue, url: request.uri }, response, request.pkgName))
    return
  }

  // Here we only retry broken JSON responses.
  // Other HTTP issues are retried by the @pnpm/network.fetch library
  try {
    resolve(await readMetadataResponse(request, response, startTime))
  } catch (error: any) { // eslint-disable-line
    retryBrokenMetadataResponse({ attempt, error, op, reject, request })
  }
}

async function requestMetadata (request: MetadataRequest): Promise<RegistryResponse> {
  const { fetchOpts, uri, cacheBypass, hasValidator } = request
  const requestOptions = {
    authHeaderValue: request.authHeaderValue,
    compress: true,
    fullMetadata: request.fullMetadata,
    ifNoneMatch: request.ifNoneMatch,
    ifModifiedSince: request.ifModifiedSince,
    retry: fetchOpts.retry,
    timeout: fetchOpts.timeout,
    headers: cacheBypass ? { 'cache-control': 'no-cache' } : undefined,
  }
  const response = await fetchOpts.fetch(uri, requestOptions) as RegistryResponse
  if (response.status !== 304) return response
  if (!hasValidator && !cacheBypass) {
    return await fetchOpts.fetch(uri, {
      ...requestOptions,
      headers: {
        'cache-control': 'no-cache',
      },
    }) as RegistryResponse
  }
  if (hasValidator && !cacheBypass && metadataResponseIsUncacheable(response.headers.get('cache-control'))) {
    // A mirror without the uncacheable flag revalidates without
    // `no-cache`, and a stale intermediary can answer that with a 304.
    // Ask once without validators; a second 304 still serves the mirror.
    return await fetchOpts.fetch(uri, {
      ...requestOptions,
      ifNoneMatch: undefined,
      ifModifiedSince: undefined,
      headers: {
        'cache-control': 'no-cache',
      },
    }) as RegistryResponse
  }
  return response
}

function toMetaFetchError (error: any, request: MetadataRequest, attempt: number): Error { // eslint-disable-line
  // Redact credentials embedded in the URL from the cause as well, not
  // just the top-level message: a reporter or debugger that renders
  // `error.cause` would otherwise print the raw URL-bearing message. The
  // `stack` string embeds the original (pre-mutation) message, so redact
  // it too — mutating `message` alone leaves the credentials in `stack`.
  if (isError(error)) {
    if (typeof error.message === 'string') error.message = redactUrlCredentials(error.message)
    if (typeof error.stack === 'string') error.stack = redactUrlCredentials(error.stack)
  }
  return isFetchTimeoutError(error)
    ? new FetchTimeoutError('META_FETCH_FAIL', request.uri, request.fetchOpts.timeout, { attempts: attempt, cause: error })
    : new PnpmError('META_FETCH_FAIL', redactUrlCredentials(`GET ${request.uri}: ${error.message as string}`), { attempts: attempt, cause: error })
}

async function readMetadataResponse (
  request: MetadataRequest,
  response: RegistryResponse,
  startTime: number
): Promise<FetchMetadataResult> {
  const meta = JSON.parse(await response.text()) as PackageMeta
  dropIncompletePublishTimes(meta)
  // Only the response headers decide cacheability, never the body.
  delete meta.uncacheable
  const elapsedMs = Date.now() - startTime
  if (elapsedMs > request.fetchOpts.fetchWarnTimeoutMs) {
    globalWarn(`Request took ${elapsedMs}ms: ${redactUrlForDisplay(request.uri)}`)
  }
  return {
    meta: normalizeAbbreviatedResponse({ fullMetadata: request.fullMetadata, meta, response }),
    etag: response.headers.get('etag') ?? undefined,
    fullMetadata: request.fullMetadata === true,
    uncacheable: metadataResponseIsUncacheable(response.headers.get('cache-control')),
  }
}

function retryBrokenMetadataResponse (
  { attempt, error, op, reject, request }: Omit<MetadataFetchAttempt, 'resolve'> & { error: any } // eslint-disable-line
): void {
  const { fetchOpts, uri } = request
  const timeout = op.retry(isFetchTimeoutError(error)
    ? new FetchTimeoutError('META_FETCH_FAIL', uri, fetchOpts.timeout, { attempts: attempt, cause: error })
    : new PnpmError('BROKEN_METADATA_JSON', error.message)
  )
  if (timeout === false) {
    reject(op.mainError())
    return
  }
  requestRetryLogger.debug({
    attempt,
    error: describeRetriedError(error),
    maxRetries: fetchOpts.retry.retries!,
    method: 'GET',
    timeout,
    url: uri,
  })
}

// Extract error properties into a plain object because Error properties
// are non-enumerable and don't serialize well through the logging system
function describeRetriedError (error: any) { // eslint-disable-line
  return {
    name: error.name,
    message: error.message,
    code: error.code,
    errno: error.errno,
    // undici wraps the actual network error in a cause property
    cause: error.cause ? {
      code: error.cause.code,
      errno: error.cause.errno,
    } : undefined,
  }
}

/**
 * A 304 answers a validator with "the body you already have is current". Sent
 * without one, because nothing was cached or the cache entry was lost, it
 * refers to a body nobody holds, so there is nothing to serve and nothing left
 * to retry.
 */
export function notModifiedWithoutCacheError (pkgName: string): PnpmError {
  return new PnpmError(
    'META_NOT_MODIFIED_WITHOUT_CACHE',
    `Registry returned 304 for ${pkgName} without an existing cache to refresh.`
  )
}

/**
 * When the resolver asked for abbreviated metadata but the registry ignored the
 * `Accept` header and returned the full document (detected via the response
 * `Content-Type`), strip it down to the abbreviated field set so downstream
 * consumers — the in-memory cache, the on-disk mirror, and the resolver — never
 * carry the megabytes of install-irrelevant data (scripts, exports, readme,
 * custom fields) that a full document contains.
 *
 * RegistriesByScope that honor the header (e.g. the npm registry) echo the abbreviated
 * `Content-Type`, so this is a no-op for them: no re-serialization, no field
 * stripping — the happy path pays nothing.
 */
function normalizeAbbreviatedResponse (
  { fullMetadata, meta, response }: {
    fullMetadata?: boolean
    meta: PackageMeta
    response: RegistryResponse
  }
): PackageMeta {
  if (fullMetadata) return meta
  if (parseMediaType(response.headers.get('content-type')) === ABBREVIATED_META_CONTENT_TYPE) return meta
  return clearMeta(meta)
}

/**
 * Extracts the media type from a `Content-Type` header value, dropping
 * parameters such as `; charset=utf-8`. Media types are case-insensitive
 * (RFC 9110 §8.3.1), so the result is lowercased for comparison.
 */
function parseMediaType (contentType: string | null): string | undefined {
  if (contentType == null) return undefined
  const semicolonIndex = contentType.indexOf(';')
  const mediaType = semicolonIndex === -1 ? contentType : contentType.slice(0, semicolonIndex)
  return mediaType.trim().toLowerCase()
}

function toUri (pkgName: string, registry: string): string {
  let encodedName: string

  if (pkgName[0] === '@') {
    encodedName = `@${encodeURIComponent(pkgName.slice(1))}`
  } else {
    encodedName = encodeURIComponent(pkgName)
  }

  return new url.URL(encodedName, registry.endsWith('/') ? registry : `${registry}/`).toString()
}

/**
 * `true` when `Cache-Control` says the metadata document is already stale.
 * A positive `max-age` stays cacheable.
 */
export function metadataResponseIsUncacheable (cacheControl: string | null): boolean {
  if (cacheControl == null) return false
  return cacheControl.split(',').some((directive) => {
    const [name, value] = directive.split('=', 2).map((part) => part.trim().toLowerCase())
    if (value == null) return name === 'no-cache' || name === 'no-store'
    return name === 'max-age' && /^\d+$/.test(value) && Number(value) === 0
  })
}
