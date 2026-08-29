import type { EnvLockfile, LockfileFile } from '@pnpm/lockfile.types'
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
  'patchedDependencies',
  'importers',
  'packages',
]
const ROOT_KEYS_ORDER = Object.fromEntries(ROOT_KEYS.map((key, index) => [key, index]))

export function sortLockfileKeys (lockfile: LockfileFile): LockfileFile
export function sortLockfileKeys (lockfile: EnvLockfile): EnvLockfile
export function sortLockfileKeys (lockfile: LockfileFile | EnvLockfile): LockfileFile | EnvLockfile {
  if (lockfile.importers != null) {
    lockfile.importers = sortDirectKeys(lockfile.importers)
    for (const [importerId, importer] of Object.entries(lockfile.importers)) {
      lockfile.importers[importerId] = sortKeysByPriority({
        priority: ROOT_KEYS_ORDER,
        deep: true,
      }, importer)
    }
  }
  if (lockfile.packages != null) {
    lockfile.packages = sortDirectKeys(lockfile.packages)
    for (const [pkgId, pkg] of Object.entries(lockfile.packages)) {
      lockfile.packages[pkgId] = sortKeysByPriority({
        priority: ORDERED_KEYS,
        deep: true,
      }, pkg)
    }
  }
  if (lockfile.snapshots != null) {
    lockfile.snapshots = sortDirectKeys(lockfile.snapshots)
    for (const [pkgId, pkg] of Object.entries(lockfile.snapshots)) {
      lockfile.snapshots[pkgId] = sortKeysByPriority({
        priority: ORDERED_KEYS,
        deep: true,
      }, pkg)
    }
  }
  if ('catalogs' in lockfile && lockfile.catalogs != null) {
    lockfile.catalogs = sortDirectKeys(lockfile.catalogs)
    for (const [catalogName, catalog] of Object.entries(lockfile.catalogs)) {
      lockfile.catalogs[catalogName] = sortDeepKeys(catalog)
    }
  }
  if ('time' in lockfile && lockfile.time != null) {
    lockfile.time = sortDirectKeys(lockfile.time)
  }
  if ('patchedDependencies' in lockfile && lockfile.patchedDependencies != null) {
    lockfile.patchedDependencies = sortDirectKeys(lockfile.patchedDependencies)
  }
  return sortKeysByPriority({ priority: ROOT_KEYS_ORDER }, lockfile)
}
