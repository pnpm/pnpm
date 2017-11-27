import logger from '@pnpm/logger'
import execa = require('execa')
import normalizeSsh = require('normalize-ssh')
import path = require('path')
import {
  GitRepositoryResolution,
  ResolveOptions,
  ResolveResult,
  TarballResolution,
  WantedDependency,
} from '..'
import {Got} from '../../network/got'
import parsePref, {HostedPackageSpec} from './parsePref'

export {HostedPackageSpec}

const gitLogger = logger // TODO: add namespace 'git-logger'

let tryGitHubApi = true

export default async function resolveGit (
  wantedDependency: WantedDependency,
  opts: ResolveOptions,
): Promise<ResolveResult | null> {
  const parsedSpec = parsePref(wantedDependency.pref, wantedDependency.alias)

  if (!parsedSpec) return null

  const isGitHubHosted = parsedSpec.hosted && parsedSpec.hosted.type === 'github'

  if (!isGitHubHosted || isSsh(wantedDependency.pref)) {
    const commit = await resolveRef(parsedSpec.fetchSpec, parsedSpec.gitCommittish || 'master')
    const resolution: GitRepositoryResolution = {
      commit,
      repo: parsedSpec.fetchSpec,
      type: 'git',
    }
    return {
      id: parsedSpec.fetchSpec
        .replace(/^.*:\/\/(git@)?/, '')
        .replace(/:/g, '+')
        .replace(/\.git$/, '') + '/' + commit,
      normalizedPref: parsedSpec.normalizedPref,
      resolution,
    }
  }

  const parts = normalizeRepoUrl(parsedSpec).split('#')
  const repo = parts[0]

  const ghSpec = {
    project: parsedSpec.hosted!.project,
    ref: parsedSpec.hosted!.committish || 'HEAD',
    user: parsedSpec.hosted!.user,
  }
  let commitId: string
  if (tryGitHubApi) {
    try {
      commitId = await tryResolveViaGitHubApi(ghSpec, opts.getJson)
    } catch (err) {
      gitLogger.warn({
        err,
        message: `Error while trying to resolve ${parsedSpec.fetchSpec} via GitHub API`,
      })

      // if it fails once, don't bother retrying for other packages
      tryGitHubApi = false

      commitId = await resolveRef(repo, ghSpec.ref)
    }
  } else {
    commitId = await resolveRef(repo, ghSpec.ref)
  }

  const tarballResolution: TarballResolution = {
    tarball: `https://codeload.github.com/${ghSpec.user}/${ghSpec.project}/tar.gz/${commitId}`,
  }
  return {
    id: ['github.com', ghSpec.user, ghSpec.project, commitId].join('/'),
    normalizedPref: parsedSpec.normalizedPref,
    resolution: tarballResolution,
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
  return hosted.getDefaultRepresentation() === 'shortcut' ? hosted.git() : hosted.toString()
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
    ref: string,
  },
  getJson: <T>(url: string, registry: string) => Promise<T>,
) {
  const url = [
    'https://api.github.com/repos',
    spec.user,
    spec.project,
    'commits',
    spec.ref,
  ].join('/')
  // TODO: investigate what should be the correct registry path here
  const body = await getJson<GitHubRepoResponse>(url, url)
  return body.sha
}

interface GitHubRepoResponse {
  sha: string
}
