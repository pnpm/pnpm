import {
  canTrustPolicyIdentities,
  exoticSubdepsCacheIdentity,
  type ExoticSubdepsScope,
  snapshotPolicyIdentities,
} from './verifyLockfileExoticSubdeps.js'
import type { VerifierCacheIdentity } from './verifyLockfileResolutionsCache.js'

export const RESOLUTION_SHAPE_CACHE_VERSION = 2

const RESOLUTION_SHAPE_CACHE_IDENTITY: VerifierCacheIdentity = {
  policy: { resolutionShapeCheck: RESOLUTION_SHAPE_CACHE_VERSION },
  canTrustPastCheck: (cached) => cached.resolutionShapeCheck === RESOLUTION_SHAPE_CACHE_VERSION,
}

const DEPENDENCY_ALIAS_CACHE_IDENTITY: VerifierCacheIdentity = {
  policy: { dependencyAliasCheck: true },
  canTrustPastCheck: (cached) => cached.dependencyAliasCheck === true,
}

const VARIATION_RESOLUTION_CACHE_IDENTITY: VerifierCacheIdentity = {
  policy: { variationResolutionCheck: true },
  canTrustPastCheck: (cached) => cached.variationResolutionCheck === true,
}

const LOCAL_SUBDEP_CACHE_IDENTITY: VerifierCacheIdentity = {
  policy: { localSubdepCheck: true },
  canTrustPastCheck: (cached) => cached.localSubdepCheck === true,
}

/**
 * Append the always-on offline structural checks' identities (plus
 * `blockExoticSubdeps` when active) to `verifiers` before consulting or
 * recording the verification cache, so a record written before one of those
 * rules existed fails `canTrustPastCheck` and forces re-verification.
 */
export function withOfflineCheckCacheIdentities (
  verifiers: readonly VerifierCacheIdentity[],
  options?: { blockExoticSubdeps?: boolean, exoticSubdepsScope?: ExoticSubdepsScope }
): VerifierCacheIdentity[] {
  const base = [RESOLUTION_SHAPE_CACHE_IDENTITY, DEPENDENCY_ALIAS_CACHE_IDENTITY, VARIATION_RESOLUTION_CACHE_IDENTITY, LOCAL_SUBDEP_CACHE_IDENTITY]
  return options?.blockExoticSubdeps
    ? [...verifiers, ...base, exoticSubdepsCacheIdentity(options.exoticSubdepsScope)]
    : [...verifiers, ...base]
}

export function snapshotVerificationPolicy (
  verifiers: readonly VerifierCacheIdentity[],
  options?: { blockExoticSubdeps?: boolean, exoticSubdepsScope?: ExoticSubdepsScope }
): Record<string, unknown> {
  return snapshotPolicyIdentities(withOfflineCheckCacheIdentities(verifiers, options))
}

export function canTrustPreparedPolicy (
  recordedPolicy: Record<string, unknown>,
  verifiers: readonly VerifierCacheIdentity[],
  options?: { blockExoticSubdeps?: boolean, exoticSubdepsScope?: ExoticSubdepsScope }
): boolean {
  return canTrustPolicyIdentities(recordedPolicy, withOfflineCheckCacheIdentities(verifiers, options))
}
