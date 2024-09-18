import assert from 'assert'
import { execSync } from 'child_process'
import semver from 'semver'

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Fn = () => any

export function retry<T extends Fn> (fn: T, retries = 1): ReturnType<T> {
  assert(retries > 0, 'At least 1 retry is specified')

  for (let i = 0; i < retries + 1; i++) {
    try {
      return fn()
    } catch {
      // ignore
    }
  }
  throw new Error('Retries exausted')
}

export function git (cmd: string): string {
  return retry(() => execSync(`git ${cmd}`, { encoding: 'utf8' }), 1)
}

export function getCommitSha (
  repo: string,
  ref: string
): string {
  assert(!repo.includes(' '), 'repo does not include spaces')
  assert(!ref.includes(' '), 'ref does not include spaces')

  return git(`ls-remote ${repo} ${ref}`)
    .split('\t')[0]
}

export function getAllRefs (
  repo: string
): Record<string, string> {
  assert(!repo.includes(' '), 'repo does not include spaces')

  return git(`ls-remote ${repo}`)
    .split('\n')
    .reduce((obj: Record<string, string>, line: string) => {
      const [commit, refName] = line.split('\t')
      obj[refName] = commit
      return obj
    }, {})
}

export function resolveTags (tags: string[], range: string): string | undefined {
  return semver.maxSatisfying(tags, range, true) ?? undefined
}

export function getCommitFromRef (
  repo: string,
  ref: string
): string {
  if (isSha(ref)) return ref

  const refs = getAllRefs(repo)
  const sha = refs[ref] ||
    refs[`refs/${ref}`] ||
    refs[`refs/tags/${ref}^{}`] || // prefer annotated tags
    refs[`refs/tags/${ref}`] ||
    refs[`refs/heads/${ref}`]

  assert(sha, `${ref} exists on repo`)

  return sha
}

export function getCommitFromRange (
  repo: string,
  range: string
): string {
  const refs = getAllRefs(repo)
  const tags = Object.keys(refs)
    // using the same semantics of version tags as https://github.com/zkat/pacote
    .filter(/^refs\/tags\/v?(\d+\.\d+\.\d+(?:[-+].+)?)(\^{})?$/.test)
    // accept annotated tags
    .map((key) => key.replace(/^refs\/tags\//, '').replace(/\^{}$/, ''))
    .filter((key) => semver.valid(key, true))

  const refVTag = resolveTags(tags, range)
  const sha =
    refVTag &&
    (refs[`refs/tags/${refVTag}^{}`] || // prefer annotated tags
      refs[`refs/tags/${refVTag}`])

  assert(sha, `Could not resolve ${range} to a commit. Available versions are: ${tags.join(', ')}`)

  return sha
}

export function isSha (ref: string): boolean {
  return /^[0-9a-f]{7,40}$/.test(ref)
}

export function isSsh (url: string): boolean {
  return /^((git\+)?ssh:\/\/|git@)/.test(url)
}
