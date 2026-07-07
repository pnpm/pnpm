import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { prependDirsToPath } from '@pnpm/shell.path'
import spawn from 'cross-spawn'

import { exit } from './exit.js'

// Sentinel env var carrying the re-exec depth. A correct setup terminates on
// the first re-exec (the child either matches what the project wants or is
// told not to switch again via extraEnv), but a misconfigured project could
// loop; this is the hard backstop against fork-bombing.
const RE_EXEC_DEPTH_ENV = 'PNPM_RE_EXEC_DEPTH'
const MAX_RE_EXEC_DEPTH = 2

/**
 * Reset the re-exec depth once a resolution is confirmed settled (the running
 * binary is the one the project wants), so the backstop counts consecutive
 * redirects of one resolution. Without the reset, an inherited depth from an
 * unrelated outer resolution (e.g. a lifecycle script invoking pnpm in a
 * different project) would accumulate toward the cap and trip it with no
 * loop present.
 */
export function clearReExecDepth (): void {
  delete process.env[RE_EXEC_DEPTH_ENV]
}

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
  /** Label for error messages, e.g. "v9.3.0". */
  target?: string
  /**
   * By default, the bin dir already leading PATH is treated as an error: in
   * the version-download flow it means the re-exec would run the same binary
   * again, i.e. infinite recursion. A caller whose child is guaranteed not to
   * re-exec (via a sentinel in `extraEnv`) can allow it — an external version
   * manager may legitimately have its bin dir first on PATH while the user
   * invoked a different pnpm.
   */
  allowBinDirAlreadyOnPath?: boolean
}

/**
 * Re-exec the current pnpm invocation through the pnpm binary at
 * `pnpmBinPath`, then exit with the child's status. Shared by
 * `switchCliVersion` (download path) and the `pnpmExecCommand` flow
 * (on-disk path).
 */
export async function reExecPnpm (pnpmBinPath: string, opts: ReExecPnpmOptions = {}): Promise<void> {
  const binDir = path.dirname(pnpmBinPath)
  const target = opts.target ?? `the binary at "${pnpmBinPath}"`

  // A malformed depth (e.g. a poisoned env var yielding NaN) must count as 0,
  // not disable the guard: NaN >= MAX would be false on every level of an
  // otherwise unbounded recursion.
  const parsedDepth = Number(process.env[RE_EXEC_DEPTH_ENV] ?? '0')
  const depth = Number.isSafeInteger(parsedDepth) && parsedDepth >= 0 ? parsedDepth : 0
  if (depth >= MAX_RE_EXEC_DEPTH) {
    throw new VersionSwitchFail(
      target,
      binDir,
      new Error(`re-exec depth exceeded ${MAX_RE_EXEC_DEPTH}; the binary keeps redirecting to a different one`)
    )
  }

  // Prepend the target bin dir to PATH so that nested `pnpm` invocations
  // (e.g. from lifecycle scripts) resolve to the same binary.
  const pnpmEnv = prependDirsToPath([binDir])
  if (!pnpmEnv.updated && !opts.allowBinDirAlreadyOnPath) {
    // We throw this error to prevent an infinite recursive call of the same pnpm version.
    throw new VersionSwitchFail(target, binDir)
  }

  // Spawn the exact pnpm file path rather than relying on PATH resolution.
  //
  // It's not safe to spawn 'pnpm' (without specifying an absolute path) and
  // expect it to resolve to the same file path computed above due to the $PATH
  // environment variable. While that does happen in most cases, there's a
  // scenario where the wanted pnpm bin dir exists, but no pnpm binary is
  // present within that directory. If that's the case, a different pnpm bin can
  // get executed, causing infinite spawn and fork bombing the user. See details
  // at https://github.com/pnpm/pnpm/pull/8679.
  const { status, signal, error } = spawn.sync(pnpmBinPath, process.argv.slice(2), {
    stdio: 'inherit',
    env: {
      ...process.env,
      ...opts.extraEnv,
      // Set after extraEnv: the depth sentinel is owned by this helper and
      // must not be clobbered by a caller.
      [pnpmEnv.name]: pnpmEnv.value,
      [RE_EXEC_DEPTH_ENV]: String(depth + 1),
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
