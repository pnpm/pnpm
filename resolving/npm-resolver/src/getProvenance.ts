import { type PackageInRegistry, type PackageMetaWithTime } from '@pnpm/registry.types'

type TrustEvidence = 'provenance' | 'trustedPublisher'

const TRUST_RANK = {
  trustedPublisher: 2,
  provenance: 1,
} as const satisfies Record<TrustEvidence, number>

export function getTrustEvidence (manifest: PackageInRegistry): TrustEvidence | undefined {
  if (manifest._npmUser?.trustedPublisher) {
    return 'trustedPublisher'
  }
  if (manifest.dist?.attestations?.provenance) {
    return 'provenance'
  }
  return undefined
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

export function isProvenanceDowngraded (
  meta: PackageMetaWithTime,
  version: string
): boolean | undefined {
  const versionPublishedAt = meta.time[version]
  if (!versionPublishedAt) {
    return undefined
  }

  const versionDate = new Date(versionPublishedAt)
  const manifest = meta.versions[version]
  if (!manifest) {
    return undefined
  }

  const strongestEvidencePriorToRequestedVersion = detectStrongestTrustEvidenceBeforeDate(meta, versionDate)
  if (strongestEvidencePriorToRequestedVersion == null) {
    return false
  }

  const currentTrustEvidence = getTrustEvidence(manifest)
  if (currentTrustEvidence == null) {
    return true
  }
  return TRUST_RANK[strongestEvidencePriorToRequestedVersion] > TRUST_RANK[currentTrustEvidence]
}
