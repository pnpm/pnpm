import type { VersionOverride } from '@pnpm/config.parse-overrides'
import { PnpmError, redactAndSanitize } from '@pnpm/error'
import semver from 'semver'

interface ResolutionError {
  code?: string
  hint?: string
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
  const name = redactAndSanitize(override.targetPkg.name)
  const hints = [describeLatestMatchingVersion(name, override, err.packageMeta?.versions), err.hint].filter(Boolean)
  return new PnpmError(
    'NO_MATCHING_VERSION',
    `Override "${redactAndSanitize(override.selector)}": "${redactAndSanitize(override.newBareSpecifier)}" targets a version of ${name} that does not exist on the registry.`,
    { hint: hints.length > 0 ? hints.join('\n\n') : undefined }
  )
}

/**
 * The registry metadata belongs to the package the forced specifier names,
 * so it describes the target only when that specifier is a plain range of the
 * target, not an alias such as `npm:other@1`.
 */
function describeLatestMatchingVersion (
  name: string,
  override: VersionOverride,
  versions: Record<string, unknown> | undefined
): string | undefined {
  const selectorRange = override.targetPkg.bareSpecifier
  if (!selectorRange || versions == null || semver.validRange(override.newBareSpecifier) == null) return undefined
  const latest = semver.maxSatisfying(Object.keys(versions), selectorRange)
  return latest == null ? undefined : `The latest release of ${name} matching "${redactAndSanitize(selectorRange)}" is "${latest}".`
}
