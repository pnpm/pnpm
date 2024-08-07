import path from 'path'
import PATH from 'path-name'

export function prependDirsToPath (prependDirs: string[], env = process.env): { name: string, value: string } {
  return {
    name: PATH,
    value: [
      ...prependDirs,
      ...(env[PATH] != null ? [env[PATH]] : []),
    ].join(path.delimiter),
  }
}
