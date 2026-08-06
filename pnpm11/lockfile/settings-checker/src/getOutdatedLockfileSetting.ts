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
  {
    catalogs,
    overrides,
    packageExtensionsChecksum,
    ignoredOptionalDependencies,
    patchedDependencies,
    autoInstallPeers,
    dedupePeers,
    excludeLinksFromLockfile,
    peersSuffixMaxLength,
    pnpmfileChecksum,
    injectWorkspacePackages,
  }: LockfileSettingsInput
): Generator<ChangedField> {
  if (!allCatalogsAreUpToDate(catalogs ?? {}, lockfile.catalogs)) {
    yield 'catalogs'
  }
  if (!equals(lockfile.overrides ?? {}, overrides ?? {})) {
    yield 'overrides'
  }
  if (lockfile.packageExtensionsChecksum !== packageExtensionsChecksum) {
    yield 'packageExtensionsChecksum'
  }
  // Compare copies: the recorded and configured arrays belong to the caller,
  // and `ignoredOptionalDependencies` is order-sensitive downstream — sorting
  // it in place can move an `!` exclusion ahead of the pattern it excludes
  // from and flip which dependencies `createMatcher` ignores.
  if (!equals([...lockfile.ignoredOptionalDependencies ?? []].sort(), [...ignoredOptionalDependencies ?? []].sort())) {
    yield 'ignoredOptionalDependencies'
  }
  if (!equals(lockfile.patchedDependencies ?? {}, patchedDependencies ?? {})) {
    yield 'patchedDependencies'
  }
  if ((lockfile.settings?.autoInstallPeers != null && lockfile.settings.autoInstallPeers !== autoInstallPeers)) {
    yield 'settings.autoInstallPeers'
  }
  if (Boolean(lockfile.settings?.dedupePeers) !== Boolean(dedupePeers)) {
    yield 'settings.dedupePeers'
  }
  if (lockfile.settings?.excludeLinksFromLockfile != null && lockfile.settings.excludeLinksFromLockfile !== excludeLinksFromLockfile) {
    yield 'settings.excludeLinksFromLockfile'
  }
  if (
    lockfile.settings?.peersSuffixMaxLength != null && lockfile.settings.peersSuffixMaxLength !== peersSuffixMaxLength ||
    lockfile.settings?.peersSuffixMaxLength == null && peersSuffixMaxLength !== DEFAULT_PEERS_SUFFIX_MAX_LENGTH
  ) {
    yield 'settings.peersSuffixMaxLength'
  }
  if (lockfile.pnpmfileChecksum !== pnpmfileChecksum) {
    yield 'pnpmfileChecksum'
  }
  if (Boolean(lockfile.settings?.injectWorkspacePackages) !== Boolean(injectWorkspacePackages)) {
    yield 'settings.injectWorkspacePackages'
  }
}
