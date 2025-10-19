import fs from 'fs'

export function createAllowBuildFunction (
  opts: {
    neverBuiltDependencies?: string[]
    onlyBuiltDependencies?: string[]
    onlyBuiltDependenciesFile?: string
  }
): undefined | ((pkgName: string) => boolean) {
  if (opts.onlyBuiltDependenciesFile || opts.onlyBuiltDependencies != null) {
    const onlyBuiltDeps = opts.onlyBuiltDependencies ?? []
    if (opts.onlyBuiltDependenciesFile) {
      onlyBuiltDeps.push(...JSON.parse(fs.readFileSync(opts.onlyBuiltDependenciesFile, 'utf8')))
    }
    const onlyBuiltDependencies = new Set(onlyBuiltDeps)
    return (pkgName) => onlyBuiltDependencies.has(pkgName)
  }
  if (opts.neverBuiltDependencies != null && opts.neverBuiltDependencies.length > 0) {
    const neverBuiltDependencies = new Set(opts.neverBuiltDependencies)
    return (pkgName) => !neverBuiltDependencies.has(pkgName)
  }
  return undefined
}

