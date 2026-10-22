import { isError, PnpmError } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import { createFetchFromRegistry } from '@pnpm/network.fetch'
import pLimit from 'p-limit'

import { NPM_SIGNING_KEYS } from './npmSigningKeys.js'
import {
  getPackument,
  isPackageSignature,
  isRegistryKeysResponse,
  type PackageSignature,
  type PackumentFetchContext,
  type PackumentVersion,
  parseJsonResponse,
  type RegistryKey,
  type SignaturePackage,
  type VerifySignaturesOptions,
} from './packument.js'
import { type SignatureIssue, verifyPackageSignatures } from './verifyPackageSignatures.js'

export type { RegistryKey, SignatureIssue, SignaturePackage, VerifySignaturesOptions }
export * from './verifyInstalledPackageSignatures.js'

export function getNpmSigningKeys (): RegistryKey[] {
  return NPM_SIGNING_KEYS.map((k) => ({ ...k }))
}

export interface SignatureVerificationResult {
  audited: number
  invalid: SignatureIssue[]
  missing: SignatureIssue[]
  verified: number
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
  const packumentContext: PackumentFetchContext = {
    getAuthHeader,
    opts,
    packumentCache: new Map(),
  }
  const limit = pLimit(opts.networkConcurrency ?? 16)

  await Promise.all(packages.map((pkg) => limit(async () => {
    const keys = keysByRegistry.get(pkg.registry) ?? []
    if (keys.length === 0) return
    await auditPackage(pkg, keys, packumentContext, result)
  })))

  result.invalid.sort(sortIssue)
  result.missing.sort(sortIssue)
  return result
}

async function auditPackage (
  pkg: SignaturePackage,
  keys: RegistryKey[],
  packumentContext: PackumentFetchContext,
  result: SignatureVerificationResult
): Promise<void> {
  const packumentData = await fetchPackumentData(pkg, packumentContext)
  if (packumentData.status === 'error') {
    result.invalid.push({ ...pkg, reason: packumentData.reason })
    return
  }
  if (packumentData.status === 'not-found') return

  result.audited++
  const { version, publishedAt } = packumentData
  if (packumentContext.opts.requireLockfileIntegrity && !pkg.integrity) {
    result.invalid.push({ ...pkg, reason: `Missing lockfile integrity for ${pkg.name}@${pkg.version}` })
    return
  }
  const validation = validatePackageMetadata(pkg, version)
  if (validation.issue != null) {
    if (validation.kind === 'invalid') result.invalid.push(validation.issue)
    else result.missing.push(validation.issue)
    return
  }

  const issue = verifyPackageSignatures(
    { ...pkg, integrity: validation.integrity, publishedAt, resolved: validation.resolved, signatures: validation.signatures },
    keys
  )
  if (issue) {
    result.invalid.push(issue)
    return
  }
  result.verified++
}

type PackumentDataResult =
  | { status: 'found', version?: PackumentVersion, publishedAt?: string }
  | { status: 'not-found' }
  | { status: 'error', reason: string }

async function fetchPackumentData (
  pkg: SignaturePackage,
  ctx: PackumentFetchContext
): Promise<PackumentDataResult> {
  try {
    const packument = await getPackument(pkg, ctx)
    if (!packument) return { status: 'not-found' }
    return {
      status: 'found',
      version: packument.versions?.[pkg.version],
      publishedAt: packument.time?.[pkg.version],
    }
  } catch (err: unknown) {
    return { status: 'error', reason: isError(err) ? err.message : String(err) }
  }
}

interface ValidatedMetadata {
  integrity: string
  resolved?: string
  signatures: PackageSignature[]
}

function validatePackageMetadata (
  pkg: SignaturePackage,
  version?: PackumentVersion
): ({ kind: 'valid', issue?: undefined } & ValidatedMetadata) | { kind: 'invalid' | 'missing', issue: SignatureIssue } {
  const integrity = pkg.integrity ?? version?.dist?.integrity
  const resolved = version?.dist?.tarball
  const rawSignatures = version?.dist?.signatures
  if (rawSignatures != null && !Array.isArray(rawSignatures)) {
    return { kind: 'invalid', issue: { ...pkg, integrity, resolved, reason: `Malformed registry signatures metadata for ${pkg.name}@${pkg.version}` } }
  }
  const signatures = rawSignatures ?? []
  if (!signatures.every(isPackageSignature)) {
    return { kind: 'invalid', issue: { ...pkg, integrity, resolved, reason: `Malformed registry signatures metadata for ${pkg.name}@${pkg.version}` } }
  }
  if (!version) {
    return { kind: 'invalid', issue: { ...pkg, reason: `Missing registry metadata for ${pkg.name}@${pkg.version}` } }
  }
  if (!integrity) {
    return { kind: 'missing', issue: { ...pkg, resolved } }
  }
  if (signatures.length === 0) {
    return { kind: 'missing', issue: { ...pkg, integrity, resolved } }
  }
  return { kind: 'valid', integrity, resolved, signatures }
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
  const keysUrl = new URL('-/npm/v1/keys', registryUrl).toString()
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
    const message = `The registry keys endpoint (at ${response.url}) responded with ${response.status}: ${await response.text()}`
    throw new PnpmError('AUDIT_SIGNATURE_KEYS_FETCH_FAIL', message)
  }

  const body = await parseJsonResponse(response, 'AUDIT_SIGNATURE_KEYS_FETCH_FAIL', 'The registry keys endpoint')
  if (!isRegistryKeysResponse(body)) {
    const message = `The registry keys endpoint (at ${response.url}) returned an unexpected body. Expected an object with a keys array; got: ${JSON.stringify(body)?.slice(0, 500) ?? String(body)}`
    throw new PnpmError('AUDIT_SIGNATURE_KEYS_FETCH_FAIL', message)
  }

  return body.keys.filter(({ keytype, scheme }) => keytype === 'ecdsa-sha2-nistp256' && scheme === 'ecdsa-sha2-nistp256')
}

function sortIssue (left: SignatureIssue, right: SignatureIssue): number {
  return `${left.name}@${left.version}`.localeCompare(`${right.name}@${right.version}`)
}
