import { PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import { isCI } from 'ci-info'
import enquirer from 'enquirer'

export interface ImmaturePickCollector {
  /** Resolver-side callback wired into `opts.onImmaturePick`. */
  record: (pkg: { name: string, version: string }) => void
  versions: Set<string>
  /**
   * `true` when the install command runs in strict-mode + interactive
   * (TTY) and the user must approve immature picks before the install
   * proceeds past peer-dep resolution. `false` in loose mode, where
   * picks are auto-persisted silently.
   */
  promptRequired: boolean
}

export interface ImmaturePickResolution {
  collector: ImmaturePickCollector
  /**
   * Forwarded to the resolver. When set, strict mode behaves like loose
   * for the picker (fall back to lowest + notify), so the prompt sees
   * every immature pick at once.
   */
  deferImmatureDecision: boolean
  /**
   * Forwarded to `resolveDependencies`. Runs between main resolution and
   * peer-dep resolution: prompts under strict + TTY, no-op in loose mode.
   */
  confirmImmaturePicks: () => Promise<void>
}

/**
 * Loose minimumReleaseAge mode (`minimumReleaseAgeStrict: false`) lets the
 * resolver install versions newer than the cutoff and the install command
 * auto-persists them to `minimumReleaseAgeExclude`. Strict mode + an
 * interactive TTY surfaces the full set of immature picks (direct AND
 * transitive) at once via a confirm prompt — the install proceeds if the
 * user approves, otherwise it aborts before touching the lockfile or
 * package.json (#10488).
 *
 * Returns `undefined` when no policy is active or strict mode is on
 * without a TTY — in those cases the resolver's existing behavior
 * (throw on first immature pick) is what we want, and there's no work
 * for this collector to do.
 */
export function setupImmaturePicks (opts: {
  minimumReleaseAge?: number
  minimumReleaseAgeStrict?: boolean
  /**
   * Override for CI detection. Defaults to `ci-info`'s `isCI` flag. The
   * pnpm config reader populates `ci` from the same source; install
   * commands forward `opts.ci` here so a `--config.ci=true` or explicit
   * CI env var keeps strict mode on the fail-fast path even when stdin
   * happens to be a TTY.
   */
  ci?: boolean
}): ImmaturePickResolution | undefined {
  if (!opts.minimumReleaseAge) return undefined
  const strictMode = opts.minimumReleaseAgeStrict === true
  // Two signals to keep CI on the fail-fast path: stdin must be a TTY (no
  // way to ask otherwise), AND CI detection must not have flagged this run
  // (some CI runners do allocate a TTY but expect deterministic
  // non-interactive behavior). Returning `undefined` here makes the
  // resolver throw on the first immature pick, preserving today's
  // strict-mode CI semantics.
  const inCi = opts.ci ?? isCI
  const canPrompt = !inCi && Boolean(process.stdin.isTTY)
  if (strictMode && !canPrompt) return undefined

  const versions = new Set<string>()
  const collector: ImmaturePickCollector = {
    versions,
    record: ({ name, version }) => {
      versions.add(`${name}@${version}`)
    },
    promptRequired: strictMode,
  }
  return {
    collector,
    deferImmatureDecision: true,
    confirmImmaturePicks: () => promptForImmaturePicksIfNeeded(collector),
  }
}

/**
 * Prompts the user with the sorted list of immature picks gathered during
 * resolution. Returns silently when the collector is empty (no immature
 * picks — common loose-mode case) or when no prompt is required.
 *
 * Default answer is `No`: typing nothing aborts. This matches the
 * defensive posture of the minimumReleaseAge policy — a user who didn't
 * mean to approve an immature pin shouldn't fall into approving it by
 * mistake.
 */
async function promptForImmaturePicksIfNeeded (collector: ImmaturePickCollector): Promise<void> {
  if (collector.versions.size === 0) return
  if (!collector.promptRequired) return

  const sorted = [...collector.versions].sort()
  const message =
    `${sorted.length} ${sorted.length === 1 ? 'version does' : 'versions do'} not meet the minimumReleaseAge constraint:\n` +
    sorted.map((entry) => `  ${entry}`).join('\n') + '\n' +
    'Add to minimumReleaseAgeExclude in pnpm-workspace.yaml and proceed with the install?'

  const answer = await enquirer.prompt<{ confirmed: boolean }>({
    type: 'confirm',
    name: 'confirmed',
    message,
    initial: false,
  })
  if (!answer.confirmed) {
    throw new PnpmError(
      'MINIMUM_RELEASE_AGE_DENIED',
      'Aborted: the immature versions were not approved.',
      {
        hint: 'Re-run the install without `minimumReleaseAgeStrict: true` to allow these versions, ' +
          'or wait for the packages to mature past the configured cutoff.',
      }
    )
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
  // Strict-mode picks already passed through the approval prompt, so the
  // log here only confirms what was persisted. Loose-mode picks haven't
  // been announced anywhere else, so the same log doubles as the discovery
  // notice.
  const reason = collector.promptRequired
    ? '(approved at the prompt)'
    : '(loose mode allowed these immature versions)'
  globalInfo(
    `Added ${sorted.length} ${sorted.length === 1 ? 'entry' : 'entries'} to minimumReleaseAgeExclude in pnpm-workspace.yaml ` +
    `${reason}:\n  ${sorted.join('\n  ')}`
  )
  collector.versions.clear()
  return sorted
}
