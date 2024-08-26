import path from 'path'
import PATH from 'path-name'

export interface PrependDirsToPathResult {
  name: string
  value: string
  updated: boolean
}

export function prependDirsToPath (prependDirs: string[], env = process.env): PrependDirsToPathResult {
  const prepend = prependDirs.join(path.delimiter)
  if (env[PATH] != null && (env[PATH] === prepend || env[PATH]!.startsWith(`${prepend}${path.delimiter}`))) {
    return {
      name: PATH,
      value: env[PATH]!,
      updated: false,
    }
  }
  return {
    name: PATH,
    value: [
      prepend,
      ...(env[PATH] != null ? [env[PATH]] : []),
    ].join(path.delimiter),
    updated: true,
  }
}
