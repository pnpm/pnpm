import * as dp from '@pnpm/deps.path'
import { LockfileMissingDependencyError } from '@pnpm/error'
import type { PackageSnapshot } from '@pnpm/lockfile.types'
import {
  type LockfileObject,
  nameVerFromPkgSnapshot,
  type ProjectId,
} from '@pnpm/lockfile.utils'
import { hoist as _hoist, HoisterDependencyKind, type HoisterResult, type HoisterTree } from '@yarnpkg/nm/hoist'

/**
 * Controls how far dependencies are hoisted, mirroring yarn's
 * `nmHoistingLimits`. Given workspace package `A` → `B` → `C`:
 *
 * - `'none'` (default): hoist as far as possible.
 *   - `/packages/A`, `/node_modules/B`, `/node_modules/C`
 * - `'workspaces'`: hoist only as far as each workspace package.
 *   - `/packages/A`, `/packages/A/node_modules/B`, `/packages/A/node_modules/C`
 * - `'dependencies'`: hoist only up to each workspace package's direct
 *   dependencies.
 *   - `/packages/A`, `/packages/A/node_modules/B`, `/packages/A/node_modules/B/node_modules/C`
 */
export type HoistingLimits = 'none' | 'workspaces' | 'dependencies'

export type { HoisterResult }

/**
 * The identity every peer variant of one package version collapses
 * onto.
 *
 * `toTree` maps the id to the first depPath it sees for it and stamps
 * that depPath on every variant as their shared `reference`, so only
 * that one depPath survives into the result. Anything indexing the
 * hoist result by package therefore has to key *and* look up by this
 * id rather than by the depPath an edge declares — otherwise every
 * edge on a collapsed variant finds nothing and drops out of the
 * layout.
 *
 * One id can still cover several directories: nodes are interned per
 * `(alias, depPath)`, so an alias exposing a package under a second
 * name gets a node, and a directory, of its own. An index over the
 * result holds the list and resolves an edge to its first entry.
 *
 * An injected directory dependency (a `directory` resolution) keeps
 * its peer suffix: every variant of it is a separate on-disk copy of
 * the local package, materialized with its own peer-resolved
 * dependency set, so collapsing the variants would rewire every
 * dependent of the losing one onto the survivor's children (Bit's
 * root components pin conflicting peers across such copies on
 * purpose). The registry collapse exists to stop peer-variant
 * explosion on large lockfiles; directory snapshots are one per
 * injected workspace package and cannot explode that way.
 */
export function getHoisterPkgId (depPath: string, pkgSnapshot: PackageSnapshot): string {
  // `resolution` is typed as required, but the lockfile is parsed from
  // untyped YAML — guard so a malformed snapshot degrades to the
  // collapsed identity instead of a TypeError here.
  const resolution = pkgSnapshot.resolution as PackageSnapshot['resolution'] | undefined
  if (resolution != null && 'directory' in resolution && resolution.directory != null) {
    return depPath
  }
  const { name, version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  return `${name}@${version}`
}

/**
 * Translate the user-facing {@link HoistingLimits} mode into the
 * `@yarnpkg/nm` hoister's per-locator border map. A name in a
 * locator's set is a hoisting border: that node's dependencies are
 * not hoisted above it. Returns `undefined` for `'none'` (and when
 * unset) so the hoister hoists as far as possible.
 */
export function getHoistingLimits (lockfile: Pick<LockfileObject, 'importers'>, mode: HoistingLimits | undefined): Map<string, Set<string>> | undefined {
  if (!mode || mode === 'none') return undefined

  const hoistingLimits = new Map<string, Set<string>>()
  const rootHoistingLimit = new Set<string>()

  for (const [importerId, importer] of Object.entries(lockfile.importers)) {
    const isWorkspaceRoot = importerId === '.'
    const encodedId = encodeURIComponent(importerId)
    if (!isWorkspaceRoot) {
      rootHoistingLimit.add(encodedId)
      if (mode !== 'dependencies') {
        // In `'workspaces'` mode it's enough to border each workspace
        // package at the root; their own direct deps don't need a
        // per-importer border.
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
  lockfile: LockfileObject,
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

  const hoistingLimits = getHoistingLimits(lockfile, opts?.hoistingLimits)
  const hoisterResult = _hoist(node, { ...opts, hoistingLimits })
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
    lockfile: LockfileObject
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
      const { name: pkgName } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
      const id = getHoisterPkgId(depPath, pkgSnapshot)
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
