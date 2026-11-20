import type { CatalogSnapshots, EnvLockfile, LockfileFile } from '@pnpm/lockfile.types'
import { sortDeepKeys, sortDirectKeys, sortKeysByPriority } from '@pnpm/object.key-sorting'

const ORDERED_KEYS = {
  resolution: 1,
  id: 2,

  name: 3,
  version: 4,

  engines: 5,
  cpu: 6,
  os: 7,
  libc: 8,

  deprecated: 9,
  hasBin: 10,
  prepare: 11,
  requiresBuild: 12,

  bundleDependencies: 13,
  peerDependencies: 14,
  peerDependenciesMeta: 15,

  dependencies: 16,
  optionalDependencies: 17,

  transitivePeerDependencies: 18,
  dev: 19,
  optional: 20,
}

type RootKey = keyof LockfileFile
const ROOT_KEYS: readonly RootKey[] = [
  'lockfileVersion',
  'settings',
  'catalogs',
  'overrides',
  'packageExtensionsChecksum',
  'pnpmfileChecksum',
  'untrackedPnpmfileReadPackageHook',
  'patchedDependencies',
  'importers',
  'packages',
]
const ROOT_KEYS_ORDER = Object.fromEntries(ROOT_KEYS.map((key, index) => [key, index]))

export function sortLockfileKeys (lockfile: LockfileFile): LockfileFile
export function sortLockfileKeys (lockfile: EnvLockfile): EnvLockfile
export function sortLockfileKeys (lockfile: LockfileFile | EnvLockfile): LockfileFile | EnvLockfile {
  if (lockfile.importers != null) {
    lockfile.importers = sortEntriesByPriority(lockfile.importers, ROOT_KEYS_ORDER)
  }
  if (lockfile.packages != null) {
    lockfile.packages = sortEntriesByPriority(lockfile.packages, ORDERED_KEYS)
  }
  if (lockfile.snapshots != null) {
    lockfile.snapshots = sortEntriesByPriority(lockfile.snapshots, ORDERED_KEYS)
  }
  if ('catalogs' in lockfile && lockfile.catalogs != null) {
    lockfile.catalogs = sortCatalogs(lockfile.catalogs)
  }
  if ('time' in lockfile && lockfile.time != null) {
    lockfile.time = sortDirectKeys(lockfile.time)
  }
  if ('patchedDependencies' in lockfile && lockfile.patchedDependencies != null) {
    lockfile.patchedDependencies = sortDirectKeys(lockfile.patchedDependencies)
  }
  return sortKeysByPriority({ priority: ROOT_KEYS_ORDER }, lockfile)
}

function sortEntriesByPriority<Entries extends Record<string, object>> (
  entries: Entries,
  priority: Record<string, number>
): Entries {
  const sorted = sortDirectKeys(entries)
  for (const [key, entry] of Object.entries(sorted)) {
    sorted[key as keyof Entries] = sortKeysByPriority({ priority, deep: true }, entry) as Entries[keyof Entries]
  }
  return sorted
}

function sortCatalogs (catalogs: CatalogSnapshots): CatalogSnapshots {
  const sorted = sortDirectKeys(catalogs)
  for (const [catalogName, catalog] of Object.entries(sorted)) {
    sorted[catalogName] = sortDeepKeys(catalog)
  }
  return sorted
}
