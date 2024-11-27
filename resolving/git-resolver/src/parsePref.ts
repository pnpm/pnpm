// cspell:ignore sshurl
import urlLib, { URL } from 'url'
import { fetch } from '@pnpm/fetch'

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
  normalizedPref: string
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

export async function parsePref (pref: string): Promise<HostedPackageSpec | null> {
  const hosted = HostedGit.fromUrl(pref)
  if (hosted != null) {
    return fromHostedGit(hosted)
  }
  const colonsPos = pref.indexOf(':')
  if (colonsPos === -1) return null
  const protocol = pref.slice(0, colonsPos)
  if (protocol && gitProtocols.has(protocol.toLocaleLowerCase())) {
    const correctPref = correctUrl(pref)
    const url = new URL(correctPref)
    if (!url?.protocol) return null

    const hash = (url.hash?.length > 1) ? decodeURIComponent(url.hash.slice(1)) : null
    return {
      fetchSpec: urlToFetchSpec(url),
      normalizedPref: pref,
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

async function fromHostedGit (hosted: any): Promise<HostedPackageSpec> { // eslint-disable-line
  let fetchSpec: string | null = null
  // try git/https url before fallback to ssh url
  const gitHttpsUrl = hosted.https({ noCommittish: true, noGitPlus: true })
  if (gitHttpsUrl && await isRepoPublic(gitHttpsUrl) && await accessRepository(gitHttpsUrl)) {
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
      if (hosted.auth && await accessRepository(httpsUrl)) {
        return {
          fetchSpec: httpsUrl,
          hosted: {
            ...hosted,
            _fill: hosted._fill,
            tarball: undefined,
          },
          normalizedPref: `git+${httpsUrl}`,
          ...parseGitParams(hosted.committish),
        }
      } else {
        try {
          // when git ls-remote private repo, it asks for login credentials.
          // use HTTP HEAD request to test whether this is a private repo, to avoid login prompt.
          // this is very similar to yarn's behavior.
          // npm instead tries git ls-remote directly which prompts user for login credentials.

          // HTTP HEAD on https://domain/user/repo, strip out ".git"
          const response = await fetch(httpsUrl.replace(/\.git$/, ''), { method: 'HEAD', follow: 0, retry: { retries: 0 } })
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
    normalizedPref: hosted.shortcut(),
    ...parseGitParams(hosted.committish),
  }
}

async function isRepoPublic (httpsUrl: string): Promise<boolean> {
  try {
    const response = await fetch(httpsUrl.replace(/\.git$/, ''), { method: 'HEAD', follow: 0, retry: { retries: 0 } })
    return response.ok
  } catch {
    return false
  }
}

async function accessRepository (repository: string): Promise<boolean> {
  try {
    await git(['ls-remote', '--exit-code', repository, 'HEAD'], { retries: 0 })
    return true
  } catch { // eslint-disable-line
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
  const parsed = urlLib.parse(gitUrl.replace(/^git\+/, '')) // eslint-disable-line n/no-deprecated-api

  if (parsed.protocol === 'ssh:' &&
    parsed.hostname &&
    parsed.pathname &&
    parsed.pathname.startsWith('/:') &&
    parsed.port === null) {
    parsed.pathname = parsed.pathname.replace(/^\/:/, '')
    return urlLib.format(parsed)
  }

  return gitUrl
}
