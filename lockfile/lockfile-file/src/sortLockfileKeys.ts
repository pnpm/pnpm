import { lexCompare } from '@pnpm/util.lex-comparator'
import sortKeys from 'sort-keys'
import { type LockfileFile } from './write'

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

const ROOT_KEYS_ORDER = {
  lockfileVersion: 1,
  settings: 2,
  // only and never are conflict options.
  neverBuiltDependencies: 3,
  onlyBuiltDependencies: 3,
  overrides: 4,
  packageExtensionsChecksum: 5,
  patchedDependencies: 6,
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
  return lexCompare(left, right)
}

export function sortLockfileKeys (lockfile: LockfileFile) {
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
  for (const key of ['specifiers', 'dependencies', 'devDependencies', 'optionalDependencies', 'time', 'patchedDependencies'] as const) {
    if (!lockfile[key]) continue
    lockfile[key] = sortKeys<any>(lockfile[key]) // eslint-disable-line @typescript-eslint/no-explicit-any
  }
  return sortKeys(lockfile, { compare: compareRootKeys })
}
