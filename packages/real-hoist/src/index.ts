import {
  Lockfile,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import * as dp from 'dependency-path'
import { hoist, HoisterTree, HoisterResult } from '@yarnpkg/nm/lib/hoist'

export { HoisterResult }

export default function hoistByLockfile (lockfile: Lockfile): HoisterResult {
  const nodes = new Map<string, HoisterTree>()
  const node: HoisterTree = {
    name: '.',
    identName: '.',
    reference: '',
    peerNames: new Set<string>([]),
    isWorkspace: true,
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
      isWorkspace: true,
      dependencies: toTree(nodes, lockfile, {
        ...importer.dependencies,
        ...importer.devDependencies,
        ...importer.optionalDependencies,
      }),
    }
    node.dependencies.add(importerNode)
  }

  return hoist(node)
}

function toTree (nodes: Map<string, HoisterTree>, lockfile: Lockfile, deps: Record<string, string>): Set<HoisterTree> {
  return new Set(Object.entries(deps).map(([alias, ref]) => {
    const depPath = dp.refToRelative(ref, alias)!
    if (!depPath) {
      let node = nodes.get(ref)
      if (!node) {
        node = {
          name: alias,
          identName: alias,
          reference: ref,
          isWorkspace: false,
          dependencies: new Set(),
          peerNames: new Set(),
        }
        nodes.set(depPath, node)
      }
      return node
    }
    let node = nodes.get(depPath)
    if (!node) {
      // const { name, version, peersSuffix } = nameVerFromPkgSnapshot(depPath, lockfile.packages![depPath])
      const pkgSnapshot = lockfile.packages![depPath]
      const pkgName = nameVerFromPkgSnapshot(depPath, pkgSnapshot).name
      node = {
        name: alias,
        identName: pkgName,
        reference: depPath,
        isWorkspace: false,
        dependencies: new Set(),
        peerNames: new Set([
          ...Object.keys(pkgSnapshot.peerDependencies ?? {}),
          ...(pkgSnapshot.transitivePeerDependencies ?? []),
        ]),
      }
      nodes.set(depPath, node)
      node.dependencies = toTree(nodes, lockfile, { ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies })
    }
    return node
  }))
}
