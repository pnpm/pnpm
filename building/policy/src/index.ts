import { expandPackageVersionSpecs } from '@pnpm/config.version-policy'
import * as dp from '@pnpm/deps.path'
import type { AllowBuild, AllowBuildContext, DepPath } from '@pnpm/types'

const TRUSTED_RESOLVED_VIA = new Set(['npm-registry', 'jsr-registry', 'named-registry', 'workspace'])

export interface BuildPackageIdentitySource {
  depPath?: string
  resolution?: unknown
  resolvedVia?: string
}

export function isBuildExplicitlyDisallowed (depPath: DepPath, allowBuild?: AllowBuild): boolean {
  if (!allowBuild) return false
  const { name, version } = dp.parse(depPath)
  return allowBuild(name ?? '', version ?? '', {
    depPath: normalizeBuildDepPath(depPath),
  }) === false
}

export function createAllowBuildFunction (
  opts: {
    dangerouslyAllowAllBuilds?: boolean
    allowBuilds?: Record<string, boolean | string>
  }
): undefined | AllowBuild {
  if (opts.dangerouslyAllowAllBuilds) return () => true
  if (opts.allowBuilds != null) {
    const allowedPackageBuilds = new Set<string>()
    const disallowedPackageBuilds = new Set<string>()
    const allowedDepPathBuilds = new Set<string>()
    const disallowedDepPathBuilds = new Set<string>()
    for (const [pkg, value] of Object.entries(opts.allowBuilds)) {
      switch (value) {
        case true:
          addAllowBuildRule(pkg, {
            depPaths: allowedDepPathBuilds,
            packageSpecs: allowedPackageBuilds,
          })
          break
        case false:
          addAllowBuildRule(pkg, {
            depPaths: disallowedDepPathBuilds,
            packageSpecs: disallowedPackageBuilds,
          })
          break
      }
    }
    const expandedAllowed = expandPackageVersionSpecs(Array.from(allowedPackageBuilds))
    const expandedDisallowed = expandPackageVersionSpecs(Array.from(disallowedPackageBuilds))
    return (pkgName, version, context?: AllowBuildContext) => {
      const pkgWithVersion = `${pkgName}@${version}`
      const depPath = context?.depPath == null ? undefined : normalizeBuildDepPath(context.depPath)
      if (depPath != null && disallowedDepPathBuilds.has(depPath)) {
        return false
      }
      if (expandedDisallowed.has(pkgName) || expandedDisallowed.has(pkgWithVersion)) {
        return false
      }
      if (depPath != null && allowedDepPathBuilds.has(depPath)) {
        return true
      }
      if (expandedAllowed.has(pkgName) || expandedAllowed.has(pkgWithVersion)) {
        if (context?.trustPackageIdentity === false) return undefined
        return true
      }
      return undefined
    }
  }
  return undefined
}

export function createAllowBuildContext (source: BuildPackageIdentitySource): AllowBuildContext {
  return {
    depPath: source.depPath == null ? undefined : normalizeBuildDepPath(source.depPath),
    trustPackageIdentity: isPackageIdentityTrustedForBuild(source),
  }
}

export function normalizeBuildDepPath (depPath: string): string {
  return dp.getPkgIdWithPatchHash(depPath as DepPath)
}

function isPackageIdentityTrustedForBuild (source: BuildPackageIdentitySource): boolean {
  if (source.resolvedVia != null) {
    return TRUSTED_RESOLVED_VIA.has(source.resolvedVia)
  }
  const resolution = source.resolution
  if (!hasTrustedPackageVersionDepPath(source.depPath)) return false
  if (resolution == null) return true
  if (!isObject(resolution)) return false
  const resolutionType = typeof resolution.type === 'string' ? resolution.type : undefined
  if (resolutionType === 'variations') {
    const variants = Array.isArray(resolution.variants) ? resolution.variants : []
    return variants.every((variant) => isPackageIdentityTrustedForBuild({
      depPath: source.depPath,
      resolution: isObject(variant) ? variant.resolution : undefined,
    }))
  }
  if (resolutionType != null) return false
  if (resolution.gitHosted === true) return false
  return true
}

function hasTrustedPackageVersionDepPath (depPath?: string): boolean {
  if (depPath == null) return false
  const parsed = dp.parse(depPath)
  return parsed.name != null && parsed.version != null && parsed.nonSemverVersion == null
}

function addAllowBuildRule (
  pkg: string,
  target: {
    depPaths: Set<string>
    packageSpecs: Set<string>
  }
): void {
  if (isDepPathAllowBuildKey(pkg)) {
    target.depPaths.add(normalizeBuildDepPath(pkg))
  } else {
    target.packageSpecs.add(pkg)
  }
}

function isDepPathAllowBuildKey (pkg: string): boolean {
  if (normalizeBuildDepPath(pkg) !== pkg) return true
  if (pkg.includes('||')) return false
  const parsed = dp.parse(pkg)
  if (parsed.nonSemverVersion != null) return isSourceLikeDepPathVersion(parsed.nonSemverVersion)
  if (parsed.name != null || pkg.startsWith('@')) return false
  return pkg.includes('/') || pkg.includes(':')
}

function isSourceLikeDepPathVersion (version: string): boolean {
  return version.includes(':') || version.includes('/') || version.includes('#')
}

function isObject (value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === 'object'
}
