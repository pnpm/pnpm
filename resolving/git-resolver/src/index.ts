import type {
  TarballResolution,
  GitResolution,
  ResolveResult,
  PkgResolutionId,
} from '@pnpm/resolver-base'
import { type HostedPackageSpec, parsePref, type PackageSpec } from './parsePref'
import { createGitHostedPkgId } from './createGitHostedPkgId'
import { getCommitFromRange, isSsh, getCommitFromRef } from './util'

export { createGitHostedPkgId }

export type { PackageSpec }

export type GitResolver = (wantedDependency: { pref: string }) => Promise<ResolveResult | null>

export function createGitResolver (opts: unknown): GitResolver {
  return async function resolveGit (
    wantedDependency
  ): Promise<ResolveResult | null> {
    const spec = await parsePref(wantedDependency.pref)
    if (spec === null) return null

    const resolution = resolveSpec(spec)

    const id =
      'tarball' in resolution
        ? ((resolution.path
          ? `${resolution.tarball}#path:${resolution.path}`
          : resolution.tarball) as PkgResolutionId)
        : createGitHostedPkgId(resolution)

    return {
      id,
      normalizedPref: spec.normalizedPref,
      resolution,
      resolvedVia: 'git-repository',
    }
  }
}

function resolveSpec (spec: PackageSpec | HostedPackageSpec): GitResolution | TarballResolution {
  const commit = spec.gitRange
    ? getCommitFromRange(spec.fetchSpec, spec.gitRange)
    : getCommitFromRef(spec.fetchSpec, spec.gitCommittish || 'HEAD') // eslint-disable-line @typescript-eslint/prefer-nullish-coalescing

  const tarball = 'hosted' in spec &&
    // don't use tarball for private repo
    !isSsh(spec.fetchSpec) &&
    // use resolved committish
    spec.hosted.tarball({ committish: commit })

  const resolution: GitResolution | TarballResolution = tarball
    ? { tarball }
    : {
      commit,
      repo: spec.fetchSpec,
      type: 'git',
    }

  if (spec.path) {
    resolution.path = spec.path
  }
  return resolution
}
