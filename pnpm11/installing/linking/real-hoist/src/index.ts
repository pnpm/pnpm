import * as dp from '@pnpm/deps.path'
import { LockfileMissingDependencyError } from '@pnpm/error'
import type { PackageSnapshot, ProjectSnapshot } from '@pnpm/lockfile.types'
import {
  type LockfileObject,
  nameVerFromPkgSnapshot,
  type ProjectId,
} from '@pnpm/lockfile.utils'
import type { DepPath } from '@pnpm/types'
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
export function getHoistingLimits (
  lockfile: Pick<LockfileObject, 'importers'>,
  mode: HoistingLimits | undefined
): Map<string, Set<string>> | undefined {
  if (!mode || mode === 'none') return undefined

  const hoistingLimits = new Map<string, Set<string>>()
  const rootHoistingLimit = new Set<string>()

  for (const [importerId, importer] of Object.entries(lockfile.importers)) {
    addImporterHoistingLimits({
      hoistingLimits,
      importer,
      importerId,
      mode,
      rootHoistingLimit,
    })
  }

  if (!hoistingLimits.has('.@')) {
    hoistingLimits.set('.@', rootHoistingLimit)
  }

  return hoistingLimits
}

interface AddImporterLimitsOptions {
  hoistingLimits: Map<string, Set<string>>
  importer: ProjectSnapshot
  importerId: string
  mode: HoistingLimits
  rootHoistingLimit: Set<string>
}

function addImporterHoistingLimits (opts: AddImporterLimitsOptions): void {
  const { hoistingLimits, importer, importerId, mode, rootHoistingLimit } = opts
  const isWorkspaceRoot = importerId === '.'
  const encodedId = encodeURIComponent(importerId)
  if (!isWorkspaceRoot) {
    rootHoistingLimit.add(encodedId)
    if (mode !== 'dependencies') {
      return
    }
  }

  const reference = isWorkspaceRoot ? '' : `workspace:${importerId}`
  const hoistingLimit = isWorkspaceRoot ? rootHoistingLimit : new Set<string>()
  hoistingLimits.set(`${encodedId}@${reference}`, hoistingLimit)

  collectDirectDeps(importer, hoistingLimit)
}

function collectDirectDeps (
  importer: ProjectSnapshot,
  targetSet: Set<string>
): void {
  const depGroups = [importer.dependencies, importer.devDependencies, importer.optionalDependencies]
  for (const deps of depGroups) {
    if (!deps) continue
    for (const dep of Object.keys(deps)) {
      targetSet.add(dep)
    }
  }
}

export function hoist (
  lockfile: LockfileObject,
  opts?: {
    hoistingLimits?: HoistingLimits
    externalDependencies?: Set<string>
    autoInstallPeers?: boolean
  }
): HoisterResult {
  const ctx: TreeContext = {
    autoInstallPeers: opts?.autoInstallPeers,
    depPathByPkgId: new Map<string, string>(),
    lockfile,
    nodes: new Map<string, HoisterTree>(),
  }
  const rootNode = buildRootHoisterTree(lockfile, ctx, opts?.externalDependencies)
  for (const [importerId, importer] of Object.entries(lockfile.importers)) {
    if (importerId === '.') continue
    rootNode.dependencies.add(buildImporterHoisterTree(importerId, importer, ctx))
  }

  const hoistingLimits = getHoistingLimits(lockfile, opts?.hoistingLimits)
  const hoisterResult = _hoist(rootNode, { ...opts, hoistingLimits })
  if (opts?.externalDependencies) {
    filterExternalDependencies(hoisterResult, opts.externalDependencies)
  }
  return hoisterResult
}

function buildRootHoisterTree (
  lockfile: LockfileObject,
  ctx: TreeContext,
  externalDependencies?: Set<string>
): HoisterTree {
  const rootImporter = lockfile.importers['.' as ProjectId]
  const externalDepsMap: Record<string, string> = {}
  for (const dep of externalDependencies ?? []) {
    externalDepsMap[dep] = 'link:'
  }
  return {
    dependencies: toTree(ctx, {
      ...rootImporter?.dependencies,
      ...rootImporter?.devDependencies,
      ...rootImporter?.optionalDependencies,
      ...externalDepsMap,
    }),
    dependencyKind: HoisterDependencyKind.WORKSPACE,
    identName: '.',
    name: '.',
    peerNames: new Set<string>([]),
    reference: '',
  }
}

function buildImporterHoisterTree (
  importerId: string,
  importer: ProjectSnapshot,
  ctx: TreeContext
): HoisterTree {
  return {
    dependencies: toTree(ctx, {
      ...importer.dependencies,
      ...importer.devDependencies,
      ...importer.optionalDependencies,
    }),
    dependencyKind: HoisterDependencyKind.WORKSPACE,
    identName: encodeURIComponent(importerId),
    name: encodeURIComponent(importerId),
    peerNames: new Set<string>([]),
    reference: `workspace:${importerId}`,
  }
}

function filterExternalDependencies (
  result: HoisterResult,
  externalDependencies: Set<string>
): void {
  for (const hoistedDep of result.dependencies.values()) {
    if (externalDependencies.has(hoistedDep.name)) {
      result.dependencies.delete(hoistedDep)
    }
  }
}

interface TreeContext {
  autoInstallPeers?: boolean
  depPathByPkgId: Map<string, string>
  lockfile: LockfileObject
  nodes: Map<string, HoisterTree>
}

function toTree (
  ctx: TreeContext,
  deps: Record<string, string>
): Set<HoisterTree> {
  return new Set(Object.entries(deps).map(([alias, ref]) => {
    const depPath = dp.refToRelative(ref, alias)
    if (!depPath) {
      return getOrCreateWorkspaceNode(ctx.nodes, alias, ref)
    }
    return getOrCreatePackageNode(ctx, alias, depPath)
  }))
}

function getOrCreateWorkspaceNode (
  nodes: Map<string, HoisterTree>,
  alias: string,
  ref: string
): HoisterTree {
  const key = `${alias}:${ref}`
  let node = nodes.get(key)
  if (!node) {
    node = {
      dependencies: new Set(),
      dependencyKind: dp.packageRootLinkTarget(ref) != null
        ? HoisterDependencyKind.WORKSPACE
        : HoisterDependencyKind.REGULAR,
      identName: alias,
      name: alias,
      peerNames: new Set(),
      reference: ref,
    }
    nodes.set(key, node)
  }
  return node
}

function getOrCreatePackageNode (
  ctx: TreeContext,
  alias: string,
  depPath: string
): HoisterTree {
  const key = `${alias}:${depPath}`
  let node = ctx.nodes.get(key)
  if (!node) {
    const pkgSnapshot = ctx.lockfile.packages?.[depPath as DepPath]
    if (!pkgSnapshot) {
      throw new LockfileMissingDependencyError(depPath)
    }
    const { name: pkgName } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
    const id = getHoisterPkgId(depPath, pkgSnapshot)
    if (!ctx.depPathByPkgId.has(id)) {
      ctx.depPathByPkgId.set(id, depPath)
    }
    node = {
      dependencies: new Set(),
      dependencyKind: HoisterDependencyKind.REGULAR,
      identName: pkgName,
      name: alias,
      peerNames: resolvePeerNames(pkgSnapshot, ctx.autoInstallPeers),
      reference: ctx.depPathByPkgId.get(id)!,
    }
    ctx.nodes.set(key, node)
    node.dependencies = toTree(ctx, {
      ...pkgSnapshot.dependencies,
      ...pkgSnapshot.optionalDependencies,
    })
  }
  return node
}

function resolvePeerNames (
  pkgSnapshot: PackageSnapshot,
  autoInstallPeers?: boolean
): Set<string> {
  if (autoInstallPeers) {
    return new Set()
  }
  return new Set([
    ...Object.keys(pkgSnapshot.peerDependencies ?? {}),
    ...(pkgSnapshot.transitivePeerDependencies ?? []),
  ])
}
