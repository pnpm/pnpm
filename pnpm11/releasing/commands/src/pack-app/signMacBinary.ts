import fs from 'node:fs'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { safeExeca as execa } from 'execa'

import { isWithinDir } from './outputPaths.js'
import type { ParsedTarget } from './targets.js'

/**
 * SEA injection invalidates the existing code signature on macOS binaries, so
 * the output must be re-signed. Native macOS hosts use `codesign`; Linux hosts
 * cross-signing a darwin target use `ldid`. Windows hosts have no readily
 * available ad-hoc signer, so we refuse to produce an unsigned output silently
 * and tell the user to re-sign on macOS or Linux.
 */
export async function adHocSignMacBinary (target: ParsedTarget, outputFile: string, dir: string): Promise<void> {
  if (target.platform !== 'darwin') return
  if (process.platform === 'darwin') {
    // `codesign` is a macOS system tool; spawn it by absolute path so a
    // repo-controlled `node_modules/.bin/codesign` on PATH can't be run in its
    // place.
    await execa('/usr/bin/codesign', ['--sign', '-', outputFile], { stdio: 'inherit' })
    return
  }
  if (process.platform === 'linux') {
    const ldid = resolveTrustedSigner('ldid', dir, outputFile)
    try {
      await execa(ldid, ['-S', outputFile], { stdio: 'inherit' })
    } catch {
      throw new PnpmError('PACK_APP_MACOS_SIGN_FAILED',
        `Cross-compiled macOS binary at ${outputFile} could not be ad-hoc signed with "ldid".`,
        { hint: 'Install ldid (https://github.com/ProcursusTeam/ldid) or re-sign the binary on macOS with "codesign --sign - <file>".' }
      )
    }
    return
  }
  throw new PnpmError('PACK_APP_MACOS_SIGN_UNSUPPORTED_HOST',
    `Cannot ad-hoc sign the macOS binary at ${outputFile} on a ${process.platform} host.`,
    { hint: 'Build macOS targets on a macOS or Linux host, or re-sign the produced binary yourself with "codesign --sign -" on macOS.' }
  )
}

// Resolve an external signer (`ldid`) to an absolute path via PATH, skipping
// any match that resolves inside the project directory — a repo could ship
// `node_modules/.bin/ldid` and, if that directory is on the developer's PATH,
// get an attacker-controlled binary executed when packaging a darwin target.
// Returns the first match outside the project, or throws PACK_APP_MACOS_SIGN_FAILED.
function resolveTrustedSigner (name: string, dir: string, outputFile: string): string {
  for (const entry of (process.env.PATH ?? '').split(path.delimiter)) {
    if (entry === '') continue
    const candidate = path.join(entry, name)
    if (isTrustedExecutable(candidate, dir)) return candidate
  }
  throw new PnpmError('PACK_APP_MACOS_SIGN_FAILED',
    `Cross-compiled macOS binary at ${outputFile} could not be ad-hoc signed with "ldid".`,
    { hint: 'Install ldid (https://github.com/ProcursusTeam/ldid) or re-sign the binary on macOS with "codesign --sign - <file>".' }
  )
}

function isTrustedExecutable (candidate: string, dir: string): boolean {
  let stat: fs.Stats
  try {
    stat = fs.statSync(candidate)
  } catch {
    return false
  }
  if (!stat.isFile()) return false
  if (isWithinDir(candidate, dir)) return false
  try {
    fs.accessSync(candidate, fs.constants.X_OK)
  } catch {
    return false
  }
  return true
}
