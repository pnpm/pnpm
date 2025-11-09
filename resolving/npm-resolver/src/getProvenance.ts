import { type PackageInRegistry, type PackageMetaWithTime } from '@pnpm/registry.types'

type Provenance = boolean | 'trustedPublisher'

export function getProvenance (manifest: PackageInRegistry): Provenance | undefined {
  const provenance = manifest._npmUser?.trustedPublisher
    ? 'trustedPublisher'
    : !!manifest.dist?.attestations?.provenance
  return provenance || undefined
}

function getHighestProvenanceBeforeDate (
  meta: PackageMetaWithTime,
  beforeDate: Date
): Provenance | undefined {
  const versionsWithDates = Object.entries(meta.versions)
    .map(([version, manifest]) => ({
      version,
      manifest,
      publishedAt: meta.time[version] ? new Date(meta.time[version]) : undefined,
    }))
    .filter((entry): entry is { version: string, manifest: PackageInRegistry, publishedAt: Date } =>
      entry.publishedAt != null &&
      !isNaN(entry.publishedAt.getTime()) &&
      entry.publishedAt <= beforeDate
    )
    .sort((a, b) => b.publishedAt.getTime() - a.publishedAt.getTime()) // Newest first

  let highestProvenance: Provenance | undefined

  for (const { manifest } of versionsWithDates) {
    const provenance = getProvenance(manifest)
    if (!provenance) continue

    if (provenance === 'trustedPublisher') {
      return 'trustedPublisher'
    } else if (!highestProvenance) {
      highestProvenance = provenance
    }
  }

  return highestProvenance
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

  const highestBefore = getHighestProvenanceBeforeDate(meta, versionDate)
  if (!highestBefore) {
    return false
  }

  const currentProvenance = getProvenance(manifest)
  if (highestBefore === 'trustedPublisher' && currentProvenance !== 'trustedPublisher') {
    return true
  }
  if (highestBefore === true && !currentProvenance) {
    return true
  }

  return false
}
