import path = require('path')

export function pathToLocalPkg (pkgName: string) {
  return path.join(__dirname, '..', 'packages', pkgName)
}

export function local (pkgName: string) {
  return `file:${pathToLocalPkg(pkgName)}`
}
