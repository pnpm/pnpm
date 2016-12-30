import path = require('path')
import {HostedPackageSpec, ResolveOptions, ResolveResult} from '.'

/**
 * Resolves a 'hosted' package hosted on 'github'.
 */
export default async function resolveGithub (spec: HostedPackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const ghSpec = parseGithubSpec(spec)
  // the ref should be a commit sha. Otherwise it would't be unique
  // and couldn't be saved in a machine store
  ghSpec.ref = await resolveRef(ghSpec)
  return {
    id: path.join('github.com', ghSpec.owner, ghSpec.repo, ghSpec.ref),
    tarball: `https://codeload.github.com/${ghSpec.owner}/${ghSpec.repo}/tar.gz/${ghSpec.ref}`
  }

  async function resolveRef (spec: GitHubSpec) {
    const url = [
      'https://api.github.com/repos',
      spec.owner,
      spec.repo,
      'commits',
      spec.ref
    ].join('/')
    const body = await opts.got.getJSON<GitHubRepoResponse>(url)
    return body.sha
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

type GitHubRepoResponse = {
  sha: string
}
