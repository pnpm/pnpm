import { type AllowBuild } from '@pnpm/types'
import { expandPackageVersionSpecs } from '@pnpm/config.version-policy'

export function createAllowBuildFunction (
  opts: {
    onlyBuiltDependencies?: string[]
  }
): undefined | AllowBuild {
  if (opts.onlyBuiltDependencies != null) {
    const onlyBuiltDependencies = expandPackageVersionSpecs(opts.onlyBuiltDependencies)
    return (pkgName, version) => onlyBuiltDependencies.has(pkgName) || onlyBuiltDependencies.has(`${pkgName}@${version}`)
  }
  return undefined
}
