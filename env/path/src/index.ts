import path from 'path'
import PATH from 'path-name'

export function prependDirsToPath (prependDirs: string[]): { name: string, value: string } {
  return {
    name: PATH,
    value: [
      ...prependDirs,
      ...(process.env[PATH] != null ? [process.env[PATH]] : []),
    ].join(path.delimiter),
  }
}
