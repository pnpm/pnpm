import { LockfileMissingDependencyError } from '@pnpm/error'
import {
  type Lockfile,
  type ProjectId,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile.utils'
import * as dp from '@pnpm/dependency-path'
import { hoist as _hoist, HoisterDependencyKind, type HoisterTree, type HoisterResult } from '@yarnpkg/nm/hoist'

export type HoistingLimits = Map<string, Set<string>>

export type { HoisterResult }

export function hoist (
  lockfile: Lockfile,
  opts?: {
    hoistingLimits?: HoistingLimits
    // This option was added for Bit CLI in order to prevent pnpm from overwriting dependencies linked by Bit.
    // However, in the future it might be useful to use it in pnpm for skipping any dependencies added by external tools.
    externalDependencies?: Set<string>
    autoInstallPeers?: boolean
  }
): HoisterResult {
  const nodes = new Map<string, HoisterTree>()
  const ctx = {
    autoInstallPeers: opts?.autoInstallPeers,
    nodes,
    lockfile,
    depPathByPkgId: new Map<string, string>(),
  }
  const _toTree = toTree.bind(null, ctx)
  const node: HoisterTree = {
    name: '.',
    identName: '.',
    reference: '',
    peerNames: new Set<string>([]),
    dependencyKind: HoisterDependencyKind.WORKSPACE,
    dependencies: _toTree({
      ...lockfile.importers['.' as ProjectId]?.dependencies,
      ...lockfile.importers['.' as ProjectId]?.devDependencies,
      ...lockfile.importers['.' as ProjectId]?.optionalDependencies,
      ...(Array.from(opts?.externalDependencies ?? [])).reduce((acc, dep) => {
        // It doesn't matter what version spec is used here.
        // This dependency will be removed from the tree anyway.
        // It is only needed to prevent the hoister from hoisting deps with this name to the root of node_modules.
        acc[dep] = 'link:'
        return acc
      }, {} as Record<string, string>),
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
      dependencies: _toTree({
        ...importer.dependencies,
        ...importer.devDependencies,
        ...importer.optionalDependencies,
      }),
    }
    node.dependencies.add(importerNode)
  }

  const hoisterResult = _hoist(node, opts)
  if (opts?.externalDependencies) {
    for (const hoistedDep of hoisterResult.dependencies.values()) {
      if (opts.externalDependencies.has(hoistedDep.name)) {
        hoisterResult.dependencies.delete(hoistedDep)
      }
    }
  }
  return hoisterResult
}

function toTree (
  { nodes, lockfile, depPathByPkgId, autoInstallPeers }: {
    autoInstallPeers?: boolean
    depPathByPkgId: Map<string, string>
    lockfile: Lockfile
    nodes: Map<string, HoisterTree>
  },
  deps: Record<string, string>
): Set<HoisterTree> {
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
      const { name: pkgName, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      const id = `${pkgName}@${version}`
      if (!depPathByPkgId.has(id)) {
        depPathByPkgId.set(id, depPath)
      }
      node = {
        name: alias,
        identName: pkgName,
        reference: depPathByPkgId.get(id)!,
        dependencyKind: HoisterDependencyKind.REGULAR,
        dependencies: new Set(),
        peerNames: new Set(autoInstallPeers
          ? []
          : [
            ...Object.keys(pkgSnapshot.peerDependencies ?? {}),
            ...(pkgSnapshot.transitivePeerDependencies ?? []),
          ]),
      }
      nodes.set(key, node)
      node.dependencies = toTree(
        { nodes, lockfile, depPathByPkgId, autoInstallPeers },
        { ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies })
    }
    return node
  }))
}
