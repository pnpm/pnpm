import { type AllowBuild } from '@pnpm/types'
import { expandPackageVersionSpecs } from '@pnpm/config.version-policy'

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
