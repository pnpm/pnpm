import type { VersionOverride } from '@pnpm/config.parse-overrides'
import * as dp from '@pnpm/deps.path'
import type {
  LockfileObject,
  PackageSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile.types'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { RequestPackageFunction } from '@pnpm/store.controller-types'
import type {
  DepPath,
  ReadPackageHook,
  RegistriesByScope,
  RegistryOptions,
} from '@pnpm/types'
import semver from 'semver'

import { resolveNewManifests, type ResolverPolicyOptions } from './fastRewriteManifests.js'
import {
  type FastOverride,
  type RewriteContext,
  rewritePackages,
  rewriteResolvedDependencies,
} from './fastRewritePackages.js'

export type { FastOverride }

export type FastRewriteOptions = ResolverPolicyOptions & {
  lockfileDir: string
  lockfileIncludeTarballUrl?: boolean
  isLockfileUpToDate: (lockfile: LockfileObject) => Promise<boolean>
  readPackageHook?: ReadPackageHook
  registriesByScope: RegistriesByScope
  registryOptionsByUrl?: Record<string, RegistryOptions>
  requestPackage: RequestPackageFunction
  verifyLockfile?: (lockfile: LockfileObject) => Promise<void>
}

export async function tryFastUpdateOverrides (
  lockfile: LockfileObject,
  opts: FastRewriteOptions & {
    overrides: Record<string, string>
    parsedOverrides: VersionOverride[]
  }
): Promise<boolean> {
  const fastOverrides = getFastOverrides(lockfile, lockfile.overrides ?? {}, opts.overrides, opts.parsedOverrides)
  if (fastOverrides == null) return false
  if (movesACatalogedPackage(lockfile, fastOverrides)) return false
  return applyFastRewrite(lockfile, fastOverrides, opts, { overrides: opts.overrides })
}

/**
 * Whether any of `fastOverrides` moves a package a catalog entry records the
 * version of. The entry would have to move with it — which is the catalog
 * rewrite's job, not this one's — so the move goes to the resolver instead.
 *
 * A `parent>child` selector names no importer edge, and only importer edges
 * are what a catalog entry resolves.
 */
function movesACatalogedPackage (lockfile: LockfileObject, fastOverrides: FastOverride[]): boolean {
  const catalogedAliases = new Set(
    Object.values(lockfile.catalogs ?? {}).flatMap((catalog) => Object.keys(catalog))
  )
  return fastOverrides.some(({ name, parent }) => parent == null && catalogedAliases.has(name))
}

/**
 * Move every package named by `fastOverrides` to its new version, rebuilding
 * the affected package entries from the new version's manifest and redirecting
 * everything that referenced the old key. `settings` is the setting block that
 * drove the rewrite, recorded alongside it.
 *
 * Returns `false` whenever the move cannot be proven safe from the lockfile
 * plus the resolved manifests — a locked child the new manifest no longer
 * admits, or a candidate the freshness check rejects.
 */
export async function applyFastRewrite (
  lockfile: LockfileObject,
  fastOverrides: FastOverride[],
  opts: FastRewriteOptions,
  settings: Partial<LockfileObject>
): Promise<boolean> {
  const rewriteContext = createRewriteContext(lockfile, fastOverrides)
  if (rewriteContext == null) return false
  const manifests = await resolveNewManifests(fastOverrides, rewriteContext.replacements, opts)
  if (manifests == null) return false

  const packages = rewritePackages(lockfile.packages ?? {}, {
    lockfileIncludeTarballUrl: opts.lockfileIncludeTarballUrl,
    manifests,
    registriesByScope: opts.registriesByScope,
    registryOptionsByUrl: opts.registryOptionsByUrl,
    rewriteContext,
  })
  if (packages == null) return false

  const importers = rewriteImporters(lockfile.importers, rewriteContext)
  const updatedLockfile: LockfileObject = {
    ...lockfile,
    importers,
    packages: pruneUnreachablePackages(importers, packages),
    ...settings,
  }
  if (!await opts.isLockfileUpToDate(updatedLockfile)) return false
  await opts.verifyLockfile?.(updatedLockfile)
  lockfile.importers = updatedLockfile.importers
  lockfile.packages = updatedLockfile.packages
  Object.assign(lockfile, settings)
  return true
}

function createRewriteContext (lockfile: LockfileObject, fastOverrides: FastOverride[]): RewriteContext | null {
  const removals = fastOverrides.filter(({ newVersion }) => newVersion == null)
  const moves = fastOverrides.filter(({ newVersion }) => newVersion != null)
  const peerNames = getPeerNames(lockfile)
  if (removals.some(({ name }) => peerNames.has(name))) return null

  const replacements = collectReplacements(lockfile, fastOverrides)
  if (replacements == null) return null

  const changedNames = new Set(
    fastOverrides
      .filter(({ newVersion }) => newVersion != null)
      .map(({ name }) => name)
  )
  return { changedNames, peerNames, removals, replacements, moves }
}

function rewriteImporters (
  importers: LockfileObject['importers'],
  rewriteContext: RewriteContext
): LockfileObject['importers'] {
  return Object.fromEntries(
    Object.entries(importers).map(([id, importer]) => [
      id,
      {
        ...importer,
        dependencies: rewriteResolvedDependencies(importer.dependencies, rewriteContext),
        devDependencies: rewriteResolvedDependencies(importer.devDependencies, rewriteContext),
        optionalDependencies: rewriteResolvedDependencies(importer.optionalDependencies, rewriteContext),
      },
    ])
  ) as LockfileObject['importers']
}

function getFastOverrides (
  lockfile: LockfileObject,
  oldOverrides: Record<string, string>,
  newOverrides: Record<string, string>,
  parsedOverrides: VersionOverride[]
): FastOverride[] | null {
  if (Object.keys(oldOverrides).some((selector) => newOverrides[selector] == null)) return null

  const changedSelectors = Object.keys(newOverrides)
    .filter((selector) => oldOverrides[selector] !== newOverrides[selector])
  if (changedSelectors.length === 0) return null

  const parsedBySelector = new Map(parsedOverrides.map((override) => [override.selector, override]))
  const changedNames = new Set<string>()
  const result: FastOverride[] = []
  for (const selector of changedSelectors) {
    const fastOverride = toFastOverride(lockfile, {
      selector,
      override: parsedBySelector.get(selector),
      newValue: newOverrides[selector],
      oldVersion: oldOverrides[selector],
      parsedOverrides,
    })
    if (fastOverride == null || changedNames.has(fastOverride.name)) return null
    changedNames.add(fastOverride.name)
    result.push(fastOverride)
  }
  return result
}

/** The move a changed selector asks for, or `null` when the fast path cannot express it. */
function toFastOverride (
  lockfile: LockfileObject,
  { selector, override, newValue, oldVersion, parsedOverrides }: {
    selector: string
    override: VersionOverride | undefined
    newValue: string
    oldVersion: string | undefined
    parsedOverrides: VersionOverride[]
  }
): FastOverride | null {
  if (
    override == null ||
    override.targetPkg.bareSpecifier != null ||
    override.converge === true
  ) {
    return null
  }
  const name = override.targetPkg.name
  if (parsedOverrides.some((candidate) => candidate.selector !== selector && candidate.targetPkg.name === name)) {
    return null
  }
  const parent = override.parentPkg == null
    ? {}
    : {
      parent: {
        name: override.parentPkg.name,
        bareSpecifier: override.parentPkg.bareSpecifier,
      },
    }
  if (newValue === '-') return { name, ...parent }
  const newVersion = overriddenVersion(lockfile, name, newValue)
  if (newVersion == null) return null
  if (oldVersion != null && semver.valid(oldVersion) == null) return null
  return { name, ...parent, newVersion, oldVersion }
}

/**
 * The version an override moves its target to.
 *
 * A range names the highest already-locked version satisfying it, because
 * `preferredVersions` makes the resolver reuse a version the graph already
 * holds rather than the highest published.
 */
function overriddenVersion (
  lockfile: LockfileObject,
  name: string,
  value: string
): string | null {
  if (semver.valid(value) != null) return value
  if (semver.validRange(value) == null) return null
  const versions: string[] = []
  for (const [depPath, snapshot] of Object.entries(lockfile.packages ?? {})) {
    const version = lockedVersionInRange(depPath, snapshot, { name, range: value })
    if (version === null) return null
    if (version !== undefined) versions.push(version)
  }
  return versions.sort(semver.rcompare)[0] ?? null
}

/**
 * The version of `name` the package at `depPath` offers within `range`:
 * `undefined` when it is another package or outside the range, `null` when
 * its key cannot be moved by the fast path.
 */
function lockedVersionInRange (
  depPath: string,
  snapshot: PackageSnapshot,
  { name, range }: { name: string, range: string }
): string | null | undefined {
  const parsed = nameVerFromPkgSnapshot(depPath, snapshot)
  if (parsed.name !== name || parsed.nonSemverVersion != null) return undefined
  if (parsed.registryName != null || dp.parseDepPath(depPath).peerDepGraphHash !== '') return null
  if (semver.valid(parsed.version) != null && semver.satisfies(parsed.version, range)) {
    return parsed.version
  }
  return undefined
}

function collectReplacements (
  lockfile: LockfileObject,
  overrides: FastOverride[]
): Map<DepPath, DepPath> | null {
  const overridesByName = new Map(overrides.map((override) => [override.name, override]))
  const replacements = new Map<DepPath, DepPath>()
  const edges = allResolvedEdges(lockfile)
  for (const [alias, reference] of edges) {
    const override = overridesByName.get(alias)
    if (override?.newVersion == null) continue
    const oldDepPath = replaceableDepPath(lockfile, { alias, reference, oldVersion: override.oldVersion })
    if (oldDepPath == null) return null
    const newDepPath = `${alias}@${override.newVersion}` as DepPath
    const previousReplacement = replacements.get(oldDepPath)
    if (previousReplacement != null && previousReplacement !== newDepPath) return null
    replacements.set(oldDepPath, newDepPath)
  }
  const reachedByAnotherAlias = edges.some(([alias, reference]) => {
    const depPath = dp.refToRelative(reference, alias)
    return depPath != null && replacements.has(depPath) && !overridesByName.has(alias)
  })
  return reachedByAnotherAlias ? null : replacements
}

/**
 * The key `reference` points at when the fast path can move it to another
 * version of `alias`, or `null`.
 */
function replaceableDepPath (
  lockfile: LockfileObject,
  { alias, reference, oldVersion }: { alias: string, reference: string, oldVersion: string | undefined }
): DepPath | null {
  const oldDepPath = dp.refToRelative(reference, alias)
  if (oldDepPath == null) return null
  const parsed = dp.parse(oldDepPath)
  const snapshot = lockfile.packages?.[oldDepPath]
  // The fast path rebuilds the dep path as `<alias>@<version>`, which
  // would drop the registry qualifier of a named-registry package.
  if (!isPlainVersionKey(parsed, alias)) return null
  if (oldVersion != null && parsed.version !== oldVersion) return null
  if (snapshot == null || !isMovableSnapshot(snapshot)) return null
  return oldDepPath
}

function isPlainVersionKey (parsed: ReturnType<typeof dp.parse>, alias: string): boolean {
  return parsed.name === alias &&
    parsed.version != null &&
    parsed.registryName == null &&
    parsed.peerDepGraphHash == null &&
    parsed.patchHash == null
}

/** Whether the snapshot is a non-optional, peer-free registry package with an integrity. */
function isMovableSnapshot (snapshot: PackageSnapshot): boolean {
  if (
    snapshot.optional === true ||
    snapshot.peerDependencies != null ||
    snapshot.peerDependenciesMeta != null
  ) {
    return false
  }
  return 'integrity' in snapshot.resolution &&
    typeof snapshot.resolution.integrity === 'string' &&
    !('type' in snapshot.resolution && snapshot.resolution.type != null)
}

function getPeerNames (lockfile: LockfileObject): Set<string> {
  const result = new Set<string>()
  for (const snapshot of Object.values(lockfile.packages ?? {})) {
    for (const name of Object.keys(snapshot.peerDependencies ?? {})) result.add(name)
    for (const name of Object.keys(snapshot.peerDependenciesMeta ?? {})) result.add(name)
    for (const name of snapshot.transitivePeerDependencies ?? []) result.add(name)
  }
  return result
}

/** Every `[alias, reference]` edge of the importers and packages, importers first. */
function allResolvedEdges (lockfile: LockfileObject): Array<[string, string]> {
  return allResolvedDependencyMaps(lockfile).flatMap((dependencies) => Object.entries(dependencies))
}

function allResolvedDependencyMaps (lockfile: LockfileObject): ResolvedDependencies[] {
  return [
    ...Object.values(lockfile.importers).flatMap((importer) => [
      importer.dependencies,
      importer.devDependencies,
      importer.optionalDependencies,
    ]),
    ...Object.values(lockfile.packages ?? {}).flatMap((snapshot) => [
      snapshot.dependencies,
      snapshot.optionalDependencies,
    ]),
  ].filter((dependencies) => dependencies != null)
}

function pruneUnreachablePackages (
  importers: LockfileObject['importers'],
  packages: Record<DepPath, PackageSnapshot>
): Record<DepPath, PackageSnapshot> {
  const reachable = new Set<DepPath>()
  const queue: DepPath[] = []
  for (const importer of Object.values(importers)) {
    enqueueDependencies(importer.dependencies)
    enqueueDependencies(importer.devDependencies)
    enqueueDependencies(importer.optionalDependencies)
  }
  for (let index = 0; index < queue.length; index++) {
    const depPath = queue[index]
    const snapshot = packages[depPath]
    if (snapshot == null) continue
    enqueueDependencies(snapshot.dependencies)
    enqueueDependencies(snapshot.optionalDependencies)
  }
  return Object.fromEntries(
    Object.entries(packages).filter(([depPath]) => reachable.has(depPath as DepPath))
  ) as Record<DepPath, PackageSnapshot>

  function enqueueDependencies (dependencies: ResolvedDependencies | undefined): void {
    for (const [alias, reference] of Object.entries(dependencies ?? {})) {
      const depPath = dp.refToRelative(reference, alias)
      if (depPath == null || reachable.has(depPath)) continue
      reachable.add(depPath)
      queue.push(depPath)
    }
  }
}
