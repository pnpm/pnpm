import { type AllowBuild, type DepPath } from '@pnpm/types'
import { expandPackageVersionSpecs } from '@pnpm/config.version-policy'
import * as dp from '@pnpm/dependency-path'
import fs from 'fs'

export function createAllowBuildFunction (
  opts: {
    dangerouslyAllowAllBuilds?: boolean
    neverBuiltDependencies?: string[]
    onlyBuiltDependencies?: string[]
    onlyBuiltDependenciesFile?: string
  }
): undefined | AllowBuild {
  if (opts.dangerouslyAllowAllBuilds) return () => true
  if (opts.onlyBuiltDependenciesFile != null || opts.onlyBuiltDependencies != null) {
    const onlyBuiltDeps = opts.onlyBuiltDependencies ?? []
    if (opts.onlyBuiltDependenciesFile) {
      onlyBuiltDeps.push(...JSON.parse(fs.readFileSync(opts.onlyBuiltDependenciesFile, 'utf8')))
    }
    const allowedDepPaths = new Set<string>()
    const allowedPackageSpecs: string[] = []
    for (const entry of onlyBuiltDeps) {
      if (isDepPathAllowBuildKey(entry)) {
        allowedDepPaths.add(dp.removePeersSuffix(entry))
      } else {
        allowedPackageSpecs.push(entry)
      }
    }
    const onlyBuiltDependencies = expandPackageVersionSpecs(allowedPackageSpecs)
    return (depPath) => {
      if (allowedDepPaths.has(dp.getPkgIdWithPatchHash(depPath))) return true
      const { name, version, nonSemverVersion } = dp.parse(depPath)
      // Package-name rules require a trusted package identity. A
      // registry-style depPath (name@semver) is the trust signal: the
      // resolution shape check rejects lockfiles where such a key is
      // backed by a non-registry resolution, so by the time scripts can
      // run, the shape proves the artifact came from a registry.
      if (name == null || version == null || nonSemverVersion != null) return false
      return onlyBuiltDependencies.has(name) || onlyBuiltDependencies.has(`${name}@${version}`)
    }
  }
  if (opts.neverBuiltDependencies != null && opts.neverBuiltDependencies.length > 0) {
    const neverBuiltDependencies = new Set(opts.neverBuiltDependencies)
    return (depPath) => {
      const { name } = dp.parse(depPath)
      return name == null || !neverBuiltDependencies.has(name)
    }
  }
  return undefined
}

/**
 * The `onlyBuiltDependencies` key under which an ignored build should be
 * approved: the package name for registry packages, the peer-suffix-free
 * depPath for git/tarball artifacts, whose name alone must not approve
 * builds.
 */
export function allowBuildKeyFromIgnoredBuild (depPath: DepPath): string {
  const pkgIdWithPatchHash = dp.getPkgIdWithPatchHash(depPath)
  const parsed = dp.parse(pkgIdWithPatchHash)
  if (parsed.nonSemverVersion != null || parsed.name == null) return pkgIdWithPatchHash
  return parsed.name
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
