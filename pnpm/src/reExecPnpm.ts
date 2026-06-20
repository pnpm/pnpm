import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { prependDirsToPath } from '@pnpm/shell.path'
import spawn from 'cross-spawn'

import { exit } from './exit.js'

// Sentinel env var carrying the re-exec depth. A correct setup terminates on
// the first re-exec (the child's version matches what the source wants, so it
// proceeds), but a misconfigured source could loop; this is the hard backstop.
const RE_EXEC_DEPTH_ENV = 'PNPM_RE_EXEC_DEPTH'
const MAX_RE_EXEC_DEPTH = 2

export class VersionSwitchFail extends PnpmError {
  constructor (target: string, wantedPnpmBinDir: string, cause?: unknown) {
    super(
      'VERSION_SWITCH_FAIL',
      `Failed to switch pnpm to ${target}. Looks like pnpm CLI is missing at "${wantedPnpmBinDir}" or is incorrect`,
      { hint: cause instanceof Error ? cause?.message : undefined })

    if (cause != null) {
      this.cause = cause
    }
  }
}

export interface ReExecPnpmOptions {
  /** Extra env vars for the spawned child (e.g. to stop it switching again). */
  extraEnv?: NodeJS.ProcessEnv
  /** Label for error messages, e.g. "v9.3.0" or "the canonical binary". */
  target?: string
}

/**
 * Re-exec the current pnpm invocation through the pnpm binary in `binDir`,
 * then exit with the child's status. Shared by `switchCliVersion` (download
 * path) and the `canonicalBinarySource` hook path (on-disk path). Sets
 * `extraEnv` on the child so the new process doesn't switch versions again.
 */
export async function reExecPnpm (binDir: string, opts: ReExecPnpmOptions = {}): Promise<void> {
  const target = opts.target ?? `the binary at "${binDir}"`

  const depth = Number(process.env[RE_EXEC_DEPTH_ENV] ?? '0')
  if (depth >= MAX_RE_EXEC_DEPTH) {
    throw new VersionSwitchFail(
      target,
      binDir,
      new Error(`re-exec depth exceeded ${MAX_RE_EXEC_DEPTH}; the binary keeps redirecting to a different one`)
    )
  }

  const pnpmEnv = prependDirsToPath([binDir])
  if (!pnpmEnv.updated) {
    // PATH already led with binDir, so spawning would re-run the same binary —
    // throw to prevent an infinite recursive fork bomb.
    throw new VersionSwitchFail(target, binDir)
  }

  // Specify the exact pnpm file path to execute, rather than relying on PATH
  // resolution, to avoid fork-bombing if binDir exists but holds no pnpm.
  // See https://github.com/pnpm/pnpm/pull/8679.
  const pnpmBinPath = path.join(binDir, 'pnpm')
  const { status, signal, error } = spawn.sync(pnpmBinPath, process.argv.slice(2), {
    stdio: 'inherit',
    env: {
      ...process.env,
      [pnpmEnv.name]: pnpmEnv.value,
      [RE_EXEC_DEPTH_ENV]: String(depth + 1),
      ...opts.extraEnv,
    },
  })

  if (error) {
    throw new VersionSwitchFail(target, binDir, error)
  }
  if (signal) {
    process.kill(process.pid, signal)
    return
  }
  await exit(status ?? 0)
}
