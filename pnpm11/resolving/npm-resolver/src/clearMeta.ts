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
 * Memoizes {@link clearMeta} by input identity. Several layers condense the
 * same parsed document — the settled-fetch memo (`memoizeFetchMetadata`), the
 * resolver's in-memory cache writes, and the `filterMetadata` disk writes —
 * and without the memo each would pin its own condensed copy for the rest of
 * the resolution phase, doubling the very footprint condensing exists to cut.
 * Entries die with their input document, so the map cannot leak. Outputs map
 * to themselves, making a repeat condense of an already-condensed document
 * the identity instead of another copy.
 */
const condensedMetas = new WeakMap<PackageMeta, PackageMeta>()

/**
 * Reduces a package metadata document to the abbreviated field set that the
 * resolver actually reads, dropping install-irrelevant fields (scripts,
 * exports, readme, custom `_`-prefixed fields, etc.). A full document can run
 * to tens of MB parsed; retaining unstripped documents for every package of a
 * large workspace is what drove installs out of memory in
 * https://github.com/pnpm/pnpm/issues/8441.
 *
 * Used in three places:
 * - The network layer (`fetch.ts`) normalizes a registry that ignored the
 *   abbreviated `Accept` header and returned a full document.
 * - The resolver (`pickPackage.ts`) narrows every packument it retains in
 *   memory — full documents fetched for optional dependencies (`libc`) or
 *   release-age `time` upgrades, and disk-mirror loads — unless the resolver
 *   serves full-metadata consumers (see `retainsFullMeta`).
 * - The settled-fetch memo (`memoizeFetchMetadata.ts`) condenses results it
 *   retains after settlement.
 *
 * Returns the same condensed object for the same input (see
 * {@link condensedMetas}), so callers may compare by identity to detect
 * whether condensing changed anything. `etag` is carried over when the input
 * has one (disk-loaded documents), so a condensed document can still answer
 * conditional-request headers.
 *
 * Null-safe on `versions` so it can be called on an unpublished package (no
 * versions), which the abbreviated path can reach.
 */
export function clearMeta (pkg: PackageMeta): PackageMeta {
  const memoized = condensedMetas.get(pkg)
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
  condensedMetas.set(pkg, condensed)
  condensedMetas.set(condensed, condensed)
  return condensed
}

/**
 * Whether a resolver keeps full packuments in memory rather than condensing
 * them with {@link clearMeta}. Only resolvers created with `fullMetadata` and
 * without `filterMetadata` qualify — their consumers read fields outside the
 * abbreviated set (e.g. `pnpm outdated --long` shows description/homepage).
 * Everything else — including a plain install that fetches full documents
 * only for optional dependencies or release-age upgrades — retains the
 * condensed form.
 */
export function retainsFullMeta (opts: { fullMetadata?: boolean, filterMetadata?: boolean }): boolean {
  return opts.fullMetadata === true && opts.filterMetadata !== true
}
