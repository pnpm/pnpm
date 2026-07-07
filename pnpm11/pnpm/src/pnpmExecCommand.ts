import fs from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { getStateDir } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import spawn from 'cross-spawn'
import { loadJsonFile } from 'load-json-file'
import { writeJsonFile } from 'write-json-file'

import { reExecPnpm } from './reExecPnpm.js'

/**
 * Sentinel set on the child of a `pnpmExecCommand` re-exec. It carries the
 * resolved binary path so that the child (and any nested pnpm invocation that
 * inherits the environment, e.g. from a lifecycle script) skips re-running
 * the command: the resolution is done once per user invocation.
 */
export const PNPM_EXEC_PATH_ENV = 'PNPM_EXEC_PATH'

const COMMAND_TIMEOUT = 60_000 // ms

export interface ApplyPnpmExecCommandOptions {
  /** Directory of the pnpm-workspace.yaml that declared the setting. */
  workspaceDir: string
}

/**
 * When the `pnpmExecCommand` setting is present, run the configured command,
 * treat its trimmed stdout as the absolute path of the pnpm binary this
 * project must run under, and re-exec into that binary if it isn't the one
 * already running (in which case this function never returns).
 *
 * Any failure — the command exiting non-zero, printing nothing, or printing a
 * non-absolute or non-existent path — is a hard error: the project delegated
 * binary selection to the command, so running the current (potentially
 * mismatched) pnpm instead would defeat the point of the setting.
 *
 * The re-exec'd child inherits {@link PNPM_EXEC_PATH_ENV} and returns early
 * here without spawning anything, so the command runs once per user
 * invocation. The `packageManager` / `devEngines.packageManager` check in
 * `main()` still runs in the child, now validating the binary the command
 * actually produced.
 */
export async function applyPnpmExecCommand (command: unknown, opts: ApplyPnpmExecCommandOptions): Promise<void> {
  if (!Array.isArray(command) || command.length === 0 || !command.every((arg) => typeof arg === 'string' && arg !== '')) {
    throw new PnpmError(
      'EXEC_COMMAND_INVALID',
      'The pnpmExecCommand setting must be an array of non-empty strings, e.g. ["my-tool", "which-pnpm"]'
    )
  }

  const resolvedByParent = process.env[PNPM_EXEC_PATH_ENV]
  if (resolvedByParent != null) {
    // A parent pnpm already ran the command and re-exec'd into its result. If
    // that result is the running binary (the normal case), there is nothing to
    // do. If it isn't — e.g. a stale sentinel inherited from an unrelated
    // parent process — re-running the command could loop, so proceed and let
    // the packageManager check surface any version mismatch.
    return
  }

  const persistSeenCommand = await noticeOnFirstUseOrChange(command, opts)

  const binPath = runPnpmExecCommand(command)
  if (persistSeenCommand != null) {
    process.stderr.write(`Resolved to ${binPath}\n`)
    // Record the command only after it resolved successfully, so a failing
    // first run doesn't silence the notice on the next (successful) one.
    await persistSeenCommand()
  }

  if (isCurrentBinary(binPath)) {
    // Mark resolution as done for nested pnpm invocations.
    process.env[PNPM_EXEC_PATH_ENV] = binPath
    return
  }

  await reExecPnpm(binPath, {
    target: `the binary resolved by pnpmExecCommand ("${displayCommand(command)}")`,
    extraEnv: { [PNPM_EXEC_PATH_ENV]: binPath },
    // The sentinel guarantees the child won't re-exec, so it is fine for the
    // resolved bin dir to already lead PATH (typical when an external version
    // manager owns PATH but the user invoked a different pnpm directly).
    allowBinDirAlreadyOnPath: true,
  })
}

interface PnpmState {
  /**
   * Trust-on-first-use record of pnpmExecCommand values, keyed by the real
   * path of the workspace directory. The value is the JSON-encoded argv. A
   * workspace whose command matches its record runs silently; an unseen
   * workspace or a changed command prints a notice to stderr first. Stored in
   * the per-user state dir, outside the repository, so a project cannot
   * pre-seed it to suppress its own notice.
   */
  pnpmExecCommands?: Record<string, string>
  [key: string]: unknown
}

/**
 * Print a notice to stderr the first time a workspace's pnpmExecCommand runs
 * under this user, and a louder one whenever the command changes — the same
 * trust-on-first-use pattern as SSH known hosts, turning a quietly edited
 * pnpm-workspace.yaml into a visible signal.
 *
 * stderr keeps stdout machine-clean (`$(pnpm --version)` etc.); a direct
 * write is used because the reporter is not initialized this early in
 * startup. State-file read/write failures fall back to printing the notice —
 * failing open on noise, never on silence.
 *
 * Returns `null` when the command matches its record (no notice printed).
 * Otherwise the notice is printed and a callback that records the command is
 * returned, for the caller to invoke once resolution succeeds.
 */
async function noticeOnFirstUseOrChange (command: string[], opts: ApplyPnpmExecCommandOptions): Promise<(() => Promise<void>) | null> {
  const workspaceKey = realpathOrSelf(opts.workspaceDir)
  const commandRecord = JSON.stringify(command)
  // The trust records deliberately live in the *default* per-user state dir,
  // not config.stateDir: `stateDir` is workspace-yaml-settable, so honoring it
  // here would let the workspace file that declares a malicious command also
  // point pnpm at a repo-controlled state file that pre-seeds its own trust
  // record, suppressing the notice. The env override (pnpm_config_state_dir)
  // is user-controlled, not repo-controlled, so it stays honored.
  const stateDir = process.env.pnpm_config_state_dir ?? process.env.PNPM_CONFIG_STATE_DIR ?? getStateDir(process)
  if (!path.isAbsolute(stateDir)) {
    // A relative state dir would resolve against the current (typically
    // repo-controlled) directory, so the trust record could be pre-seeded.
    // Reachable only with an empty home dir or a relative env override.
    return printNoticeWithoutTrustStore(command)
  }
  const stateFile = path.join(stateDir, 'pnpm-state.json')

  let state: PnpmState | undefined
  try {
    state = await loadJsonFile(stateFile)
  } catch (err: unknown) {
    // A missing state file is the normal first run, and an unparsable one is
    // rewritten below (nothing valid is lost). Any other read failure (e.g.
    // permissions) leaves a file whose other keys the persist step would
    // clobber, so skip persistence for this run.
    if (util.types.isNativeError(err) && 'code' in err && err.code !== 'ENOENT') {
      return printNoticeWithoutTrustStore(command)
    }
  }

  const seen = state?.pnpmExecCommands?.[workspaceKey]
  if (seen === commandRecord) return null

  if (seen == null) {
    printFirstUseNotice(command)
  } else {
    process.stderr.write(
      'The pnpmExecCommand for this workspace has changed:\n' +
      `  was: ${displaySeenCommand(seen)}\n` +
      `  now: ${displayCommand(command)}\n`
    )
  }

  return async () => {
    try {
      await writeJsonFile(stateFile, {
        ...state,
        pnpmExecCommands: {
          ...state?.pnpmExecCommands,
          [workspaceKey]: commandRecord,
        },
      })
    } catch {
      // If the state can't be persisted the notice repeats next run. Noise is
      // an acceptable failure mode; a suppressed notice is not.
    }
  }
}

/**
 * The fallback when the trust store can't be consulted safely: the notice
 * prints on every run (noise over silence) and nothing is recorded.
 */
function printNoticeWithoutTrustStore (command: string[]): () => Promise<void> {
  printFirstUseNotice(command)
  return async () => {}
}

function printFirstUseNotice (command: string[]): void {
  process.stderr.write(
    'Resolving the pnpm binary with pnpmExecCommand:\n' +
    `> ${displayCommand(command)}\n`
  )
}

function displaySeenCommand (seen: string): string {
  try {
    const parsed: unknown = JSON.parse(seen)
    if (Array.isArray(parsed) && parsed.every((arg) => typeof arg === 'string')) {
      return displayCommand(parsed)
    }
  } catch {}
  // A corrupted record still gets shown (escaped) rather than crashing the
  // notice that is reporting the change away from it.
  return escapeControlCharacters(seen)
}

function displayCommand (command: string[]): string {
  return escapeControlCharacters(command.join(' '))
}

const SHORT_CONTROL_CHARACTER_ESCAPES: Record<string, string> = {
  '\b': '\\b',
  '\t': '\\t',
  '\n': '\\n',
  '\f': '\\f',
  '\r': '\\r',
}

/**
 * The notice is a trust signal, so argv elements must not be able to forge it
 * (or hide parts of it) with embedded newlines or terminal escape sequences.
 * Control characters are rendered as their JSON escape. Kept in sync with
 * pacquet's `escape_control_characters`.
 */
function escapeControlCharacters (text: string): string {
  return text.replace(/\p{Cc}/gu, (ch) => SHORT_CONTROL_CHARACTER_ESCAPES[ch] ?? `\\u${ch.codePointAt(0)!.toString(16).padStart(4, '0')}`)
}

function realpathOrSelf (dir: string): string {
  try {
    return fs.realpathSync(dir)
  } catch {
    return dir
  }
}

function runPnpmExecCommand (command: string[]): string {
  const [cmd, ...args] = command
  // stderr is inherited so the tool's own diagnostics reach the user directly.
  const { status, error, stdout } = spawn.sync(cmd, args, {
    stdio: ['ignore', 'pipe', 'inherit'],
    timeout: COMMAND_TIMEOUT,
  })

  if (error != null || status !== 0) {
    throw new PnpmError(
      'EXEC_COMMAND_FAIL',
      `The pnpmExecCommand ("${displayCommand(command)}") failed${status != null ? ` with exit code ${status}` : ''}`,
      {
        hint: error instanceof Error ? error.message : undefined,
      }
    )
  }

  const binPath = stdout.toString().trim()
  if (binPath === '') {
    throw new PnpmError(
      'EXEC_COMMAND_NO_OUTPUT',
      `The pnpmExecCommand ("${displayCommand(command)}") printed no path to stdout`
    )
  }
  if (!path.isAbsolute(binPath)) {
    throw new PnpmError(
      'EXEC_COMMAND_RELATIVE_PATH',
      `The pnpmExecCommand ("${displayCommand(command)}") printed a non-absolute path: "${binPath}"`
    )
  }
  if (!isFile(binPath)) {
    throw new PnpmError(
      'EXEC_COMMAND_BAD_PATH',
      `The pnpmExecCommand ("${displayCommand(command)}") printed a path that is not an existing file: "${binPath}"`
    )
  }
  return binPath
}

function isFile (binPath: string): boolean {
  try {
    return fs.statSync(binPath).isFile()
  } catch {
    return false
  }
}

function isCurrentBinary (binPath: string): boolean {
  // process.argv[1] is the running pnpm entry script (or the SEA exec path).
  const current = process.argv[1]
  if (current == null) return false
  try {
    return fs.realpathSync(binPath) === fs.realpathSync(current)
  } catch {
    return false
  }
}
