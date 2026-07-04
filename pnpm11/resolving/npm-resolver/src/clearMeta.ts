import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { pick } from 'ramda'

// The list taken from https://github.com/npm/registry/blob/master/docs/responses/package-metadata.md#abbreviated-version-object
// with the addition of 'libc'
const ABBREVIATED_VERSION_FIELDS = [
  'name',
  'version',
  'bin',
  'directories',
  'devDependencies',
  'optionalDependencies',
  'dependencies',
  'peerDependencies',
  'dist',
  'engines',
  'peerDependenciesMeta',
  'cpu',
  'os',
  'libc',
  'deprecated',
  'bundleDependencies',
  'bundledDependencies',
  'hasInstallScript',
  '_npmUser',
] as const

/**
 * Reduces a package metadata document to the abbreviated field set that the
 * resolver actually reads, dropping install-irrelevant fields (scripts,
 * exports, readme, custom `_`-prefixed fields, etc.).
 *
 * Used in two places:
 * - The network layer (`fetch.ts`) normalizes a registry that ignored the
 *   abbreviated `Accept` header and returned a full document.
 * - The resolver (`pickPackage.ts`) narrows a deliberately-fetched full
 *   document into the `filterMetadata` cache slot.
 *
 * Null-safe on `versions` so it can be called on an unpublished package (no
 * versions), which the abbreviated path can reach.
 */
export function clearMeta (pkg: PackageMeta): PackageMeta {
  // A null prototype so that a registry-controlled version key named
  // `__proto__` becomes a regular own property instead of mutating the
  // prototype of the map (js/prototype-polluting-assignment).
  const versions: PackageMeta['versions'] = Object.create(null)
  for (const [version, info] of Object.entries(pkg.versions ?? {})) {
    versions[version] = pick(ABBREVIATED_VERSION_FIELDS, info)
  }

  return {
    name: pkg.name,
    'dist-tags': pkg['dist-tags'],
    versions,
    time: pkg.time,
    modified: pkg.modified,
  }
}
