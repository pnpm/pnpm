import type { WantedPackageManager } from '@pnpm/config.reader'
import semver from 'semver'

/**
 * Decides whether the resolved pnpm integrity info should be written to
 * `pnpm-lock.yaml` under the project's `packageManagerDependencies` section.
 *
 * - `devEngines.packageManager` always persists (supports ranges / dist-tags
 *   that need pinning to be reproducible).
 * - The legacy `packageManager` field only persists when the pinned version
 *   is pnpm v12 or newer. Older pins already contain an exact version in the
 *   manifest itself, so the lockfile entry would only add churn — and the
 *   quiet behavior keeps the v10 → v11 transition painless.
 */
export function shouldPersistLockfile (pm: Pick<WantedPackageManager, 'version' | 'fromDevEngines'>): boolean {
  if (pm.fromDevEngines === true) return true
  if (pm.version == null || semver.valid(pm.version) == null) return false
  return semver.major(pm.version) >= 12
}
