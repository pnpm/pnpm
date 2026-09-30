import type { ProjectManifest } from '@pnpm/types'

// Settings that pnpm reads from `pnpm-workspace.yaml` and never from the `pnpm`
// field of `package.json` — either because they moved there in v11, or because
// (like `update`) they were introduced later and only ever lived there. When one
// of these appears in the `pnpm` field, pnpm warns that it is ignored. Keys not
// in this set (e.g. `app`, or anything set by third-party tooling that piggybacks
// on the `pnpm` namespace) are left alone to avoid false-positive warnings.
const MIGRATED_PNPM_FIELD_KEYS = new Set<string>([
  'allowBuilds',
  'allowedDeprecatedVersions',
  'allowUnusedPatches',
  'audit',
  'auditConfig',
  'configDependencies',
  'executionEnv',
  'ignoredOptionalDependencies',
  'neverBuiltDependencies',
  'onlyBuiltDependencies',
  'onlyBuiltDependenciesFile',
  'overrides',
  'packageExtensions',
  'patchedDependencies',
  'peerDependencyRules',
  'requiredScripts',
  'supportedArchitectures',
  'update',
  'updateConfig',
])

// The migrated keys whose values the lockfile records. An install that ignores
// one of them rewrites the lockfile without it.
const LOCKFILE_RECORDED_PNPM_FIELD_KEYS = new Set<string>([
  'ignoredOptionalDependencies',
  'overrides',
  'packageExtensions',
  'patchedDependencies',
])

/**
 * The migrated keys the manifest still declares under `pnpm` whose values the
 * lockfile records.
 */
export function getIgnoredLockfilePnpmFieldKeys (manifest: ProjectManifest): string[] {
  return getIgnoredPnpmFieldKeys(manifest).filter(key => LOCKFILE_RECORDED_PNPM_FIELD_KEYS.has(key))
}

export function getIgnoredPnpmFieldKeys (manifest: ProjectManifest): string[] {
  const legacyField = (manifest as { pnpm?: unknown }).pnpm
  if (legacyField == null || typeof legacyField !== 'object' || Array.isArray(legacyField)) {
    return []
  }
  return Object.keys(legacyField as Record<string, unknown>).filter(k => MIGRATED_PNPM_FIELD_KEYS.has(k))
}
