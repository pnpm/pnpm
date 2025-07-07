// cspell:ignore sshurl
import urlLib, { URL } from 'node:url'
import { fetchWithAgent } from '@pnpm/fetch'
import { type AgentOptions } from '@pnpm/network.agent'

import git from 'graceful-git'
import HostedGit from 'hosted-git-info'

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

export async function parseBareSpecifier (bareSpecifier: string, opts: AgentOptions): Promise<HostedPackageSpec | null> {
  const hosted = HostedGit.fromUrl(bareSpecifier)
  if (hosted != null) {
    return fromHostedGit(hosted, opts)
  }
  const colonsPos = bareSpecifier.indexOf(':')
  if (colonsPos === -1) return null
  const protocol = bareSpecifier.slice(0, colonsPos)
  if (protocol && gitProtocols.has(protocol.toLocaleLowerCase())) {
    const correctBareSpecifier = correctUrl(bareSpecifier)
    const url = new URL(correctBareSpecifier)
    if (!url?.protocol) return null

    const hash = (url.hash?.length > 1) ? decodeURIComponent(url.hash.slice(1)) : null
    return {
      fetchSpec: urlToFetchSpec(url),
      normalizedBareSpecifier: bareSpecifier,
      ...parseGitParams(hash),
    }
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

async function fromHostedGit (hosted: any, agentOptions: AgentOptions): Promise<HostedPackageSpec> { // eslint-disable-line
  let fetchSpec: string | null = null
  // try git/https url before fallback to ssh url
  const gitHttpsUrl = hosted.https({ noCommittish: true, noGitPlus: true })
  if (gitHttpsUrl && await isRepoPublic(gitHttpsUrl, agentOptions) && await accessRepository(gitHttpsUrl)) {
    fetchSpec = gitHttpsUrl
  } else {
    const gitSshUrl = hosted.ssh({ noCommittish: true })
    if (gitSshUrl && await accessRepository(gitSshUrl)) {
      fetchSpec = gitSshUrl
    }
  }

  if (!fetchSpec) {
    const httpsUrl: string | null = hosted.https({ noGitPlus: true, noCommittish: true })
    if (httpsUrl) {
      if ((hosted.auth || !await isRepoPublic(httpsUrl, agentOptions)) && await accessRepository(httpsUrl)) {
        return {
          fetchSpec: httpsUrl,
          hosted: {
            ...hosted,
            _fill: hosted._fill,
            tarball: undefined,
          },
          normalizedBareSpecifier: `git+${httpsUrl}`,
          ...parseGitParams(hosted.committish),
        }
      } else {
        try {
          // when git ls-remote private repo, it asks for login credentials.
          // use HTTP HEAD request to test whether this is a private repo, to avoid login prompt.
          // this is very similar to yarn classic's behavior.
          // npm instead tries git ls-remote directly which prompts user for login credentials.

          // HTTP HEAD on https://domain/user/repo, strip out ".git"
          const response = await fetchWithAgent(httpsUrl.replace(/\.git$/, ''), { method: 'HEAD', follow: 0, retry: { retries: 0 }, agentOptions })
          if (response.ok) {
            fetchSpec = httpsUrl
          }
        } catch {
          // ignore
        }
      }
    }
  }

  if (!fetchSpec) {
    // use ssh url for likely private repo
    fetchSpec = hosted.sshurl({ noCommittish: true })
  }

  return {
    fetchSpec: fetchSpec!,
    hosted: {
      ...hosted,
      _fill: hosted._fill,
      tarball: hosted.tarball,
    },
    normalizedBareSpecifier: hosted.shortcut(),
    ...parseGitParams(hosted.committish),
  }
}

async function isRepoPublic (httpsUrl: string, agentOptions: AgentOptions): Promise<boolean> {
  try {
    const response = await fetchWithAgent(httpsUrl.replace(/\.git$/, ''), { method: 'HEAD', follow: 0, retry: { retries: 0 }, agentOptions })
    return response.ok
  } catch {
    return false
  }
}

async function accessRepository (repository: string): Promise<boolean> {
  try {
    await git(['ls-remote', '--exit-code', repository, 'HEAD'], { retries: 0 })
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
    const [, host] = auth.split('@')
    if (host.includes(':') && !/:\d+$/.test(host)) {
      const authArr = auth.split(':')
      const protocol = gitUrl.split('://')[0]
      gitUrl = `${protocol}://${authArr.slice(0, -1).join(':') + '/' + authArr[authArr.length - 1]}${pathname.length ? '/' + pathname.join('/') : ''}${hash}`
    }
  }
  return gitUrl
}
