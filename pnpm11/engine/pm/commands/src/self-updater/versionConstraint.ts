import { inferRangeSpecStyle, versionWithRangeSpecStyle } from '@pnpm/pkg-manifest.utils'
import type { ProjectManifest } from '@pnpm/types'
import semver from 'semver'

/**
 * Rewrites `engines.pnpm` with the same rule as the devEngines pin
 * (see updateVersionConstraint), keeping a range such as `*` or `12.2`
 * that accepts the target as is.
 * Returns whether the manifest changed.
 */
export function pinPnpmInEngines (manifest: ProjectManifest, targetVersion: string): boolean {
  const engines = manifest.engines
  if (engines?.pnpm == null) return false
  const namesFullVersion = semver.valid(engines.pnpm.replace(/^[\^~=]+/, '')) != null
  if (!namesFullVersion && semver.satisfies(targetVersion, engines.pnpm, { includePrerelease: true })) return false
  const updated = updateVersionConstraint(engines.pnpm, targetVersion)
  if (updated === engines.pnpm) return false
  engines.pnpm = updated
  return true
}

/**
 * Returns the updated version constraint for devEngines.packageManager.
 * - Exact versions and simple ranges (^, ~) are rewritten to the new version,
 *   preserving the range operator — matching `pnpm update` and `pnpm runtime set`.
 * - Complex ranges (>=x <y, etc.) that still satisfy the new version are left
 *   unchanged (the exact version is pinned in the lockfile instead).
 * - Complex ranges that no longer satisfy the new version fall back to a caret
 *   range with the new version (`^${newVersion}`).
 */
export function updateVersionConstraint (current: string | undefined, newVersion: string): string | undefined {
  if (current == null) return newVersion
  const rangeSpecStyle = inferRangeSpecStyle(current)
  if (rangeSpecStyle != null) {
    return versionWithRangeSpecStyle(newVersion, rangeSpecStyle)
  }
  if (semver.satisfies(newVersion, current, { includePrerelease: true })) return current
  return `^${newVersion}`
}
