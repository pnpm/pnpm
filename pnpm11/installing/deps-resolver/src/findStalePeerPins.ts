import { resolveFromCatalog } from '@pnpm/catalogs.resolver'
import type { Catalogs } from '@pnpm/catalogs.types'
import type { LockfileObject, ResolvedDependencies } from '@pnpm/lockfile.types'
import type { ProjectManifest } from '@pnpm/types'
import semver from 'semver'

import { getPinnedNameVer } from './resolveDependencies.js'

type ManifestDependencies = Pick<ProjectManifest, 'dependencies' | 'devDependencies' | 'optionalDependencies'>

/**
 * Collects the specifiers the given projects declare for each package in
 * their dependencies, devDependencies, and optionalDependencies, with
 * `catalog:` specifiers replaced by the catalog entry.
 */
export function collectDirectDependencySpecs (
  manifests: ManifestDependencies[],
  catalogs: Catalogs
): Map<string, string[]> {
  const specsByName = new Map<string, string[]>()
  for (const manifest of manifests) {
    for (const deps of [manifest.dependencies, manifest.devDependencies, manifest.optionalDependencies]) {
      for (const [alias, bareSpecifier] of Object.entries(deps ?? {})) {
        const catalogLookup = resolveFromCatalog(catalogs, { alias, bareSpecifier })
        const spec = catalogLookup.type === 'found' ? catalogLookup.resolution.specifier : bareSpecifier
        const specs = specsByName.get(alias)
        if (specs == null) {
          specsByName.set(alias, [spec])
        } else {
          specs.push(spec)
        }
      }
    }
  }
  return specsByName
}

/**
 * Returns the auto-installed peers of an importer (peer dependencies it does
 * not also declare as a dependency) whose locked version no workspace
 * project's specifier for that package accepts any more, although one of them
 * still overlaps the peer range. Such a peer has to be re-resolved, or it
 * stays on its first resolution while the projects that provide it move on
 * (pnpm/pnpm#11800).
 */
export function findStalePeerPins (
  resolvedDependencies: ResolvedDependencies,
  opts: {
    directSpecsByName: Map<string, string[]>
    lockfile: LockfileObject
    manifest: ProjectManifest
  }
): Set<string> {
  const { manifest } = opts
  const stale = new Set<string>()
  for (const [alias, peerRange] of Object.entries(manifest.peerDependencies ?? {})) {
    if (
      !Object.hasOwn(resolvedDependencies, alias) ||
      manifest.dependencies?.[alias] != null ||
      manifest.devDependencies?.[alias] != null ||
      manifest.optionalDependencies?.[alias] != null ||
      semver.validRange(peerRange) == null
    ) continue
    const overlappingSpecs = opts.directSpecsByName.get(alias)?.filter((spec) =>
      semver.validRange(spec) != null && semver.intersects(spec, peerRange)
    )
    if (!overlappingSpecs?.length) continue
    const pinned = getPinnedNameVer(opts.lockfile, resolvedDependencies[alias], alias)
    if (pinned != null && !overlappingSpecs.some((spec) => semver.satisfies(pinned.version, spec, true))) {
      stale.add(alias)
    }
  }
  return stale
}
