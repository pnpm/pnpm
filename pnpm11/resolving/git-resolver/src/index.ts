import assert from 'node:assert'
import util from 'node:util'

import { PnpmError, redactAndSanitize } from '@pnpm/error'
import type { DispatcherOptions } from '@pnpm/network.fetch'
import type { GitResolution, LatestInfo, LatestQuery, PkgResolutionId, ResolveOptions, ResolveResult, TarballResolution } from '@pnpm/resolving.resolver-base'
import semver from 'semver'

import { createGitHostedPkgId } from './createGitHostedPkgId.js'
import { lsRemote } from './lsRemote.js'
import { type HostedPackageSpec, parseBareSpecifier } from './parseBareSpecifier.js'

export { createGitHostedPkgId }

export type { HostedPackageSpec }

export interface GitResolveResult extends ResolveResult {
  normalizedBareSpecifier?: string
  resolution: GitResolution | TarballResolution
  resolvedVia: 'git-repository'
}

export type GitResolver = (
  wantedDependency: { bareSpecifier: string },
  opts?: Pick<ResolveOptions, 'currentPkg' | 'update'>
) => Promise<GitResolveResult | null>

export function createGitResolver (
  opts: DispatcherOptions
): GitResolver {
  return async function resolveGit (wantedDependency, resolveOpts?): Promise<GitResolveResult | null> {
    const parsedSpecFunc = parseBareSpecifier(wantedDependency.bareSpecifier, opts)

    if (parsedSpecFunc == null) return null

    // Skip resolution if we have currentPkg and not updating
    if (resolveOpts?.currentPkg && !resolveOpts.update) {
      const currentResolution = resolveOpts.currentPkg.resolution
      // Return existing resolution for git packages
      if ('type' in currentResolution && currentResolution.type === 'git') {
        return {
          id: resolveOpts.currentPkg.id,
          resolution: currentResolution as GitResolution,
          resolvedVia: 'git-repository',
        }
      }
      // Also handle tarballs from git (e.g., GitHub hosted)
      if ('tarball' in currentResolution && currentResolution.tarball) {
        return {
          id: resolveOpts.currentPkg.id,
          resolution: currentResolution as TarballResolution,
          resolvedVia: 'git-repository',
        }
      }
    }

    const parsedSpec = await parsedSpecFunc()
    const bareSpecifier = parsedSpec.gitCommittish == null || parsedSpec.gitCommittish === ''
      ? 'HEAD'
      : parsedSpec.gitCommittish
    let commit: string
    try {
      commit = await resolveRef(parsedSpec.fetchSpec, bareSpecifier, parsedSpec.gitRange)
    } catch (err: unknown) {
      assert(util.types.isNativeError(err))
      throw gitResolveError(err, wantedDependency.bareSpecifier, parsedSpec.fetchSpec)
    }
    let resolution: GitResolution | TarballResolution | undefined

    if ((parsedSpec.hosted != null) && !isSsh(parsedSpec.fetchSpec)) {
      // don't use tarball for ssh url, they are likely private repo
      const hosted = parsedSpec.hosted
      // use resolved committish
      hosted.committish = commit
      const tarball = hosted.tarball?.()

      if (tarball) {
        resolution = { tarball, gitHosted: true }
      }
    }

    if (resolution == null) {
      resolution = {
        commit,
        repo: parsedSpec.fetchSpec,
        type: 'git',
      }
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
      normalizedBareSpecifier: parsedSpec.normalizedBareSpecifier,
      resolution,
      resolvedVia: 'git-repository',
    }
  }
}

// Git deps have no concept of "latest" — we'd need to query the host's tag list
// to know about newer commits, which isn't a uniform thing across protocols.
// Claim the dep so the dispatcher stops; the caller still surfaces a
// ref-mismatch report if the lockfile shifted to a different commit.
export async function resolveLatestFromGit (query: LatestQuery): Promise<LatestInfo | undefined> {
  const bareSpecifier = query.wantedDependency.bareSpecifier
  if (!bareSpecifier) return undefined
  const parsedSpecFunc = parseBareSpecifier(bareSpecifier, {})
  if (parsedSpecFunc == null) return undefined
  return {}
}

function resolveVTags (vTags: string[], range: string): string | null {
  return semver.maxSatisfying(vTags, range, true)
}

export async function getRepoRefs (repo: string, ref: string | null): Promise<Record<string, string>> {
  // `--` keeps a repo URL that starts with a dash (e.g. from a malicious
  // config value) from being parsed as a git flag, matching the Rust runner.
  const gitArgs = ['--', repo]
  if (ref) {
    gitArgs.push(ref)
    // Also request the peeled ref for annotated tags (e.g., refs/tags/v1.0.0^{})
    // This is needed because annotated tags have their own SHA, and we need the commit SHA they point to
    gitArgs.push(`${ref}^{}`)
  }
  const result = await lsRemote(gitArgs, { retries: 1 })
  const refs: Record<string, string> = {}
  for (const line of result.stdout.split('\n')) {
    const [commit, refName] = line.split('\t')
    if (commit && refName) refs[refName] = commit
  }
  return refs
}

async function resolveRef (repo: string, ref: string, range?: string): Promise<string> {
  const committish = ref.match(/^[0-9a-f]{7,40}$/) !== null
  if (committish && ref.length === 40) {
    return ref
  }
  const refs = await getRepoRefs(repo, (range ?? committish) ? null : ref)
  const result = resolveRefFromRefs(refs, repo, ref, committish, range)
  if (committish && !result.startsWith(ref)) {
    throw new PnpmError('GIT_AMBIGUOUS_REF', `resolved commit ${result} from commit-ish reference ${ref}`)
  }
  return result
}

function resolveRefFromRefs (refs: { [ref: string]: string }, repo: string, ref: string, committish: boolean, range?: string): string {
  if (!range) {
    let commitId =
      refs[ref] ||
      refs[`refs/${ref}`] ||
      refs[`refs/tags/${ref}^{}`] || // prefer annotated tags
      refs[`refs/tags/${ref}`] ||
      refs[`refs/heads/${ref}`]

    if (!commitId) {
      // check for a partial commit
      // Use Set to deduplicate since multiple refs can point to the same commit
      const commits = committish ? [...new Set(Object.values(refs).filter((value: string) => value.startsWith(ref)))] : []
      if (commits.length === 1) {
        commitId = commits[0]
      } else {
        throw new Error(`Could not resolve ${ref} to a commit of ${redactAndSanitize(repo)}.`)
      }
    }

    return commitId
  } else {
    const vTags = [...new Set(
      Object.keys(refs)
        // using the same semantics of version tags as https://github.com/zkat/pacote
        .filter((key: string) => /^refs\/tags\/v?\d+\.\d+\.\d+(?:[-+].+)?(?:\^\{\})?$/.test(key))
        .map((key: string) => {
          return key
            .replace(/^refs\/tags\//, '')
            .replace(/\^\{\}$/, '') // accept annotated tags
        })
        .filter((key: string) => semver.valid(key, true))
    )]
    const refVTag = resolveVTags(vTags, range)
    const commitId = refVTag &&
      (refs[`refs/tags/${refVTag}^{}`] || // prefer annotated tags
        refs[`refs/tags/${refVTag}`])

    if (!commitId) {
      throw new Error(`Could not resolve ${range} to a commit of ${redactAndSanitize(repo)}. Available versions are: ${vTags.join(', ')}`)
    }

    return commitId
  }
}

/**
 * Restate a failed `git ls-remote` as `ERR_PNPM_GIT_RESOLVE_FAILED`, naming the
 * dependency it was resolving. Errors that describe the refs the remote did
 * return (an unknown ref, an ambiguous commit-ish) already say which repository
 * they came from and are left alone.
 */
function gitResolveError (err: Error, bareSpecifier: string, repo: string): Error {
  if ((err as { code?: string }).code !== 'ERR_PNPM_GIT_LS_REMOTE_FAILED') return err
  return new PnpmError(
    'GIT_RESOLVE_FAILED',
    `Failed to resolve git dependency "${redactAndSanitize(bareSpecifier)}": ${err.message}`,
    { hint: httpsTransportHint(repo) }
  )
}

/**
 * Guidance for a specifier that resolved over HTTPS on a machine whose git
 * cannot use that transport, or `undefined` when the resolution already went
 * over SSH — there, the transport that failed is the one the specifier asked
 * for.
 *
 * Substituting the transport is git's job rather than pnpm's: the URL pnpm
 * records has to work for every machine that installs the lockfile, while
 * `insteadOf` rewrites it for this one only.
 */
function httpsTransportHint (repo: string): string | undefined {
  let url: URL
  try {
    url = new URL(repo)
  } catch {
    return undefined
  }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') return undefined
  const host = redactAndSanitize(url.host)
  const hostname = redactAndSanitize(url.hostname)
  return `pnpm resolves this specifier over HTTPS because it does not ask for SSH, and the URL it records has to work on every machine that installs the lockfile.

If git can only reach ${hostname} over SSH here, substitute the transport locally, leaving the recorded URL alone:

    git config --global url."git@${hostname}:".insteadOf "${url.protocol}//${host}/"`
}

function isSsh (gitSpec: string): boolean {
  return gitSpec.slice(0, 10) === 'git+ssh://' ||
    gitSpec.slice(0, 4) === 'git@'
}
