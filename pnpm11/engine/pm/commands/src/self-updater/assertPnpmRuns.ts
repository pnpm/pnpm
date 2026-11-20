import type { SpawnSyncReturns } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import spawn from 'cross-spawn'

/**
 * Throws unless the pnpm CLI in `binDir` can execute — a release can install
 * cleanly and still not run, when its wrapper kept the placeholder bin of a
 * platform package that shipped without a native.
 *
 * Only that it runs is asserted; reading `--version` output would tie the check
 * to whatever startup decides to print. Exported as a test seam.
 */
export function assertPnpmRuns (binDir: string, version: string): void {
  const pnpmBinPath = path.join(binDir, 'pnpm')
  // pnpm prints its version only after loading config and switching versions,
  // so probing from the caller's directory answers with their pin rather than
  // the release under test.
  const probeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-self-update-check-'))
  let result: SpawnSyncReturns<string>
  try {
    result = spawn.sync(pnpmBinPath, ['--version'], {
      encoding: 'utf8',
      cwd: probeDir,
      timeout: 30_000,
    })
  } finally {
    try {
      fs.rmSync(probeDir, { recursive: true, force: true })
    } catch {}
  }
  const { status, error, stderr } = result
  if (error == null && status === 0) return
  // A signal leaves `status` null, which macOS produces for a binary its
  // signature check rejects — the exact shape of an incorrectly signed release.
  const exit = status != null ? `code ${status}` : 'a signal'
  const reason = error != null
    ? error.message
    : `it exited with ${exit}${(stderr ?? '').trim() ? `: ${stderr.trim()}` : ''}`
  throw new PnpmError(
    'BROKEN_PNPM_INSTALL',
    `The pnpm v${version} that was just installed cannot run: ${reason}`,
    {
      hint: `The installation at "${pnpmBinPath}" was discarded and the currently active pnpm was left in place, so pnpm still works. A release that installs but cannot run is a packaging fault — please report it at https://github.com/pnpm/pnpm/issues. To move to a different version meanwhile, pass one to "pnpm self-update".`,
    }
  )
}
