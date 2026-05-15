import * as dp from '@pnpm/deps.path'
import type { PackageSnapshot, PackageSnapshots } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'

/**
 * Normalize a package snapshot so its `resolution` field is always populated.
 *
 * In a workspace with `injectWorkspacePackages: true`, a workspace dep that has
 * peer deps which resolve differently per consumer gets multiple lockfile
 * entries: one base, plus one per peer-resolution variant. The variant entries
 * **intentionally** omit `resolution` because they inherit it from the base —
 * this is a legal shape produced by pnpm's own lockfile writer.
 *
 * Direct dereferences like `'directory' in pkgSnapshot.resolution` crash on
 * variant snapshots, since `'X' in undefined` throws before any `&&` can
 * short-circuit. Iteration sites in graph-builder, deploy, hoisted-graph,
 * sbom-collect, and tree-builder all hit this. Rather than guarding each
 * dereference, callers invoke this helper at the top of the loop body so
 * every downstream access sees a fully-formed snapshot.
 *
 * Resolution strategy:
 *
 *   1. **Inherit from base.** Strip the peer-variant suffix from `depPath` and
 *      look up the base entry. If it has a `resolution`, clone it onto the
 *      variant.
 *   2. **Synthesize from `file:` depPath.** If the base entry has been pruned
 *      (e.g. `turbo prune --docker` only keeps the variant referenced by the
 *      consumer), synthesize a directory resolution from the `file:` prefix
 *      in the depPath. This matches what pnpm's writer would have produced
 *      for the base entry: workspace deps with peer-resolution variants live
 *      under `packages/<name>` and serialize as `{ directory, type: 'directory' }`.
 *      Do not widen this to other `file:` shapes (`file:./local-tarball.tgz`
 *      etc.) — those reach a different code path.
 *   3. **Give up.** Return the input untouched. Callers can decide whether
 *      to throw, skip, or treat as a "broken lockfile" condition.
 */
export function inheritOrSynthesizeResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  packages: PackageSnapshots | undefined
): PackageSnapshot {
  if (pkgSnapshot.resolution != null) return pkgSnapshot
  const basePath = dp.removeSuffix(depPath as DepPath) as DepPath
  const baseSnapshot = packages?.[basePath]
  if (baseSnapshot?.resolution != null) {
    return { ...pkgSnapshot, resolution: baseSnapshot.resolution }
  }
  const nonSemverVersion = dp.parse(depPath).nonSemverVersion
  if (nonSemverVersion?.startsWith('file:')) {
    return {
      ...pkgSnapshot,
      resolution: { directory: nonSemverVersion.slice('file:'.length), type: 'directory' },
    }
  }
  return pkgSnapshot
}
