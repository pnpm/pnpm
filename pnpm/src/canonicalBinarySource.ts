import path from 'node:path'

import { packageManager } from '@pnpm/cli.meta'
import type { Config, ConfigContext } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import { requireHooks } from '@pnpm/hooks.pnpmfile'

import { reExecPnpm } from './reExecPnpm.js'

/**
 * When `canonicalBinarySource` is `"pnpmfile"`, load the project (and global)
 * pnpmfile early — before any version switching or store access — and ask its
 * `getCanonicalBinaryPath` hook which pnpm binary this project must run under.
 *
 * Returns without doing anything when the source is unset. Otherwise:
 *  - the hook returns a path different from the running binary -> re-exec into it
 *    (never returns; the process is replaced via reExecPnpm -> exit).
 *  - the hook returns null/undefined or a path equal to the running binary ->
 *    proceed with the current binary (this is how recursion terminates).
 *  - the source is "pnpmfile" but no hook is provided -> throw.
 *
 * Config-dependency plugin pnpmfiles are intentionally NOT consulted here: they
 * can't be installed before the version is settled, and excluding them keeps a
 * published plugin from silently redirecting the user's pnpm binary.
 */
export async function applyCanonicalBinarySource (config: Config, context: ConfigContext): Promise<void> {
  if (config.canonicalBinarySource == null) return
  if (config.canonicalBinarySource !== 'pnpmfile') return
  if (config.ignorePnpmfile) {
    throw new PnpmError(
      'CANONICAL_BINARY_SOURCE_WITH_IGNORE_PNPMFILE',
      'canonicalBinarySource is "pnpmfile" but pnpmfiles are ignored (ignore-pnpmfile is set)'
    )
  }

  const rootDir = config.lockfileDir ?? context.rootProjectManifestDir ?? config.dir
  const { hooks } = await requireHooks(rootDir, {
    globalPnpmfile: config.globalPnpmfile,
    pnpmfiles: typeof config.pnpmfile === 'string' ? [config.pnpmfile] : (config.pnpmfile ?? []),
    tryLoadDefaultPnpmfile: config.pnpmfile == null,
  })

  if (!hooks.getCanonicalBinaryPath) {
    throw new PnpmError(
      'CANONICAL_BINARY_SOURCE_HOOK_MISSING',
      'canonicalBinarySource is set to "pnpmfile" but no "getCanonicalBinaryPath" hook was found in the pnpmfile',
      { hint: 'Define `hooks.getCanonicalBinaryPath` in your .pnpmfile, or remove the `canonicalBinarySource` setting.' }
    )
  }

  let canonicalBinaryPath: string | null | undefined
  try {
    canonicalBinaryPath = await hooks.getCanonicalBinaryPath({
      currentPnpmVersion: packageManager.version,
      rootDir,
    })
  } catch (err: unknown) {
    throw new PnpmError(
      'CANONICAL_BINARY_PATH_HOOK_FAILED',
      `The getCanonicalBinaryPath hook threw an error: ${err instanceof Error ? err.message : String(err)}`
    )
  }

  // null/undefined => the running binary is already canonical; proceed.
  if (canonicalBinaryPath == null) return

  if (typeof canonicalBinaryPath !== 'string' || canonicalBinaryPath === '') {
    throw new PnpmError(
      'CANONICAL_BINARY_PATH_INVALID',
      `The getCanonicalBinaryPath hook returned an invalid path: ${JSON.stringify(canonicalBinaryPath)}`
    )
  }

  // Safety net beyond the hook's own version check: if the returned binary is
  // the one already executing, proceed instead of re-executing into ourselves.
  if (isCurrentBinary(canonicalBinaryPath)) return

  // The hook returns the path to the pnpm executable; reExecPnpm prepends its
  // directory to PATH and spawns it. pm-on-fail=ignore guards against the child
  // (which may also see canonicalBinarySource) trying to switch again, in
  // tandem with the hook returning null once versions match.
  const binDir = path.dirname(canonicalBinaryPath)
  await reExecPnpm(binDir, {
    target: 'the canonical binary',
    extraEnv: { pnpm_config_pm_on_fail: 'ignore' },
  })
}

function isCurrentBinary (canonicalBinaryPath: string): boolean {
  // process.argv[1] is the running pnpm entry script (or the SEA exec path).
  const current = process.argv[1]
  if (current == null) return false
  try {
    return path.resolve(canonicalBinaryPath) === path.resolve(current)
  } catch {
    return false
  }
}
