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
    const allowedGitRepoBuilds = new Set<string>()
    const disallowedGitRepoBuilds = new Set<string>()
    for (const [pkg, value] of Object.entries(opts.allowBuilds)) {
      switch (value) {
        case true:
          addAllowBuildRule(pkg, {
            depPaths: allowedDepPathBuilds,
            gitRepos: allowedGitRepoBuilds,
            packageSpecs: allowedPackageBuilds,
          })
          break
        case false:
          addAllowBuildRule(pkg, {
            depPaths: disallowedDepPathBuilds,
            gitRepos: disallowedGitRepoBuilds,
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
      const gitRepoKey = getGitRepoAllowBuildKeyFromDepPath(pkgIdWithPatchHash)
      if (gitRepoKey != null && disallowedGitRepoBuilds.has(gitRepoKey)) {
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
      if (gitRepoKey != null && allowedGitRepoBuilds.has(gitRepoKey)) {
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
    gitRepos: Set<string>
    packageSpecs: Set<string>
  }
): void {
  if (isGitRepoAllowBuildKey(pkg)) {
    target.gitRepos.add(pkg)
    return
  }
  if (isDepPathAllowBuildKey(pkg)) {
    target.depPaths.add(dp.removePeersSuffix(pkg))
  } else {
    target.packageSpecs.add(pkg)
  }
}

function isGitRepoAllowBuildKey (pkg: string): boolean {
  return !pkg.includes('#') && isGitRepoDepPath(pkg)
}

function getGitRepoAllowBuildKeyFromDepPath (depPath: string): string | undefined {
  if (isGitRepoDepPath(depPath)) {
    const refStart = depPath.indexOf('#')
    return refStart === -1 ? depPath : depPath.slice(0, refStart)
  }
  // Packages installed from a git host as a downloaded tarball (e.g. the
  // `github:` shortcut, which pnpm fetches from codeload.github.com rather
  // than cloning) have a depPath built from the tarball URL, not a `git+`
  // clone URL, so the check above misses them. Normalize the tarball URL back
  // to the same `git+https://<host>/<repo>.git` repo key that a clone of the
  // same repository would produce, so a single hashless `allowBuilds` entry
  // approves the package however pnpm happened to fetch it.
  return gitHostedTarballRepoKey(depPath)
}

function isGitRepoDepPath (depPath: string): boolean {
  return depPath.startsWith('git+') || depPath.includes('@git+')
}

// Reconstructs the committish-free repository URL from the download URL of a
// git host that pnpm fetches as a tarball instead of cloning. The patterns
// mirror the tarball templates in @pnpm/git-resolver (which come from
// hosted-git-info, except GitLab's, which that package overrides). The host of
// each known template is anchored so a look-alike download host (e.g.
// `codeload.github.com.example.com`) cannot be rewritten into an unrelated
// repo key.
const GIT_HOSTED_TARBALL_REPO_URL_MATCHERS: Array<(tarballUrl: string) => string | undefined> = [
  // GitHub: https://codeload.github.com/<owner>/<repo>/tar.gz/<committish>
  makeTarballRepoUrlMatcher(/^https:\/\/codeload\.github\.com\/([^/]+)\/([^/]+)\/tar\.gz\//, (m) => `github.com/${m[1]}/${m[2]}`),
  // Bitbucket: https://bitbucket.org/<owner>/<repo>/get/<committish>.tar.gz
  makeTarballRepoUrlMatcher(/^https:\/\/bitbucket\.org\/([^/]+)\/([^/]+)\/get\//, (m) => `bitbucket.org/${m[1]}/${m[2]}`),
  // GitLab (incl. self-hosted): https://<host>/<group…>/<repo>/-/archive/<ref>/…
  // The project path may contain nested groups, so match up to the
  // `/-/archive/<ref>/` marker rather than a fixed number of path segments.
  makeTarballRepoUrlMatcher(/^https:\/\/([^/]+)\/(.+?)\/-\/archive\/[^/]+\//, (m) => `${m[1]}/${m[2]}`),
]

function makeTarballRepoUrlMatcher (
  re: RegExp,
  toRepoPath: (m: RegExpExecArray) => string
): (tarballUrl: string) => string | undefined {
  return (tarballUrl) => {
    const match = re.exec(tarballUrl)
    return match == null ? undefined : `git+https://${toRepoPath(match)}.git`
  }
}

function gitHostedTarballRepoKey (pkgIdWithPatchHash: string): string | undefined {
  const { name, nonSemverVersion } = dp.parse(pkgIdWithPatchHash)
  if (name == null || nonSemverVersion == null) return undefined
  for (const match of GIT_HOSTED_TARBALL_REPO_URL_MATCHERS) {
    const repoUrl = match(nonSemverVersion)
    if (repoUrl != null) return `${name}@${repoUrl}`
  }
  return undefined
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
