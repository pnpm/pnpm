import { createMatcher } from '@pnpm/config.matcher'
import type {
  LockfileObject,
  ProjectSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile.types'

import { recordDroppedEdge } from './droppedEdges.js'
import type { GraphEdits } from './tryComposeFastUpdates.js'

/**
 * Remove every optional dependency a widened `ignoredOptionalDependencies`
 * now ignores, from importers and packages alike, recording the severed
 * edges in `edits` for the shared epilogue. `false` — a narrowed list, a
 * new exclusion, or a previous configuration that ignores by default — leaves
 * the caller on the full-resolution path, because only a resolution can bring
 * packages back.
 */
export function tryFastUpdateIgnoredOptionalDependencies (
  lockfile: LockfileObject,
  ignoredOptionalDependencies: string[],
  edits: GraphEdits
): boolean {
  const previous = new Set(lockfile.ignoredOptionalDependencies ?? [])
  const current = new Set(ignoredOptionalDependencies)
  const addedPatterns = [...current].filter((pattern) => !previous.has(pattern))
  const previousIgnoresByDefault = previous.size > 0 && [...previous].every((pattern) => pattern.startsWith('!'))
  if (
    previous.size === current.size ||
    [...previous].some((pattern) => !current.has(pattern)) ||
    addedPatterns.some((pattern) => pattern.startsWith('!')) ||
    previousIgnoresByDefault
  ) {
    return false
  }

  const isIgnored = createMatcher(ignoredOptionalDependencies)
  for (const importer of Object.values(lockfile.importers)) {
    removeIgnoredOptionalDependencies(importer, isIgnored, edits)
  }
  for (const snapshot of Object.values(lockfile.packages ?? {})) {
    removeIgnoredOptionalDependencies(snapshot, isIgnored, edits)
  }
  lockfile.ignoredOptionalDependencies = [...current].sort()
  return true
}

function removeIgnoredOptionalDependencies (
  snapshot: Pick<ProjectSnapshot, 'dependencies' | 'optionalDependencies'> & {
    specifiers?: ResolvedDependencies
  },
  isIgnored: (dependency: string) => boolean,
  edits: GraphEdits
): void {
  const removed = Object.keys(snapshot.optionalDependencies ?? {}).filter(isIgnored)
  for (const dependency of removed) {
    for (const references of [snapshot.optionalDependencies, snapshot.dependencies]) {
      if (references?.[dependency] == null) continue
      recordDroppedEdge(edits.dropped, dependency, references[dependency])
      delete references[dependency]
    }
    delete snapshot.specifiers?.[dependency]
  }
  if (snapshot.optionalDependencies != null && Object.keys(snapshot.optionalDependencies).length === 0) {
    delete snapshot.optionalDependencies
  }
  if (snapshot.dependencies != null && Object.keys(snapshot.dependencies).length === 0) {
    delete snapshot.dependencies
  }
}
