import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { LockfileObject, ResolvedDependencies } from '@pnpm/lockfile.types'
import { EXISTING_VERSION_SELECTOR_WEIGHT, type PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { ProjectManifest } from '@pnpm/types'
import semver from 'semver'

import { getPinnedNameVer } from './resolveDependencies.js'

type ManifestDependencies = Pick<ProjectManifest, 'dependencies' | 'devDependencies' | 'optionalDependencies'>

/**
 * Collects the distinct semver ranges the given projects declare for each
 * package in their dependencies, devDependencies, and optionalDependencies,
 * with `catalog:` specifiers replaced by the catalog entry. Specifiers that
 * are not semver ranges are left out.
 */
export function collectDirectDependencySpecs (
  manifests: ManifestDependencies[],
  catalogs: Catalogs
): Map<string, Set<string>> {
  const specsByName = new Map<string, Set<string>>()
  for (const manifest of manifests) {
    for (const deps of [manifest.dependencies, manifest.devDependencies, manifest.optionalDependencies]) {
      for (const [alias, bareSpecifier] of Object.entries(deps ?? {})) {
        const catalogLookup = resolveFromCatalog(catalogs, { alias, bareSpecifier })
        const spec = catalogLookup.type === 'found' ? catalogLookup.resolution.specifier : bareSpecifier
        if (semver.validRange(spec) == null) continue
        const specs = specsByName.get(alias)
        if (specs == null) {
          specsByName.set(alias, new Set([spec]))
        } else {
          specs.add(spec)
        }
      }
    }
  }
  return specsByName
}

/**
 * Returns, by alias, the locked versions of the importer's auto-installed
 * peers (peer dependencies it does not also declare as a dependency) that no
 * workspace project's specifier for that package accepts any more, although
 * one of them still overlaps the peer range. Such a peer has to be re-resolved, or it
 * stays on its first resolution while the projects that provide it move on
 * (pnpm/pnpm#11800).
 */
export function findStalePeerPins (
  resolvedDependencies: ResolvedDependencies,
  opts: {
    directSpecsByName: Map<string, Set<string>>
    lockfile: LockfileObject
    manifest: ProjectManifest
  }
): Map<string, string> {
  const { manifest } = opts
  const stale = new Map<string, string>()
  for (const [alias, peerRange] of Object.entries(manifest.peerDependencies ?? {})) {
    if (
      !Object.hasOwn(resolvedDependencies, alias) ||
      manifest.dependencies?.[alias] != null ||
      manifest.devDependencies?.[alias] != null ||
      manifest.optionalDependencies?.[alias] != null ||
      semver.validRange(peerRange) == null
    ) continue
    const directSpecs = opts.directSpecsByName.get(alias)
    if (directSpecs == null) continue
    const overlappingSpecs = [...directSpecs].filter((spec) => semver.intersects(spec, peerRange))
    if (!overlappingSpecs.length) continue
    const pinned = getPinnedNameVer(opts.lockfile, resolvedDependencies[alias], alias)
    if (pinned != null && !overlappingSpecs.some((spec) => semver.satisfies(pinned.version, spec, true))) {
      stale.set(alias, pinned.version)
    }
  }
  return stale
}

/**
 * Drops the given peers from the importer's locked dependencies and the
 * lockfile's weight from their locked versions in its preferred versions, so
 * the peers resolve the way a fresh install resolves them. Returns copies and
 * leaves the inputs unchanged.
 */
export function releaseStalePeerPins (
  stalePeerPins: Map<string, string>,
  opts: {
    preferredVersions: PreferredVersions
    resolvedDependencies: ResolvedDependencies
  }
): { preferredVersions: PreferredVersions, resolvedDependencies: ResolvedDependencies } {
  const resolvedDependencies = { ...opts.resolvedDependencies }
  // Null-prototype: keyed by package names from manifests and the lockfile.
  const preferredVersions: PreferredVersions = Object.assign(Object.create(null), opts.preferredVersions)
  for (const [alias, pinnedVersion] of stalePeerPins) {
    delete resolvedDependencies[alias]
    const selector = opts.preferredVersions[alias]?.[pinnedVersion]
    if (typeof selector !== 'object' || selector.selectorType !== 'version' || selector.weight < EXISTING_VERSION_SELECTOR_WEIGHT) continue
    const selectors = Object.assign(Object.create(null), opts.preferredVersions[alias])
    const manifestWeight = selector.weight - EXISTING_VERSION_SELECTOR_WEIGHT
    if (manifestWeight > 0) {
      selectors[pinnedVersion] = { selectorType: 'version', weight: manifestWeight }
    } else {
      delete selectors[pinnedVersion]
    }
    preferredVersions[alias] = selectors
  }
  return { preferredVersions, resolvedDependencies }
}
