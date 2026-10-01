import path from 'node:path'

export const isWindows = process.platform === 'win32'

export function isAscii (text: string): boolean {
  return Buffer.byteLength(text) === text.length
}

export interface NormalizedPathEnvVar {
  win32: string
  posix: string
  [index: number]: { win32: string, posix: string }
}

export function normalizePathEnvVar (nodePath: undefined | string | string[]): NormalizedPathEnvVar {
  if (!nodePath || !nodePath.length) {
    return {
      win32: '',
      posix: '',
    }
  }
  const split = (typeof nodePath === 'string' ? nodePath.split(path.delimiter) : Array.from(nodePath))
  const result = {} as NormalizedPathEnvVar
  for (let entryIndex = 0; entryIndex < split.length; entryIndex++) {
    const win32 = split[entryIndex].split('/').join('\\')
    const posix = isWindows
      ? split[entryIndex].split('\\').join('/').replace(/^([^:\\/]*):/, (_, drive) => `/mnt/${drive.toLowerCase()}`)
      : split[entryIndex]

    result.win32 = result.win32 ? `${result.win32};${win32}` : win32
    result.posix = result.posix ? `${result.posix}:${posix}` : posix

    result[entryIndex] = { win32, posix }
  }
  return result
}

/**
 * Escape `text` for a `.cmd` file, where `%` would otherwise expand as a
 * variable reference, even inside double quotes.
 */
export function cmdEscape (text: string): string {
  return text.replaceAll('%', '%%')
}

export function shSingleQuote (text: string): string {
  return `'${text.replaceAll("'", "'\\''")}'`
}

export function shimTarget (src: string): string {
  return `cmd-shim-target=${src.split('\\').join('/')}`
}
