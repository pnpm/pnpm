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

// Memoized by input identity: several layers condense the same parsed
// document, and the WeakMap makes them share one condensed copy instead of
// pinning one each. Outputs map to themselves so re-condensing is the
// identity.
const condensedPackuments = new WeakMap<PackageMeta, PackageMeta>()

/**
 * Reduces a package metadata document to the abbreviated field set that the
 * resolver actually reads, dropping install-irrelevant fields (scripts,
 * exports, readme, custom `_`-prefixed fields, etc.). Retaining unstripped
 * full documents — tens of MB parsed for a popular package — is what drove
 * large installs out of memory (https://github.com/pnpm/pnpm/issues/8441).
 * `etag` is carried over so a condensed document can still answer
 * conditional-request headers.
 *
 * Null-safe on `versions` so it can be called on an unpublished package (no
 * versions), which the abbreviated path can reach.
 */
export function clearMeta (pkg: PackageMeta): PackageMeta {
  const memoized = condensedPackuments.get(pkg)
  if (memoized != null) return memoized

  // A null prototype so that a registry-controlled version key named
  // `__proto__` becomes a regular own property instead of mutating the
  // prototype of the map (js/prototype-polluting-assignment).
  const versions: PackageMeta['versions'] = Object.create(null)
  for (const [version, info] of Object.entries(pkg.versions ?? {})) {
    versions[version] = pick(ABBREVIATED_VERSION_FIELDS, info)
  }

  const condensed: PackageMeta = {
    name: pkg.name,
    'dist-tags': pkg['dist-tags'],
    versions,
    time: pkg.time,
    modified: pkg.modified,
  }
  if (pkg.etag != null) {
    condensed.etag = pkg.etag
  }
  condensedPackuments.set(pkg, condensed)
  condensedPackuments.set(condensed, condensed)
  return condensed
}

/**
 * Whether a resolver keeps full packuments rather than condensing them with
 * {@link clearMeta}: only `fullMetadata` without `filterMetadata`, whose
 * consumers read fields outside the abbreviated set (`pnpm outdated --long`
 * shows description/homepage).
 */
export function retainsFullMeta (opts: { fullMetadata?: boolean, filterMetadata?: boolean }): boolean {
  return opts.fullMetadata === true && opts.filterMetadata !== true
}
