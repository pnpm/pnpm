import { ResolveResult } from '@pnpm/resolver-base'
import parsePref, { HostedPackageSpec } from './parsePref'
import git = require('graceful-git')
import semver = require('semver')

export { HostedPackageSpec }

export default function (
  opts: {}
) {
  return async function resolveGit (
    wantedDependency: {pref: string}
  ): Promise<ResolveResult | null> {
    const parsedSpec = await parsePref(wantedDependency.pref)

    if (!parsedSpec) return null

    const commit = await resolveRef(parsedSpec.fetchSpec, parsedSpec.gitCommittish ?? 'master', parsedSpec.gitRange)
    let resolution

    if (parsedSpec.hosted && !isSsh(parsedSpec.fetchSpec)) {
      // don't use tarball for ssh url, they are likely private repo
      const hosted = parsedSpec.hosted
      // use resolved committish
      hosted.committish = commit
      const tarball = hosted.tarball?.()

      if (tarball) {
        resolution = { tarball }
      }
    }

    if (!resolution) {
      resolution = {
        commit,
        repo: parsedSpec.fetchSpec,
        type: 'git',
      } as ({ type: string } & object)
    }

    return {
      id: parsedSpec.fetchSpec
        .replace(/^.*:\/\/(git@)?/, '')
        .replace(/:/g, '+')
        .replace(/\.git$/, '') + '/' + commit,
      normalizedPref: parsedSpec.normalizedPref,
      resolution,
      resolvedVia: 'git-repository',
    }
  }
}

function resolveVTags (vTags: string[], range: string) {
  return semver.maxSatisfying(vTags, range, true)
}

async function getRepoRefs (repo: string, ref: string | null) {
  const gitArgs = ['ls-remote', '--refs', repo]
  if (ref) {
    gitArgs.push(ref)
  }
  // graceful-git by default retries 10 times, reduce to single retry
  const result = await git(gitArgs, { retries: 1 })
  const refs = result.stdout.split('\n').reduce((obj: object, line: string) => {
    const commitAndRef = line.split('\t')
    const commit = commitAndRef[0]
    const refName = commitAndRef[1]
    obj[refName] = commit
    return obj
  }, {})
  return refs
}

async function resolveRef (repo: string, ref: string, range?: string) {
  if (ref.match(/^[0-9a-f]{40}$/)) {
    return ref
  }
  const refs = await getRepoRefs(repo, range ? null : ref)
  return resolveRefFromRefs(refs, repo, ref, range)
}

function resolveRefFromRefs (refs: {[ref: string]: string}, repo: string, ref: string, range?: string) {
  if (!range) {
    const commitId =
      refs[ref] ||
      refs[`refs/tags/${ref}^{}`] || // prefer annotated tags
      refs[`refs/tags/${ref}`] ||
      refs[`refs/heads/${ref}`]

    if (!commitId) {
      throw new Error(`Could not resolve ${ref} to a commit of ${repo}.`)
    }

    return commitId
  } else {
    const vTags =
      Object.keys(refs)
        // using the same semantics of version tags as https://github.com/zkat/pacote
        .filter((key: string) => /^refs\/tags\/v?(\d+\.\d+\.\d+(?:[-+].+)?)(\^{})?$/.test(key))
        .map((key: string) => {
          return key
            .replace(/^refs\/tags\//, '')
            .replace(/\^{}$/, '') // accept annotated tags
        })
        .filter((key: string) => semver.valid(key, true))
    const refVTag = resolveVTags(vTags, range)
    const commitId = refVTag &&
      (refs[`refs/tags/${refVTag}^{}`] || // prefer annotated tags
      refs[`refs/tags/${refVTag}`])

    if (!commitId) {
      throw new Error(`Could not resolve ${range} to a commit of ${repo}. Available versions are: ${vTags.join(', ')}`)
    }

    return commitId
  }
}

function isSsh (gitSpec: string): boolean {
  return gitSpec.substr(0, 10) === 'git+ssh://' ||
    gitSpec.substr(0, 4) === 'git@'
}
