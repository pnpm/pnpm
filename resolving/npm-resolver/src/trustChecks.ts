import { PnpmError } from '@pnpm/error'
import { type PackageInRegistry, type PackageMetaWithTime } from '@pnpm/registry.types'
import { type PackageVersionPolicy } from '@pnpm/types'

type TrustEvidence = 'provenance' | 'trustedPublisher'

const TRUST_RANK = {
  trustedPublisher: 2,
  provenance: 1,
} as const satisfies Record<TrustEvidence, number>

export function failIfTrustDowngraded (
  meta: PackageMetaWithTime,
  version: string,
  trustPolicyExclude?: PackageVersionPolicy
): void {
  if (trustPolicyExclude) {
    const excludeResult = trustPolicyExclude(meta.name)
    if (excludeResult === true) {
      return
    }
    if (Array.isArray(excludeResult) && excludeResult.includes(version)) {
      return
    }
  }

  const versionPublishedAt = meta.time[version]
  if (!versionPublishedAt) {
    throw new PnpmError(
      'TRUST_CHECK_FAIL',
      `Missing time for version ${version} of ${meta.name} in metadata`
    )
  }

  const versionDate = new Date(versionPublishedAt)
  const manifest = meta.versions[version]
  if (!manifest) {
    throw new PnpmError(
      'TRUST_CHECK_FAIL',
      `Missing version object for version ${version} of ${meta.name} in metadata`
    )
  }

  const strongestEvidencePriorToRequestedVersion = detectStrongestTrustEvidenceBeforeDate(meta, versionDate)
  if (strongestEvidencePriorToRequestedVersion == null) {
    return
  }

  const currentTrustEvidence = getTrustEvidence(manifest)
  if (currentTrustEvidence == null || TRUST_RANK[strongestEvidencePriorToRequestedVersion] > TRUST_RANK[currentTrustEvidence]) {
    throw new PnpmError(
      'TRUST_DOWNGRADE',
      `High-risk trust downgrade for "${meta.name}@${version}" (possible package takeover)`,
      {
        hint: `Earlier versions had ${prettyPrintTrustEvidence(strongestEvidencePriorToRequestedVersion)}, ` +
          `but this version has ${prettyPrintTrustEvidence(currentTrustEvidence)}. ` +
          'A trust downgrade may indicate a supply chain incident.',
      }
    )
  }
}

function prettyPrintTrustEvidence (trustEvidence: TrustEvidence | undefined): string {
  switch (trustEvidence) {
  case 'trustedPublisher': return 'trusted publisher'
  case 'provenance': return 'provenance attestation'
  default: return 'no trust evidence'
  }
}

function detectStrongestTrustEvidenceBeforeDate (
  meta: PackageMetaWithTime,
  beforeDate: Date
): TrustEvidence | undefined {
  let best: TrustEvidence | undefined

  for (const [version, manifest] of Object.entries(meta.versions)) {
    const ts = meta.time[version]
    if (!ts) continue

    const publishedAt = new Date(ts)
    if (!(publishedAt < beforeDate)) continue

    const trustEvidence = getTrustEvidence(manifest)
    if (!trustEvidence) continue

    if (trustEvidence === 'trustedPublisher') {
      return 'trustedPublisher'
    }
    best ||= 'provenance'
  }

  return best
}

export function getTrustEvidence (manifest: PackageInRegistry): TrustEvidence | undefined {
  if (manifest._npmUser?.trustedPublisher) {
    return 'trustedPublisher'
  }
  if (manifest.dist?.attestations?.provenance) {
    return 'provenance'
  }
  return undefined
}
