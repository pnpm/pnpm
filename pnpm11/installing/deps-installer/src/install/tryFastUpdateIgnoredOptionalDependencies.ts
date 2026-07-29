import { createMatcher } from '@pnpm/config.matcher'
import { pruneSharedLockfile } from '@pnpm/lockfile.pruner'
import type {
  LockfileObject,
  ProjectSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile.types'

export function tryFastUpdateIgnoredOptionalDependencies (
  lockfile: LockfileObject,
  ignoredOptionalDependencies: string[]
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
    removeIgnoredOptionalDependencies(importer, isIgnored)
  }
  for (const snapshot of Object.values(lockfile.packages ?? {})) {
    removeIgnoredOptionalDependencies(snapshot, isIgnored)
  }
  lockfile.ignoredOptionalDependencies = [...current].sort()

  const pruned = pruneSharedLockfile(lockfile)
  if (pruned.packages == null) {
    delete lockfile.packages
  } else {
    lockfile.packages = pruned.packages
  }
  return true
}

function removeIgnoredOptionalDependencies (
  snapshot: Pick<ProjectSnapshot, 'dependencies' | 'optionalDependencies'> & {
    specifiers?: ResolvedDependencies
  },
  isIgnored: (dependency: string) => boolean
): void {
  const removed = Object.keys(snapshot.optionalDependencies ?? {}).filter(isIgnored)
  for (const dependency of removed) {
    delete snapshot.optionalDependencies![dependency]
    delete snapshot.dependencies?.[dependency]
    delete snapshot.specifiers?.[dependency]
  }
  if (snapshot.optionalDependencies != null && Object.keys(snapshot.optionalDependencies).length === 0) {
    delete snapshot.optionalDependencies
  }
  if (snapshot.dependencies != null && Object.keys(snapshot.dependencies).length === 0) {
    delete snapshot.dependencies
  }
}
