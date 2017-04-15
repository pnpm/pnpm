import execa = require('execa')
import {
  PackageSpec,
  HostedPackageSpec,
  ResolveOptions,
  ResolveResult,
  TarballResolution,
  GitRepositoryResolution,
} from '.'
import logger from 'pnpm-logger'
import path = require('path')
import normalizeSsh = require('normalize-ssh')
import {Got} from '../network/got'

const gitLogger = logger('git-logger')

let tryGitHubApi = true

export default async function resolveGit (parsedSpec: HostedPackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const isGitHubHosted = parsedSpec.type === 'git' && parsedSpec.hosted.type === 'github'
  const parts = normalizeRepoUrl(parsedSpec).split('#')
  const repo = parts[0]
  const ref = parts[1] || 'master'

  if (!isGitHubHosted || isSsh(parsedSpec.rawSpec)) {
    const commitId = await resolveRef(repo, ref)
    const resolution: GitRepositoryResolution = {
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

  const ghSpec = {
    user: parsedSpec.hosted.user,
    project: parsedSpec.hosted.project,
    ref: parsedSpec.hosted.committish || 'HEAD',
  }
  let commitId: string
  if (tryGitHubApi) {
    try {
      commitId = await tryResolveViaGitHubApi(ghSpec, opts.got)
    } catch (err) {
      gitLogger.warn({
        message: `Error while trying to resolve ${parsedSpec.fetchSpec} via GitHub API`,
        err,
      })

      // if it fails once, don't bother retrying for other packages
      tryGitHubApi = false

      commitId = await resolveRef(repo, ref)
    }
  } else {
    commitId = await resolveRef(repo, ref)
  }

  const resolution: TarballResolution = {
    tarball: `https://codeload.github.com/${ghSpec.user}/${ghSpec.project}/tar.gz/${commitId}`,
  }
  return {
    id: ['github.com', ghSpec.user, ghSpec.project, commitId].join('/'),
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

function normalizeRepoUrl (parsedSpec: HostedPackageSpec) {
  const hosted = <any>parsedSpec.hosted // tslint:disable-line
  return hosted.getDefaultRepresentation() == 'shortcut' ? hosted.git() : hosted.toString()
}

function isSsh (gitSpec: string): boolean {
  return gitSpec.substr(0, 10) === 'git+ssh://'
    || gitSpec.substr(0, 4) === 'git@'
}

/**
 * Resolves a 'hosted' package hosted on 'github'.
 */
async function tryResolveViaGitHubApi (
  spec: {
    user: string,
    project: string,
    ref: string
  },
  got: Got
) {
  const url = [
    'https://api.github.com/repos',
    spec.user,
    spec.project,
    'commits',
    spec.ref
  ].join('/')
  const body = await got.getJSON<GitHubRepoResponse>(url)
  return body.sha
}

type GitHubRepoResponse = {
  sha: string
}
