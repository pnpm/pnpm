import mem from 'mem'
import * as execa from 'execa'

export function getSystemNodeVersionNonCached (): string {
  // @ts-expect-error
  if (process['pkg'] != null) {
    return execa.sync('node', ['--version']).stdout.toString()
  }
  return process.version
}

export const getSystemNodeVersion = mem(getSystemNodeVersionNonCached)
