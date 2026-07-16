import url from 'node:url'
import util from 'node:util'

import { requestRetryLogger } from '@pnpm/core-loggers'
import {
  FetchError,
  type FetchErrorRequest,
  type FetchErrorResponse,
  PnpmError,
  redactUrlCredentials,
} from '@pnpm/error'
import type { FetchFromRegistry, RetryTimeoutOptions } from '@pnpm/fetching.types'
import { globalWarn } from '@pnpm/logger'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import * as retry from '@zkochan/retry'
import semver from 'semver'

import { clearMeta } from './clearMeta.js'

/**
 * Content type of an abbreviated (install-oriented) package metadata document.
 * A spec-compliant registry echoes this in the response `Content-Type` when it
 * honors the abbreviated `Accept` header. Its absence signals that the registry
 * ignored the header and served the full document instead.
 * https://github.com/npm/registry/blob/main/docs/responses/package-metadata.md
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
  /**
   * The raw registry response body, used only to mirror the response to disk
   * without re-serializing `meta`. A fresh fetch always sets it, and every
   * caller sharing that in-flight request sees it. Once the request settles
   * the phase-long memo cache drops the body (see memoizeFetchMetadata.ts),
   * so later cache hits see `undefined` and the cache never pins the body.
   */
  jsonText: string | undefined
  etag?: string
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
  // Fallback: detect a trailing "<digits>.<digits>.<digits>" appended to a name
  // with no separator (e.g. "foo1.0.0"). We walk backwards through three
  // digit-blocks separated by dots; this is O(n) and free of regex backtracking.
  let i = pkgName.length
  i = consumeTrailingDigits(pkgName, i)
  if (i === pkgName.length || i === 0 || pkgName.charCodeAt(i - 1) !== 46 /* '.' */) return undefined
  i--
  const beforePatch = i
  i = consumeTrailingDigits(pkgName, i)
  if (i === beforePatch || i === 0 || pkgName.charCodeAt(i - 1) !== 46) return undefined
  i--
  const beforeMinor = i
  i = consumeTrailingDigits(pkgName, i)
  if (i === beforeMinor || i === 0) return undefined
  if (semver.valid(pkgName.slice(i)) == null) return undefined
  let prefix = pkgName.slice(0, i)
  if (prefix.endsWith('@')) prefix = prefix.slice(0, -1)
  return prefix.length > 0 ? prefix : undefined
}

function consumeTrailingDigits (s: string, end: number): number {
  let i = end
  while (i > 0) {
    const c = s.charCodeAt(i - 1)
    if (c < 48 || c > 57) break
    i--
  }
  return i
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
  fullMetadata?: boolean
  etag?: string
  modified?: string
}

export async function fetchMetadataFromFromRegistry (
  fetchOpts: FetchMetadataFromFromRegistryOptions,
  pkgName: string,
  {
    authHeaderValue,
    etag: cachedEtag,
    fullMetadata,
    modified: cachedModified,
    registry,
  }: FetchMetadataOptions
): Promise<FetchMetadataResult | FetchMetadataNotModifiedResult> {
  const uri = toUri(pkgName, registry)
  const op = retry.operation(fetchOpts.retry)
  const ifModifiedSince = cachedModified ? new Date(cachedModified).toUTCString() : undefined
  const hasValidator = Boolean(cachedEtag || ifModifiedSince)
  return new Promise((resolve, reject) => {
    op.attempt(async (attempt) => {
      let response: RegistryResponse
      const startTime = Date.now()
      try {
        const requestOptions = {
          authHeaderValue,
          compress: true,
          fullMetadata,
          ifNoneMatch: cachedEtag,
          ifModifiedSince,
          retry: fetchOpts.retry,
          timeout: fetchOpts.timeout,
        }
        response = await fetchOpts.fetch(uri, requestOptions) as RegistryResponse
        if (response.status === 304 && !hasValidator) {
          response = await fetchOpts.fetch(uri, {
            ...requestOptions,
            headers: {
              'cache-control': 'no-cache',
            },
          }) as RegistryResponse
        }
      } catch (error: any) { // eslint-disable-line
        // Redact credentials embedded in the URL from the cause as well, not
        // just the top-level message: a reporter or debugger that renders
        // `error.cause` would otherwise print the raw URL-bearing message. The
        // `stack` string embeds the original (pre-mutation) message, so redact
        // it too — mutating `message` alone leaves the credentials in `stack`.
        if (util.types.isNativeError(error)) {
          if (typeof error.message === 'string') error.message = redactUrlCredentials(error.message)
          if (typeof error.stack === 'string') error.stack = redactUrlCredentials(error.stack)
        }
        reject(new PnpmError('META_FETCH_FAIL', redactUrlCredentials(`GET ${uri}: ${error.message as string}`), { attempts: attempt, cause: error }))
        return
      }
      if (response.status === 304) {
        if (!hasValidator) {
          reject(new PnpmError(
            'META_NOT_MODIFIED_WITHOUT_CACHE',
            `Registry returned 304 for ${pkgName} without an existing cache to refresh.`
          ))
          return
        }
        resolve({ notModified: true })
        return
      }
      if (response.status >= 400) {
        const request = {
          authHeaderValue,
          url: uri,
        }
        reject(new RegistryResponseError(request, response, pkgName))
        return
      }

      // Here we only retry broken JSON responses.
      // Other HTTP issues are retried by the @pnpm/network.fetch library
      try {
        const jsonText = await response.text()
        const meta = JSON.parse(jsonText) as PackageMeta
        // Check if request took longer than expected
        const elapsedMs = Date.now() - startTime
        if (elapsedMs > fetchOpts.fetchWarnTimeoutMs) {
          globalWarn(`Request took ${elapsedMs}ms: ${uri}`)
        }
        resolve({
          ...normalizeAbbreviatedResponse({ fullMetadata, meta, jsonText, response }),
          etag: response.headers.get('etag') ?? undefined,
        })
      } catch (error: any) { // eslint-disable-line
        const timeout = op.retry(
          new PnpmError('BROKEN_METADATA_JSON', error.message)
        )
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
        }
        requestRetryLogger.debug({
          attempt,
          error: errorInfo,
          maxRetries: fetchOpts.retry.retries!,
          method: 'GET',
          timeout,
          url: uri,
        })
      }
    })
  })
}

/**
 * When the resolver asked for abbreviated metadata but the registry ignored the
 * `Accept` header and returned the full document (detected via the response
 * `Content-Type`), strip it down to the abbreviated field set so downstream
 * consumers — the in-memory cache, the on-disk mirror, and the resolver — never
 * carry the megabytes of install-irrelevant data (scripts, exports, readme,
 * custom fields) that a full document contains.
 *
 * Registries that honor the header (e.g. the npm registry) echo the abbreviated
 * `Content-Type`, so this is a no-op for them: no re-serialization, no field
 * stripping — the happy path pays nothing.
 */
function normalizeAbbreviatedResponse (
  { fullMetadata, meta, jsonText, response }: {
    fullMetadata?: boolean
    meta: PackageMeta
    jsonText: string
    response: RegistryResponse
  }
): { meta: PackageMeta, jsonText: string } {
  if (fullMetadata) return { meta, jsonText }
  if (parseMediaType(response.headers.get('content-type')) === ABBREVIATED_META_CONTENT_TYPE) return { meta, jsonText }
  const normalized = clearMeta(meta)
  return { meta: normalized, jsonText: JSON.stringify(normalized) }
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
