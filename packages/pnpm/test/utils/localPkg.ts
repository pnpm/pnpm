import path = require('path')

export function pathToLocalPkg (pkgName: string) {
  return path.join(__dirname, '../../../../fixtures', pkgName)
}

export function local (pkgName: string) {
  return `file:${pathToLocalPkg(pkgName)}`
}
