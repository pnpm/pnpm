import type {
  DedupeCheckIssues,
  ResolutionChangesByAlias,
  SnapshotsChanges,
} from '@pnpm/installing.dedupe.types'
import type { LockfileObject, ResolvedDependencies } from '@pnpm/lockfile.types'
import { DEPENDENCIES_FIELDS, type DepPath } from '@pnpm/types'

import { DedupeCheckIssuesError } from './DedupeCheckIssuesError.js'

const PACKAGE_SNAPSHOT_DEP_FIELDS = ['dependencies', 'optionalDependencies'] as const

// Dry-run diffs importers by both the per-group dependency fields and the
// flat `specifiers` map, so it catches every importer rewrite a real install
// would persist: the per-group fields surface a dependency moving between
// `dependencies`/`devDependencies`/`optionalDependencies`, while `specifiers`
// surfaces a specifier-only edit (same resolved version). `specifiers` is
// last so that on a per-alias collision its update wins — rendering the
// specifier delta rather than the re-resolved version, which is cleared in
// memory for a specifier-mismatched dep.
const IMPORTER_DRY_RUN_FIELDS = [...DEPENDENCIES_FIELDS, 'specifiers'] as const

/**
 * Compute the changes between two lockfiles, as added/removed/updated
 * importer and package snapshots. Unlike {@link dedupeDiffCheck} this never
 * throws — callers that only want to report the diff (e.g. `install
 * --dry-run`) consume the result directly.
 *
 * `includeImporterSpecifiers` also diffs each importer's direct-dependency
 * `specifier`s, not just their resolved versions. `pnpm install --dry-run`
 * sets it so a specifier-only manifest edit (which a real install would
 * persist to the lockfile) is reported; `dedupe --check` leaves it off
 * because a specifier change is irrelevant to deduplication.
 */
export function calcDedupeCheckIssues (
  prev: LockfileObject,
  next: LockfileObject,
  opts?: { includeImporterSpecifiers?: boolean }
): DedupeCheckIssues {
  const importerFields = opts?.includeImporterSpecifiers ? IMPORTER_DRY_RUN_FIELDS : DEPENDENCIES_FIELDS
  return {
    importerIssuesByImporterId: diffSnapshots(prev.importers, next.importers, importerFields),
    packageIssuesByDepPath: diffSnapshots(prev.packages ?? {}, next.packages ?? {}, PACKAGE_SNAPSHOT_DEP_FIELDS),
  }
}

export function countDedupeCheckIssues (issues: DedupeCheckIssues): number {
  return (
    countChangedSnapshots(issues.importerIssuesByImporterId) +
    countChangedSnapshots(issues.packageIssuesByDepPath)
  )
}

export function dedupeDiffCheck (prev: LockfileObject, next: LockfileObject): void {
  const issues = calcDedupeCheckIssues(prev, next)

  if (countDedupeCheckIssues(issues) > 0) {
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
  prev: Record<DepPath, TSnapshot>,
  next: Record<DepPath, TSnapshot>,
  fields: ReadonlyArray<PossiblyResolvedDependenciesKeys<TSnapshot>>
): SnapshotsChanges {
  const removed: string[] = []
  const updated: Record<string, ResolutionChangesByAlias> = {}

  for (const [id, prevSnapshot] of Object.entries(prev)) {
    const nextSnapshot = next[id as DepPath]

    if (nextSnapshot == null) {
      removed.push(id)
      continue
    }

    const updates: ResolutionChangesByAlias = {}
    for (const dependencyField of fields) {
      Object.assign(updates, getResolutionUpdates(prevSnapshot[dependencyField] ?? {}, nextSnapshot[dependencyField] ?? {}))
    }

    if (Object.keys(updates).length > 0) {
      updated[id] = updates
    }
  }

  const added = (Object.keys(next) as DepPath[]).filter(id => prev[id] == null)

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
