import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { LockfileObject } from '@pnpm/lockfile.types'

import { type DroppedEdges, peerSuffixesAreIndependentOf } from './droppedEdges.js'
import { pruneUnreferencedCatalogEntries } from './tryFastUpdateCatalogs.js'
import { tryFastUpdateIgnoredOptionalDependencies } from './tryFastUpdateIgnoredOptionalDependencies.js'
import { type Project, tryFastUpdateImporters } from './tryFastUpdateImporters.js'
import {
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
   * Whether an edge into or out of `optionalDependencies` moved, which
   * changes the reachability-derived `optional` flags for a subtree.
   */
  movedAcrossOptional: boolean
}

/** The drift dimensions the composed sync handlers can absorb. */
export interface FastUpdateDrift {
  importers?: boolean
  ignoredOptionalDependencies?: boolean
  patchedDependencies?: boolean
  settings?: boolean
}

export interface ComposeFastUpdatesOptions {
  drift: FastUpdateDrift
  projects: Project[]
  /** Whether importers of removed projects are dropped from the lockfile. */
  pruneLockfileImporters?: boolean
  ignoredOptionalDependencies?: string[]
  patchedDependencies?: FastPatchedDependenciesUpdateOptions
  settings?: FastSettingsUpdateOptions
}

/**
 * Rewrite `candidate` in place of a full resolution, composing every handler
 * with absorbable drift onto the one candidate — no handler requires being
 * the only change. Removals apply first, then the shared graph epilogue
 * (`finishGraphEdits`), then patch rekeying — after the prune, so its guards
 * see the packages a full resolution would see — and the settings block last.
 *
 * `false` — drift some handler cannot express — leaves the caller on the
 * full-resolution path; `candidate` may be partially rewritten by then, which
 * the coordinator's discard-on-failure makes safe.
 */
export function tryComposeFastUpdates (
  candidate: LockfileObject,
  opts: ComposeFastUpdatesOptions
): boolean {
  const edits: GraphEdits = { dropped: new Set(), movedAcrossOptional: false }
  if (opts.drift.importers && !tryFastUpdateImporters(candidate, {
    projects: opts.projects,
    pruneLockfileImporters: opts.pruneLockfileImporters ?? false,
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
  return true
}

/**
 * Settle the graph after every handler has run. The prune also recomputes
 * each package's `optional` flag from what still reaches it. `false` — a
 * surviving peer suffix embeds a dropped package, so its key would need a
 * rewrite, not a prune — leaves the caller on the full-resolution path.
 */
function finishGraphEdits (candidate: LockfileObject, edits: GraphEdits): boolean {
  if (edits.dropped.size > 0 || edits.movedAcrossOptional) {
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
