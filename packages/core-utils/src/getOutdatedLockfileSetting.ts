import { type Lockfile, type PatchFile } from '@pnpm/lockfile.types'
import equals from 'ramda/src/equals'

export type ChangedField =
  | 'patchedDependencies'
  | 'overrides'
  | 'packageExtensionsChecksum'
  | 'ignoredOptionalDependencies'
  | 'settings.autoInstallPeers'
  | 'settings.excludeLinksFromLockfile'
  | 'settings.peersSuffixMaxLength'
  | 'pnpmfileChecksum'

export function getOutdatedLockfileSetting (
  lockfile: Lockfile,
  {
    overrides,
    packageExtensionsChecksum,
    ignoredOptionalDependencies,
    patchedDependencies,
    autoInstallPeers,
    excludeLinksFromLockfile,
    peersSuffixMaxLength,
    pnpmfileChecksum,
  }: {
    overrides?: Record<string, string>
    packageExtensionsChecksum?: string
    patchedDependencies?: Record<string, PatchFile>
    ignoredOptionalDependencies?: string[]
    autoInstallPeers?: boolean
    excludeLinksFromLockfile?: boolean
    peersSuffixMaxLength?: number
    pnpmfileChecksum?: string
  }
): ChangedField | null {
  if (!equals(lockfile.overrides ?? {}, overrides ?? {})) {
    return 'overrides'
  }
  if (lockfile.packageExtensionsChecksum !== packageExtensionsChecksum) {
    return 'packageExtensionsChecksum'
  }
  if (!equals(lockfile.ignoredOptionalDependencies?.sort() ?? [], ignoredOptionalDependencies?.sort() ?? [])) {
    return 'ignoredOptionalDependencies'
  }
  if (!equals(lockfile.patchedDependencies ?? {}, patchedDependencies ?? {})) {
    return 'patchedDependencies'
  }
  if ((lockfile.settings?.autoInstallPeers != null && lockfile.settings.autoInstallPeers !== autoInstallPeers)) {
    return 'settings.autoInstallPeers'
  }
  if (lockfile.settings?.excludeLinksFromLockfile != null && lockfile.settings.excludeLinksFromLockfile !== excludeLinksFromLockfile) {
    return 'settings.excludeLinksFromLockfile'
  }
  if (
    lockfile.settings?.peersSuffixMaxLength != null && lockfile.settings.peersSuffixMaxLength !== peersSuffixMaxLength ||
    lockfile.settings?.peersSuffixMaxLength == null && peersSuffixMaxLength !== 1000
  ) {
    return 'settings.peersSuffixMaxLength'
  }
  if (lockfile.pnpmfileChecksum !== pnpmfileChecksum) {
    return 'pnpmfileChecksum'
  }
  return null
}
