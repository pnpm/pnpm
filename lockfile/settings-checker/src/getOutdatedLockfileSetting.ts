import { type Catalogs } from '@pnpm/catalogs.types'
import { type LockfileObject, type PatchFile } from '@pnpm/lockfile.types'
import { allCatalogsAreUpToDate } from '@pnpm/lockfile.verification'
import { equals } from 'ramda'

export type ChangedField =
  | 'catalogs'
  | 'patchedDependencies'
  | 'overrides'
  | 'packageExtensionsChecksum'
  | 'ignoredOptionalDependencies'
  | 'settings.autoInstallPeers'
  | 'settings.excludeLinksFromLockfile'
  | 'settings.peersSuffixMaxLength'
  | 'settings.injectWorkspacePackages'
  | 'pnpmfileChecksum'

export function getOutdatedLockfileSetting (
  lockfile: LockfileObject,
  {
    catalogs,
    overrides,
    packageExtensionsChecksum,
    ignoredOptionalDependencies,
    patchedDependencies,
    autoInstallPeers,
    excludeLinksFromLockfile,
    peersSuffixMaxLength,
    pnpmfileChecksum,
    injectWorkspacePackages,
  }: {
    catalogs?: Catalogs
    overrides?: Record<string, string>
    packageExtensionsChecksum?: string
    patchedDependencies?: Record<string, PatchFile>
    ignoredOptionalDependencies?: string[]
    autoInstallPeers?: boolean
    excludeLinksFromLockfile?: boolean
    peersSuffixMaxLength?: number
    pnpmfileChecksum?: string
    injectWorkspacePackages?: boolean
  }
): ChangedField | null {
  const catalogsConfig = catalogs ?? {}
  if (Object.keys(catalogsConfig).length > 0 && !allCatalogsAreUpToDate(catalogsConfig, lockfile.catalogs)) {
    return 'catalogs'
  }
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
  if (Boolean(lockfile.settings?.injectWorkspacePackages) !== Boolean(injectWorkspacePackages)) {
    return 'settings.injectWorkspacePackages'
  }
  return null
}
