import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { PnpmError } from '@pnpm/error'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath, PackageVersionPolicy } from '@pnpm/types'
import semver from 'semver'

export type PublishedAtLookup = (name: string, version: string) => Promise<Date | undefined>

export interface RevalidateLockfileMinimumReleaseAgeOptions {
  minimumReleaseAge: number
  minimumReleaseAgeExclude?: string[]
  now?: number
}

interface Violation {
  pkgId: string
  publishedAt: Date
}

/**
 * Re-applies the `minimumReleaseAge` policy to every npm-registry-resolved entry
 * in an existing lockfile. Used when the install path skips resolution (because
 * the lockfile is up-to-date) — without this pass, a freshly-published version
 * that was added to the lockfile while the policy was bypassed locally would be
 * installed by other consumers and CI without being checked.
 */
export async function revalidateLockfileAgainstMinimumReleaseAge (
  lockfile: LockfileObject,
  lookupPublishedAt: PublishedAtLookup,
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
  const candidates = new Map<string, { name: string, version: string }>()
  for (const [depPath, snapshot] of Object.entries(lockfile.packages)) {
    if (!isNpmRegistryResolution(snapshot.resolution)) continue
    const { name, version, nonSemverVersion } = nameVerFromPkgSnapshot(depPath as DepPath, snapshot)
    // Non-semver versions identify URL tarballs, file: references, git refs,
    // etc. — these aren't resolved through the npm registry, so the publish-date
    // policy doesn't apply and a registry lookup would 404.
    if (!name || !version || nonSemverVersion || !semver.valid(version)) continue
    if (isExcluded(excludePolicy, name, version)) continue
    candidates.set(`${name}@${version}`, { name, version })
  }

  const lookups = await Promise.all(
    Array.from(candidates.values(), async ({ name, version }) => ({
      name,
      version,
      publishedAt: await lookupPublishedAt(name, version),
    }))
  )

  const violations: Violation[] = []
  for (const { name, version, publishedAt } of lookups) {
    if (!publishedAt) continue
    const ts = publishedAt.getTime()
    if (Number.isNaN(ts)) continue
    if (ts > cutoff) {
      violations.push({ pkgId: `${name}@${version}`, publishedAt })
    }
  }

  if (violations.length === 0) return

  const details = violations
    .map((v) => `  ${v.pkgId} (published ${v.publishedAt.toISOString()})`)
    .join('\n')
  throw new PnpmError(
    'MINIMUM_RELEASE_AGE_LOCKFILE_VIOLATION',
    `The lockfile contains versions that do not meet the minimumReleaseAge constraint:\n${details}`,
    {
      hint: 'These versions were published more recently than the minimumReleaseAge cutoff. ' +
        'Either re-run resolution with "pnpm install --no-frozen-lockfile" to pick mature versions, ' +
        'or add them to minimumReleaseAgeExclude.',
    }
  )
}

function createExcludePolicy (patterns: string[]): PackageVersionPolicy {
  // Match the wrapping done by the full-resolution path
  // (installing/deps-resolver/src/resolveDependencyTree.ts) so the error code is
  // identical regardless of which path surfaced the invalid pattern.
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
