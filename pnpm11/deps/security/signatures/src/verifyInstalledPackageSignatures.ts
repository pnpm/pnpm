import { isError, redactAndSanitize } from '@pnpm/error'
import type { GetAuthHeader } from '@pnpm/fetching.types'
import pLimit from 'p-limit'

import {
  getPackument,
  isPackageSignature,
  type Packument,
  type RegistryKey,
  type VerifySignaturesOptions,
} from './packument.js'
import { verifyPackageSignatures } from './verifyPackageSignatures.js'

export interface InstalledPackageToVerify {
  name: string
  /** The registry the package was installed from — the packument (and its signatures) is fetched from here. */
  registry: string
  version: string
  /** Integrity of the bytes actually installed on disk (from the lockfile). */
  integrity: string
}

export type SignatureFailureCategory = 'invalid' | 'absent' | 'unreachable' | 'uncovered'

export interface InstalledSignatureFailure {
  name: string
  version: string
  /**
   * The registry the package was installed from (see
   * {@link InstalledPackageToVerify.registry}), with inline `user:pass@`
   * credentials and control characters stripped so the failure is safe to print or log.
   */
  registry: string
  reason: string
  category: SignatureFailureCategory
}

export interface InstalledSignatureVerificationResult {
  verified: boolean
  failures: InstalledSignatureFailure[]
}

export interface VerifyInstalledSignaturesOptions extends VerifySignaturesOptions {
  /**
   * A registry to consult for signature metadata when a package's own registry
   * cannot provide a verifiable signature.
   */
  fallbackRegistry?: string
}

export async function verifyInstalledPackageSignatures (
  packages: InstalledPackageToVerify[],
  trustedKeys: RegistryKey[],
  getAuthHeader: GetAuthHeader,
  opts: VerifyInstalledSignaturesOptions
): Promise<InstalledSignatureVerificationResult> {
  const ctx: SignatureVerificationContext = {
    cacheNamespace: 'primary',
    trustedKeys,
    getAuthHeader,
    opts,
    packumentCache: new Map(),
  }
  const limit = pLimit(opts.networkConcurrency ?? 16)

  const failures: InstalledSignatureFailure[] = []
  await Promise.all(packages.map((pkg) => limit(async () => {
    const failure = await findSignatureFailure(pkg, ctx)
    if (failure != null) {
      failures.push({ name: pkg.name, version: pkg.version, registry: redactAndSanitize(pkg.registry), ...failure })
    }
  })))

  failures.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
  return { verified: failures.length === 0, failures }
}

/** State shared by every verification of one installed-package batch. */
interface SignatureVerificationContext {
  cacheNamespace: 'fallback' | 'primary'
  trustedKeys: RegistryKey[]
  getAuthHeader: GetAuthHeader
  opts: VerifyInstalledSignaturesOptions
  packumentCache: Map<string, Promise<Packument | undefined>>
}

async function findSignatureFailure (
  pkg: InstalledPackageToVerify,
  ctx: SignatureVerificationContext
): Promise<{ reason: string, category: SignatureFailureCategory } | undefined> {
  if (!pkg.integrity.startsWith('sha512-')) {
    return {
      reason: `${pkg.name}@${pkg.version} is pinned by a non-sha512 integrity, which npm registry signatures cannot cover`,
      category: 'uncovered',
    }
  }

  const primary = await attemptSignatureVerification(pkg, pkg.registry, ctx)
  if (primary == null) return undefined

  const { fallbackRegistry } = ctx.opts
  if (fallbackRegistry == null || equalRegistries(pkg.registry, fallbackRegistry)) return primary

  return resolveFallbackFailure(pkg, fallbackRegistry, primary, ctx)
}

async function resolveFallbackFailure (
  pkg: InstalledPackageToVerify,
  fallbackRegistry: string,
  primary: { reason: string, category: SignatureFailureCategory },
  ctx: SignatureVerificationContext
): Promise<{ reason: string, category: SignatureFailureCategory } | undefined> {
  const secondary = await attemptSignatureVerification(pkg, fallbackRegistry, {
    ...ctx,
    cacheNamespace: 'fallback',
    opts: {
      ...ctx.opts,
      retry: { ...ctx.opts.retry, retries: 0 },
    },
  })
  if (secondary == null) return undefined
  if (primary.category === 'invalid') return primary
  if (secondary.category !== 'unreachable') return secondary

  return {
    reason: `${primary.reason}; the fallback registry (${redactAndSanitize(fallbackRegistry)}) could not be consulted either: ${secondary.reason}`,
    category: 'unreachable',
  }
}

async function attemptSignatureVerification (
  pkg: InstalledPackageToVerify,
  registry: string,
  ctx: SignatureVerificationContext
): Promise<{ reason: string, category: SignatureFailureCategory } | undefined> {
  const displayRegistry = redactAndSanitize(registry)
  let packument: Packument | undefined
  try {
    packument = await getPackument({ ...pkg, registry }, ctx)
  } catch (err: unknown) {
    return { reason: redactAndSanitize(isError(err) ? err.message : String(err)), category: 'unreachable' }
  }
  if (!packument) return { reason: `${pkg.name} is not published on ${displayRegistry}`, category: 'absent' }

  const version = packument.versions?.[pkg.version]
  if (!version) return { reason: `${pkg.name}@${pkg.version} was not found on ${displayRegistry}`, category: 'absent' }

  const rawSignatures = version.dist?.signatures
  if (rawSignatures != null && !Array.isArray(rawSignatures)) {
    return { reason: `malformed registry signatures metadata for ${pkg.name}@${pkg.version}`, category: 'absent' }
  }
  const signatures = rawSignatures ?? []
  if (!signatures.every(isPackageSignature)) {
    return { reason: `malformed registry signatures metadata for ${pkg.name}@${pkg.version}`, category: 'absent' }
  }
  if (signatures.length === 0) {
    return { reason: `${pkg.name}@${pkg.version} has no registry signature on ${displayRegistry}`, category: 'absent' }
  }

  const issue = verifyPackageSignatures(
    { ...pkg, integrity: pkg.integrity, publishedAt: packument.time?.[pkg.version], signatures },
    ctx.trustedKeys
  )
  return issue == null ? undefined : { reason: issue.reason ?? 'invalid registry signature', category: 'invalid' }
}

export function equalRegistries (left: string, right: string): boolean {
  return normalizeRegistryUrl(left) === normalizeRegistryUrl(right)
}

function normalizeRegistryUrl (registry: string): string {
  const withSlash = redactAndSanitize(registry.endsWith('/') ? registry : `${registry}/`)
  try {
    return new URL(withSlash).toString().toLowerCase()
  } catch {
    return withSlash.toLowerCase()
  }
}
