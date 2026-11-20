import type { Catalogs } from '@pnpm/catalogs.types'
import type { LockfileObject } from '@pnpm/lockfile.types'
import { allCatalogsAreUpToDate } from '@pnpm/lockfile.verification'
import { equals } from 'ramda'

export type ChangedField =
  | 'catalogs'
  | 'patchedDependencies'
  | 'overrides'
  | 'packageExtensionsChecksum'
  | 'ignoredOptionalDependencies'
  | ChangedSettingsField
  | 'pnpmfileChecksum'

export type ChangedSettingsField =
  | 'settings.autoInstallPeers'
  | 'settings.dedupePeers'
  | 'settings.excludeLinksFromLockfile'
  | 'settings.peersSuffixMaxLength'
  | 'settings.injectWorkspacePackages'

export const DEFAULT_PEERS_SUFFIX_MAX_LENGTH = 1000

export interface LockfileSettingsInput {
  catalogs?: Catalogs
  overrides?: Record<string, string>
  packageExtensionsChecksum?: string
  patchedDependencies?: Record<string, string>
  ignoredOptionalDependencies?: string[]
  autoInstallPeers?: boolean
  dedupePeers?: boolean
  excludeLinksFromLockfile?: boolean
  peersSuffixMaxLength?: number
  pnpmfileChecksum?: string
  /**
   * Skips comparing `pnpmfileChecksum`, for an install that loads no
   * pnpmfile because of `ignorePnpmfile`. The flag skips the pnpmfile for
   * that run only, so the checksum the lockfile records still stands.
   */
  ignorePnpmfileChecksum?: boolean
  injectWorkspacePackages?: boolean
}

export function getOutdatedLockfileSetting (
  lockfile: LockfileObject,
  settings: LockfileSettingsInput
): ChangedField | null {
  for (const changedField of outdatedLockfileSettings(lockfile, settings)) {
    return changedField
  }
  return null
}

/**
 * Every setting recorded in the lockfile that the current configuration
 * no longer matches. Use it when a caller has to know that a given setting
 * is the *only* thing that changed; {@link getOutdatedLockfileSetting} is
 * the cheaper choice when just the first mismatch is needed.
 */
export function getOutdatedLockfileSettings (
  lockfile: LockfileObject,
  settings: LockfileSettingsInput
): ChangedField[] {
  return Array.from(outdatedLockfileSettings(lockfile, settings))
}

function * outdatedLockfileSettings (
  lockfile: LockfileObject,
  input: LockfileSettingsInput
): Generator<ChangedField> {
  yield * outdatedTopLevelFields(lockfile, input)
  yield * outdatedSettingsFields(lockfile, input)
}

function * outdatedTopLevelFields (
  lockfile: LockfileObject,
  input: LockfileSettingsInput
): Generator<ChangedField> {
  if (!allCatalogsAreUpToDate(input.catalogs ?? {}, lockfile.catalogs)) {
    yield 'catalogs'
  }
  if (!equals(lockfile.overrides ?? {}, input.overrides ?? {})) {
    yield 'overrides'
  }
  if (lockfile.packageExtensionsChecksum !== input.packageExtensionsChecksum) {
    yield 'packageExtensionsChecksum'
  }
  // Compare copies: the recorded and configured arrays belong to the caller,
  // and `ignoredOptionalDependencies` is order-sensitive downstream — sorting
  // it in place can move an `!` exclusion ahead of the pattern it excludes
  // from and flip which dependencies `createMatcher` ignores.
  if (!equals([...lockfile.ignoredOptionalDependencies ?? []].sort(), [...input.ignoredOptionalDependencies ?? []].sort())) {
    yield 'ignoredOptionalDependencies'
  }
  if (!equals(lockfile.patchedDependencies ?? {}, input.patchedDependencies ?? {})) {
    yield 'patchedDependencies'
  }
  if (!input.ignorePnpmfileChecksum && lockfile.pnpmfileChecksum !== input.pnpmfileChecksum) {
    yield 'pnpmfileChecksum'
  }
}

function * outdatedSettingsFields (
  lockfile: LockfileObject,
  input: LockfileSettingsInput
): Generator<ChangedSettingsField> {
  const settings = lockfile.settings
  if (settings?.autoInstallPeers != null && settings.autoInstallPeers !== input.autoInstallPeers) {
    yield 'settings.autoInstallPeers'
  }
  if (Boolean(settings?.dedupePeers) !== Boolean(input.dedupePeers)) {
    yield 'settings.dedupePeers'
  }
  if (settings?.excludeLinksFromLockfile != null && settings.excludeLinksFromLockfile !== input.excludeLinksFromLockfile) {
    yield 'settings.excludeLinksFromLockfile'
  }
  if (isPeersSuffixMaxLengthOutdated(settings?.peersSuffixMaxLength, input.peersSuffixMaxLength)) {
    yield 'settings.peersSuffixMaxLength'
  }
  if (Boolean(settings?.injectWorkspacePackages) !== Boolean(input.injectWorkspacePackages)) {
    yield 'settings.injectWorkspacePackages'
  }
}

function isPeersSuffixMaxLengthOutdated (lockfileValue: number | undefined, inputValue: number | undefined): boolean {
  if (lockfileValue != null) return lockfileValue !== inputValue
  return inputValue !== DEFAULT_PEERS_SUFFIX_MAX_LENGTH
}
