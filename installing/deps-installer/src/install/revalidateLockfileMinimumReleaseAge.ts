import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { PnpmError } from '@pnpm/error'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath, PackageVersionPolicy } from '@pnpm/types'
import pLimit from 'p-limit'
import semver from 'semver'

export type ManifestLookupResult =
  | { status: 'ok', publishedAt: Date }
  | { status: 'manifest-unavailable', reason?: string }
  | { status: 'version-not-in-manifest' }

export type ManifestLookup = (name: string, version: string, tarballUrl?: string) => Promise<ManifestLookupResult>

export interface RevalidateLockfileMinimumReleaseAgeOptions {
  minimumReleaseAge: number
  minimumReleaseAgeExclude?: string[]
  /** Caps concurrent manifest lookups so a large lockfile doesn't queue
   * thousands of inflight registry fetches behind the host's dispatcher
   * connection pool. Defaults to 16, which matches the floor pnpm uses for
   * its package-requester queue. */
  concurrency?: number
  now?: number
}

interface Violation {
  pkgId: string
  reason: string
}

const MAX_VIOLATIONS_TO_PRINT = 20

/**
 * Re-applies the `minimumReleaseAge` policy to every npm-registry-resolved
 * entry in an existing lockfile. The resolution-time filter inside
 * `pickPackage` only fires when pnpm is choosing a version for the first
 * time; once a version is pinned in `pnpm-lock.yaml` (e.g. by a developer
 * who bypassed the policy locally), the install paths that skip resolution
 * never re-check it — defeating the supply-chain protection the setting is
 * meant to provide. This gate runs *after* resolution decisions are settled
 * and before any tarball is fetched, so a poisoned lockfile cannot reach the
 * filesystem.
 *
 * Designed for fail-closed semantics: if a manifest can't be loaded or the
 * pinned version is missing from the manifest, that itself is reported as a
 * violation rather than silently skipped — otherwise a registry hiccup or an
 * unpublished version would re-open the same bypass this gate is meant to
 * close. This mirrors the approach taken in bun's
 * `enforceLockfileAgeFilter` (oven-sh/bun#30526).
 */
export async function revalidateLockfileAgainstMinimumReleaseAge (
  lockfile: LockfileObject,
  lookupManifest: ManifestLookup,
  opts: RevalidateLockfileMinimumReleaseAgeOptions
): Promise<void> {
  if (!lockfile.packages) return
  const cutoff = (opts.now ?? Date.now()) - opts.minimumReleaseAge * 60 * 1000
  const excludePolicy = opts.minimumReleaseAgeExclude?.length
    ? createExcludePolicy(opts.minimumReleaseAgeExclude)
    : undefined

  // depPath can include peer-dependency and patch_hash suffixes (e.g.
  // `react@18.0.0(peer)(patch_hash=…)`); the same (name, version) pair may
  // therefore appear multiple times. Dedupe so we issue at most one lookup
  // per package version.
  const candidates = new Map<string, { name: string, version: string, tarballUrl?: string }>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages)) {
    if (!isNpmRegistryResolution(snapshot.resolution)) continue
    const { name, version, nonSemverVersion } = nameVerFromPkgSnapshot(depPath as DepPath, snapshot)
    // Non-semver versions identify URL tarballs, file: references, git refs,
    // etc. — these aren't resolved through the npm registry, so the publish-date
    // policy doesn't apply and a registry lookup would 404.
    if (!name || !version || nonSemverVersion || !semver.valid(version)) continue
    if (isExcluded(excludePolicy, name, version)) continue
    const tarballUrl = (snapshot.resolution as { tarball?: string } | null | undefined)?.tarball
    candidates.set(`${name}@${version}`, { name, version, tarballUrl })
  }

  const violations: Violation[] = []
  // 16 mirrors the floor of pnpm's package-requester network-concurrency
  // (Math.min(64, Math.max(workers*3, 16))); keep them aligned so the
  // revalidation pass doesn't push past what the rest of the install respects.
  const limit = pLimit(opts.concurrency ?? 16)
  await Promise.all(
    Array.from(candidates.values(), ({ name, version, tarballUrl }) => limit(async () => {
      const pkgId = `${name}@${version}`
      let result: ManifestLookupResult
      try {
        result = await lookupManifest(name, version, tarballUrl)
      } catch (err) {
        const reason = err instanceof Error ? err.message : String(err)
        violations.push({
          pkgId,
          reason: `could not be checked against minimumReleaseAge (${reason})`,
        })
        return
      }
      switch (result.status) {
        case 'ok': {
          const ts = result.publishedAt.getTime()
          if (Number.isNaN(ts)) {
            violations.push({
              pkgId,
              reason: 'publish timestamp is not a valid date',
            })
            return
          }
          if (ts > cutoff) {
            violations.push({
              pkgId,
              reason: `was published at ${result.publishedAt.toISOString()}, within the minimumReleaseAge cutoff (${new Date(cutoff).toISOString()})`,
            })
          }
          return
        }
        case 'manifest-unavailable':
          violations.push({
            pkgId,
            reason: `could not be checked against minimumReleaseAge (manifest unavailable${result.reason ? `: ${result.reason}` : ''})`,
          })
          return
        case 'version-not-in-manifest':
          violations.push({
            pkgId,
            reason: 'could not be checked against minimumReleaseAge (version not present in registry manifest)',
          })
      }
    }))
  )

  if (violations.length === 0) return

  // Stable order so the error output is deterministic.
  violations.sort((a, b) => a.pkgId.localeCompare(b.pkgId))
  // Cap the per-entry breakdown so a poisoned lockfile with hundreds of fresh
  // versions doesn't flood the terminal / CI log; the full count is in the
  // header and the remainder is summarized at the end.
  const visible = violations.slice(0, MAX_VIOLATIONS_TO_PRINT)
  const omitted = violations.length - visible.length
  const breakdown = visible.map((v) => `  ${v.pkgId} ${v.reason}`).join('\n')
  const details = omitted > 0
    ? `${breakdown}\n  …and ${omitted} more`
    : breakdown
  throw new PnpmError(
    'MINIMUM_RELEASE_AGE_LOCKFILE_VIOLATION',
    `${violations.length} lockfile entries do not satisfy the minimumReleaseAge policy:\n${details}`,
    {
      hint: 'To unblock the install you can:\n' +
        '  1. Remove the offending entries from pnpm-lock.yaml and re-run "pnpm install --no-frozen-lockfile" so they get re-resolved against the policy.\n' +
        '  2. Lower the minimumReleaseAge value so the locked versions fall within the cutoff.\n' +
        '  3. Add the affected packages to minimumReleaseAgeExclude if they are explicitly trusted.',
    }
  )
}

function createExcludePolicy (patterns: string[]): PackageVersionPolicy {
  // Mirror the wrapping done by the full-resolution path
  // (installing/deps-resolver/src/resolveDependencyTree.ts) so the error code
  // is identical regardless of which path surfaced the invalid pattern.
  try {
    return createPackageVersionPolicy(patterns)
  } catch (err) {
    if (!err || typeof err !== 'object' || !('message' in err)) throw err
    throw new PnpmError(
      'INVALID_MINIMUM_RELEASE_AGE_EXCLUDE',
      `Invalid value in minimumReleaseAgeExclude: ${(err as { message: string }).message}`
    )
  }
}

function isExcluded (policy: PackageVersionPolicy | undefined, name: string, version: string): boolean {
  if (!policy) return false
  const result = policy(name)
  if (result === true) return true
  if (Array.isArray(result) && result.includes(version)) return true
  return false
}

function isNpmRegistryResolution (resolution: unknown): boolean {
  if (resolution == null || typeof resolution !== 'object') return false
  // Only plain tarball resolutions (npm registry / named registries) have no
  // `type` field. Git / directory / binary / custom resolutions all carry one.
  if ('type' in resolution && (resolution as { type?: unknown }).type != null) return false
  // Git-hosted tarballs (codeload/gitlab/bitbucket) are special-cased in the
  // resolver and aren't subject to release-age policy.
  if ('gitHosted' in resolution && (resolution as { gitHosted?: boolean }).gitHosted) return false
  return 'tarball' in resolution || 'integrity' in resolution
}
