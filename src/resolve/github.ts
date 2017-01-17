import semver = require('semver')
import path = require('path')
import {HostedPackageSpec, ResolveOptions, ResolveResult} from '.'

/**
 * Resolves a 'hosted' package hosted on 'github'.
 */
export default async function resolveGithub (spec: HostedPackageSpec, opts: ResolveOptions): Promise<ResolveResult> {
  const ghSpec = parseGithubSpec(spec)

  let commit = null

  // Try to treat `ref` as version range first
  if (semver.validRange(ghSpec.ref)) {
    const tagMap = await getTagMap(ghSpec)
    const versions = Object.keys(tagMap)
    const maxVersion = semver.maxSatisfying(versions, ghSpec.ref, true)
    if (maxVersion) {
      commit = tagMap[maxVersion]
    }
  }

  // Resolve commit from `ref`
  if (commit == null) {
    commit  = await resolveRef(ghSpec)
  }

  return {
    id: path.join('github.com', ghSpec.owner, ghSpec.repo, commit),
    tarball: `https://codeload.github.com/${ghSpec.owner}/${ghSpec.repo}/tar.gz/${commit}`
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

  async function getTagMap (spec: GitHubSpec) {
    const url = `https://api.github.com/repos/${spec.owner}/${spec.repo}/tags`
    const body = await consumePaginated<GitHubTagsResponse>(url)
    const tagMap = {}
    body.forEach(tag => {
      if (semver.valid(tag.name)) {
        tagMap[tag.name] = tag.commit.sha
      }
    })
    return tagMap
  }

  async function consumePaginated<T>(url: string): Promise<T[]> {
    let results: T[] = []
    let page = 1
    while (true) {
      const nextChunk = await opts.got.getJSON<T[]>(`${url}?page=${page}`)
      if (nextChunk.length === 0) {
        break
      }
      page = page + 1
      results = results.concat(nextChunk)
    }
    return results
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

type GitHubTagsResponse = {
  name: string,
  commit: {sha: string}
}
