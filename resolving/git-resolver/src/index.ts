import { type TarballResolution, type GitResolution, type ResolveResult, type PkgResolutionId } from '@pnpm/resolver-base'
import git from 'graceful-git'
import semver from 'semver'
import { parsePref, type HostedPackageSpec } from './parsePref'
import { createGitHostedPkgId } from './createGitHostedPkgId'
import { type AgentOptions } from '@pnpm/network.agent'

export { createGitHostedPkgId }

export type { HostedPackageSpec }

export type GitResolver = (wantedDependency: {
  pref: string
}) => Promise<ResolveResult | null>

export function createGitResolver (
  opts: AgentOptions
): GitResolver {
  return async function resolveGit (wantedDependency): Promise<ResolveResult | null> {
    const parsedSpec = await parsePref(wantedDependency.pref, opts)

    if (parsedSpec == null) return null

    const pref = parsedSpec.gitCommittish == null || parsedSpec.gitCommittish === ''
      ? 'HEAD'
      : parsedSpec.gitCommittish
    const commit = await resolveRef(parsedSpec.fetchSpec, pref, parsedSpec.gitRange)
    let resolution

    if ((parsedSpec.hosted != null) && !isSsh(parsedSpec.fetchSpec)) {
      // don't use tarball for ssh url, they are likely private repo
      const hosted = parsedSpec.hosted
      // use resolved committish
      hosted.committish = commit
      const tarball = hosted.tarball?.()

      if (tarball) {
        resolution = { tarball } as TarballResolution
      }
    }

    if (resolution == null) {
      resolution = {
        commit,
        repo: parsedSpec.fetchSpec,
        type: 'git',
      } as GitResolution
    }

    if (parsedSpec.path) {
      resolution.path = parsedSpec.path
    }

    let id: PkgResolutionId
    if ('tarball' in resolution) {
      id = resolution.tarball as PkgResolutionId
      if (resolution.path) {
        id = `${id}#path:${resolution.path}` as PkgResolutionId
      }
    } else {
      id = createGitHostedPkgId(resolution)
    }

    return {
      id,
      normalizedPref: parsedSpec.normalizedPref,
      resolution,
      resolvedVia: 'git-repository',
    }
  }
}

function resolveVTags (vTags: string[], range: string): string | null {
  return semver.maxSatisfying(vTags, range, true)
}

async function getRepoRefs (repo: string, ref: string | null): Promise<Record<string, string>> {
  const gitArgs = [repo]
  if (ref !== 'HEAD') {
    gitArgs.unshift('--refs')
  }
  if (ref) {
    gitArgs.push(ref)
  }
  // graceful-git by default retries 10 times, reduce to single retry
  const result = await git(['ls-remote', ...gitArgs], { retries: 1 })
  const refs: Record<string, string> = {}
  for (const line of result.stdout.split('\n')) {
    const [commit, refName] = line.split('\t')
    refs[refName] = commit
  }
  return refs
}

async function resolveRef (repo: string, ref: string, range?: string): Promise<string> {
  if (ref.match(/^[0-9a-f]{7,40}$/) != null) {
    return ref
  }
  const refs = await getRepoRefs(repo, range ? null : ref)
  return resolveRefFromRefs(refs, repo, ref, range)
}

function resolveRefFromRefs (refs: { [ref: string]: string }, repo: string, ref: string, range?: string): string {
  if (!range) {
    const commitId =
      refs[ref] ||
      refs[`refs/${ref}`] ||
      refs[`refs/tags/${ref}^{}`] || // prefer annotated tags
      refs[`refs/tags/${ref}`] ||
      refs[`refs/heads/${ref}`]

    if (!commitId) {
      throw new Error(`Could not resolve ${ref} to a commit of ${repo}.`)
    }

    return commitId
  } else {
    const vTags =
      Object.keys(refs)
        // using the same semantics of version tags as https://github.com/zkat/pacote
        .filter((key: string) => /^refs\/tags\/v?\d+\.\d+\.\d+(?:[-+].+)?(?:\^\{\})?$/.test(key))
        .map((key: string) => {
          return key
            .replace(/^refs\/tags\//, '')
            .replace(/\^\{\}$/, '') // accept annotated tags
        })
        .filter((key: string) => semver.valid(key, true))
    const refVTag = resolveVTags(vTags, range)
    const commitId = refVTag &&
      (refs[`refs/tags/${refVTag}^{}`] || // prefer annotated tags
      refs[`refs/tags/${refVTag}`])

    if (!commitId) {
      throw new Error(`Could not resolve ${range} to a commit of ${repo}. Available versions are: ${vTags.join(', ')}`)
    }

    return commitId
  }
}

function isSsh (gitSpec: string): boolean {
  return gitSpec.slice(0, 10) === 'git+ssh://' ||
    gitSpec.slice(0, 4) === 'git@'
}
