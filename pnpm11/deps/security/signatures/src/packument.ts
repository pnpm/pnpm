import { isError, PnpmError } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type RetryTimeoutOptions } from '@pnpm/network.fetch'

export interface RegistryKey {
  expires: string | null
  key: string
  keyid: string
  keytype: string
  scheme: string
}

export interface RegistryKeysResponse {
  keys: RegistryKey[]
}

export interface PackageSignature {
  keyid: string
  sig: string
}

export interface PackumentVersion {
  dist?: {
    integrity?: string
    shasum?: string
    signatures?: unknown
    tarball?: string
  }
}

export interface Packument {
  time?: Record<string, string>
  versions?: Record<string, PackumentVersion>
}

export interface SignaturePackage {
  /** Digest selected by the caller, which registry metadata cannot override. */
  integrity?: string
  name: string
  registry: string
  version: string
}

export interface VerifySignaturesOptions extends CreateFetchFromRegistryOptions {
  /** Reject a package without a caller-supplied integrity. */
  requireLockfileIntegrity?: boolean
  networkConcurrency?: number
  retry?: RetryTimeoutOptions
  timeout?: number
}

export interface PackumentFetchContext {
  cacheNamespace?: string
  getAuthHeader: GetAuthHeader
  opts: VerifySignaturesOptions
  packumentCache: Map<string, Promise<Packument | undefined>>
}

export async function getPackument (
  pkg: SignaturePackage,
  ctx: PackumentFetchContext
): Promise<Packument | undefined> {
  const cacheKey = `${ctx.cacheNamespace ?? ''}:${pkg.registry}:${pkg.name}`
  let packument = ctx.packumentCache.get(cacheKey)
  if (!packument) {
    packument = fetchPackument(pkg, ctx.getAuthHeader, ctx.opts)
    ctx.packumentCache.set(cacheKey, packument)
  }
  return packument
}

async function fetchPackument (
  pkg: SignaturePackage,
  getAuthHeader: GetAuthHeader,
  opts: VerifySignaturesOptions
): Promise<Packument | undefined> {
  const registryUrl = pkg.registry.endsWith('/') ? pkg.registry : `${pkg.registry}/`
  const packumentUrl = toUri(pkg.name, registryUrl)
  const fetchFromRegistry = createFetchFromRegistry(opts)

  const response = await fetchFromRegistry(packumentUrl, {
    authHeaderValue: getAuthHeader(registryUrl),
    fullMetadata: true,
    method: 'GET',
    retry: opts.retry,
    timeout: opts.timeout,
  })

  if (response.status === 404) return undefined
  if (response.status !== 200) {
    const message = `The packument endpoint (at ${response.url}) responded with ${response.status}: ${await response.text()}`
    throw new PnpmError('AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL', message)
  }

  const body = await parseJsonResponse(response, 'AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL', 'The packument endpoint')
  if (!isPackument(body)) {
    const message = `The packument endpoint (at ${response.url}) returned an unexpected body. Expected an object with versions; got: ${JSON.stringify(body)?.slice(0, 500) ?? String(body)}`
    throw new PnpmError('AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL', message)
  }
  return body
}

export async function parseJsonResponse (
  response: { url: string, text: () => Promise<string> },
  errorCode: string,
  endpointDescription: string
): Promise<unknown> {
  const rawBody = await response.text()
  try {
    return JSON.parse(rawBody)
  } catch (err: unknown) {
    const reason = isError(err) ? err.message : String(err)
    throw new PnpmError(errorCode, `${endpointDescription} (at ${response.url}) returned invalid JSON: ${reason}. Response body: ${rawBody.slice(0, 500)}`)
  }
}

export function toUri (pkgName: string, registry: string): string {
  const encodedName = pkgName.startsWith('@')
    ? `@${encodeURIComponent(pkgName.slice(1))}`
    : encodeURIComponent(pkgName)
  const base = registry.endsWith('/') ? registry : `${registry}/`
  return new URL(encodedName, base).toString()
}

export function isRegistryKeysResponse (body: unknown): body is RegistryKeysResponse {
  return typeof body === 'object' && body != null &&
    Array.isArray((body as RegistryKeysResponse).keys) &&
    (body as RegistryKeysResponse).keys.every((key) => typeof key === 'object' && key != null &&
      typeof key.keyid === 'string' &&
      typeof key.keytype === 'string' &&
      typeof key.scheme === 'string' &&
      typeof key.key === 'string' &&
      (key.expires == null || typeof key.expires === 'string'))
}

export function isPackument (body: unknown): body is Packument {
  return typeof body === 'object' && body != null && typeof (body as Packument).versions === 'object' && (body as Packument).versions != null
}

export function isPackageSignature (signature: unknown): signature is PackageSignature {
  return typeof signature === 'object' && signature != null &&
    typeof (signature as PackageSignature).keyid === 'string' &&
    typeof (signature as PackageSignature).sig === 'string'
}
