import type { VersionOverride } from '@pnpm/config.parse-overrides'
import { PnpmError } from '@pnpm/error'
import semver from 'semver'

interface ResolutionError {
  code?: string
  packageMeta?: {
    versions?: Record<string, unknown>
  }
}

/**
 * Reframes an `ERR_PNPM_NO_MATCHING_VERSION` raised for a specifier that a
 * user override forced onto the dependency. The dependency stack of the
 * original error points at whichever package happened to depend on the
 * target, while the entry to fix is the override itself. Returns `null` when
 * the failure was not caused by an override.
 */
export function toOverrideNoMatchingVersionError (
  err: ResolutionError,
  wantedDependency: { alias?: string, bareSpecifier: string },
  parsedOverrides?: readonly VersionOverride[]
): PnpmError | null {
  if (err.code !== 'ERR_PNPM_NO_MATCHING_VERSION' || !parsedOverrides?.length || !wantedDependency.alias) return null
  const override = parsedOverrides.find(({ targetPkg, newBareSpecifier }) =>
    targetPkg.name === wantedDependency.alias && newBareSpecifier === wantedDependency.bareSpecifier)
  if (override == null) return null
  const selectorRange = override.targetPkg.bareSpecifier
  const versions = Object.keys(err.packageMeta?.versions ?? {})
  const best = selectorRange && versions.length > 0 ? semver.maxSatisfying(versions, selectorRange) : null
  return new PnpmError(
    'NO_MATCHING_VERSION',
    `Override "${override.selector}": "${override.newBareSpecifier}" targets a version of ${override.targetPkg.name} that does not exist on the registry.`,
    best != null
      ? { hint: `The latest release of ${override.targetPkg.name} matching "${selectorRange}" is "${best}".` }
      : undefined
  )
}
