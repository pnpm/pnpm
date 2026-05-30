import url from 'node:url'

import { requestRetryLogger } from '@pnpm/core-loggers'
import {
  FetchError,
  type FetchErrorRequest,
  type FetchErrorResponse,
  PnpmError,
} from '@pnpm/error'
import type { FetchFromRegistry, RetryTimeoutOptions } from '@pnpm/fetching.types'
import { globalWarn } from '@pnpm/logger'
import type { PackageMeta } from '@pnpm/resolving.registry.types'
import * as retry from '@zkochan/retry'
import semver from 'semver'

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
  jsonText: string
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
  return new Promise((resolve, reject) => {
    op.attempt(async (attempt) => {
      let response: RegistryResponse
      const startTime = Date.now()
      try {
        response = await fetchOpts.fetch(uri, {
          authHeaderValue,
          compress: true,
          fullMetadata,
          ifNoneMatch: cachedEtag,
          ifModifiedSince: cachedModified ? new Date(cachedModified).toUTCString() : undefined,
          retry: fetchOpts.retry,
          timeout: fetchOpts.timeout,
        }) as RegistryResponse
      } catch (error: any) { // eslint-disable-line
        reject(new PnpmError('META_FETCH_FAIL', `GET ${uri}: ${error.message as string}`, { attempts: attempt, cause: error }))
        return
      }
      if (response.status === 304) {
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
          meta,
          jsonText,
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

function toUri (pkgName: string, registry: string): string {
  let encodedName: string

  if (pkgName[0] === '@') {
    encodedName = `@${encodeURIComponent(pkgName.slice(1))}`
  } else {
    encodedName = encodeURIComponent(pkgName)
  }

  return new url.URL(encodedName, registry.endsWith('/') ? registry : `${registry}/`).toString()
}
