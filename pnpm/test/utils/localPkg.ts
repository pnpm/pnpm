import path from 'node:path'

export function pathToLocalPkg(pkgName: string): string {
  return path.join(__dirname, '../../../../fixtures', pkgName)
}

export function local(pkgName: string): string {
  return `file:${pathToLocalPkg(pkgName)}`
}
