import type {
  CatalogSnapshots,
  LockfileObject,
  LockfileSettings,
  PackageSnapshot,
  PackageSnapshots,
  ProjectSnapshot,
  ResolvedCatalogEntry,
} from '@pnpm/lockfile.types'
import type { DepPath, ProjectId } from '@pnpm/types'
import { comverToSemver } from 'comver-to-semver'
import semver from 'semver'

export function mergeLockfileChanges (ours: LockfileObject, theirs: LockfileObject): LockfileObject {
  const newLockfile: LockfileObject = {
    importers: mergeImporters(ours.importers, theirs.importers),
    lockfileVersion: pickLockfileVersion(ours.lockfileVersion, theirs.lockfileVersion),
  }
  applyConfigFields(newLockfile, ours, theirs)
  applyDependencyCollections(newLockfile, ours, theirs)

  if (ours.packages || theirs.packages) {
    newLockfile.packages = mergePackageSnapshots(ours.packages, theirs.packages)
  }

  mergeForeignKeys(newLockfile, ours, theirs)
  return newLockfile
}

function pickLockfileVersion (
  ourVersion: LockfileObject['lockfileVersion'],
  theirVersion: LockfileObject['lockfileVersion']
): LockfileObject['lockfileVersion'] {
  return semver.gt(comverToSemver(theirVersion.toString()), comverToSemver(ourVersion.toString()))
    ? theirVersion
    : ourVersion
}

function applyConfigFields (target: LockfileObject, ours: LockfileObject, theirs: LockfileObject): void {
  const settings = mergeSettings(ours.settings, theirs.settings)
  if (settings) target.settings = settings

  const catalogs = mergeCatalogs(ours.catalogs, theirs.catalogs)
  if (catalogs) target.catalogs = catalogs

  const overrides = mergeShallowDict(ours.overrides, theirs.overrides)
  if (overrides) target.overrides = overrides

  const patchedDependencies = mergeShallowDict(ours.patchedDependencies, theirs.patchedDependencies)
  if (patchedDependencies) target.patchedDependencies = patchedDependencies

  const checksum = ours.packageExtensionsChecksum ?? theirs.packageExtensionsChecksum
  if (checksum) target.packageExtensionsChecksum = checksum

  const pnpmfileChecksum = ours.pnpmfileChecksum ?? theirs.pnpmfileChecksum
  if (pnpmfileChecksum) target.pnpmfileChecksum = pnpmfileChecksum

  const hookState = resolveHookState(ours.untrackedPnpmfileReadPackageHook, theirs.untrackedPnpmfileReadPackageHook)
  if (hookState != null) target.untrackedPnpmfileReadPackageHook = hookState
}

function applyDependencyCollections (target: LockfileObject, ours: LockfileObject, theirs: LockfileObject): void {
  const ignored = mergeUniqueArrays(ours.ignoredOptionalDependencies, theirs.ignoredOptionalDependencies)
  if (ignored.length) target.ignoredOptionalDependencies = ignored

  const neverBuilt = mergeUniqueArrays(ours.neverBuiltDependencies, theirs.neverBuiltDependencies)
  if (neverBuilt.length) target.neverBuiltDependencies = neverBuilt

  const onlyBuilt = mergeUniqueArrays(ours.onlyBuiltDependencies, theirs.onlyBuiltDependencies)
  if (onlyBuilt.length) target.onlyBuiltDependencies = onlyBuilt

  const time = mergeShallowDict(ours.time, theirs.time)
  if (time) target.time = time
}

function resolveHookState (ourHook?: boolean, theirHook?: boolean): boolean | undefined {
  if (ourHook === theirHook) return ourHook
  return true
}

function mergeUniqueArrays<Item> (first?: Item[], second?: Item[]): Item[] {
  return [...new Set([...first ?? [], ...second ?? []])]
}

function mergeShallowDict<Value> (
  first?: Record<string, Value>,
  second?: Record<string, Value>
): Record<string, Value> | undefined {
  if (!first && !second) return undefined
  const merged = { ...first, ...second }
  return Object.keys(merged).length > 0 ? merged : undefined
}

function mergeImporters (
  ourImporters: Record<ProjectId, ProjectSnapshot>,
  theirImporters: Record<ProjectId, ProjectSnapshot>
): Record<ProjectId, ProjectSnapshot> {
  const merged: Record<ProjectId, ProjectSnapshot> = {}
  const allIds = Array.from(new Set([
    ...Object.keys(ourImporters),
    ...Object.keys(theirImporters),
  ])) as ProjectId[]

  for (const id of allIds) {
    merged[id] = mergeSingleImporter(
      Object.hasOwn(ourImporters, id) ? ourImporters[id] : undefined,
      Object.hasOwn(theirImporters, id) ? theirImporters[id] : undefined
    )
  }
  return merged
}

function mergeSingleImporter (
  ourImporter?: ProjectSnapshot,
  theirImporter?: ProjectSnapshot
): ProjectSnapshot {
  const result: ProjectSnapshot = { specifiers: {} }
  for (const depType of ['dependencies', 'devDependencies', 'optionalDependencies'] as const) {
    const mergedDeps = mergeDict(
      ourImporter?.[depType] ?? {},
      theirImporter?.[depType] ?? {},
      mergeVersions
    )
    if (Object.keys(mergedDeps).length > 0) {
      result[depType] = mergedDeps
    }
  }
  result.specifiers = mergeDict(
    ourImporter?.specifiers ?? {},
    theirImporter?.specifiers ?? {},
    takeChangedValue
  )
  return result
}

function mergePackageSnapshots (
  ourPackages?: PackageSnapshots,
  theirPackages?: PackageSnapshots
): PackageSnapshots {
  const merged: PackageSnapshots = {}
  const allPaths = Array.from(new Set([
    ...Object.keys(ourPackages ?? {}),
    ...Object.keys(theirPackages ?? {}),
  ])) as DepPath[]

  for (const depPath of allPaths) {
    const ourPkg = ourPackages && Object.hasOwn(ourPackages, depPath) ? ourPackages[depPath] : undefined
    const theirPkg = theirPackages && Object.hasOwn(theirPackages, depPath) ? theirPackages[depPath] : undefined
    merged[depPath] = mergeSinglePackage(ourPkg, theirPkg)
  }
  return merged
}

function mergeSinglePackage (ourPkg?: PackageSnapshot, theirPkg?: PackageSnapshot): PackageSnapshot {
  const merged = { ...ourPkg, ...theirPkg }
  for (const depType of ['dependencies', 'optionalDependencies'] as const) {
    const mergedDeps = mergeDict(
      ourPkg?.[depType] ?? {},
      theirPkg?.[depType] ?? {},
      mergeVersions
    )
    if (Object.keys(mergedDeps).length > 0) {
      merged[depType] = mergedDeps
    } else {
      delete merged[depType]
    }
  }
  return merged as PackageSnapshot
}

const KNOWN_TOP_LEVEL_KEYS = new Set([
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

function mergeForeignKeys (target: LockfileObject, ours: LockfileObject, theirs: LockfileObject): void {
  for (const [key, value] of Object.entries(theirs)) {
    if (!KNOWN_TOP_LEVEL_KEYS.has(key) && !Object.hasOwn(target, key)) {
      setTopLevelProperty(target, key, value)
    }
  }
  for (const [key, value] of Object.entries(ours)) {
    if (!KNOWN_TOP_LEVEL_KEYS.has(key)) {
      setTopLevelProperty(target, key, value)
    }
  }
}

function setTopLevelProperty (target: LockfileObject, key: string, value: unknown): void {
  Object.defineProperty(target, key, {
    value,
    writable: true,
    enumerable: true,
    configurable: true,
  })
}

function mergeSettings (
  ours?: LockfileSettings,
  theirs?: LockfileSettings
): LockfileSettings | undefined {
  if (!ours && !theirs) return undefined
  const settings: LockfileSettings = {}
  mergeBooleanSettings(settings, ours, theirs)
  mergePeersSettings(settings, ours, theirs)
  return Object.keys(settings).length > 0 ? settings : undefined
}

function mergeBooleanSettings (target: LockfileSettings, ours?: LockfileSettings, theirs?: LockfileSettings): void {
  if (ours?.autoInstallPeers != null || theirs?.autoInstallPeers != null) {
    target.autoInstallPeers = (ours?.autoInstallPeers ?? false) || (theirs?.autoInstallPeers ?? false)
  }
  if (ours?.excludeLinksFromLockfile != null || theirs?.excludeLinksFromLockfile != null) {
    target.excludeLinksFromLockfile = (ours?.excludeLinksFromLockfile ?? false) || (theirs?.excludeLinksFromLockfile ?? false)
  }
  if (ours?.injectWorkspacePackages != null || theirs?.injectWorkspacePackages != null) {
    target.injectWorkspacePackages = (ours?.injectWorkspacePackages ?? false) || (theirs?.injectWorkspacePackages ?? false)
  }
}

function mergePeersSettings (target: LockfileSettings, ours?: LockfileSettings, theirs?: LockfileSettings): void {
  if (ours?.dedupePeers != null || theirs?.dedupePeers != null) {
    target.dedupePeers = ours?.dedupePeers ?? theirs?.dedupePeers
  }
  if (ours?.peersSuffixMaxLength != null || theirs?.peersSuffixMaxLength != null) {
    target.peersSuffixMaxLength = ours?.peersSuffixMaxLength ?? theirs?.peersSuffixMaxLength
  }
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
    merged[catalogName] = mergeSingleCatalog(ours?.[catalogName], theirs?.[catalogName])
  }
  return Object.keys(merged).length > 0 ? merged : undefined
}

function mergeSingleCatalog (
  ourCatalog: Record<string, ResolvedCatalogEntry> = {},
  theirCatalog: Record<string, ResolvedCatalogEntry> = {}
): Record<string, ResolvedCatalogEntry> {
  const merged: Record<string, ResolvedCatalogEntry> = {}
  const allDepNames = new Set([
    ...Object.keys(ourCatalog),
    ...Object.keys(theirCatalog),
  ])
  for (const name of allDepNames) {
    const ourEntry = ourCatalog[name]
    const theirEntry = theirCatalog[name]
    if (ourEntry && theirEntry) {
      merged[name] = {
        specifier: takeChangedValue(ourEntry.specifier, theirEntry.specifier),
        version: mergeVersions(ourEntry.version, theirEntry.version),
      }
    } else {
      merged[name] = (ourEntry ?? theirEntry)!
    }
  }
  return merged
}

type ValueMerger<Value> = (ourValue: Value, theirValue: Value) => Value

function mergeDict<Value> (
  ourDict: Record<string, Value>,
  theirDict: Record<string, Value>,
  valueMerger: ValueMerger<Value>
): Record<string, Value> {
  const newDict: Record<string, Value> = {}
  const allKeys = new Set([...Object.keys(ourDict), ...Object.keys(theirDict)])
  for (const key of allKeys) {
    const ourVal = Object.hasOwn(ourDict, key) ? ourDict[key] : undefined
    const theirVal = Object.hasOwn(theirDict, key) ? theirDict[key] : undefined
    const changedValue = valueMerger(
      ourVal as Value,
      theirVal as Value
    )
    if (changedValue) {
      newDict[key] = changedValue
    }
  }
  return newDict
}

function takeChangedValue<Value> (ourValue: Value, theirValue: Value): Value {
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
