import { type ResolvedDependencies, type Lockfile } from '@pnpm/lockfile-types'
import {
  type ResolutionChangesByAlias,
  type DedupeCheckIssues,
  type SnapshotsChanges,
} from '@pnpm/dedupe.types'
import { DEPENDENCIES_FIELDS } from '@pnpm/types'
import { DedupeCheckIssuesError } from './DedupeCheckIssuesError'

const PACKAGE_SNAPSHOT_DEP_FIELDS = ['dependencies', 'optionalDependencies'] as const

export function dedupeDiffCheck (prev: Lockfile, next: Lockfile): void {
  const issues: DedupeCheckIssues = {
    importerIssuesByImporterId: diffSnapshots(prev.importers, next.importers, DEPENDENCIES_FIELDS),
    packageIssuesByDepPath: diffSnapshots(prev.packages ?? {}, next.packages ?? {}, PACKAGE_SNAPSHOT_DEP_FIELDS),
  }

  const changesCount =
    countChangedSnapshots(issues.importerIssuesByImporterId) +
    countChangedSnapshots(issues.packageIssuesByDepPath)

  if (changesCount > 0) {
    throw new DedupeCheckIssuesError(issues)
  }
}

/**
 * Get all the keys of an object T where the value extends some type U.
 */
type KeysOfValue<T, U> = KeyValueMatch<T, keyof T, U>
type KeyValueMatch<T, K, U> = K extends keyof T
  ? T[K] extends U ? K : never
  : never

/**
 * Given a PackageSnapshot or ProjectSnapshot, returns the keys where values
 * match ResolvedDependencies.
 *
 * Unfortunately the ResolvedDependencies interface is just
 * Record<string,string> so this also matches the "engines" and "specifiers"
 * block.
 */
type PossiblyResolvedDependenciesKeys<TSnapshot> = KeysOfValue<TSnapshot, ResolvedDependencies | undefined>

function diffSnapshots<TSnapshot> (
  prev: Record<string, TSnapshot>,
  next: Record<string, TSnapshot>,
  fields: ReadonlyArray<PossiblyResolvedDependenciesKeys<TSnapshot>>
): SnapshotsChanges {
  const removed: string[] = []
  const updated: Record<string, ResolutionChangesByAlias> = {}

  for (const [id, prevSnapshot] of Object.entries(prev)) {
    const nextSnapshot = next[id]

    if (nextSnapshot == null) {
      removed.push(id)
      continue
    }

    const updates = fields.reduce((acc: ResolutionChangesByAlias, dependencyField) => ({
      ...acc,
      ...getResolutionUpdates(prevSnapshot[dependencyField] ?? {}, nextSnapshot[dependencyField] ?? {}),
    }), {})

    if (Object.keys(updates).length > 0) {
      updated[id] = updates
    }
  }

  const added = Object.keys(next).filter(id => prev[id] == null)

  return { added, removed, updated }
}

function getResolutionUpdates (prev: ResolvedDependencies, next: ResolvedDependencies): ResolutionChangesByAlias {
  const updates: ResolutionChangesByAlias = {}

  for (const [alias, prevResolution] of Object.entries(prev)) {
    const nextResolution = next[alias]

    if (prevResolution === nextResolution) {
      continue
    }

    updates[alias] = nextResolution == null
      ? { type: 'removed', prev: prevResolution }
      : { type: 'updated', prev: prevResolution, next: nextResolution }
  }

  const newAliases = Object.entries(next).filter(([alias]) => prev[alias] == null)
  for (const [alias, nextResolution] of newAliases) {
    updates[alias] = { type: 'added', next: nextResolution }
  }

  return updates
}

export function countChangedSnapshots (snapshotChanges: SnapshotsChanges): number {
  return snapshotChanges.added.length + snapshotChanges.removed.length + Object.keys(snapshotChanges.updated).length
}
