import * as dp from '@pnpm/deps.path'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type { LockfileObject } from '@pnpm/lockfile.types'

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
  /** Aliases whose importer or package edges were severed. */
  dropped: Set<string>
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
  ignoredOptionalDependencies?: string[]
  patchedDependencies?: FastPatchedDependenciesUpdateOptions
  settings?: FastSettingsUpdateOptions
}

/**
 * Rewrite `candidate` in place of a full resolution for the drift the
 * lockfile itself proves is safe to absorb, composing every applicable
 * handler onto the one candidate: manifest drift (compatible range changes,
 * dependency group moves, removals), a widened `ignoredOptionalDependencies`,
 * a changed `patchedDependencies`, and a setting change that cannot affect
 * the recorded graph. Drift that spans several of those dimensions at once is
 * absorbed in one pass — no handler requires being the only change.
 *
 * The handlers run in a fixed order. Removals land first (manifest drift,
 * then newly ignored optional dependencies), then the shared epilogue prunes
 * what nothing reaches and refuses candidates whose surviving peer suffixes
 * embed a dropped package. Patches rekey after that, so the unused-patch
 * guard and the rekey plan see the packages a full resolution would see. The
 * settings block, which touches no graph, is recorded last.
 *
 * `false` — drift some handler cannot express — leaves the caller on the
 * full-resolution path; `candidate` may be partially rewritten by then, which
 * the coordinator's discard-on-failure makes safe. The caller still validates
 * the candidate with the freshness gates before committing.
 */
export function tryComposeFastUpdates (
  candidate: LockfileObject,
  opts: ComposeFastUpdatesOptions
): boolean {
  const edits: GraphEdits = { dropped: new Set(), movedAcrossOptional: false }
  if (opts.drift.importers && !tryFastUpdateImporters(candidate, opts.projects, edits)) {
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

/**
 * Whether no surviving package resolves a peer through one of `dropped`.
 *
 * A dropped package that some package reaches as a peer is embedded in that
 * package's key, so removing it would rekey the dependent rather than only
 * prune. A peer suffix pnpm shortened into a hash cannot be read to rule that
 * out.
 */
function peerSuffixesAreIndependentOf (lockfile: LockfileObject, dropped: Set<string>): boolean {
  return Object.keys(lockfile.packages ?? {}).every((depPath) => {
    const { peersIndex } = dp.indexOfDepPathSuffix(depPath)
    if (peersIndex === -1) return true
    const peers = depPath.substring(peersIndex)
    return peers
      .replace(/^\(/, '')
      .replace(/\)$/, '')
      .split(')(')
      .every((segment) => segment.includes('@')) &&
      ![...dropped].some((alias) => peers.includes(`${alias}@`))
  })
}
