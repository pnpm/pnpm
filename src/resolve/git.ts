import execa = require('execa')
import {PackageSpec, HostedPackageSpec, ResolveOptions, ResolveResult, Resolution} from '.'
import hostedGitInfo = require('@zkochan/hosted-git-info')
import logger from 'pnpm-logger'
import path = require('path')
import {Got} from '../network/got'

const gitLogger = logger('git-logger')

let tryGitHubApi = true

export default async function resolveGit (parsedSpec: PackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const hspec = <HostedPackageSpec>parsedSpec
  const isGitHubHosted = parsedSpec.type === 'hosted' && hspec.hosted.type === 'github'
  const parts = normalizeRepoUrl(parsedSpec.spec).split('#')
  const repo = parts[0]
  const ref = parts[1] || 'master'

  if (!isGitHubHosted || isSsh(parsedSpec.spec)) {
    const commitId = await resolveRef(repo, ref)
    const resolution: Resolution = {
      type: 'git-repo',
      repo,
      commitId,
    }
    return {
      id: repo
        .replace(/^.*:\/\/(git@)?/, '')
        .replace(/:/g, '+')
        .replace(/\.git$/, '') + '/' + commitId,
      resolution,
    }
  }

  const ghSpec = parseGithubSpec(hspec)
  let commitId: string
  if (tryGitHubApi) {
    try {
      commitId = await tryResolveViaGitHubApi(ghSpec, opts.got)
    } catch (err) {
      gitLogger.warn({
        message: `Error while trying to resolve ${parsedSpec.spec} via GitHub API`,
        err,
      })

      // if it fails once, don't bother retrying for other packages
      tryGitHubApi = false

      commitId = await resolveRef(repo, ref)
    }
  } else {
    commitId = await resolveRef(repo, ref)
  }

  const resolution: Resolution = {
    type: 'tarball',
    tarball: `https://codeload.github.com/${ghSpec.owner}/${ghSpec.repo}/tar.gz/${commitId}`,
  }
  return {
    id: ['github.com', ghSpec.owner, ghSpec.repo, commitId].join('/'),
    resolution,
  }
}

async function resolveRef (repo: string, ref: string) {
  const result = await execa('git', ['ls-remote', '--refs', repo, ref])
  // should output something like:
  //   572bc3d4e16220c2e986091249e62a5913294b25    	refs/heads/master

  // if no ref was found, assume that ref is the commit ID
  if (!result.stdout) return ref

  return result.stdout.match(/^[a-z0-9]+/)[0]
}

function normalizeRepoUrl (repoUrl: string) {
  const hosted = hostedGitInfo.fromUrl(repoUrl)
  if (!hosted) return repoUrl
  return hosted.getDefaultRepresentation() == 'shortcut' ? hosted.git() : hosted.toString()
}

function isSsh (gitSpec: string): boolean {
  return gitSpec.substr(0, 10) === 'git+ssh://'
    || gitSpec.substr(0, 4) === 'git@'
}

/**
 * Resolves a 'hosted' package hosted on 'github'.
 */
async function tryResolveViaGitHubApi (spec: GitHubSpec, got: Got) {
  const url = [
    'https://api.github.com/repos',
    spec.owner,
    spec.repo,
    'commits',
    spec.ref
  ].join('/')
  const body = await got.getJSON<GitHubRepoResponse>(url)
  return body.sha
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
