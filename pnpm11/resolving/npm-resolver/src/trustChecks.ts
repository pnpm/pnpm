import util from 'node:util'

import { PnpmError } from '@pnpm/error'
import { filterPkgMetadataVersions } from '@pnpm/resolving.registry.pkg-metadata-filter'
import type { PackageInRegistry, PackageMeta, PackageMetaWithTime } from '@pnpm/resolving.registry.types'
import type { PackageVersionPolicy } from '@pnpm/types'
import semver from 'semver'

import { warnMissingTimeFieldOnce } from './pickPackage.js'
import { assertMetaHasTime } from './pickPackageFromMeta.js'

type TrustEvidence = 'provenance' | 'trustedPublisher' | 'stagedPublish'

const TRUST_RANK = {
  stagedPublish: 3,
  trustedPublisher: 2,
  provenance: 1,
} as const satisfies Record<TrustEvidence, number>

type TrustCheckOptions = Parameters<typeof failIfTrustDowngraded>[2]

export interface TrustedPick {
  pickedPackage: PackageInRegistry
  /** Candidates set aside as trust downgrades before `pickedPackage`, in the order they were picked. */
  rejectedVersions: string[]
}

/**
 * Returns `pickedPackage` when it passes {@link failIfTrustDowngraded}.
 * Otherwise sets it aside and asks `repick` for the next candidate from the
 * packument without it, the way `minimumReleaseAge` narrows the candidates,
 * until one passes. Throws the first downgrade when no candidate is left.
 *
 * Every version is checked against the full packument: setting a version
 * aside never removes the history that another version is compared with.
 */
export function pickWithoutTrustDowngrade (
  meta: PackageMeta,
  pickedPackage: PackageInRegistry,
  opts: {
    repick: (meta: PackageMeta) => PackageInRegistry | null
    trustCheck: TrustCheckOptions
  }
): TrustedPick {
  const rejectedVersions = new Set<string>()
  let firstDowngrade: unknown
  let candidate: PackageInRegistry | null = pickedPackage
  while (candidate != null && !rejectedVersions.has(candidate.version)) {
    try {
      failIfTrustDowngraded(meta, candidate.version, opts.trustCheck)
      return { pickedPackage: candidate, rejectedVersions: [...rejectedVersions] }
    } catch (err: unknown) {
      if (!isTrustDowngradeError(err)) throw err
      firstDowngrade ??= err
      rejectedVersions.add(candidate.version)
    }
    candidate = opts.repick(filterPkgMetadataVersions(meta, (version) => !rejectedVersions.has(version)))
  }
  throw firstDowngrade
}

function isTrustDowngradeError (err: unknown): boolean {
  return util.types.isNativeError(err) && 'code' in err && err.code === 'ERR_PNPM_TRUST_DOWNGRADE'
}

export function failIfTrustDowngraded (
  meta: PackageMeta,
  version: string,
  opts?: {
    trustPolicyExclude?: PackageVersionPolicy
    trustPolicyIgnoreAfter?: number
    /**
     * The `minimumReleaseAgeIgnoreMissingTime` opt-in, which declares that
     * the registry cannot date its releases. The downgrade check orders
     * history by publish date, so a packument with no `time` map leaves it
     * nothing to order and the check is skipped with a warning rather than
     * aborting the install.
     *
     * Scoped to the whole map being absent, which `dropIncompletePublishTimes`
     * makes the only shape a registry that dates some of its versions can
     * reach here in. A packument that dates every version it lists is instead
     * saying it does not have this one, so that shape keeps failing closed
     * however this flag is set.
     */
    ignoreMissingTimeField?: boolean
  }
): void {
  if (opts?.trustPolicyExclude) {
    const excludeResult = opts.trustPolicyExclude(meta.name)
    if (excludeResult === true) {
      return
    }
    if (Array.isArray(excludeResult) && excludeResult.includes(version)) {
      return
    }
  }

  if (meta.time == null && opts?.ignoreMissingTimeField) {
    warnMissingTimeFieldOnce(meta.name, 'trustPolicy')
    return
  }
  assertMetaHasTime(meta)

  const versionPublishedAt = meta.time[version]
  if (!versionPublishedAt) {
    throw new PnpmError(
      'TRUST_CHECK_FAIL',
      `Missing time for version ${version} of ${meta.name} in metadata`
    )
  }

  const versionDate = new Date(versionPublishedAt)
  if (opts?.trustPolicyIgnoreAfter) {
    const now = new Date()
    const minutesSincePublish = (now.getTime() - versionDate.getTime()) / (1000 * 60)
    if (minutesSincePublish > opts.trustPolicyIgnoreAfter) {
      return
    }
  }
  const manifest = meta.versions[version]
  if (!manifest) {
    throw new PnpmError(
      'TRUST_CHECK_FAIL',
      `Missing version object for version ${version} of ${meta.name} in metadata`
    )
  }

  const strongestEvidencePriorToRequestedVersion = detectStrongestTrustEvidenceBeforeDate(meta, versionDate, {
    excludePrerelease: !semver.prerelease(version, true),
  })
  if (strongestEvidencePriorToRequestedVersion == null) {
    return
  }

  const currentTrustEvidence = getTrustEvidence(manifest)
  if (currentTrustEvidence == null || TRUST_RANK[strongestEvidencePriorToRequestedVersion] > TRUST_RANK[currentTrustEvidence]) {
    throw new PnpmError(
      'TRUST_DOWNGRADE',
      `High-risk trust downgrade for "${meta.name}@${version}" (possible package takeover)`,
      {
        hint: 'Trust checks are based solely on publish date, not semver. ' +
          'A package cannot be installed if any earlier-published version had stronger trust evidence. ' +
          `Earlier versions had ${prettyPrintTrustEvidence(strongestEvidencePriorToRequestedVersion)}, ` +
          `but this version has ${prettyPrintTrustEvidence(currentTrustEvidence)}. ` +
          'A trust downgrade may indicate a supply chain incident.',
      }
    )
  }
}

function prettyPrintTrustEvidence (trustEvidence: TrustEvidence | undefined): string {
  switch (trustEvidence) {
    case 'stagedPublish': return 'staged publish'
    case 'trustedPublisher': return 'trusted publisher'
    case 'provenance': return 'provenance attestation'
    default: return 'no trust evidence'
  }
}

function detectStrongestTrustEvidenceBeforeDate (
  meta: PackageMetaWithTime,
  beforeDate: Date,
  options: {
    excludePrerelease: boolean
  }
): TrustEvidence | undefined {
  let best: TrustEvidence | undefined

  for (const [version, manifest] of Object.entries(meta.versions)) {
    if (options.excludePrerelease && semver.prerelease(version, true)) continue
    const ts = meta.time[version]
    if (!ts) continue

    const publishedAt = new Date(ts)
    if (!(publishedAt < beforeDate)) continue

    const trustEvidence = getTrustEvidence(manifest)
    if (!trustEvidence) continue

    if (best === undefined || TRUST_RANK[trustEvidence] > TRUST_RANK[best]) {
      best = trustEvidence
      if (best === 'stagedPublish') {
        return best
      }
    }
  }

  return best
}

export function getTrustEvidence (manifest: PackageInRegistry): TrustEvidence | undefined {
  if (manifest._npmUser?.approver) {
    return 'stagedPublish'
  }
  if (manifest._npmUser?.trustedPublisher && manifest.dist?.attestations?.provenance) {
    return 'trustedPublisher'
  }
  if (manifest.dist?.attestations?.provenance) {
    return 'provenance'
  }
  return undefined
}
