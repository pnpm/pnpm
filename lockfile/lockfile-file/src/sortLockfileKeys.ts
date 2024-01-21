import { lexCompare } from '@pnpm/util.lex-comparator'
import sortKeys from 'sort-keys'
import { type LockfileFileV9, type LockfileFile } from '@pnpm/lockfile-types'

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
  'dependencies',
  'optionalDependencies',
  'devDependencies',
  'dependenciesMeta',
  'importers',
  'packages',
]
const ROOT_KEYS_ORDER = Object.fromEntries(ROOT_KEYS.map((key, index) => [key, index]))

function compareWithPriority (priority: Record<string, number>, left: string, right: string): number {
  const leftPriority = priority[left]
  const rightPriority = priority[right]
  if (leftPriority != null && rightPriority != null) return leftPriority - rightPriority
  if (leftPriority != null) return -1
  if (rightPriority != null) return 1
  return lexCompare(left, right)
}

export function sortLockfileKeys (lockfile: LockfileFileV9): LockfileFileV9 {
  const compareRootKeys = compareWithPriority.bind(null, ROOT_KEYS_ORDER)
  if (lockfile.importers != null) {
    lockfile.importers = sortKeys(lockfile.importers)
    for (const [importerId, importer] of Object.entries(lockfile.importers)) {
      lockfile.importers[importerId] = sortKeys(importer, {
        compare: compareRootKeys,
        deep: true,
      })
    }
  }
  if (lockfile.packages != null) {
    lockfile.packages = sortKeys(lockfile.packages)
    for (const [pkgId, pkg] of Object.entries(lockfile.packages)) {
      lockfile.packages[pkgId] = sortKeys(pkg, {
        compare: compareWithPriority.bind(null, ORDERED_KEYS),
        deep: true,
      })
    }
  }
  if (lockfile.snapshots != null) {
    lockfile.snapshots = sortKeys(lockfile.snapshots)
    for (const [pkgId, pkg] of Object.entries(lockfile.snapshots)) {
      lockfile.snapshots[pkgId] = sortKeys(pkg, {
        compare: compareWithPriority.bind(null, ORDERED_KEYS),
        deep: true,
      })
    }
  }
  if (lockfile.catalogs != null) {
    lockfile.catalogs = sortKeys(lockfile.catalogs)
    for (const [catalogName, catalog] of Object.entries(lockfile.catalogs)) {
      lockfile.catalogs[catalogName] = sortKeys(catalog, {
        compare: lexCompare,
        deep: true,
      })
    }
  }
  for (const key of ['dependencies', 'devDependencies', 'optionalDependencies', 'time', 'patchedDependencies'] as const) {
    if (!lockfile[key]) continue
    lockfile[key] = sortKeys<any>(lockfile[key]) // eslint-disable-line @typescript-eslint/no-explicit-any
  }
  return sortKeys(lockfile, { compare: compareRootKeys })
}
