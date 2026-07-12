import type { VersioningSettings } from '@pnpm/types'

import { InvalidWorkspaceManifestError } from './errors/InvalidWorkspaceManifestError.js'

const BUMP_TYPES = ['patch', 'minor', 'major'] as const
const CHANGELOG_STORAGE_MODES = ['registry', 'repository'] as const

export function assertValidWorkspaceManifestVersioning (manifest: { packages?: readonly string[], versioning?: unknown }): asserts manifest is { versioning?: VersioningSettings } {
  if (manifest.versioning == null) {
    return
  }

  const versioning = assertPlainObject(manifest.versioning, 'versioning')

  if (versioning.fixed != null) {
    if (!Array.isArray(versioning.fixed)) {
      throw new InvalidWorkspaceManifestError(`Expected versioning.fixed to be an array of arrays, but found - ${typeof versioning.fixed}`)
    }
    for (const group of versioning.fixed) {
      if (!Array.isArray(group) || group.some((name) => typeof name !== 'string')) {
        throw new InvalidWorkspaceManifestError('Expected every versioning.fixed group to be an array of package names')
      }
    }
  }

  if (versioning.ignore != null) {
    if (!Array.isArray(versioning.ignore) || versioning.ignore.some((name) => typeof name !== 'string')) {
      throw new InvalidWorkspaceManifestError('Expected versioning.ignore to be an array of package names')
    }
  }

  if (versioning.maxBump != null) {
    if (!(BUMP_TYPES as readonly unknown[]).includes(versioning.maxBump)) {
      throw new InvalidWorkspaceManifestError(`Expected versioning.maxBump to be one of ${BUMP_TYPES.join(', ')}, but found - ${String(versioning.maxBump)}`)
    }
  }

  if (versioning.lanes != null) {
    const lanes = assertPlainObject(versioning.lanes, 'versioning.lanes')
    for (const [pkgName, lane] of Object.entries(lanes)) {
      if (typeof lane !== 'string' || lane === '') {
        throw new InvalidWorkspaceManifestError(`Expected versioning.lanes entry for ${pkgName} to be a non-empty lane name`)
      }
    }
  }

  if (versioning.changelog != null) {
    const changelog = assertPlainObject(versioning.changelog, 'versioning.changelog')
    if (changelog.format != null && typeof changelog.format !== 'string') {
      throw new InvalidWorkspaceManifestError(`Expected versioning.changelog.format to be a string, but found - ${typeof changelog.format}`)
    }
    if (changelog.storage != null && !(CHANGELOG_STORAGE_MODES as readonly unknown[]).includes(changelog.storage)) {
      throw new InvalidWorkspaceManifestError(`Expected versioning.changelog.storage to be one of ${CHANGELOG_STORAGE_MODES.join(', ')}, but found - ${String(changelog.storage)}`)
    }
  }
}

function assertPlainObject (value: unknown, fieldName: string): Record<string, unknown> {
  if (Array.isArray(value)) {
    throw new InvalidWorkspaceManifestError(`Expected ${fieldName} field to be an object, but found - array`)
  }
  if (typeof value !== 'object' || value === null) {
    throw new InvalidWorkspaceManifestError(`Expected ${fieldName} field to be an object, but found - ${value === null ? 'null' : typeof value}`)
  }
  return value as Record<string, unknown>
}
