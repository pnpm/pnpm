import { redactAndSanitize } from '@pnpm/error'
import type { ProjectManifest } from '@pnpm/types'
import { map as mapValues } from 'ramda'

import { assertValidOverrides, createVersionReferencesReplacer } from './getOptionsFromRootManifest.js'

const MAX_DISPLAYED_RESOLUTIONS = 10

/**
 * Returns the overrides to use given the root manifest's deprecated
 * `resolutions` field. The resolutions become the overrides only when no
 * overrides are configured. Either way a warning is pushed to `warnings`.
 */
export function applyRootResolutions (
  manifest: ProjectManifest,
  overrides: Record<string, string> | undefined,
  warnings: string[]
): Record<string, string> | undefined {
  const resolutions: unknown = manifest.resolutions
  if (resolutions == null) return overrides
  assertValidOverrides(resolutions, 'resolutions')
  if (Object.keys(resolutions).length === 0) return overrides
  if (overrides != null && Object.keys(overrides).length > 0) {
    warnings.push('The "resolutions" field in package.json is ignored because "overrides" in pnpm-workspace.yaml takes precedence. Remove "resolutions" from package.json.')
    return overrides
  }
  const replaceVersionReference = createVersionReferencesReplacer(manifest)
  // `${NAME}` is not a `$name` reference. Resolutions keep it literal.
  const resolved = mapValues((spec) => spec.startsWith('${') ? spec : replaceVersionReference(spec), resolutions)
  const entries = Object.entries(resolved).map(([selector, spec]) => {
    const original = resolutions[selector]
    return original === spec
      ? `  ${redactAndSanitize(selector)}: ${redactAndSanitize(spec)}`
      : `  ${redactAndSanitize(selector)}: ${redactAndSanitize(original)} -> ${redactAndSanitize(spec)}`
  })
  const displayed = entries.slice(0, MAX_DISPLAYED_RESOLUTIONS)
  if (entries.length > MAX_DISPLAYED_RESOLUTIONS) {
    displayed.push(`  ...and ${entries.length - MAX_DISPLAYED_RESOLUTIONS} more`)
  }
  warnings.push(`The "resolutions" field in package.json is deprecated. We attempted to migrate your resolutions to pnpm overrides. Please verify:\n${displayed.join('\n')}\nUse the "overrides" field in pnpm-workspace.yaml instead.`)
  return resolved
}
