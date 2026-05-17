import { PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import { isCI } from 'ci-info'
import enquirer from 'enquirer'

/**
 * Shape returned by `installing/deps-installer`'s
 * `collectLockfileResolutionViolations`. Re-declared locally so the
 * commands layer can react to the violations without depending on the
 * deps-installer's private install types.
 *
 * Verifier codes are the contract surface for downstream UX —
 * `MINIMUM_RELEASE_AGE_VIOLATION` and `TRUST_DOWNGRADE` today; new
 * resolvers / verifiers add their own codes without touching this
 * file. The install command filters by code to decide what to do
 * (persist to an exclude list, prompt, log, etc.).
 */
export interface ResolverViolation {
  name: string
  version: string
  code: string
  reason: string
}

const MINIMUM_RELEASE_AGE_CODE = 'MINIMUM_RELEASE_AGE_VIOLATION'

export interface ImmaturePicksPlan {
  /**
   * Wires the install command into the resolver-agnostic
   * `onAfterResolveDependencyTree` checkpoint, called between
   * `resolveDependencyTree` and `resolvePeers` so the abort happens
   * before peer-dep work runs. The handler picks one of three
   * paths based on mode + TTY: strict + TTY prompts and persists on
   * approval; strict no-TTY throws with the full violation list;
   * loose is a no-op (the persist path at the end of the install
   * handles the picks). Throws to abort the install cleanly when
   * the user declines or when no prompt is possible.
   */
  onAfterResolveDependencyTree: (
    violations: readonly ResolverViolation[]
  ) => Promise<void>
  /**
   * Filters the install result's violations down to the
   * `name@version` strings the install command will write into the
   * workspace manifest's `minimumReleaseAgeExclude`. Returns
   * `undefined` when nothing needs to be persisted so callers can
   * skip the workspace manifest update entirely.
   */
  pickEntriesToPersist: (violations: readonly ResolverViolation[]) => string[] | undefined
}

/**
 * Loose minimumReleaseAge mode (`minimumReleaseAgeStrict: false`)
 * lets the resolver install versions newer than the cutoff and the
 * install command auto-persists them to `minimumReleaseAgeExclude`.
 * Strict mode + an interactive TTY surfaces the full set of immature
 * picks (direct AND transitive) at once via a confirm prompt — the
 * install proceeds if the user approves, otherwise it aborts before
 * touching the lockfile or package.json (#10488). Strict mode in CI
 * or any other non-TTY context aborts hard with the same violation
 * list so the failure pinpoints every offending entry, not just the
 * first one the resolver picked.
 *
 * Returns `undefined` only when no minimumReleaseAge policy is
 * active — there's no work for the plan to do in that case.
 */
export function setupImmaturePicks (opts: {
  minimumReleaseAge?: number
  minimumReleaseAgeStrict?: boolean
  /**
   * Override for CI detection. Defaults to `ci-info`'s `isCI` flag.
   */
  ci?: boolean
}): ImmaturePicksPlan | undefined {
  if (!opts.minimumReleaseAge) return undefined
  const strictMode = opts.minimumReleaseAgeStrict === true
  const inCi = opts.ci ?? isCI
  const canPrompt = !inCi && Boolean(process.stdin.isTTY)

  return {
    pickEntriesToPersist: (violations) => pickImmatureEntries(violations, strictMode),
    onAfterResolveDependencyTree: async (violations) => {
      if (!strictMode) return
      const immature = filterImmatureViolations(violations)
      if (immature.length === 0) return
      if (canPrompt) {
        await promptForApproval(immature)
      } else {
        throw failOnImmature(immature)
      }
    },
  }
}

function failOnImmature (immature: readonly ResolverViolation[]): PnpmError {
  const sorted = [...immature].sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
  const list = sorted.map((v) => `  ${v.name}@${v.version} ${v.reason}`).join('\n')
  return new PnpmError(
    'NO_MATURE_MATCHING_VERSION',
    `${sorted.length} ${sorted.length === 1 ? 'version does' : 'versions do'} not meet the minimumReleaseAge constraint:\n${list}`,
    {
      hint: 'Run the install interactively to approve these picks, or add them to ' +
        'minimumReleaseAgeExclude in pnpm-workspace.yaml, or wait for the packages ' +
        'to mature past the configured cutoff.',
    }
  )
}

function filterImmatureViolations (violations: readonly ResolverViolation[]): ResolverViolation[] {
  return violations.filter((v) => v.code === MINIMUM_RELEASE_AGE_CODE)
}

function pickImmatureEntries (
  violations: readonly ResolverViolation[],
  promptRequired: boolean
): string[] | undefined {
  const immature = filterImmatureViolations(violations)
  if (immature.length === 0) return undefined
  const sorted = [...new Set(immature.map((v) => `${v.name}@${v.version}`))].sort()
  // Strict-mode picks already passed through the approval prompt, so
  // the log here only confirms what was persisted. Loose-mode picks
  // haven't been announced anywhere else, so the same log doubles as
  // the discovery notice.
  const reason = promptRequired
    ? '(approved at the prompt)'
    : '(loose mode allowed these immature versions)'
  globalInfo(
    `Added ${sorted.length} ${sorted.length === 1 ? 'entry' : 'entries'} to minimumReleaseAgeExclude in pnpm-workspace.yaml ` +
    `${reason}:\n  ${sorted.join('\n  ')}`
  )
  return sorted
}

async function promptForApproval (immature: readonly ResolverViolation[]): Promise<void> {
  const sorted = [...immature].sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
  const message =
    `${sorted.length} ${sorted.length === 1 ? 'version does' : 'versions do'} not meet the minimumReleaseAge constraint:\n` +
    sorted.map((v) => `  ${v.name}@${v.version}`).join('\n') + '\n' +
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
