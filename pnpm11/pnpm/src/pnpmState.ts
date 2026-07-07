import path from 'node:path'
import util from 'node:util'

import { getStateDir } from '@pnpm/config.reader'
import { loadJsonFile } from 'load-json-file'
import { writeJsonFile } from 'write-json-file'

/**
 * The shape of `pnpm-state.json`, the per-user scratch state file. Every
 * feature that stores something here owns one top-level key and must write
 * through {@link updatePnpmState} so it merges with (never clobbers) the keys
 * of the other features.
 */
export interface PnpmState {
  /** Timestamp of the last update check (`checkForUpdates`). */
  lastUpdateCheck?: string
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
 * The per-user state dir for security-sensitive state (the pnpmExecCommand
 * trust records). Deliberately not the configured `stateDir`: that setting is
 * workspace-yaml-settable, so honoring it would let the workspace file that
 * declares a malicious command also point pnpm at a repo-controlled state
 * file that pre-seeds its own trust record, suppressing the notice. The env
 * override is user-controlled, not repo-controlled, so it stays honored (an
 * exported-but-empty value counts as unset, matching the config layer).
 *
 * Returns `null` when the resolved dir is not absolute (an empty home dir or
 * a relative env override): a relative dir would resolve against the current,
 * typically repo-controlled, directory.
 */
export function getDefaultStateDir (): string | null {
  const envOverride = [process.env.PNPM_CONFIG_STATE_DIR, process.env.pnpm_config_state_dir]
    .find((value) => value != null && value !== '')
  const stateDir = envOverride ?? getStateDir(process)
  return path.isAbsolute(stateDir) ? stateDir : null
}

export interface PnpmStateReadResult {
  state: PnpmState | undefined
  /**
   * Whether the state file can be rewritten without data loss: the file was
   * read, is missing (first run), or is unparsable (nothing valid to lose —
   * rewriting recovers the file). `false` on any other read failure (e.g.
   * permissions), where a write would clobber keys that failed to load.
   */
  writable: boolean
  /**
   * The read failure behind `writable: false`, for the caller to surface —
   * persistence silently stopping with no diagnostic trail would be
   * impossible to debug. Callers own the reporting because they run at
   * different stages of startup (before or after the reporter is
   * initialized).
   */
  readError?: Error
}

export async function readPnpmState (stateDir: string): Promise<PnpmStateReadResult> {
  try {
    return { state: await loadJsonFile<PnpmState>(stateFile(stateDir)), writable: true }
  } catch (err: unknown) {
    // A parse failure (SyntaxError, no fs `code`) and ENOENT are both
    // writable; any other fs error means the file's contents may be intact
    // but unreadable, so writing would clobber them.
    const unreadable = util.types.isNativeError(err) && 'code' in err && err.code !== 'ENOENT'
    if (unreadable) {
      return {
        state: undefined,
        writable: false,
        readError: new Error(`Failed to read ${stateFile(stateDir)}: ${(err as Error).message}`),
      }
    }
    return { state: undefined, writable: true }
  }
}

/**
 * Re-read the state, apply `update` to the fresh copy, merge its top-level
 * keys over that copy, and write the result. Deriving the update from the
 * freshly read state (rather than passing a precomputed object built from an
 * earlier read) keeps writes from dropping what another process wrote in the
 * meantime — both other features' top-level keys and entries inside a shared
 * map like `pnpmExecCommands`. The read-modify-write is not synchronized, so
 * a small window remains, but losing it only costs a repeated notice or
 * update check.
 */
export async function updatePnpmState (stateDir: string, update: (state: PnpmState | undefined) => Partial<PnpmState>): Promise<void> {
  const { state, writable } = await readPnpmState(stateDir)
  if (!writable) return
  await writeJsonFile(stateFile(stateDir), { ...state, ...update(state) })
}

function stateFile (stateDir: string): string {
  return path.join(stateDir, 'pnpm-state.json')
}
