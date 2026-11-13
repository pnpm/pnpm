import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { LockfileObject, ResolvedDependencies } from '@pnpm/lockfile.types'
import { EXISTING_VERSION_SELECTOR_WEIGHT, pinnedByLockfile, type PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { ProjectId, ProjectManifest } from '@pnpm/types'
import semver from 'semver'

import { getPinnedNameVer } from './resolveDependencies.js'

type ManifestDependencies = Pick<ProjectManifest, 'dependencies' | 'devDependencies' | 'optionalDependencies' | 'peerDependencies'>

export interface DirectDependencySpecs {
  ranges: Map<string, Set<string>>
  rangePins: Map<string, Set<string>>
}

export function collectDirectDependencySpecs (
  importers: Array<{ id: ProjectId, manifest: ManifestDependencies }>,
  catalogs: Catalogs,
  lockfile: LockfileObject
): DirectDependencySpecs {
  const peerAliases = new Set(importers.flatMap(({ manifest }) => Object.keys(manifest.peerDependencies ?? {})
    .filter((alias) => manifest.dependencies?.[alias] == null && manifest.devDependencies?.[alias] == null && manifest.optionalDependencies?.[alias] == null)))
  const specs = { ranges: new Map<string, Set<string>>(), rangePins: new Map<string, Set<string>>() }
  for (const { id, manifest } of importers) {
    const snapshot = lockfile.importers[id]
    for (const deps of [manifest.dependencies, manifest.devDependencies, manifest.optionalDependencies]) {
      addSemverSpecs(specs, { catalogs, deps, lockfile, snapshot, peerAliases })
    }
  }
  return specs
}

function addSemverSpecs (
  directSpecs: DirectDependencySpecs,
  opts: { catalogs: Catalogs, deps: Record<string, string> | undefined, lockfile: LockfileObject, snapshot: LockfileObject['importers'][ProjectId] | undefined, peerAliases: Set<string> }
): void {
  for (const [alias, bareSpecifier] of Object.entries(opts.deps ?? {})) {
    const catalogLookup = resolveFromCatalog(opts.catalogs, { alias, bareSpecifier })
    const spec = catalogLookup.type === 'found' ? catalogLookup.resolution.specifier : bareSpecifier
    if (semver.validRange(spec) == null) continue
    if (opts.peerAliases.has(alias)) {
      const reference = opts.snapshot?.optionalDependencies?.[alias] ?? opts.snapshot?.devDependencies?.[alias] ?? opts.snapshot?.dependencies?.[alias]
      recordRangePin(directSpecs.rangePins, { alias, spec, lockfile: opts.lockfile, reference })
    }
    const specs = directSpecs.ranges.get(alias)
    if (specs == null) {
      directSpecs.ranges.set(alias, new Set([spec]))
    } else {
      specs.add(spec)
    }
  }
}

function recordRangePin (
  rangePins: Map<string, Set<string>>,
  opts: { alias: string, spec: string, lockfile: LockfileObject, reference: string | undefined }
): void {
  if (semver.clean(opts.spec) != null || !opts.reference) return
  const pinned = getPinnedNameVer(opts.lockfile, opts.reference, opts.alias)
  if (!pinned || !semver.satisfies(pinned.version, opts.spec, true)) return
  const pins = rangePins.get(opts.alias) ?? new Set<string>()
  pins.add(pinned.version)
  rangePins.set(opts.alias, pins)
}

/**
 * Returns, by alias, the locked versions of the importer's auto-installed
 * peers (peer dependencies it does not also declare as a dependency) that a
 * workspace project's specifier for that package rejects, although the
 * specifier overlaps the peer range. Such a peer has to be re-resolved, or it
 * stays on its first resolution while the projects that provide it move on
 * (pnpm/pnpm#11800, pnpm/pnpm#17058). A current range pin stays valid unless an
 * overlapping exact specifier names another version.
 */
export function findStalePeerPins (
  resolvedDependencies: ResolvedDependencies,
  opts: {
    directSpecsByName: Map<string, Set<string>>
    rangePins: Map<string, Set<string>>
    lockfile: LockfileObject
    manifest: ProjectManifest
  }
): Map<string, string> {
  const { manifest } = opts
  const stale = new Map<string, string>()
  for (const [alias, peerRange] of Object.entries(manifest.peerDependencies ?? {})) {
    if (!isAutoInstalledPeerWithRange({ alias, manifest, peerRange, resolvedDependencies })) continue
    const directSpecs = opts.directSpecsByName.get(alias)
    if (directSpecs == null) continue
    const overlappingSpecs = [...directSpecs].filter((spec) => semver.intersects(spec, peerRange))
    if (!overlappingSpecs.length) continue
    const pinned = getPinnedNameVer(opts.lockfile, resolvedDependencies[alias], alias)
    if (pinned != null && isStalePeerPin(pinned.version, { alias, peerRange, rangePins: opts.rangePins, overlappingSpecs })) {
      stale.set(alias, pinned.version)
    }
  }
  return stale
}

function isStalePeerPin (
  version: string,
  opts: { alias: string, peerRange: string, rangePins: Map<string, Set<string>>, overlappingSpecs: string[] }
): boolean {
  if (opts.rangePins.get(opts.alias)?.has(version)) {
    const exactProviderChanged = opts.overlappingSpecs.some((spec) => {
      const exact = semver.clean(spec)
      return exact != null && exact !== version && semver.satisfies(exact, opts.peerRange, true)
    })
    if (!exactProviderChanged) return false
  }
  return opts.overlappingSpecs.some((spec) => !semver.satisfies(version, spec, true))
}

function isAutoInstalledPeerWithRange (
  opts: {
    alias: string
    manifest: ProjectManifest
    peerRange: string
    resolvedDependencies: ResolvedDependencies
  }
): boolean {
  const { alias, manifest } = opts
  return Object.hasOwn(opts.resolvedDependencies, alias) &&
    manifest.dependencies?.[alias] == null &&
    manifest.devDependencies?.[alias] == null &&
    manifest.optionalDependencies?.[alias] == null &&
    semver.validRange(opts.peerRange) != null
}

/**
 * Drops the given peers from the importer's locked dependencies and the
 * lockfile's weight from their locked versions in its preferred versions, so
 * the peers resolve the way a fresh install resolves them. Returns copies and
 * leaves the inputs unchanged.
 *
 * The versions a project's specifier names exactly lose the lockfile's weight
 * too. Their specifier already weights them, and the lockfile's weight would
 * outrank a higher version that only a new specifier names.
 */
export function releaseStalePeerPins (
  stalePeerPins: Map<string, string>,
  opts: {
    directSpecsByName: Map<string, Set<string>>
    preferredVersions: PreferredVersions
    resolvedDependencies: ResolvedDependencies
  }
): { preferredVersions: PreferredVersions, resolvedDependencies: ResolvedDependencies } {
  const resolvedDependencies = { ...opts.resolvedDependencies }
  // Null-prototype: keyed by package names from manifests and the lockfile.
  const preferredVersions: PreferredVersions = Object.assign(Object.create(null), opts.preferredVersions)
  for (const [alias, pinnedVersion] of stalePeerPins) {
    delete resolvedDependencies[alias]
    if (opts.preferredVersions[alias] == null) continue
    const selectors = Object.assign(Object.create(null), opts.preferredVersions[alias])
    const namedVersions = [...opts.directSpecsByName.get(alias) ?? []]
      .map((spec) => semver.clean(spec))
      .filter((version) => version != null)
    for (const version of [pinnedVersion, ...namedVersions]) {
      removeLockfileWeight(selectors, version)
    }
    preferredVersions[alias] = selectors
  }
  return { preferredVersions, resolvedDependencies }
}

function removeLockfileWeight (selectors: PreferredVersions[string], version: string): void {
  const selector = selectors[version]
  if (typeof selector !== 'object' || selector.selectorType !== 'version' || !pinnedByLockfile(selector.weight)) return
  const manifestWeight = selector.weight - EXISTING_VERSION_SELECTOR_WEIGHT
  if (manifestWeight > 0) {
    selectors[version] = { selectorType: 'version', weight: manifestWeight }
  } else {
    delete selectors[version]
  }
}
