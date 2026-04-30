import crypto from 'node:crypto'
import url from 'node:url'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type RetryTimeoutOptions } from '@pnpm/network.fetch'
import pLimit from 'p-limit'

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

export interface VerifySignaturesOptions extends CreateFetchFromRegistryOptions {
  networkConcurrency?: number
  retry?: RetryTimeoutOptions
  timeout?: number
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

export async function verifySignatures(
  packages: SignaturePackage[],
  getAuthHeader: GetAuthHeader,
  opts: VerifySignaturesOptions
): Promise<SignatureVerificationResult> {
  const registries = new Set(packages.map(({ registry }) => registry))
  const keysByRegistry = await getKeysByRegistry(registries, getAuthHeader, opts)

  const result: SignatureVerificationResult = {
    audited: 0,
    invalid: [],
    missing: [],
    verified: 0,
  }
  // Registries without signing keys are not counted as audited: there is no
  // registry trust root to verify against.
  const packumentCache = new Map<string, Promise<Packument | undefined>>()
  const limit = pLimit(opts.networkConcurrency ?? 16)

  await Promise.all(packages.map((pkg) => limit(async () => {
    const keys = keysByRegistry.get(pkg.registry) ?? []
    if (keys.length === 0) return

    let version: PackumentVersion | undefined
    let publishedAt: string | undefined
    try {
      const packument = await getPackument(pkg, getAuthHeader, opts, packumentCache)
      if (!packument) return
      result.audited++
      version = packument.versions?.[pkg.version]
      publishedAt = packument.time?.[pkg.version]
    } catch (err: unknown) {
      result.invalid.push({ ...pkg, reason: util.types.isNativeError(err) ? err.message : String(err) })
      return
    }

    const integrity = version?.dist?.integrity
    const resolved = version?.dist?.tarball
    const signatures = version?.dist?.signatures ?? []
    if (!version) {
      result.invalid.push({ ...pkg, reason: `Missing registry metadata for ${pkg.name}@${pkg.version}` })
      return
    }
    if (!integrity) {
      result.missing.push({ ...pkg, resolved })
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
  })))

  result.invalid.sort(sortIssue)
  result.missing.sort(sortIssue)
  return result
}

async function getKeysByRegistry (
  registries: Set<string>,
  getAuthHeader: GetAuthHeader,
  opts: VerifySignaturesOptions
): Promise<Map<string, RegistryKey[]>> {
  const keysByRegistry = new Map<string, RegistryKey[]>()
  await Promise.all(Array.from(registries, async (registry) => {
    const keys = await fetchRegistryKeys(registry, getAuthHeader, opts)
    keysByRegistry.set(registry, keys)
  }))
  return keysByRegistry
}

async function fetchRegistryKeys (
  registry: string,
  getAuthHeader: GetAuthHeader,
  opts: VerifySignaturesOptions
): Promise<RegistryKey[]> {
  const registryUrl = registry.endsWith('/') ? registry : `${registry}/`
  const keysUrl = new url.URL('-/npm/v1/keys', registryUrl).toString()
  const fetchFromRegistry = createFetchFromRegistry(opts)

  const response = await fetchFromRegistry(keysUrl, {
    authHeaderValue: getAuthHeader(registryUrl),
    method: 'GET',
    retry: opts.retry,
    timeout: opts.timeout,
  })

  if (response.status === 404 || response.status === 400) {
    return []
  }

  if (response.status !== 200) {
    const code = 'AUDIT_SIGNATURE_KEYS_FETCH_FAIL'
    const message = `The registry keys endpoint (at ${response.url}) responded with ${response.status}: ${await response.text()}`
    throw new PnpmError(code, message)
  }

  const rawBody = await response.text()
  let body: unknown

  try {
    body = JSON.parse(rawBody)
  } catch (err: unknown) {
    const reason = util.types.isNativeError(err) ? err.message : String(err)
    const code = 'AUDIT_SIGNATURE_KEYS_FETCH_FAIL'
    const message = `The registry keys endpoint (at ${response.url}) returned invalid JSON: ${reason}. Response body: ${rawBody.slice(0, 500)}`
    throw new PnpmError(code, message)
  }

  if (!isRegistryKeysResponse(body)) {
    const code = 'AUDIT_SIGNATURE_KEYS_FETCH_FAIL'
    const message = `The registry keys endpoint (at ${response.url}) returned an unexpected body. Expected an object with a keys array; got: ${JSON.stringify(body)?.slice(0, 500) ?? String(body)}`
    throw new PnpmError(code, message)
  }

  // npm registry signing currently uses ECDSA P-256 keys. Sigstore provenance
  // attestations are intentionally handled separately from this registry check.
  return body.keys.filter(({ keytype, scheme }) => keytype === 'ecdsa-sha2-nistp256' && scheme === 'ecdsa-sha2-nistp256')
}


async function getPackument(
  pkg: SignaturePackage,
  getAuthHeader: GetAuthHeader,
  opts: VerifySignaturesOptions,
  packumentCache: Map<string, Promise<Packument | undefined>>
): Promise<Packument | undefined> {
  const cacheKey = `${pkg.registry}:${pkg.name}`
  let packument = packumentCache.get(cacheKey)
  if (!packument) {
    // Multiple installed versions share one full packument fetch.
    packument = fetchPackument(pkg, getAuthHeader, opts)
    packumentCache.set(cacheKey, packument)
  }
  return packument
}

async function fetchPackument(
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

  if (response.status === 404) {
    return undefined
  }

  if (response.status !== 200) {
    const code = 'AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL'
    const message = `The packument endpoint (at ${response.url}) responded with ${response.status}: ${await response.text()}`
    throw new PnpmError(code, message)
  }

  const rawBody = await response.text()
  let body: unknown

  try {
    body = JSON.parse(rawBody)
  } catch (err: unknown) {
    const reason = util.types.isNativeError(err) ? err.message : String(err)
    const code = 'AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL'
    const message = `The packument endpoint (at ${response.url}) returned invalid JSON: ${reason}. Response body: ${rawBody.slice(0, 500)}`
    throw new PnpmError(code, message)
  }

  if (!isPackument(body)) {
    const code = 'AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL'
    const message = `The packument endpoint (at ${response.url}) returned an unexpected body. Expected an object with versions; got: ${JSON.stringify(body)?.slice(0, 500) ?? String(body)}`
    throw new PnpmError(code, message)
  }

  return body
}

function verifyPackageSignatures(
  pkg: SignaturePackage & {
    integrity: string
    publishedAt?: string
    resolved?: string
    signatures: PackageSignature[]
  },
  keys: RegistryKey[]
): SignatureIssue | undefined {
  // Registry signatures cover the package identity and content integrity.
  const message = `${pkg.name}@${pkg.version}:${pkg.integrity}`
  const publishedTime = pkg.publishedAt ? Date.parse(pkg.publishedAt) : undefined

  for (const signature of pkg.signatures) {
    const key = keys.find(({ keyid }) => keyid === signature.keyid)
    if (!key) {
      const reason = `${pkg.name}@${pkg.version} has a registry signature with keyid ${signature.keyid} but no corresponding public key can be found`
      return toSignatureIssue(pkg, reason)
    }
    // Without publish time metadata we cannot safely compare against key expiry,
    // so keep verifying with the key instead of failing closed on incomplete metadata.
    if (key.expires && publishedTime != null && publishedTime >= Date.parse(key.expires)) {
      const reason = `${pkg.name}@${pkg.version} has a registry signature with keyid ${signature.keyid} but the corresponding public key has expired ${key.expires}`
      return toSignatureIssue(pkg, reason)
    }
    const verifier = crypto.createVerify('SHA256')
    verifier.write(message)
    verifier.end()
    const pem = `-----BEGIN PUBLIC KEY-----\n${key.key}\n-----END PUBLIC KEY-----`
    if (!verifier.verify(pem, signature.sig, 'base64')) {
      const reason = `${pkg.name}@${pkg.version} has an invalid registry signature with keyid ${signature.keyid}`
      return toSignatureIssue(pkg, reason)
    }
  }
  return undefined
}

function toSignatureIssue(
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

function toUri(pkgName: string, registry: string): string {
  let encodedName: string
  if (pkgName[0] === '@') {
    encodedName = `@${encodeURIComponent(pkgName.slice(1))}`
  } else {
    encodedName = encodeURIComponent(pkgName)
  }
  return new url.URL(encodedName, registry.endsWith('/') ? registry : `${registry}/`).toString()
}

function isRegistryKeysResponse(body: unknown): body is RegistryKeysResponse {
  return typeof body === 'object' && body != null &&
    Array.isArray((body as RegistryKeysResponse).keys) &&
    (body as RegistryKeysResponse).keys.every((key) => typeof key === 'object' && key != null &&
      typeof key.keyid === 'string' &&
      typeof key.keytype === 'string' &&
      typeof key.scheme === 'string' &&
      typeof key.key === 'string' &&
      (key.expires == null || typeof key.expires === 'string'))
}

function isPackument(body: unknown): body is Packument {
  return typeof body === 'object' && body != null && typeof (body as Packument).versions === 'object' && (body as Packument).versions != null
}

function sortIssue(a: SignatureIssue, b: SignatureIssue): number {
  return `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`)
}
