import { LockfileMissingDependencyError } from '@pnpm/error'
import {
  type Lockfile,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import * as dp from '@pnpm/dependency-path'
import { hoist as _hoist, HoisterDependencyKind, type HoisterTree, type HoisterResult } from '@yarnpkg/nm'

export type HoistingLimits = Map<string, Set<string>>

export type { HoisterResult }

/**
 * In the (default) "none" mode, dependencies are hoisted as far as possible.
 * For example, given workspace package "A" which depends on package "B" which
 * depends on package "C", the layout on disk will be:
 *
 * - /packages/A
 * - /node_modules/B
 * - /node_modules/C
 *
 * In the "workspaces" mode, dependencies are hoisted only as far as each
 * workspace package. Given the same example, the layout would be:
 *
 * - /packages/A
 * - /packages/A/node_modules/B
 * - /packages/A/node_modules/C
 *
 * In the "dependencies" mode, dependencies are only hoisted up to the direct
 * dependencies of each workspace package. Given the same example:
 *
 * - /packages/A
 * - /packages/A/node_modules/B
 * - /packages/A/node_modules/B/node_modules/C
 */
export type HoistingLimitMode = 'none' | 'workspaces' | 'dependencies'

export function getHoistingLimits (lockfile: Lockfile, mode: HoistingLimitMode | undefined): HoistingLimits | undefined {
  if (!mode || mode === 'none') return undefined

  const hoistingLimits = new Map<string, Set<string>>()
  const rootHoistingLimit = new Set<string>()

  for (const [importerId, importer] of Object.entries(lockfile.importers)) {
    const isWorkspaceRoot = importerId === '.'
    const encodedId = encodeURIComponent(importerId)
    if (!isWorkspaceRoot) {
      rootHoistingLimit.add(encodedId)
      if (mode !== 'dependencies') {
        // If we're not going to prevent hoisting of non-direct dependencies,
        // skip the below for all but the workspace root.
        continue
      }
    }

    const reference = isWorkspaceRoot ? '' : `workspace:${importerId}`
    const hoistingLimit = isWorkspaceRoot ? rootHoistingLimit : new Set<string>()

    hoistingLimits.set(`${encodedId}@${reference}`, hoistingLimit)

    for (const deps of [importer.dependencies, importer.devDependencies, importer.optionalDependencies]) {
      if (!deps) continue
      for (const dep of Object.keys(deps)) {
        hoistingLimit.add(dep)
      }
    }
  }

  return hoistingLimits
}

export function hoist (
  lockfile: Lockfile,
  opts?: {
    hoistingLimits?: HoistingLimits
    // This option was added for Bit CLI in order to prevent pnpm from overwriting dependencies linked by Bit.
    // However, in the future it might be useful to use it in pnpm for skipping any dependencies added by external tools.
    externalDependencies?: Set<string>
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
      dependencies: toTree(nodes, lockfile, {
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
