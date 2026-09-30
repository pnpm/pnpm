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
    validateFixedGroups(versioning.fixed)
  }

  if (versioning.epics != null) {
    validateEpics(versioning.epics)
  }

  if (versioning.ignore != null) {
    validateIgnore(versioning.ignore)
  }

  if (versioning.maxBump != null) {
    validateMaxBump(versioning.maxBump)
  }

  if (versioning.lanes != null) {
    validateLanes(versioning.lanes)
  }

  if (versioning.changelog != null) {
    validateChangelog(versioning.changelog)
  }
}

function validateFixedGroups (fixed: unknown): void {
  if (!Array.isArray(fixed)) {
    throw new InvalidWorkspaceManifestError(`Expected versioning.fixed to be an array of arrays, but found - ${typeof fixed}`)
  }
  for (const group of fixed) {
    if (!Array.isArray(group) || group.some((name) => typeof name !== 'string' || name === '')) {
      throw new InvalidWorkspaceManifestError('Expected every versioning.fixed group to be an array of package names')
    }
  }
}

function validateEpics (epics: unknown): void {
  if (!Array.isArray(epics)) {
    throw new InvalidWorkspaceManifestError(`Expected versioning.epics to be an array, but found - ${typeof epics}`)
  }
  for (const epic of epics) {
    validateEpicEntry(epic)
  }
}

function validateEpicEntry (epic: unknown): void {
  const entry = assertPlainObject(epic, 'versioning.epics entry')
  if (typeof entry.lead !== 'string' || entry.lead === '') {
    throw new InvalidWorkspaceManifestError('Expected every versioning.epics entry to have a non-empty "lead" package reference')
  }
  if (!Array.isArray(entry.packages) || entry.packages.length === 0 || entry.packages.some((selector) => typeof selector !== 'string' || selector === '')) {
    throw new InvalidWorkspaceManifestError(`Expected versioning.epics entry for "${entry.lead}" to have a non-empty "packages" array of selector strings`)
  }
}

function validateIgnore (ignore: unknown): void {
  if (!Array.isArray(ignore) || ignore.some((name) => typeof name !== 'string' || name === '')) {
    throw new InvalidWorkspaceManifestError('Expected versioning.ignore to be an array of package names')
  }
}

function validateMaxBump (maxBump: unknown): void {
  if (!(BUMP_TYPES as readonly unknown[]).includes(maxBump)) {
    throw new InvalidWorkspaceManifestError(`Expected versioning.maxBump to be one of ${BUMP_TYPES.join(', ')}, but found - ${String(maxBump)}`)
  }
}

function validateLanes (lanesValue: unknown): void {
  const lanes = assertPlainObject(lanesValue, 'versioning.lanes')
  for (const [pkgName, lane] of Object.entries(lanes)) {
    if (typeof lane !== 'string' || lane === '') {
      throw new InvalidWorkspaceManifestError(`Expected versioning.lanes entry for ${pkgName} to be a non-empty lane name`)
    }
    if (lane.toLowerCase() === 'main') {
      throw new InvalidWorkspaceManifestError(`Invalid versioning.lanes entry for ${pkgName}: "main" is the reserved default lane. Remove the entry instead.`)
    }
  }
}

function validateChangelog (changelogValue: unknown): void {
  const changelog = assertPlainObject(changelogValue, 'versioning.changelog')
  if (changelog.format != null && typeof changelog.format !== 'string') {
    throw new InvalidWorkspaceManifestError(`Expected versioning.changelog.format to be a string, but found - ${typeof changelog.format}`)
  }
  if (changelog.storage != null && !(CHANGELOG_STORAGE_MODES as readonly unknown[]).includes(changelog.storage)) {
    throw new InvalidWorkspaceManifestError(`Expected versioning.changelog.storage to be one of ${CHANGELOG_STORAGE_MODES.join(', ')}, but found - ${String(changelog.storage)}`)
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
