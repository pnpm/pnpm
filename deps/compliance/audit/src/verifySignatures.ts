import crypto from 'node:crypto'
import url from 'node:url'

import { PnpmError } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import { type DispatcherOptions, fetchWithDispatcher, type RetryTimeoutOptions } from '@pnpm/network.fetch'

export interface SignaturePackage {
  name: string
  registry: string
  version: string
}

export interface SignatureIssue extends SignaturePackage {
  integrity?: string
  reason?: string
  resolved?: string
}

export interface SignatureVerificationResult {
  audited: number
  invalid: SignatureIssue[]
  missing: SignatureIssue[]
  verified: number
}

interface RegistryKey {
  expires: string | null
  key: string
  keyid: string
  keytype: string
  scheme: string
}

interface RegistryKeysResponse {
  keys: RegistryKey[]
}

interface PackageSignature {
  keyid: string
  sig: string
}

interface PackumentVersion {
  dist?: {
    integrity?: string
    shasum?: string
    signatures?: PackageSignature[]
    tarball?: string
  }
}

interface Packument {
  time?: Record<string, string>
  versions?: Record<string, PackumentVersion>
}

export async function verifySignatures (
  packages: SignaturePackage[],
  getAuthHeader: GetAuthHeader,
  opts: {
    dispatcherOptions?: DispatcherOptions
    retry?: RetryTimeoutOptions
    timeout?: number
  }
): Promise<SignatureVerificationResult> {
  const registries = new Set(packages.map(({ registry }) => registry))
  const keysByRegistry = new Map<string, RegistryKey[]>()
  await Promise.all(Array.from(registries, async (registry) => {
    keysByRegistry.set(registry, await fetchRegistryKeys(registry, getAuthHeader, opts))
  }))

  const result: SignatureVerificationResult = {
    audited: 0,
    invalid: [],
    missing: [],
    verified: 0,
  }
  const packumentCache = new Map<string, Promise<Packument>>()

  await pMap(packages, async (pkg) => {
    const keys = keysByRegistry.get(pkg.registry) ?? []
    if (keys.length === 0) return
    result.audited++

    let version: PackumentVersion | undefined
    let publishedAt: string | undefined
    try {
      const packument = await getPackument(pkg, getAuthHeader, opts, packumentCache)
      version = packument.versions?.[pkg.version]
      publishedAt = packument.time?.[pkg.version]
    } catch (err: unknown) {
      result.invalid.push({ ...pkg, reason: err instanceof Error ? err.message : String(err) })
      return
    }

    const integrity = version?.dist?.integrity
    const resolved = version?.dist?.tarball
    const signatures = version?.dist?.signatures ?? []
    if (!version || !integrity) {
      result.invalid.push({ ...pkg, integrity, reason: `Missing registry metadata for ${pkg.name}@${pkg.version}`, resolved })
      return
    }
    if (signatures.length === 0) {
      result.missing.push({ ...pkg, integrity, resolved })
      return
    }

    const issue = verifyPackageSignatures({ ...pkg, integrity, publishedAt, resolved, signatures }, keys)
    if (issue) {
      result.invalid.push(issue)
      return
    }
    result.verified++
  }, 20)

  result.invalid.sort(sortIssue)
  result.missing.sort(sortIssue)
  return result
}

async function fetchRegistryKeys (
  registry: string,
  getAuthHeader: GetAuthHeader,
  opts: {
    dispatcherOptions?: DispatcherOptions
    retry?: RetryTimeoutOptions
    timeout?: number
  }
): Promise<RegistryKey[]> {
  const registryUrl = registry.endsWith('/') ? registry : `${registry}/`
  const keysUrl = new url.URL('-/npm/v1/keys', registryUrl).toString()
  const res = await fetchWithDispatcher(keysUrl, {
    dispatcherOptions: opts.dispatcherOptions ?? {},
    headers: getAuthHeaders(getAuthHeader(registryUrl)),
    method: 'GET',
    retry: opts.retry,
    timeout: opts.timeout,
  })
  if (res.status === 404 || res.status === 400) return []
  if (res.status !== 200) {
    throw new PnpmError('AUDIT_SIGNATURE_KEYS_FETCH_FAIL', `The registry keys endpoint (at ${keysUrl}) responded with ${res.status}: ${await res.text()}`)
  }
  const rawBody = await res.text()
  let body: unknown
  try {
    body = JSON.parse(rawBody)
  } catch (err: unknown) {
    const reason = err instanceof Error ? err.message : String(err)
    throw new PnpmError('AUDIT_SIGNATURE_KEYS_FETCH_FAIL', `The registry keys endpoint (at ${keysUrl}) returned invalid JSON: ${reason}. Response body: ${rawBody.slice(0, 500)}`)
  }
  if (!isRegistryKeysResponse(body)) {
    throw new PnpmError('AUDIT_SIGNATURE_KEYS_FETCH_FAIL', `The registry keys endpoint (at ${keysUrl}) returned an unexpected body. Expected an object with a keys array; got: ${JSON.stringify(body)?.slice(0, 500) ?? String(body)}`)
  }
  return body.keys.filter(({ keytype, scheme }) => keytype === 'ecdsa-sha2-nistp256' && scheme === 'ecdsa-sha2-nistp256')
}

async function getPackument (
  pkg: SignaturePackage,
  getAuthHeader: GetAuthHeader,
  opts: {
    dispatcherOptions?: DispatcherOptions
    retry?: RetryTimeoutOptions
    timeout?: number
  },
  packumentCache: Map<string, Promise<Packument>>
): Promise<Packument> {
  const cacheKey = `${pkg.registry}:${pkg.name}`
  let packument = packumentCache.get(cacheKey)
  if (!packument) {
    packument = fetchPackument(pkg, getAuthHeader, opts)
    packumentCache.set(cacheKey, packument)
  }
  return packument
}

async function fetchPackument (
  pkg: SignaturePackage,
  getAuthHeader: GetAuthHeader,
  opts: {
    dispatcherOptions?: DispatcherOptions
    retry?: RetryTimeoutOptions
    timeout?: number
  }
): Promise<Packument> {
  const registryUrl = pkg.registry.endsWith('/') ? pkg.registry : `${pkg.registry}/`
  const packumentUrl = toUri(pkg.name, registryUrl)
  const res = await fetchWithDispatcher(packumentUrl, {
    dispatcherOptions: opts.dispatcherOptions ?? {},
    headers: {
      Accept: 'application/json',
      ...getAuthHeaders(getAuthHeader(registryUrl)),
    },
    method: 'GET',
    retry: opts.retry,
    timeout: opts.timeout,
  })
  if (res.status !== 200) {
    throw new PnpmError('AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL', `The packument endpoint (at ${packumentUrl}) responded with ${res.status}: ${await res.text()}`)
  }
  const rawBody = await res.text()
  let body: unknown
  try {
    body = JSON.parse(rawBody)
  } catch (err: unknown) {
    const reason = err instanceof Error ? err.message : String(err)
    throw new PnpmError('AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL', `The packument endpoint (at ${packumentUrl}) returned invalid JSON: ${reason}. Response body: ${rawBody.slice(0, 500)}`)
  }
  if (!isPackument(body)) {
    throw new PnpmError('AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL', `The packument endpoint (at ${packumentUrl}) returned an unexpected body. Expected an object with versions; got: ${JSON.stringify(body)?.slice(0, 500) ?? String(body)}`)
  }
  return body
}

function verifyPackageSignatures (
  pkg: SignaturePackage & {
    integrity: string
    publishedAt?: string
    resolved?: string
    signatures: PackageSignature[]
  },
  keys: RegistryKey[]
): SignatureIssue | undefined {
  const message = `${pkg.name}@${pkg.version}:${pkg.integrity}`
  const publishedTime = Date.parse(pkg.publishedAt ?? '2015-01-01T00:00:00.000Z')

  for (const signature of pkg.signatures) {
    const key = keys.find(({ keyid }) => keyid === signature.keyid)
    if (!key) {
      return toSignatureIssue(pkg, `${pkg.name}@${pkg.version} has a registry signature with keyid ${signature.keyid} but no corresponding public key can be found`)
    }
    if (key.expires && publishedTime >= Date.parse(key.expires)) {
      return toSignatureIssue(pkg, `${pkg.name}@${pkg.version} has a registry signature with keyid ${signature.keyid} but the corresponding public key has expired ${key.expires}`)
    }
    const verifier = crypto.createVerify('SHA256')
    verifier.write(message)
    verifier.end()
    const pem = `-----BEGIN PUBLIC KEY-----\n${key.key}\n-----END PUBLIC KEY-----`
    if (!verifier.verify(pem, signature.sig, 'base64')) {
      return toSignatureIssue(pkg, `${pkg.name}@${pkg.version} has an invalid registry signature with keyid ${signature.keyid}`)
    }
  }
  return undefined
}

function toSignatureIssue (
  pkg: SignaturePackage & { integrity?: string, resolved?: string },
  reason: string
): SignatureIssue {
  return {
    integrity: pkg.integrity,
    name: pkg.name,
    reason,
    registry: pkg.registry,
    resolved: pkg.resolved,
    version: pkg.version,
  }
}

function isRegistryKeysResponse (body: unknown): body is RegistryKeysResponse {
  return typeof body === 'object' && body != null &&
    Array.isArray((body as RegistryKeysResponse).keys) &&
    (body as RegistryKeysResponse).keys.every((key) => typeof key === 'object' && key != null &&
      typeof key.keyid === 'string' &&
      typeof key.keytype === 'string' &&
      typeof key.scheme === 'string' &&
      typeof key.key === 'string' &&
      (key.expires == null || typeof key.expires === 'string'))
}

function isPackument (body: unknown): body is Packument {
  return typeof body === 'object' && body != null && typeof (body as Packument).versions === 'object' && (body as Packument).versions != null
}

function getAuthHeaders (authHeaderValue?: string): Record<string, string> {
  return authHeaderValue ? { Authorization: authHeaderValue } : {}
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

async function pMap<T> (items: T[], mapper: (item: T) => Promise<void>, concurrency: number): Promise<void> {
  let nextIndex = 0
  async function worker (): Promise<void> {
    const item = items[nextIndex++]
    if (item == null) return
    await mapper(item)
    await worker()
  }
  const workers = Array.from({ length: Math.min(concurrency, items.length) }, worker)
  await Promise.all(workers)
}

function sortIssue (a: SignatureIssue, b: SignatureIssue): number {
  return `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`)
}
