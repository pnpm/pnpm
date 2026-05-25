import { confirm } from '@inquirer/prompts'
import { PnpmError } from '@pnpm/error'
import { globalInfo } from '@pnpm/logger'
import { MINIMUM_RELEASE_AGE_VIOLATION_CODE } from '@pnpm/resolving.npm-resolver'
import { isCI } from 'ci-info'

/**
 * Shape returned by `installing/deps-installer`'s
 * `collectResolutionPolicyViolations` and the inline accumulator on
 * the resolveDependencies result. Re-declared locally so the commands
 * layer can react without depending on the deps-installer's private
 * install types.
 *
 * Verifier codes (today: `MINIMUM_RELEASE_AGE_VIOLATION` and
 * `TRUST_DOWNGRADE`) are the contract surface for downstream UX.
 * Each `PolicyHandler` below filters violations by code to decide
 * what to do with them (prompt, persist to an exclude list, log,
 * abort).
 */
export interface PolicyViolation {
  name: string
  version: string
  code: string
  reason: string
}

/**
 * Workspace-manifest patch a per-policy handler can request. Each
 * field maps to a `pnpm-workspace.yaml` exclude-list array; the
 * install command forwards these to `updateWorkspaceManifest` so the
 * workspace writer dedupes and appends them in one pass.
 *
 * New policies that want auto-persistence add their field here AND
 * teach `updateWorkspaceManifest` how to honor it.
 */
export interface WorkspaceManifestPolicyUpdates {
  addedMinimumReleaseAgeExcludes?: string[]
}

/**
 * What the install command asks of each registered policy handler.
 * Both hooks are optional — a handler that only wants to abort can
 * skip `pickManifestUpdates`; a handler that only wants to persist
 * can skip `handleResolutionPolicyViolations`.
 */
interface PolicyHandler {
  /**
   * Runs between `resolveDependencyTree` and `resolvePeers`. Throw to
   * abort the install before any lockfile / package.json /
   * modules-dir mutation. Receives the full violations list across
   * every policy — handlers filter by `code` for their own.
   */
  handleResolutionPolicyViolations?: (violations: readonly PolicyViolation[]) => Promise<void>
  /**
   * Called at the install's tail to assemble the workspace-manifest
   * patch. Returns `undefined` (or an empty object) when this
   * handler has nothing to persist for the current batch.
   */
  pickManifestUpdates?: (violations: readonly PolicyViolation[]) => WorkspaceManifestPolicyUpdates | undefined
}

/**
 * Aggregated plan the install command consumes. The `handleResolutionPolicyViolations`
 * call fans out across every registered handler in registration order;
 * any handler can throw to abort. `pickManifestUpdates` merges the
 * per-handler patches into one bag so the workspace writer runs once.
 */
export interface PolicyHandlersPlan {
  handleResolutionPolicyViolations: (violations: readonly PolicyViolation[]) => Promise<void>
  pickManifestUpdates: (violations: readonly PolicyViolation[]) => WorkspaceManifestPolicyUpdates | undefined
}

export interface PolicyHandlersOptions {
  minimumReleaseAge?: number
  minimumReleaseAgeStrict?: boolean
  /**
   * Pass `false` for `--no-save` installs. Handlers that would
   * persist to the workspace manifest refuse to enter modes where
   * approval is durably required (today: strict minimumReleaseAge)
   * so the prompt never offers an action it can't honor.
   */
  save?: boolean
  /**
   * Override for CI detection. Defaults to `ci-info`'s `isCI` flag.
   */
  ci?: boolean
}

/**
 * Composes the per-policy handlers the install command needs for the
 * current opts. Returns `undefined` only when no handler reports
 * activity — saves the install command an empty no-op call at every
 * checkpoint when no policies are configured.
 *
 * Today only the minimumReleaseAge handler is registered. Future
 * policies (trustPolicy UX, license policy, etc.) plug in by
 * exporting a sibling `create<Name>PolicyHandler(opts)` and getting
 * pushed into the `handlers` list below.
 */
export function setupPolicyHandlers (opts: PolicyHandlersOptions): PolicyHandlersPlan | undefined {
  const handlers: PolicyHandler[] = []
  const minimumReleaseAge = createMinimumReleaseAgeHandler(opts)
  if (minimumReleaseAge) handlers.push(minimumReleaseAge)

  if (handlers.length === 0) return undefined

  return {
    handleResolutionPolicyViolations: async (violations) => {
      // Sequential, not parallel: a TTY prompt from handler N would
      // race with a different prompt from N+1, and we want a clean
      // throw to short-circuit before later handlers ask for input.
      for (const handler of handlers) {
        if (handler.handleResolutionPolicyViolations) {
          // eslint-disable-next-line no-await-in-loop
          await handler.handleResolutionPolicyViolations(violations)
        }
      }
    },
    pickManifestUpdates: (violations) => {
      const merged: WorkspaceManifestPolicyUpdates = {}
      let any = false
      for (const handler of handlers) {
        if (!handler.pickManifestUpdates) continue
        const patch = handler.pickManifestUpdates(violations)
        if (patch == null) continue
        // Shallow merge — handlers own disjoint fields by convention,
        // so there's no collision policy to encode here yet.
        for (const [key, value] of Object.entries(patch)) {
          if (value == null) continue
          ;(merged as Record<string, unknown>)[key] = value
          any = true
        }
      }
      return any ? merged : undefined
    },
  }
}

/**
 * minimumReleaseAge policy handler.
 *
 * Loose mode (`minimumReleaseAgeStrict: false`) lets the resolver
 * install versions newer than the cutoff and auto-persists them to
 * `minimumReleaseAgeExclude`. Strict mode + an interactive TTY
 * surfaces the full set of immature picks (direct AND transitive) at
 * once via a confirm prompt — the install proceeds if the user
 * approves, otherwise it aborts before touching the lockfile or
 * package.json (#10488). Strict mode in CI or any other non-TTY
 * context aborts hard with the same violation list so the failure
 * pinpoints every offending entry, not just the first one the
 * resolver picked.
 *
 * Strict mode combined with `--no-save` is rejected up-front — the
 * approval prompt promises persistence the install command's
 * `opts.save !== false` gate would block, leaving the lockfile
 * holding approved-but-unlisted immature picks that the next install
 * would reject.
 *
 * Returns `undefined` when minimumReleaseAge is not active.
 */
function createMinimumReleaseAgeHandler (opts: PolicyHandlersOptions): PolicyHandler | undefined {
  if (!opts.minimumReleaseAge) return undefined
  const strictMode = opts.minimumReleaseAgeStrict === true
  const persistenceEnabled = opts.save !== false
  const inCi = opts.ci ?? isCI
  const canPrompt = !inCi && Boolean(process.stdin.isTTY)

  return {
    handleResolutionPolicyViolations: async (violations) => {
      if (!strictMode) return
      const immature = filterImmatureViolations(violations)
      if (immature.length === 0) return
      if (!persistenceEnabled) {
        throw new PnpmError(
          'STRICT_MIN_RELEASE_AGE_REQUIRES_SAVE',
          'minimumReleaseAgeStrict cannot be combined with --no-save: ' +
          'approval would require writing to minimumReleaseAgeExclude in pnpm-workspace.yaml, ' +
          'which --no-save prevents.',
          {
            hint: 'Drop --no-save so the exclude list can be persisted, or set ' +
              'minimumReleaseAgeStrict: false to let the install proceed without prompting ' +
              '(the lockfile would still trigger the auto-collect on the next normal install).',
          }
        )
      }
      if (canPrompt) {
        await promptForApproval(immature)
      } else {
        throw failOnImmature(immature)
      }
    },
    pickManifestUpdates: (violations) => {
      const entries = pickImmatureEntries(violations, strictMode)
      return entries ? { addedMinimumReleaseAgeExcludes: entries } : undefined
    },
  }
}

function filterImmatureViolations (violations: readonly PolicyViolation[]): PolicyViolation[] {
  return violations.filter((v) => v.code === MINIMUM_RELEASE_AGE_VIOLATION_CODE)
}

function pickImmatureEntries (
  violations: readonly PolicyViolation[],
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
    : '(set minimumReleaseAgeStrict to true to gate these updates with a prompt)'
  globalInfo(
    `Added ${sorted.length} ${sorted.length === 1 ? 'entry' : 'entries'} to minimumReleaseAgeExclude in pnpm-workspace.yaml ` +
    `${reason}:\n  ${sorted.join('\n  ')}`
  )
  return sorted
}

function failOnImmature (immature: readonly PolicyViolation[]): PnpmError {
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

async function promptForApproval (immature: readonly PolicyViolation[]): Promise<void> {
  const sorted = [...immature].sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
  const message =
    `${sorted.length} ${sorted.length === 1 ? 'version does' : 'versions do'} not meet the minimumReleaseAge constraint:\n` +
    sorted.map((v) => `  ${v.name}@${v.version}`).join('\n') + '\n' +
    'Add to minimumReleaseAgeExclude in pnpm-workspace.yaml and proceed with the install?'
  let confirmed: boolean
  try {
    confirmed = await confirm({ message, default: false })
  } catch (err) {
    if (err instanceof Error && err.name === 'ExitPromptError') {
      confirmed = false
    } else {
      throw err
    }
  }
  if (!confirmed) {
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
