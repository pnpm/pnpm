import npa = require('npm-package-arg')
import {Dependencies} from './types'
import {PackageSpec} from './resolve'

export default function (deps: Dependencies): PackageSpec[] {
  if (!deps) return []
  const pkgs = Object.keys(deps).map(pkgName => `${pkgName}@${deps[pkgName]}`)
  return <PackageSpec[]>pkgs.map(npa)
}
