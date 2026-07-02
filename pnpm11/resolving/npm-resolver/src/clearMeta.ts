import type { PackageMeta } from '@pnpm/resolving.registry.types'
import { pick } from 'ramda'

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
  const versions: PackageMeta['versions'] = {}
  for (const [version, info] of Object.entries(pkg.versions ?? {})) {
    // The list taken from https://github.com/npm/registry/blob/master/docs/responses/package-metadata.md#abbreviated-version-object
    // with the addition of 'libc'
    versions[version] = pick([
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
    ], info)
  }

  return {
    name: pkg.name,
    'dist-tags': pkg['dist-tags'],
    versions,
    time: pkg.time,
    modified: pkg.modified,
  }
}
