import { detectIfCurrentPkgIsExecutable } from '@pnpm/cli-meta'
import mem from 'mem'
import * as execa from 'execa'

export function getSystemNodeVersionNonCached (): string {
  if (detectIfCurrentPkgIsExecutable()) {
    return execa.sync('node', ['--version']).stdout.toString()
  }
  return process.version
}

export const getSystemNodeVersion = mem(getSystemNodeVersionNonCached)
