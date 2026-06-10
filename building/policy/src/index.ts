import { expandPackageVersionSpecs } from '@pnpm/config.version-policy'
import * as dp from '@pnpm/deps.path'
import type { AllowBuild, AllowBuildContext, DepPath } from '@pnpm/types'

export function isBuildExplicitlyDisallowed (depPath: DepPath, allowBuild?: AllowBuild): boolean {
  return allowBuild?.(depPath) === false
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
    return (depPath, context?: AllowBuildContext) => {
      const pkgIdWithPatchHash = dp.getPkgIdWithPatchHash(depPath)
      if (disallowedDepPathBuilds.has(pkgIdWithPatchHash)) {
        return false
      }
      const { name, version, nonSemverVersion } = dp.parse(depPath)
      const nameAtVersion = name != null && version != null ? `${name}@${version}` : undefined
      if (
        (name != null && expandedDisallowed.has(name)) ||
        (nameAtVersion != null && expandedDisallowed.has(nameAtVersion))
      ) {
        return false
      }
      if (allowedDepPathBuilds.has(pkgIdWithPatchHash)) {
        return true
      }
      // Package-name rules require a trusted package identity. A
      // registry-style depPath (name@semver) is the trust signal: the
      // lockfile verification gate rejects lockfiles where such a key is
      // backed by a non-registry resolution, so by the time scripts can
      // run, the shape proves the artifact came from a registry. The
      // override exists for callers that must evaluate name rules under
      // legacy semantics (e.g. comparing against a policy recorded before
      // identity trust existed).
      const trustPackageIdentity = context?.trustPackageIdentity ??
        (name != null && version != null && nonSemverVersion == null)
      if (!trustPackageIdentity) return undefined
      if (
        (name != null && expandedAllowed.has(name)) ||
        (nameAtVersion != null && expandedAllowed.has(nameAtVersion))
      ) {
        return true
      }
      return undefined
    }
  }
  return undefined
}

/**
 * The `allowBuilds` key under which an ignored build should be approved:
 * the package name for registry packages, the peer-suffix-free depPath for
 * git/tarball artifacts, whose name alone must not approve builds.
 */
export function allowBuildKeyFromIgnoredBuild (depPath: DepPath): string {
  const pkgIdWithPatchHash = dp.getPkgIdWithPatchHash(depPath)
  const parsed = dp.parse(pkgIdWithPatchHash)
  if (parsed.nonSemverVersion != null || parsed.name == null) return pkgIdWithPatchHash
  return parsed.name
}

function addAllowBuildRule (
  pkg: string,
  target: {
    depPaths: Set<string>
    packageSpecs: Set<string>
  }
): void {
  if (isDepPathAllowBuildKey(pkg)) {
    target.depPaths.add(dp.removePeersSuffix(pkg))
  } else {
    target.packageSpecs.add(pkg)
  }
}

function isDepPathAllowBuildKey (pkg: string): boolean {
  if (dp.removePeersSuffix(pkg) !== pkg) return true
  if (pkg.includes('||')) return false
  const parsed = dp.parse(pkg)
  if (parsed.nonSemverVersion != null) return isSourceLikeDepPathVersion(parsed.nonSemverVersion)
  if (parsed.name != null || pkg.startsWith('@')) return false
  return pkg.includes('/') || pkg.includes(':')
}

function isSourceLikeDepPathVersion (version: string): boolean {
  return version.includes(':') || version.includes('/') || version.includes('#')
}
