import type {
  CatalogSnapshots,
  LockfileObject,
  LockfileSettings,
  PackageSnapshot,
  PackageSnapshots,
  ResolvedCatalogEntry,
} from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'
import { comverToSemver } from 'comver-to-semver'
import semver from 'semver'

export function mergeLockfileChanges (ours: LockfileObject, theirs: LockfileObject): LockfileObject {
  const newLockfile: LockfileObject = {
    importers: {},
    lockfileVersion: semver.gt(comverToSemver(theirs.lockfileVersion.toString()), comverToSemver(ours.lockfileVersion.toString()))
      ? theirs.lockfileVersion
      : ours.lockfileVersion,
  }
  const settings = mergeSettings(ours.settings, theirs.settings)
  if (settings) {
    newLockfile.settings = settings
  }
  const catalogs = mergeCatalogs(ours.catalogs, theirs.catalogs)
  if (catalogs) {
    newLockfile.catalogs = catalogs
  }
  if (ours.overrides || theirs.overrides) {
    const overrides = {
      ...ours.overrides,
      ...theirs.overrides,
    }
    if (Object.keys(overrides).length > 0) {
      newLockfile.overrides = overrides
    }
  }
  const packageExtensionsChecksum = ours.packageExtensionsChecksum ?? theirs.packageExtensionsChecksum
  if (packageExtensionsChecksum) {
    newLockfile.packageExtensionsChecksum = packageExtensionsChecksum
  }
  const pnpmfileChecksum = ours.pnpmfileChecksum ?? theirs.pnpmfileChecksum // Install should automatically detect change later
  if (pnpmfileChecksum) {
    newLockfile.pnpmfileChecksum = pnpmfileChecksum
  }
  const untrackedPnpmfileReadPackageHook =
    ours.untrackedPnpmfileReadPackageHook === theirs.untrackedPnpmfileReadPackageHook
      ? ours.untrackedPnpmfileReadPackageHook
      : true
  if (untrackedPnpmfileReadPackageHook != null) {
    newLockfile.untrackedPnpmfileReadPackageHook = untrackedPnpmfileReadPackageHook
  }

  const ignoredOptionalDependencies = [...new Set([
    ...ours.ignoredOptionalDependencies ?? [],
    ...theirs.ignoredOptionalDependencies ?? [],
  ])]
  if (ignoredOptionalDependencies.length) {
    newLockfile.ignoredOptionalDependencies = ignoredOptionalDependencies
  }

  if (ours.patchedDependencies || theirs.patchedDependencies) {
    const patchedDependencies = {
      ...ours.patchedDependencies,
      ...theirs.patchedDependencies,
    }
    if (Object.keys(patchedDependencies).length > 0) {
      newLockfile.patchedDependencies = patchedDependencies
    }
  }

  const neverBuiltDependencies = [...new Set([
    ...ours.neverBuiltDependencies ?? [],
    ...theirs.neverBuiltDependencies ?? [],
  ])]
  if (neverBuiltDependencies.length) {
    newLockfile.neverBuiltDependencies = neverBuiltDependencies
  }

  const onlyBuiltDependencies = [...new Set([
    ...ours.onlyBuiltDependencies ?? [],
    ...theirs.onlyBuiltDependencies ?? [],
  ])]
  if (onlyBuiltDependencies.length) {
    newLockfile.onlyBuiltDependencies = onlyBuiltDependencies
  }

  if (ours.time || theirs.time) {
    const time = {
      ...ours.time,
      ...theirs.time,
    }
    if (Object.keys(time).length > 0) {
      newLockfile.time = time
    }
  }

  for (const importerId of Array.from(new Set([...Object.keys(ours.importers), ...Object.keys(theirs.importers)] as ProjectId[]))) {
    newLockfile.importers[importerId] = {
      specifiers: {},
    }
    for (const key of ['dependencies', 'devDependencies', 'optionalDependencies'] as const) {
      newLockfile.importers[importerId][key] = mergeDict(
        ours.importers[importerId]?.[key] ?? {},
        theirs.importers[importerId]?.[key] ?? {},
        mergeVersions
      )
      if (Object.keys(newLockfile.importers[importerId][key] ?? {}).length === 0) {
        delete newLockfile.importers[importerId][key]
      }
    }
    newLockfile.importers[importerId].specifiers = mergeDict(
      ours.importers[importerId]?.specifiers ?? {},
      theirs.importers[importerId]?.specifiers ?? {},
      takeChangedValue
    )
  }

  const packages: PackageSnapshots = {}
  for (const depPath of (Array.from(new Set([...Object.keys(ours.packages ?? {}), ...Object.keys(theirs.packages ?? {})]))) as DepPath[]) {
    const ourPkg = ours.packages?.[depPath]
    const theirPkg = theirs.packages?.[depPath]
    const pkg = {
      ...ourPkg,
      ...theirPkg,
    }
    for (const key of ['dependencies', 'optionalDependencies'] as const) {
      pkg[key] = mergeDict(
        ourPkg?.[key] ?? {},
        theirPkg?.[key] ?? {},
        mergeVersions
      )
      if (Object.keys(pkg[key] ?? {}).length === 0) {
        delete pkg[key]
      }
    }
    packages[depPath] = pkg as PackageSnapshot
  }
  newLockfile.packages = packages

  const knownKeys = new Set([
    'catalogs',
    'ignoredOptionalDependencies',
    'importers',
    'lockfileVersion',
    'neverBuiltDependencies',
    'onlyBuiltDependencies',
    'overrides',
    'packageExtensionsChecksum',
    'packages',
    'patchedDependencies',
    'pnpmfileChecksum',
    'settings',
    'snapshots',
    'time',
    'untrackedPnpmfileReadPackageHook',
  ])
  for (const [key, value] of Object.entries(theirs)) {
    if (!knownKeys.has(key) && !(key in newLockfile)) {
      (newLockfile as unknown as Record<string, unknown>)[key] = value
    }
  }
  for (const [key, value] of Object.entries(ours)) {
    if (!knownKeys.has(key)) {
      (newLockfile as unknown as Record<string, unknown>)[key] = value
    }
  }

  return newLockfile
}

function mergeSettings (
  ours?: LockfileSettings,
  theirs?: LockfileSettings
): LockfileSettings | undefined {
  if (!ours && !theirs) return undefined
  const settings: LockfileSettings = {}
  if (ours?.autoInstallPeers != null || theirs?.autoInstallPeers != null) {
    settings.autoInstallPeers = (ours?.autoInstallPeers ?? false) || (theirs?.autoInstallPeers ?? false)
  }
  if (ours?.excludeLinksFromLockfile != null || theirs?.excludeLinksFromLockfile != null) {
    settings.excludeLinksFromLockfile = (ours?.excludeLinksFromLockfile ?? false) || (theirs?.excludeLinksFromLockfile ?? false)
  }
  if (ours?.dedupePeers != null || theirs?.dedupePeers != null) {
    settings.dedupePeers = ours?.dedupePeers ?? theirs?.dedupePeers
  }
  if (ours?.injectWorkspacePackages != null || theirs?.injectWorkspacePackages != null) {
    settings.injectWorkspacePackages = (ours?.injectWorkspacePackages ?? false) || (theirs?.injectWorkspacePackages ?? false)
  }
  if (ours?.peersSuffixMaxLength != null || theirs?.peersSuffixMaxLength != null) {
    settings.peersSuffixMaxLength = ours?.peersSuffixMaxLength ?? theirs?.peersSuffixMaxLength
  }
  return Object.keys(settings).length > 0 ? settings : undefined
}

function mergeCatalogs (
  ours?: CatalogSnapshots,
  theirs?: CatalogSnapshots
): CatalogSnapshots | undefined {
  if (!ours && !theirs) return undefined
  const merged: CatalogSnapshots = {}
  const allCatalogNames = new Set([
    ...Object.keys(ours ?? {}),
    ...Object.keys(theirs ?? {}),
  ])
  for (const catalogName of allCatalogNames) {
    const ourCatalog = ours?.[catalogName] ?? {}
    const theirCatalog = theirs?.[catalogName] ?? {}
    const mergedCatalog: Record<string, ResolvedCatalogEntry> = {}
    const allDepNames = new Set([
      ...Object.keys(ourCatalog),
      ...Object.keys(theirCatalog),
    ])
    for (const depName of allDepNames) {
      const ourEntry = ourCatalog[depName]
      const theirEntry = theirCatalog[depName]
      if (ourEntry && theirEntry) {
        mergedCatalog[depName] = {
          specifier: takeChangedValue(ourEntry.specifier, theirEntry.specifier),
          version: mergeVersions(ourEntry.version, theirEntry.version),
        }
      } else {
        mergedCatalog[depName] = (ourEntry ?? theirEntry)!
      }
    }
    merged[catalogName] = mergedCatalog
  }
  return Object.keys(merged).length > 0 ? merged : undefined
}

type ValueMerger<T> = (ourValue: T, theirValue: T) => T

function mergeDict<T> (
  ourDict: Record<string, T>,
  theirDict: Record<string, T>,
  valueMerger: ValueMerger<T>
): Record<string, T> {
  const newDict: Record<string, T> = {}
  for (const key of Object.keys(ourDict).concat(Object.keys(theirDict))) {
    const changedValue = valueMerger(
      ourDict[key],
      theirDict[key]
    )
    if (changedValue) {
      newDict[key] = changedValue
    }
  }
  return newDict
}

function takeChangedValue<T> (ourValue: T, theirValue: T): T {
  if (ourValue === theirValue || theirValue == null) return ourValue
  return theirValue
}

function mergeVersions (ourValue: string, theirValue: string): string {
  if (ourValue === theirValue || !theirValue) return ourValue
  if (!ourValue) return theirValue
  const [ourVersion] = ourValue.split('(')
  const [theirVersion] = theirValue.split('(')
  const validOurVersion = semver.valid(ourVersion)
  const validTheirVersion = semver.valid(theirVersion)

  if (validOurVersion && validTheirVersion) {
    return semver.gt(ourVersion, theirVersion) ? ourValue : theirValue
  }

  // Non-semver versions (link:, file:, git URLs, etc.) — prefer theirs
  return theirValue
}
