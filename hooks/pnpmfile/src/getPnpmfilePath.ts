import path from 'path'

export function getPnpmfilePath (prefix: string, pnpmfile?: string): string {
  if (!pnpmfile) {
    pnpmfile = '.pnpmfile.cjs'
  } else if (path.isAbsolute(pnpmfile)) {
    return pnpmfile
  }
  return path.join(prefix, pnpmfile)
}
