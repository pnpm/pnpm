import { LockfileMissingDependencyError } from '@pnpm/error'
import {
  Lockfile,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import * as dp from 'dependency-path'
import { hoist, HoisterDependencyKind, HoisterTree, HoisterResult } from '@yarnpkg/nm/lib/hoist'

export type HoistingLimits = Map<string, Set<string>>

export { HoisterResult }

export default function hoistByLockfile (
  lockfile: Lockfile,
  opts?: {
    hoistingLimits?: HoistingLimits
  }
): HoisterResult {
  const nodes = new Map<string, HoisterTree>()
  const node: HoisterTree = {
    name: '.',
    identName: '.',
    reference: '',
    peerNames: new Set<string>([]),
    dependencyKind: HoisterDependencyKind.WORKSPACE,
    dependencies: toTree(nodes, lockfile, {
      ...lockfile.importers['.']?.dependencies,
      ...lockfile.importers['.']?.devDependencies,
      ...lockfile.importers['.']?.optionalDependencies,
    }),
  }
  for (const [importerId, importer] of Object.entries(lockfile.importers)) {
    if (importerId === '.') continue
    const importerNode: HoisterTree = {
      name: encodeURIComponent(importerId),
      identName: encodeURIComponent(importerId),
      reference: `workspace:${importerId}`,
      peerNames: new Set<string>([]),
      dependencyKind: HoisterDependencyKind.WORKSPACE,
      dependencies: toTree(nodes, lockfile, {
        ...importer.dependencies,
        ...importer.devDependencies,
        ...importer.optionalDependencies,
      }),
    }
    node.dependencies.add(importerNode)
  }

  return hoist(node, opts)
}

function toTree (nodes: Map<string, HoisterTree>, lockfile: Lockfile, deps: Record<string, string>): Set<HoisterTree> {
  return new Set(Object.entries(deps).map(([alias, ref]) => {
    const depPath = dp.refToRelative(ref, alias)!
    if (!depPath) {
      const key = `${alias}:${ref}`
      let node = nodes.get(key)
      if (!node) {
        node = {
          name: alias,
          identName: alias,
          reference: ref,
          dependencyKind: HoisterDependencyKind.REGULAR,
          dependencies: new Set(),
          peerNames: new Set(),
        }
        nodes.set(key, node)
      }
      return node
    }
    const key = `${alias}:${depPath}`
    let node = nodes.get(key)
    if (!node) {
      const pkgSnapshot = lockfile.packages![depPath]
      if (!pkgSnapshot) {
        throw new LockfileMissingDependencyError(depPath)
      }
      const pkgName = nameVerFromPkgSnapshot(depPath, pkgSnapshot).name
      node = {
        name: alias,
        identName: pkgName,
        reference: depPath,
        dependencyKind: HoisterDependencyKind.REGULAR,
        dependencies: new Set(),
        peerNames: new Set([
          ...Object.keys(pkgSnapshot.peerDependencies ?? {}),
          ...(pkgSnapshot.transitivePeerDependencies ?? []),
        ]),
      }
      nodes.set(key, node)
      node.dependencies = toTree(nodes, lockfile, { ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies })
    }
    return node
  }))
}
