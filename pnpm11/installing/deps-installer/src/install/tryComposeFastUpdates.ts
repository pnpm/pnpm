import type { Catalogs } from '@pnpm/catalogs.types'
import type { VersionOverride } from '@pnpm/config.parse-overrides'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { LockfileObject } from '@pnpm/lockfile.types'
import type { WorkspacePackages } from '@pnpm/resolving.resolver-base'

import { type DroppedEdges, peerSuffixesAreIndependentOf } from './droppedEdges.js'
import { pruneUnreferencedCatalogEntries, tryFastUpdateCatalogs } from './tryFastUpdateCatalogs.js'
import { tryFastUpdateCatalogVersions } from './tryFastUpdateCatalogVersions.js'
import { tryFastUpdateIgnoredOptionalDependencies } from './tryFastUpdateIgnoredOptionalDependencies.js'
import { type Project, tryFastUpdateImporters } from './tryFastUpdateImporters.js'
import {
  type FastRewriteOptions,
  tryFastUpdateOverrides,
} from './tryFastUpdateOverrides.js'
import {
  everyConfiguredPatchIsApplied,
  type FastPatchedDependenciesUpdateOptions,
  tryFastUpdatePatchedDependencies,
} from './tryFastUpdatePatchedDependencies.js'
import {
  type FastSettingsUpdateOptions,
  tryFastUpdateSettings,
} from './tryFastUpdateSettings.js'

/**
 * What the fast-update handlers did to the dependency graph, so the
 * maintenance that keeps the lockfile consistent — pruning, the peer-suffix
 * safety check, catalog entry pruning, and the `optional` flag recompute —
 * runs once over the combined result instead of once per handler.
 */
export interface GraphEdits {
  /** What the severed importer or package edges pointed at. */
  dropped: DroppedEdges
  /**
   * Whether an edge was added or moved into or out of `optionalDependencies`,
   * which changes the reachability-derived `optional` flags for a subtree.
   */
  optionalFlagsAreStale: boolean
}

/** The drift dimensions the composed handlers can absorb. */
export interface FastUpdateDrift {
  importers?: boolean
  ignoredOptionalDependencies?: boolean
  patchedDependencies?: boolean
  settings?: boolean
  catalogs?: boolean
  overrides?: boolean
}

export type FastCatalogsUpdateOptions = FastRewriteOptions & {
  catalogs: Catalogs
  overrides: Record<string, string>
  parsedOverrides: VersionOverride[]
}

export type FastOverridesUpdateOptions = FastRewriteOptions & {
  overrides: Record<string, string>
  parsedOverrides: VersionOverride[]
}

export interface ComposeFastUpdatesOptions {
  drift: FastUpdateDrift
  projects: Project[]
  workspacePackages: WorkspacePackages
  /**
   * Whether `resolutionMode` resolves a direct dependency to its lowest
   * satisfying version rather than its highest.
   */
  resolutionPicksLowest: boolean
  /** Whether importers of removed projects are dropped from the lockfile. */
  pruneLockfileImporters?: boolean
  ignoredOptionalDependencies?: string[]
  patchedDependencies?: FastPatchedDependenciesUpdateOptions
  settings?: FastSettingsUpdateOptions
  catalogs?: FastCatalogsUpdateOptions
  overrides?: FastOverridesUpdateOptions
}

/**
 * Rewrite `candidate` in place of a full resolution, composing every handler
 * with absorbable drift onto the one candidate — no handler requires being
 * the only change. Removals apply first, then the shared graph epilogue
 * (`finishGraphEdits`), then patch rekeying — after the prune, so its guards
 * see the packages a full resolution would see — the settings block, and the
 * two resolver-consulting rewrites last: catalogs before overrides, the order
 * a resolution applies them in. What survives is then held to
 * `everyConfiguredPatchIsApplied`.
 *
 * `false` — drift some handler cannot express — leaves the caller on the
 * full-resolution path; `candidate` may be partially rewritten by then, which
 * the coordinator's discard-on-failure makes safe.
 */
export async function tryComposeFastUpdates (
  candidate: LockfileObject,
  opts: ComposeFastUpdatesOptions
): Promise<boolean> {
  const edits: GraphEdits = { dropped: new Set(), optionalFlagsAreStale: false }
  if (opts.drift.importers && !tryFastUpdateImporters(candidate, {
    projects: opts.projects,
    pruneLockfileImporters: opts.pruneLockfileImporters ?? false,
    workspacePackages: opts.workspacePackages,
    resolutionPicksLowest: opts.resolutionPicksLowest,
  }, edits)) {
    return false
  }
  if (opts.drift.ignoredOptionalDependencies) {
    if (!tryFastUpdateIgnoredOptionalDependencies(candidate, opts.ignoredOptionalDependencies ?? [], edits)) {
      return false
    }
  }
  if (!finishGraphEdits(candidate, edits)) return false
  if (opts.drift.patchedDependencies && !tryFastUpdatePatchedDependencies(candidate, opts.patchedDependencies!)) {
    return false
  }
  if (opts.drift.settings && !tryFastUpdateSettings(candidate, opts.settings!)) {
    return false
  }
  if (opts.drift.catalogs && !await applyCatalogsUpdate(candidate, opts.catalogs!)) {
    return false
  }
  if (opts.drift.overrides && !await tryFastUpdateOverrides(candidate, opts.overrides!)) {
    return false
  }
  if (opts.patchedDependencies != null &&
    !everyConfiguredPatchIsApplied(candidate, opts.patchedDependencies)) {
    return false
  }
  return true
}

/**
 * Move the catalog entries the configuration changed, taking the range-only
 * rewrite first so the entries it settles never reach the resolver.
 */
async function applyCatalogsUpdate (
  candidate: LockfileObject,
  opts: FastCatalogsUpdateOptions
): Promise<boolean> {
  // The range-only rewrite keeps the entries it cannot handle, so a mixed
  // update still leaves an exact move for the rewrite below.
  const rewroteRanges = tryFastUpdateCatalogs(candidate, {
    catalogs: opts.catalogs,
    overrides: opts.overrides,
  })
  const rewrite = await tryFastUpdateCatalogVersions(candidate, opts)
  // Only the "nothing moved" outcome may fall back on the range-only result.
  // A move this cannot express has to reach the resolver, even though the
  // range-only rewrite succeeded.
  if (rewrite === 'applied') return true
  return rewrite === 'nothing-to-move' && rewroteRanges
}

/**
 * Settle the graph after every handler has run. The prune also recomputes
 * each package's `optional` flag from what still reaches it. `false` — a
 * surviving peer suffix embeds a dropped package, so its key would need a
 * rewrite, not a prune — leaves the caller on the full-resolution path.
 */
function finishGraphEdits (candidate: LockfileObject, edits: GraphEdits): boolean {
  if (edits.dropped.size > 0 || edits.optionalFlagsAreStale) {
    const pruned = pruneSharedLockfile(candidate)
    if (pruned.packages == null) {
      delete candidate.packages
    } else {
      candidate.packages = pruned.packages
    }
  }
  if (edits.dropped.size > 0) {
    // Pruned first: a peer-dependent package that the removals themselves
    // make unreachable needs no rekeying, so only the survivors are checked.
    if (!peerSuffixesAreIndependentOf(candidate, edits.dropped)) return false
    pruneUnreferencedCatalogEntries(candidate)
  }
  return true
}
