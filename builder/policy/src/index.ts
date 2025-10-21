import { type AllowBuild } from '@pnpm/types'
import { expandPackageVersionSpecs } from '@pnpm/config.version-policy'
import fs from 'fs'

export function createAllowBuildFunction (
  opts: {
    neverBuiltDependencies?: string[]
    onlyBuiltDependencies?: string[]
    onlyBuiltDependenciesFile?: string
  }
): undefined | AllowBuild {
  if (opts.onlyBuiltDependenciesFile != null || opts.onlyBuiltDependencies != null) {
    const onlyBuiltDeps = opts.onlyBuiltDependencies ?? []
    if (opts.onlyBuiltDependenciesFile) {
      onlyBuiltDeps.push(...JSON.parse(fs.readFileSync(opts.onlyBuiltDependenciesFile, 'utf8')))
    }
    const onlyBuiltDependencies = expandPackageVersionSpecs(onlyBuiltDeps)
    return (pkgName, version) => onlyBuiltDependencies.has(pkgName) || onlyBuiltDependencies.has(`${pkgName}@${version}`)
  }
  if (opts.neverBuiltDependencies != null && opts.neverBuiltDependencies.length > 0) {
    const neverBuiltDependencies = new Set(opts.neverBuiltDependencies)
    return (pkgName) => !neverBuiltDependencies.has(pkgName)
  }
  return undefined
}
