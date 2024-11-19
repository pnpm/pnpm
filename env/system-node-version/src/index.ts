import { detectIfCurrentPkgIsExecutable } from '@pnpm/cli-meta'
import mem from 'mem'
import * as execa from 'execa'

export function getSystemNodeVersionNonCached (): string | undefined {
  if (detectIfCurrentPkgIsExecutable()) {
    try {
      return execa.sync('node', ['--version']).stdout.toString()
    } catch {
      // Node.js is not installed on the system
      return undefined
    }
  }
  return process.version
}

export const getSystemNodeVersion = mem(getSystemNodeVersionNonCached)
