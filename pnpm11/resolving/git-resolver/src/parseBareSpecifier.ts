// cspell:ignore sshurl
import urlLib, { URL } from 'node:url'

import { type DispatcherOptions, fetchWithDispatcher } from '@pnpm/network.fetch'
import HostedGit from 'hosted-git-info'

import { lsRemote } from './lsRemote.js'

export interface HostedPackageSpec {
  fetchSpec: string
  hosted?: {
    type: string
    user: string
    project: string
    committish: string
    tarball: () => string | undefined
  }
  normalizedBareSpecifier: string
  gitCommittish: string | null
  gitRange?: string
  path?: string
}

const gitProtocols = new Set([
  'git',
  'git+http',
  'git+https',
  'git+rsync',
  'git+ftp',
  'git+file',
  'git+ssh',
  'ssh',
])

export function parseBareSpecifier (bareSpecifier: string, opts: DispatcherOptions): null | (() => Promise<HostedPackageSpec>) {
  const hosted = HostedGit.fromUrl(bareSpecifier)
  if (hosted != null) {
    return () => fromHostedGit(hosted, opts)
  }
  const colonsPos = bareSpecifier.indexOf(':')
  if (colonsPos === -1) return null
  const protocol = bareSpecifier.slice(0, colonsPos)

  // Also detect http/https URLs ending in .git as git repositories
  const isGitUrl = gitProtocols.has(protocol.toLocaleLowerCase()) ||
    ((protocol === 'http' || protocol === 'https') && /\.git(?:#|$)/.test(bareSpecifier))
  if (protocol && isGitUrl) {
    const correctBareSpecifier = correctUrl(bareSpecifier)
    const url = new URL(correctBareSpecifier)
    if (!url?.protocol) return null

    const hash = (url.hash?.length > 1) ? decodeURIComponent(url.hash.slice(1)) : null
    return async () => ({
      fetchSpec: urlToFetchSpec(url),
      normalizedBareSpecifier: bareSpecifier,
      ...parseGitParams(hash),
    })
  }
  return null
}

function urlToFetchSpec (url: URL): string {
  url.hash = ''
  const fetchSpec = urlLib.format(url)
  if (fetchSpec.startsWith('git+')) {
    return fetchSpec.slice(4)
  }
  return fetchSpec
}

async function fromHostedGit (hosted: any, dispatcherOptions: DispatcherOptions): Promise<HostedPackageSpec> { // eslint-disable-line
  let fetchSpec: string | null = null
  const httpsUrl: string | null = hosted.https({ noCommittish: true, noGitPlus: true })
  const sshUrl: string | null = hosted.ssh({ noCommittish: true })
  // SSH is probed before the HTTPS fallbacks (and used as the last-resort guess)
  // only when the user explicitly wrote an SSH URL. For every other representation
  // (`shortcut`, `https`, ...) an SSH remote was never asked for, and recording one
  // in the lockfile breaks installs in environments without SSH keys, so every
  // HTTPS transport is exhausted first.
  //
  // Such a specifier therefore resolves over HTTPS whether or not git can reach
  // the host that way on this machine, and its HTTPS access is not probed at all:
  // the probe could not change what is recorded, and a machine that reaches the
  // host only over SSH substitutes the transport itself through git's
  // `url.<base>.insteadOf`. Only an explicit SSH URL, which HTTPS may displace,
  // is worth the round-trip.
  const preferSsh = hosted.default === 'sshurl'
  const repoIsPublic = httpsUrl != null && await isRepoPublic(httpsUrl, dispatcherOptions)
  if (httpsUrl && repoIsPublic && (!preferSsh || await accessRepository(httpsUrl))) {
    fetchSpec = httpsUrl
  }
  if (!fetchSpec && preferSsh && sshUrl && await accessRepository(sshUrl)) {
    fetchSpec = sshUrl
  }
  if (!fetchSpec && httpsUrl) {
    if ((hosted.auth || !repoIsPublic) && await accessRepository(httpsUrl)) {
      // Reachable over HTTPS without being provably public, so resolve as
      // `type: git` against this exact URL: the host's archive endpoint would
      // carry neither the URL's credentials nor ambient ones (helpers, tokens).
      return {
        fetchSpec: httpsUrl,
        hosted: {
          ...hosted,
          _fill: hosted._fill,
          tarball: undefined,
        },
        // `httpsUrl` is the `ls-remote` target, so it carries no committish.
        normalizedBareSpecifier: hosted.https(),
        ...parseGitParams(hosted.committish),
      }
    }
    if (repoIsPublic) {
      fetchSpec = httpsUrl
    }
  }
  if (!fetchSpec && !preferSsh && sshUrl && await accessRepository(sshUrl)) {
    fetchSpec = sshUrl
  }

  if (!fetchSpec) {
    fetchSpec = preferSsh ? hosted.sshurl({ noCommittish: true }) : httpsUrl
  }

  return {
    fetchSpec: fetchSpec!,
    hosted: {
      ...hosted,
      tarballtemplate: hosted.type === 'gitlab' ? gitlabTarballTemplate : hosted.tarballtemplate,
      _fill: hosted._fill,
      // Same rationale as the early return above: without proof that the repo
      // is public, the host's anonymous archive endpoint cannot be assumed to
      // work, so the resolution must stay `type: git`.
      tarball: repoIsPublic ? hosted.tarball : undefined,
    },
    normalizedBareSpecifier: hosted.shortcut(),
    ...parseGitParams(hosted.committish),
  }
}

// hosted-git-info's default GitLab tarball URL contains an encoded slash
// (`%2F`) which survives into the virtual store directory name and makes
// Node refuse to import the package (ERR_INVALID_MODULE_SPECIFIER).
function gitlabTarballTemplate ({ domain, user, project, committish }: { domain: string, user: string, project: string, committish: string | null }): string {
  const ref = committish ? encodeURIComponent(committish) : 'HEAD'
  return `https://${domain}/${user}/${project}/-/archive/${ref}/${project}-${ref}.tar.gz`
}

// An HTTP HEAD on the project page (without ".git") instead of `git ls-remote`:
// probing a private repo with ls-remote would trigger a credential prompt. This is
// very similar to yarn classic's behavior; npm instead tries git ls-remote directly,
// which prompts for login credentials. Transient failures (429/5xx/network errors)
// are retried by the fetch layer so registry throttling of CI runners is not
// mistaken for a private repository.
async function isRepoPublic (httpsUrl: string, dispatcherOptions: DispatcherOptions): Promise<boolean> {
  try {
    const response = await fetchWithDispatcher(httpsUrl.replace(/\.git$/, ''), {
      method: 'HEAD',
      redirect: 'manual',
      retry: { retries: 2, factor: 2, minTimeout: 500, maxTimeout: 2_000 },
      dispatcherOptions,
    })
    return response.ok
  } catch {
    return false
  }
}

async function accessRepository (repository: string): Promise<boolean> {
  try {
    await lsRemote(['--exit-code', repository, 'HEAD'], { retries: 0 })
    return true
  } catch {
    return false
  }
}

type GitParsedParams = Pick<HostedPackageSpec, 'gitCommittish' | 'gitRange' | 'path'>

function parseGitParams (committish: string | null): GitParsedParams {
  const result: GitParsedParams = { gitCommittish: null }
  if (!committish) {
    return result
  }

  const params = committish.split('&')
  for (const param of params) {
    if (param.length >= 7 && param.slice(0, 7) === 'semver:') {
      result.gitRange = param.slice(7)
    } else if (param.slice(0, 5) === 'path:') {
      result.path = param.slice(5)
    } else {
      result.gitCommittish = param
    }
  }
  return result
}

// handle SCP-like URLs
// see https://github.com/yarnpkg/yarn/blob/5682d55/src/util/git.js#L103
function correctUrl (gitUrl: string): string {
  let _gitUrl = gitUrl.replace(/^git\+/, '')
  if (_gitUrl.startsWith('ssh://')) {
    const hashIndex = _gitUrl.indexOf('#')
    let hash = ''
    if (hashIndex !== -1) {
      hash = _gitUrl.slice(hashIndex)
      _gitUrl = _gitUrl.slice(0, hashIndex)
    }
    const [auth, ...pathname] = _gitUrl.slice(6).split('/')
    const userInfoEnd = auth.lastIndexOf('@')
    const host = userInfoEnd === -1 ? auth : auth.slice(userInfoEnd + 1)
    // The colons of a bracketed IPv6 literal belong to the address.
    const bracketEnd = host.startsWith('[') ? host.indexOf(']') : -1
    const afterHost = bracketEnd === -1 ? host : host.slice(bracketEnd + 1)
    if (afterHost.includes(':') && !/:\d+$/.test(afterHost)) {
      const authArr = auth.split(':')
      const protocol = gitUrl.split('://')[0]
      gitUrl = `${protocol}://${authArr.slice(0, -1).join(':') + '/' + authArr[authArr.length - 1]}${pathname.length ? '/' + pathname.join('/') : ''}${hash}`
    }
  }
  return gitUrl
}
