import {delimiter} from './createPkgId'
import {HostedPackageSpec, ResolveOptions, ResolveResult} from '.'
import {fetchFromRemoteTarball, FetchOptions} from './fetch'

/**
 * Resolves a 'hosted' package hosted on 'github'.
 */
export default async function resolveGithub (spec: HostedPackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const ghSpec = parseGithubSpec(spec)
  const dist = {
    tarball: `https://codeload.github.com/${ghSpec.owner}/${ghSpec.repo}/tar.gz/${ghSpec.ref}`
  }
  return {
    id: ['github', ghSpec.owner, ghSpec.repo, ghSpec.ref].join(delimiter),
    fetch: (target: string, opts: FetchOptions) => fetchFromRemoteTarball(target, dist, opts)
  }
}

const PARSE_GITHUB_RE = /^github:([^\/]+)\/([^#]+)(#(.+))?$/

function parseGithubSpec (spec: HostedPackageSpec): GitHubSpec {
  const m = PARSE_GITHUB_RE.exec(spec.hosted.shortcut)
  if (!m) {
    throw new Error('cannot parse: ' + spec.hosted.shortcut)
  }
  const owner = m[1]
  const repo = m[2]
  const ref = m[4] || 'HEAD'
  return {owner, repo, ref}
}

type GitHubSpec = {
  owner: string,
  repo: string,
  ref: string
}
