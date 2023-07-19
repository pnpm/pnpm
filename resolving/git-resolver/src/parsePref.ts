import url, { URL } from 'url'
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
    const urlparse = new URL(correctPref)
    if (!urlparse?.protocol) return null

    const committish = (urlparse.hash?.length > 1) ? decodeURIComponent(urlparse.hash.slice(1)) : null
    return {
      fetchSpec: urlToFetchSpec(urlparse),
      normalizedPref: pref,
      ...setGitCommittish(committish),
    }
  }
  return null
}

function urlToFetchSpec (urlparse: URL) {
  urlparse.hash = ''
  const fetchSpec = url.format(urlparse)
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
          ...setGitCommittish(hosted.committish),
        }
      } else {
        try {
          // when git ls-remote private repo, it asks for login credentials.
          // use HTTP HEAD request to test whether this is a private repo, to avoid login prompt.
          // this is very similar to yarn's behaviour.
          // npm instead tries git ls-remote directly which prompts user for login credentials.

          // HTTP HEAD on https://domain/user/repo, strip out ".git"
          const response = await fetch(httpsUrl.replace(/\.git$/, ''), { method: 'HEAD', follow: 0, retry: { retries: 0 } })
          if (response.ok) {
            fetchSpec = httpsUrl
          }
        } catch (e) {
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
    ...setGitCommittish(hosted.committish),
  }
}

async function isRepoPublic (httpsUrl: string) {
  try {
    const response = await fetch(httpsUrl.replace(/\.git$/, ''), { method: 'HEAD', follow: 0, retry: { retries: 0 } })
    return response.ok
  } catch (_err) {
    return false
  }
}

async function accessRepository (repository: string) {
  try {
    await git(['ls-remote', '--exit-code', repository, 'HEAD'], { retries: 0 })
    return true
  } catch (err: any) { // eslint-disable-line
    return false
  }
}

function setGitCommittish (committish: string | null) {
  if (committish !== null && committish.length >= 7 && committish.slice(0, 7) === 'semver:') {
    return {
      gitCommittish: null,
      gitRange: committish.slice(7),
    }
  }
  return { gitCommittish: committish }
}

// handle SCP-like URLs
// see https://github.com/yarnpkg/yarn/blob/5682d55/src/util/git.js#L103
function correctUrl (giturl: string) {
  const parsed = url.parse(giturl.replace(/^git\+/, '')) // eslint-disable-line n/no-deprecated-api

  if (parsed.protocol === 'ssh:' &&
    parsed.hostname &&
    parsed.pathname &&
    parsed.pathname.startsWith('/:') &&
    parsed.port === null) {
    parsed.pathname = parsed.pathname.replace(/^\/:/, '')
    return url.format(parsed)
  }

  return giturl
}
