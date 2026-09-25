import type { PackageMeta } from '@pnpm/resolving.registry.types'

/**
 * Drops `time` unless it carries a publish timestamp for every version the
 * packument lists.
 *
 * Registries may answer with a partial map: npmmirror adds `time` to its
 * abbreviated documents but fills it in only for the versions it has synced
 * since it started recording publish times, leaving the rest out. A partial
 * map is indistinguishable from a complete one at the point of use, so the
 * `minimumReleaseAge` filter reads every absent timestamp as "not mature"
 * and silently drops the version — resolution then falls back to the lowest
 * match.
 *
 * A map that can't decide maturity is worth nothing to the resolver, so it
 * is normalized away where the document is parsed. Every packument past that
 * point then carries either a complete `time` or none at all — the shape the
 * npm registry's own abbreviated documents have, and the one the rest of the
 * resolver is written against.
 *
 * A packument with no versions keeps whatever `time` it has — there is
 * nothing for the map to be incomplete about — and a version whose entry is
 * an empty string counts as absent.
 */
export function dropIncompletePublishTimes (meta: PackageMeta): void {
  if (meta.time == null) return
  for (const version in meta.versions) {
    if (!Object.hasOwn(meta.versions, version)) continue
    if (!Object.hasOwn(meta.time, version) || !meta.time[version]) {
      delete meta.time
      return
    }
  }
}
