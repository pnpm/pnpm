import { expandPackageVersionSpecs } from '@pnpm/config.version-policy'
import * as dp from '@pnpm/deps.path'
import type { AllowBuild, DepPath } from '@pnpm/types'

export function isBuildExplicitlyDisallowed (depPath: DepPath, allowBuild?: AllowBuild): boolean {
  if (!allowBuild) return false
  const { name, version } = dp.parse(depPath)
  if (!name || !version) return false
  return allowBuild(name, version) === false
}

export function createAllowBuildFunction (
  opts: {
    dangerouslyAllowAllBuilds?: boolean
    allowBuilds?: Record<string, boolean | string>
  }
): undefined | AllowBuild {
  if (opts.dangerouslyAllowAllBuilds) return () => true
  if (opts.allowBuilds != null) {
    const allowedBuilds = new Set<string>()
    const disallowedBuilds = new Set<string>()
    for (const [pkg, value] of Object.entries(opts.allowBuilds)) {
      switch (value) {
        case true:
          allowedBuilds.add(pkg)
          break
        case false:
          disallowedBuilds.add(pkg)
          break
      }
    }
    const expandedAllowed = expandPackageVersionSpecs(Array.from(allowedBuilds))
    const expandedDisallowed = expandPackageVersionSpecs(Array.from(disallowedBuilds))
    return (pkgName, version) => {
      const pkgWithVersion = `${pkgName}@${version}`
      if (expandedDisallowed.has(pkgName) || expandedDisallowed.has(pkgWithVersion)) {
        return false
      }
      if (expandedAllowed.has(pkgName) || expandedAllowed.has(pkgWithVersion)) {
        return true
      }
      return undefined
    }
  }
  return undefined
}
