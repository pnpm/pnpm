import { globalInfo } from '@pnpm/logger'

export interface ImmaturePickCollector {
  /** Resolver-side callback wired into `opts.onImmaturePick`. */
  record: (pkg: { name: string, version: string }) => void
  versions: Set<string>
}

/**
 * Loose minimumReleaseAge mode (`minimumReleaseAgeStrict: false`) lets the
 * resolver install versions newer than the cutoff. This collector is what
 * the install opts thread down to the npm resolver via `onImmaturePick`,
 * so the install command can persist those picks to the workspace
 * manifest's `minimumReleaseAgeExclude` after the install succeeds — a
 * subsequent install (including one promoted to strict mode) then accepts
 * the same versions without prompting the user to manually exclude each.
 *
 * Returns `undefined` when the active config can't surface any immature
 * picks (no `minimumReleaseAge`, or strict mode on); the resolver never
 * fires the callback in those modes, so we'd just be allocating a Set
 * for nothing.
 */
export function createImmaturePickCollector (
  opts: { minimumReleaseAge?: number, minimumReleaseAgeStrict?: boolean }
): ImmaturePickCollector | undefined {
  if (!opts.minimumReleaseAge || opts.minimumReleaseAgeStrict) return undefined
  const versions = new Set<string>()
  return {
    versions,
    record: ({ name, version }) => {
      versions.add(`${name}@${version}`)
    },
  }
}

/**
 * Empties the collector and returns the sorted entries the install command
 * should pass to `updateWorkspaceManifest({ addedMinimumReleaseAgeExcludes })`.
 * Logs a single info message so the user sees what was auto-persisted; the
 * workspace manifest writer itself dedupes against existing entries so
 * repeated drains across recursive iterations don't append duplicates.
 *
 * Clears the underlying Set before returning so a follow-up install issued
 * in the same process doesn't re-announce entries that have already been
 * written.
 */
export function drainImmaturePicks (
  collector: ImmaturePickCollector | undefined
): string[] | undefined {
  if (!collector || collector.versions.size === 0) return undefined
  const sorted = [...collector.versions].sort()
  globalInfo(
    `Added ${sorted.length} ${sorted.length === 1 ? 'entry' : 'entries'} to minimumReleaseAgeExclude in pnpm-workspace.yaml ` +
    `(loose mode allowed these immature versions):\n  ${sorted.join('\n  ')}`
  )
  collector.versions.clear()
  return sorted
}
