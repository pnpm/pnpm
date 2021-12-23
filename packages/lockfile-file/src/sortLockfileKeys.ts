import sortKeys from 'sort-keys'
import { LockfileFile } from './write'

const ORDERED_KEYS = {
  resolution: 1,
  id: 2,

  name: 3,
  version: 4,

  engines: 5,
  cpu: 6,
  os: 7,

  deprecated: 8,
  hasBin: 9,
  prepare: 10,
  requiresBuild: 11,

  bundleDependencies: 12,
  peerDependencies: 13,
  peerDependenciesMeta: 14,

  dependencies: 15,
  optionalDependencies: 16,

  transitivePeerDependencies: 17,
  dev: 18,
  optional: 19,
}

const ROOT_KEYS_ORDER = {
  lockfileVersion: 1,
  neverBuiltDependencies: 2,
  overrides: 3,
  packageExtensionsChecksum: 4,
  specifiers: 10,
  dependencies: 11,
  optionalDependencies: 12,
  devDependencies: 13,
  dependenciesMeta: 14,
  importers: 15,
  packages: 16,
}

function compareWithPriority (priority: Record<string, number>, left: string, right: string) {
  const leftPriority = priority[left]
  const rightPriority = priority[right]
  if (leftPriority && rightPriority) return leftPriority - rightPriority
  if (leftPriority) return -1
  if (rightPriority) return 1
  return left.localeCompare(right)
}

export function sortLockfileKeys (lockfile: LockfileFile) {
  const compareRootKeys = compareWithPriority.bind(null, ROOT_KEYS_ORDER)
  if (lockfile.importers != null) {
    lockfile.importers = sortKeys(lockfile.importers)
    for (const importerId of Object.keys(lockfile.importers)) {
      lockfile.importers[importerId] = sortKeys(lockfile.importers[importerId], {
        compare: compareRootKeys,
        deep: true,
      })
    }
  }
  if (lockfile.packages != null) {
    lockfile.packages = sortKeys(lockfile.packages)
    for (const pkgId of Object.keys(lockfile.packages)) {
      lockfile.packages[pkgId] = sortKeys(lockfile.packages[pkgId], {
        compare: compareWithPriority.bind(null, ORDERED_KEYS),
        deep: true,
      })
    }
  }
  for (const key of ['specifiers', 'dependencies', 'devDependencies', 'optionalDependencies']) {
    if (!lockfile[key]) continue
    lockfile[key] = sortKeys(lockfile[key])
  }
  return sortKeys(lockfile, { compare: compareRootKeys })
}
