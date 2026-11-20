import path from 'node:path'

import { readProjectManifestOnly } from '@pnpm/cli.utils'
import { prependDirsToPath } from '@pnpm/shell.path'
import which from 'which'

import { getNearestProgram, getNearestScript } from './buildCommandNotFoundHint.js'

export async function createExecCommandNotFoundHint (
  programName: string,
  opts: {
    dir: string
    implicitlyFellbackFromRun: boolean
    workspaceDir?: string
    modulesDir: string
  }
): Promise<string | undefined> {
  if (opts.implicitlyFellbackFromRun) {
    let nearestScript: string | null | undefined
    try {
      nearestScript = getNearestScript(programName, (await readProjectManifestOnly(opts.dir)).scripts)
    } catch {}
    if (nearestScript) {
      return `Did you mean "pnpm ${nearestScript}"?`
    }
    const nearestProgram = getNearestProgram({
      programName,
      dir: opts.dir,
      workspaceDir: opts.workspaceDir,
      modulesDir: opts.modulesDir,
    })
    if (nearestProgram) {
      return `Did you mean "pnpm ${nearestProgram}"?`
    }
    return undefined
  }
  const nearestProgram = getNearestProgram({
    programName,
    dir: opts.dir,
    workspaceDir: opts.workspaceDir,
    modulesDir: opts.modulesDir,
  })
  if (nearestProgram) {
    return `Did you mean "pnpm exec ${nearestProgram}"?`
  }
  return undefined
}

export interface CommandError extends Error {
  originalMessage: string
  shortMessage: string
}

export function isErrorCommandNotFound (command: string, error: CommandError, prefix: string, prependPaths: string[]): boolean {
  if (error.originalMessage === `spawn ${command} ENOENT`) {
    return true
  }

  // On Windows, execa 9.x uses cross-spawn only for command parsing (not spawning),
  // so cross-spawn's ENOENT hook never fires. Non-existent commands get wrapped as
  // `cmd.exe /c <command>` which exits with code 1 instead of emitting ENOENT.
  // Fall back to checking if the command exists in PATH, resolving relative paths
  // against the exec prefix to correctly handle --filter contexts.
  if (process.platform === 'win32') {
    const absolutePrependPaths = prependPaths.map(prependPath => path.resolve(prefix, prependPath))
    const { value: searchPath } = prependDirsToPath(absolutePrependPaths)
    return !which.sync(command, { nothrow: true, path: searchPath })
  }

  return false
}
