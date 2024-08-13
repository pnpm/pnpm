// cspell:ignore sshurl
import urlLib, { URL } from 'url'
import { fetch } from '@pnpm/fetch'

import GitHost from 'hosted-git-info'
import assert from 'assert'
import { spawnSync } from 'child_process'

export interface GitParams {
  gitCommittish: string | null
  gitRange?: string
  path?: string
}

export interface PackageSpec extends GitParams {
  fetchSpec: string
  normalizedPref: string
}

export interface HostedPackageSpec extends PackageSpec {
  hosted: GitHost
}

const gitProtocols = new Set([
  'git',
  'http',
  'https',
  'rsync',
  'ftp',
  'file',
  'ssh',
  'git+http',
  'git+https',
  'git+rsync',
  'git+ftp',
  'git+file',
  'git+ssh',
])

export async function parsePref (
  pref: string
): Promise<PackageSpec | HostedPackageSpec | null> {
  const hosted = GitHost.fromUrl(pref)
  if (hosted != null) {
    return fromHostedGit(hosted)
  }
  try {
    return parseGitUrl(pref)
  } catch (err) {
    return null
  }
}

async function fromHostedGit (hosted: GitHost): Promise<HostedPackageSpec> {
  const gitHttpsUrl = hosted.https({ noCommittish: true, noGitPlus: true })
  const gitSshUrl = hosted.sshurl({ noCommittish: true, noGitPlus: false })

  const hasAuth = Boolean(hosted.auth)
  const isAccessibleViaFetch = await isRepoPublic(gitHttpsUrl)
  const isAccessibleViaGit = await isRepoAccessible(gitHttpsUrl)
  const isPublic = isAccessibleViaGit || isAccessibleViaFetch

  // try git/https url before fallback to ssh
  const fetchSpec =
    isPublic || hasAuth
      ? gitHttpsUrl
      : gitSshUrl

  if (!isPublic) {
    hosted.tarball = () => ''
  }

  return {
    fetchSpec,
    hosted,
    normalizedPref: hosted.shortcut(),
    ...parseGitParams(hosted.committish),
  }
}

/**
 * When git ls-remote private repo, it asks for login credentials.
 * use HTTP HEAD request to test whether this is a private repo, to avoid login prompt.
 * this is very similar to yarn's behavior.
 * npm instead tries git ls-remote directly which prompts user for login credentials.
 *
 * HTTP HEAD on https://domain/user/repo, strip out ".git"
 */
async function isRepoPublic (httpsUrl: string): Promise<boolean> {
  try {
    const response = await fetch(httpsUrl.replace(/\.git$/, ''), {
      method: 'HEAD',
      follow: 0,
      retry: { retries: 0 },
    })
    return response.ok
  } catch (_err) {
    return false
  }
}

/**
 * Returns true if the provided repository uri is accessible via the git cli.
 */
async function isRepoAccessible (repository: string): Promise<boolean> {
  try {
    spawnSync('git', ['ls-remote', '--exit-code', repository, 'HEAD'], { shell: true })
    return true
  } catch (err) {
    return false
  }
}

function parseGitParams (committish: string | undefined): GitParams {
  const result: GitParams = { gitCommittish: null }
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

/**
 * @throws {AssertionError} If `pref` does not feature a valid git protocol
 * @throws {TypeError} If the resultant `fetchSpec` is not a valid URL
 * @throws {URIError} If the `auth` property is present but cannot be decoded
 * @returns A validated package specification
 */
function parseGitUrl (pref: string): PackageSpec {
  const parsed = urlLib.parse(pref.replace(/^git\+/, '')) // eslint-disable-line n/no-deprecated-api

  assert(
    parsed.protocol && gitProtocols.has(parsed.protocol.slice(0, -1)),
    'pref features a valid git protocol'
  )

  // handle SCP-like URLs
  // see https://github.com/yarnpkg/yarn/blob/5682d55/src/util/git.js#L103
  if (
    parsed.protocol === 'ssh:' &&
    parsed.hostname &&
    parsed.pathname &&
    parsed.pathname.startsWith('/:') &&
    parsed.port === null
  ) {
    parsed.pathname = parsed.pathname.replace(/^\/:/, '')
  }

  const hash =
    parsed.hash?.length && parsed.hash.length > 1
      ? decodeURIComponent(parsed.hash.slice(1))
      : undefined
  parsed.hash = ''

  const fetchSpec = urlLib.format(parsed)
  // throw if result is not a valid url
  new URL(fetchSpec) // eslint-disable-line no-new

  return {
    fetchSpec,
    normalizedPref: pref,
    ...parseGitParams(hash),
  }
}
