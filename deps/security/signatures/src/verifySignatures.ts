import crypto from 'node:crypto'
import url from 'node:url'
import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type RetryTimeoutOptions } from '@pnpm/network.fetch'
import pLimit from 'p-limit'

import { NPM_SIGNING_KEYS } from './npmSigningKeys.js'

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
    signatures?: unknown
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
    const rawSignatures = version?.dist?.signatures
    if (rawSignatures != null && !Array.isArray(rawSignatures)) {
      result.invalid.push({ ...pkg, integrity, resolved, reason: `Malformed registry signatures metadata for ${pkg.name}@${pkg.version}` })
      return
    }
    const signatures = rawSignatures ?? []
    if (!signatures.every(isPackageSignature)) {
      result.invalid.push({ ...pkg, integrity, resolved, reason: `Malformed registry signatures metadata for ${pkg.name}@${pkg.version}` })
      return
    }
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

  const body = await parseJsonResponse(response, 'AUDIT_SIGNATURE_KEYS_FETCH_FAIL', 'The registry keys endpoint')

  if (!isRegistryKeysResponse(body)) {
    const code = 'AUDIT_SIGNATURE_KEYS_FETCH_FAIL'
    const message = `The registry keys endpoint (at ${response.url}) returned an unexpected body. Expected an object with a keys array; got: ${JSON.stringify(body)?.slice(0, 500) ?? String(body)}`
    throw new PnpmError(code, message)
  }

  // npm registry signing currently uses ECDSA P-256 keys. Sigstore provenance
  // attestations are intentionally handled separately from this registry check.
  return body.keys.filter(({ keytype, scheme }) => keytype === 'ecdsa-sha2-nistp256' && scheme === 'ecdsa-sha2-nistp256')
}


async function getPackument (
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

  if (response.status === 404) {
    return undefined
  }

  if (response.status !== 200) {
    const code = 'AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL'
    const message = `The packument endpoint (at ${response.url}) responded with ${response.status}: ${await response.text()}`
    throw new PnpmError(code, message)
  }

  const body = await parseJsonResponse(response, 'AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL', 'The packument endpoint')

  if (!isPackument(body)) {
    const code = 'AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL'
    const message = `The packument endpoint (at ${response.url}) returned an unexpected body. Expected an object with versions; got: ${JSON.stringify(body)?.slice(0, 500) ?? String(body)}`
    throw new PnpmError(code, message)
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
  // Registry signatures cover the package identity and content integrity.
  const message = `${pkg.name}@${pkg.version}:${pkg.integrity}`
  const publishedTime = pkg.publishedAt ? Date.parse(pkg.publishedAt) : undefined

  // A package is accepted as soon as ONE signature made by a trusted key
  // validates. Signatures from unknown/expired/invalid keys are recorded but do
  // not on their own fail the package — otherwise a key rotation (a packument
  // carrying multiple signatures) breaks, and a mirror could force a failure
  // just by appending a junk signature. We fail only when no signature validates
  // against a trusted key.
  const failures: string[] = []
  for (const signature of pkg.signatures) {
    const key = keys.find(({ keyid }) => keyid === signature.keyid)
    if (!key) {
      failures.push(`${pkg.name}@${pkg.version} has a registry signature with keyid ${signature.keyid} but no corresponding public key can be found`)
      continue
    }
    // Key expiry is a consistency check, not a security boundary: the publish
    // time comes from the same unauthenticated packument as the signatures, so
    // a forger holding an expired trusted key could backdate it anyway. The
    // signature verification below is what gates acceptance. That is why a
    // missing publish time keeps the key usable instead of failing closed —
    // the same trade-off npm's pacote makes by substituting a pre-expiry date.
    if (key.expires && publishedTime != null && publishedTime >= Date.parse(key.expires)) {
      failures.push(`${pkg.name}@${pkg.version} has a registry signature with keyid ${signature.keyid} but the corresponding public key has expired ${key.expires}`)
      continue
    }
    const pem = `-----BEGIN PUBLIC KEY-----\n${key.key}\n-----END PUBLIC KEY-----`
    // crypto.verify can throw on malformed PEM key material or signature bytes
    // returned by the registry; treat any failure as an invalid signature so
    // one bad key doesn't crash the whole audit.
    let verified: boolean
    try {
      const verifier = crypto.createVerify('SHA256')
      verifier.write(message)
      verifier.end()
      verified = verifier.verify(pem, signature.sig, 'base64')
    } catch {
      verified = false
    }
    if (verified) return undefined
    failures.push(`${pkg.name}@${pkg.version} has an invalid registry signature with keyid ${signature.keyid}`)
  }
  return toSignatureIssue(pkg, pickMostTellingFailure(pkg, failures))
}

/**
 * The reason to surface when no signature validated against a trusted key.
 * Prefer an invalid signature from a known key (a tamper signal) over an
 * unknown-key or expiry reason, since unknown keys may just be junk a mirror
 * appended.
 */
function pickMostTellingFailure (
  pkg: SignaturePackage,
  failures: string[]
): string {
  if (failures.length === 0) {
    return `${pkg.name}@${pkg.version} has no registry signature from a trusted key`
  }
  return failures.find((reason) => reason.includes('invalid registry signature')) ?? failures[0]
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

async function parseJsonResponse (
  response: { url: string, text: () => Promise<string> },
  errorCode: string,
  endpointDescription: string
): Promise<unknown> {
  const rawBody = await response.text()
  try {
    return JSON.parse(rawBody)
  } catch (err: unknown) {
    const reason = util.types.isNativeError(err) ? err.message : String(err)
    throw new PnpmError(errorCode, `${endpointDescription} (at ${response.url}) returned invalid JSON: ${reason}. Response body: ${rawBody.slice(0, 500)}`)
  }
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

function isPackageSignature (signature: unknown): signature is PackageSignature {
  return typeof signature === 'object' && signature != null &&
    typeof (signature as PackageSignature).keyid === 'string' &&
    typeof (signature as PackageSignature).sig === 'string'
}

function sortIssue (a: SignatureIssue, b: SignatureIssue): number {
  return `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`)
}

export type { RegistryKey }

/**
 * The trusted npm signing keys used to verify package-manager binaries before
 * pnpm spawns them — npm's public keys embedded in the CLI. There is
 * deliberately no way to override or disable them at runtime: a verification
 * off-switch would be a footgun, and npm mirrors work without one (they proxy
 * the same signed packument, which is verified against these keys). The keys
 * are refreshed at release time by the update-npm-signing-keys script.
 */
export function getNpmSigningKeys (): RegistryKey[] {
  return NPM_SIGNING_KEYS.map((k) => ({ ...k }))
}

export interface InstalledPackageToVerify {
  name: string
  /** The registry the package was installed from — the packument (and its signatures) is fetched from here. */
  registry: string
  version: string
  /** Integrity of the bytes actually installed on disk (from the lockfile). */
  integrity: string
}

/**
 * Why a package failed signature verification:
 * - `invalid`: a registry signature is present but does not validate over the
 *   installed bytes — a strong tamper signal.
 * - `absent`: the package/version is not on the (canonical) registry, or carries
 *   no signature — suspicious for a package that is expected to be signed.
 * - `unreachable`: the trust root could not be consulted (registry advertised no
 *   signing keys, or the network request failed) — typically transient/offline,
 *   not evidence of tampering.
 */
export type SignatureFailureCategory = 'invalid' | 'absent' | 'unreachable'

export interface InstalledSignatureFailure {
  name: string
  version: string
  reason: string
  category: SignatureFailureCategory
}

export interface InstalledSignatureVerificationResult {
  verified: boolean
  failures: InstalledSignatureFailure[]
}

/**
 * Verifies that the bytes installed on disk are exactly what the registry
 * signed for `name@version`. The signed message is built from the
 * caller-supplied installed {@link InstalledPackageToVerify.integrity}, not
 * from the integrity in the freshly-fetched packument — so if the integrity
 * on disk was tampered with (or fetched from a different registry), the
 * registry's signature will not validate over it.
 *
 * Signatures are verified against the caller-supplied `trustedKeys` (npm's
 * embedded public keys, see {@link getNpmSigningKeys}) rather than keys fetched
 * from a registry — so a registry the caller cannot vouch for cannot answer with
 * its own key pair. The packument (which carries the signatures) is fetched from
 * each package's own registry; an npm mirror works transparently because it
 * proxies the same signed packument.
 *
 * A package counts as a failure when the package is unsigned/unpublished, or
 * when a signature is present but does not validate over the installed bytes.
 */
export async function verifyInstalledPackageSignatures (
  packages: InstalledPackageToVerify[],
  trustedKeys: RegistryKey[],
  getAuthHeader: GetAuthHeader,
  opts: VerifySignaturesOptions
): Promise<InstalledSignatureVerificationResult> {
  const packumentCache = new Map<string, Promise<Packument | undefined>>()
  const limit = pLimit(opts.networkConcurrency ?? 16)

  const failures: InstalledSignatureFailure[] = []
  await Promise.all(packages.map((pkg) => limit(async () => {
    const failure = await findSignatureFailure(pkg, trustedKeys, getAuthHeader, opts, packumentCache)
    if (failure != null) {
      failures.push({ name: pkg.name, version: pkg.version, ...failure })
    }
  })))

  failures.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
  return { verified: failures.length === 0, failures }
}

async function findSignatureFailure (
  pkg: InstalledPackageToVerify,
  trustedKeys: RegistryKey[],
  getAuthHeader: GetAuthHeader,
  opts: VerifySignaturesOptions,
  packumentCache: Map<string, Promise<Packument | undefined>>
): Promise<{ reason: string, category: SignatureFailureCategory } | undefined> {
  let packument: Packument | undefined
  try {
    packument = await getPackument(pkg, getAuthHeader, opts, packumentCache)
  } catch (err: unknown) {
    return { reason: util.types.isNativeError(err) ? err.message : String(err), category: 'unreachable' }
  }
  if (!packument) return { reason: `${pkg.name} is not published on ${pkg.registry}`, category: 'absent' }

  const version = packument.versions?.[pkg.version]
  if (!version) return { reason: `${pkg.name}@${pkg.version} was not found on ${pkg.registry}`, category: 'absent' }

  const rawSignatures = version.dist?.signatures
  if (rawSignatures != null && !Array.isArray(rawSignatures)) {
    return { reason: `malformed registry signatures metadata for ${pkg.name}@${pkg.version}`, category: 'absent' }
  }
  const signatures = rawSignatures ?? []
  if (!signatures.every(isPackageSignature)) {
    return { reason: `malformed registry signatures metadata for ${pkg.name}@${pkg.version}`, category: 'absent' }
  }
  if (signatures.length === 0) {
    return { reason: `${pkg.name}@${pkg.version} has no registry signature`, category: 'absent' }
  }

  // The message is built from the installed integrity, so a signature only
  // validates when the installed bytes match what the registry signed.
  const issue = verifyPackageSignatures(
    { ...pkg, integrity: pkg.integrity, publishedAt: packument.time?.[pkg.version], signatures },
    trustedKeys
  )
  return issue == null ? undefined : { reason: issue.reason ?? 'invalid registry signature', category: 'invalid' }
}
