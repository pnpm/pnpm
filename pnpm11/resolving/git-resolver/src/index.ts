import assert from 'node:assert'

import { isError, PnpmError, redactAndSanitize } from '@pnpm/error'
import type { DispatcherOptions } from '@pnpm/network.fetch'
import type { GitResolution, LatestInfo, LatestQuery, PkgResolutionId, ResolveOptions, ResolveResult, TarballResolution } from '@pnpm/resolving.resolver-base'
import semver from 'semver'

import { createGitHostedPkgId } from './createGitHostedPkgId.js'
import { lsRemote } from './lsRemote.js'
import { type HostedPackageSpec, parseBareSpecifier } from './parseBareSpecifier.js'

export { createGitHostedPkgId }

export type { HostedPackageSpec }

export interface GitResolveResult extends ResolveResult {
  normalizedBareSpecifier?: string
  resolution: GitResolution | TarballResolution
  resolvedVia: 'git-repository'
}

export type GitResolver = (
  wantedDependency: { bareSpecifier: string },
  opts?: Pick<ResolveOptions, 'currentPkg' | 'update'>
) => Promise<GitResolveResult | null>

export function createGitResolver (
  opts: DispatcherOptions
): GitResolver {
  return async function resolveGit (wantedDependency, resolveOpts?): Promise<GitResolveResult | null> {
    const parsedSpecFunc = parseBareSpecifier(wantedDependency.bareSpecifier, opts)
    if (parsedSpecFunc == null) return null

    const existing = resolveExistingGitPkg(resolveOpts)
    if (existing) return existing

    const parsedSpec = await parsedSpecFunc()
    const commit = await resolveGitCommit(parsedSpec, wantedDependency.bareSpecifier)
    return buildGitResolution(parsedSpec, commit)
  }
}

function resolveExistingGitPkg (
  resolveOpts?: Pick<ResolveOptions, 'currentPkg' | 'update'>
): GitResolveResult | null {
  if (!resolveOpts?.currentPkg || resolveOpts.update) {
    return null
  }
  const currentResolution = resolveOpts.currentPkg.resolution
  if ('type' in currentResolution && currentResolution.type === 'git') {
    return {
      id: resolveOpts.currentPkg.id,
      resolution: currentResolution as GitResolution,
      resolvedVia: 'git-repository',
    }
  }
  if ('tarball' in currentResolution && currentResolution.tarball) {
    return {
      id: resolveOpts.currentPkg.id,
      resolution: currentResolution as TarballResolution,
      resolvedVia: 'git-repository',
    }
  }
  return null
}

async function resolveGitCommit (
  parsedSpec: HostedPackageSpec,
  bareSpecifierInput: string
): Promise<string> {
  const bareSpecifier = parsedSpec.gitCommittish == null || parsedSpec.gitCommittish === ''
    ? 'HEAD'
    : parsedSpec.gitCommittish
  try {
    return await resolveRef(parsedSpec.fetchSpec, bareSpecifier, parsedSpec.gitRange)
  } catch (err: unknown) {
    assert(isError(err))
    throw gitResolveError(err, bareSpecifierInput, parsedSpec.fetchSpec)
  }
}

function buildGitResolution (
  parsedSpec: HostedPackageSpec,
  commit: string
): GitResolveResult {
  const resolution = createResolutionObject(parsedSpec, commit)
  const id = createResolutionPkgId(resolution)
  return {
    id,
    normalizedBareSpecifier: parsedSpec.normalizedBareSpecifier,
    resolution,
    resolvedVia: 'git-repository',
  }
}

function createResolutionObject (
  parsedSpec: HostedPackageSpec,
  commit: string
): GitResolution | TarballResolution {
  let resolution: GitResolution | TarballResolution | undefined
  if ((parsedSpec.hosted != null) && !isSsh(parsedSpec.fetchSpec)) {
    const hosted = parsedSpec.hosted
    hosted.committish = commit
    const tarball = hosted.tarball?.()
    if (tarball) {
      resolution = { tarball, gitHosted: true }
    }
  }
  if (resolution == null) {
    resolution = {
      commit,
      repo: parsedSpec.fetchSpec,
      type: 'git',
    }
  }
  if (parsedSpec.path) {
    resolution.path = parsedSpec.path
  }
  return resolution
}

function createResolutionPkgId (resolution: GitResolution | TarballResolution): PkgResolutionId {
  if ('tarball' in resolution) {
    let id = resolution.tarball as PkgResolutionId
    if (resolution.path) {
      id = `${id}#path:${resolution.path}` as PkgResolutionId
    }
    return id
  }
  return createGitHostedPkgId(resolution)
}

// Git deps have no concept of "latest" — we'd need to query the host's tag list
// to know about newer commits, which isn't a uniform thing across protocols.
// Claim the dep so the dispatcher stops; the caller still surfaces a
// ref-mismatch report if the lockfile shifted to a different commit.
export async function resolveLatestFromGit (query: LatestQuery): Promise<LatestInfo | undefined> {
  const bareSpecifier = query.wantedDependency.bareSpecifier
  if (!bareSpecifier) return undefined
  const parsedSpecFunc = parseBareSpecifier(bareSpecifier, {})
  if (parsedSpecFunc == null) return undefined
  return {}
}

function resolveVTags (vTags: string[], range: string): string | null {
  return semver.maxSatisfying(vTags, range, true)
}

export async function getRepoRefs (repo: string, ref: string | null): Promise<Record<string, string>> {
  // `--` keeps a repo URL that starts with a dash (e.g. from a malicious
  // config value) from being parsed as a git flag.
  const gitArgs = ['--', repo]
  if (ref) {
    gitArgs.push(ref)
    // Also request the peeled ref for annotated tags (e.g., refs/tags/v1.0.0^{})
    // This is needed because annotated tags have their own SHA, and we need the commit SHA they point to
    gitArgs.push(`${ref}^{}`)
  }
  const result = await lsRemote(gitArgs, { retries: 1 })
  const refs: Record<string, string> = {}
  for (const line of result.stdout.split('\n')) {
    const [commit, refName] = line.split('\t')
    if (commit && refName) refs[refName] = commit
  }
  return refs
}

async function resolveRef (repo: string, ref: string, range?: string): Promise<string> {
  const committish = ref.match(/^[0-9a-f]{7,40}$/) !== null
  if (committish && ref.length === 40) {
    return ref
  }
  const refs = await getRepoRefs(repo, (range ?? committish) ? null : ref)
  const result = resolveRefFromRefs(refs, repo, ref, committish, range)
  if (committish && !result.startsWith(ref)) {
    throw new PnpmError('GIT_AMBIGUOUS_REF', `resolved commit ${result} from commit-ish reference ${ref}`)
  }
  return result
}

function resolveRefFromRefs (refs: { [ref: string]: string }, repo: string, ref: string, committish: boolean, range?: string): string {
  if (!range) {
    return resolveExactRef(refs, repo, ref, committish)
  }
  return resolveRangeRef(refs, repo, range)
}

function resolveExactRef (refs: { [ref: string]: string }, repo: string, ref: string, committish: boolean): string {
  const commitId =
    refs[ref] ||
    refs[`refs/${ref}`] ||
    refs[`refs/tags/${ref}^{}`] || // prefer annotated tags
    refs[`refs/tags/${ref}`] ||
    refs[`refs/heads/${ref}`]

  if (commitId) {
    return commitId
  }

  // check for a partial commit
  const commits = committish ? [...new Set(Object.values(refs).filter((value: string) => value.startsWith(ref)))] : []
  if (commits.length === 1) {
    return commits[0]
  }
  throw new Error(`Could not resolve ${ref} to a commit of ${redactAndSanitize(repo)}.`)
}

function resolveRangeRef (refs: { [ref: string]: string }, repo: string, range: string): string {
  const vTags = [...new Set(
    Object.keys(refs)
      .filter((key: string) => /^refs\/tags\/v?\d+\.\d+\.\d+(?:[-+].+)?(?:\^\{\})?$/.test(key))
      .map((key: string) => key.replace(/^refs\/tags\//, '').replace(/\^\{\}$/, ''))
      .filter((key: string) => semver.valid(key, true))
  )]
  const refVTag = resolveVTags(vTags, range)
  const commitId = refVTag &&
    (refs[`refs/tags/${refVTag}^{}`] || refs[`refs/tags/${refVTag}`])

  if (!commitId) {
    throw new Error(`Could not resolve ${range} to a commit of ${redactAndSanitize(repo)}. Available versions are: ${vTags.join(', ')}`)
  }
  return commitId
}

/**
 * Restate a failed `git ls-remote` as `ERR_PNPM_GIT_RESOLVE_FAILED`, naming the
 * dependency it was resolving. Errors that describe the refs the remote did
 * return (an unknown ref, an ambiguous commit-ish) already say which repository
 * they came from and are left alone.
 */
function gitResolveError (err: Error, bareSpecifier: string, repo: string): Error {
  if ((err as { code?: string }).code !== 'ERR_PNPM_GIT_LS_REMOTE_FAILED') return err
  return new PnpmError(
    'GIT_RESOLVE_FAILED',
    `Failed to resolve git dependency "${redactAndSanitize(bareSpecifier)}": ${err.message}`,
    { hint: httpsTransportHint(repo) ?? sshPublicKeyHint(repo, err.message) }
  )
}

/**
 * Guidance for a specifier that resolved over HTTPS on a machine whose git
 * cannot use that transport, or `undefined` when the resolution already went
 * over SSH — there, the transport that failed is the one the specifier asked
 * for.
 *
 * Substituting the transport is git's job rather than pnpm's: the URL pnpm
 * records has to work for every machine that installs the lockfile, while
 * `insteadOf` rewrites it for this one only.
 */
function httpsTransportHint (repo: string): string | undefined {
  let url: URL
  try {
    url = new URL(repo)
  } catch {
    return undefined
  }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') return undefined
  const host = redactAndSanitize(url.host)
  const hostname = redactAndSanitize(url.hostname)
  return `pnpm resolves this specifier over HTTPS because it does not ask for SSH, and the URL it records has to work on every machine that installs the lockfile.

If git can only reach ${hostname} over SSH here, substitute the transport locally, leaving the recorded URL alone:

    git config --global url."git@${hostname}:".insteadOf "${url.protocol}//${host}/"`
}

/**
 * Guidance when `git ls-remote` of an SSH remote fails with
 * `Permission denied (publickey)`, or `undefined` for any other failure.
 *
 * The specifier asked for SSH, so the hint is how to authenticate that
 * transport, plus a local HTTPS rewrite that leaves the recorded URL alone.
 * It does not apply to a lockfile clone: resolution is skipped while the
 * lockfile is up to date, and that failure is reported by the git fetcher.
 */
function sshPublicKeyHint (repo: string, detail: string): string | undefined {
  if (!isPublicKeyRefusal(detail)) return undefined
  const remote = parseSshRemote(repo)
  if (remote == null) return undefined
  const rewrite = remote.insteadOf == null
    ? ''
    : ` To reach ${remote.hostname} over HTTPS on this machine, leaving the recorded URL alone:

    git config --global url."https://${remote.hostname}/".insteadOf "${remote.insteadOf}"`
  return `Git refused the SSH key for ${remote.hostname} (Permission denied (publickey)).

Make sure ssh-agent has a key for that host loaded:

    ssh-add -l

If the repository is public, use an HTTPS specifier so pnpm records a URL that installs without a key.${rewrite}`
}

/**
 * Whether git's stderr carries OpenSSH's `Permission denied (...)` list of
 * refused methods with `publickey` among them. The detail also echoes the
 * host, so the word alone could be part of a host name.
 */
function isPublicKeyRefusal (detail: string): boolean {
  return detail.toLowerCase()
    .split('permission denied (')
    .slice(1)
    .some((rest) => rest.split(')')[0].includes('publickey'))
}

interface SshRemote {
  hostname: string
  /**
   * `undefined` unless the remote logs in as `git`. The prefix has to repeat
   * the user to match, and userinfo is never copied into a hint, so a
   * password or a token used as the user name cannot reach it.
   */
  insteadOf?: string
}

/**
 * `undefined` when `repo` is not an SSH reference or its host is not
 * {@link isShellSafeHost | shell safe}.
 */
function parseSshRemote (repo: string): SshRemote | undefined {
  const sshUrl = repo.startsWith('git+') ? repo.slice('git+'.length) : repo
  if (sshUrl.startsWith('ssh://')) {
    return parseSshUrlRemote(sshUrl)
  }
  return parseScpRemote(repo)
}

function parseSshUrlRemote (sshUrl: string): SshRemote | undefined {
  let url: URL
  try {
    url = new URL(sshUrl)
  } catch {
    return undefined
  }
  const hostname = redactAndSanitize(url.hostname)
  if (!isShellSafeHost(hostname)) return undefined
  const port = url.port === '' ? '' : `:${url.port}`
  return {
    hostname,
    insteadOf: url.username === 'git' ? `ssh://git@${hostname}${port}/` : undefined,
  }
}

function parseScpRemote (repo: string): SshRemote | undefined {
  if (repo.includes('://')) return undefined
  const colonPos = repo.indexOf(':')
  if (colonPos === -1) return undefined
  const authority = repo.slice(0, colonPos)
  const atPos = authority.lastIndexOf('@')
  if (atPos === -1) return undefined
  const hostname = redactAndSanitize(authority.slice(atPos + 1))
  if (!isShellSafeHost(hostname)) return undefined
  return {
    hostname,
    insteadOf: authority.slice(0, atPos) === 'git' ? `git@${hostname}:` : undefined,
  }
}

/**
 * A host safe to interpolate into the `git config` line of {@link sshPublicKeyHint}.
 *
 * The line is a command a user may paste, and the host comes from the
 * specifier.
 */
function isShellSafeHost (hostname: string): boolean {
  const bracketed = hostname.startsWith('[') && hostname.endsWith(']')
  const body = bracketed ? hostname.slice(1, -1) : hostname
  if (body === '' || body.startsWith('-') || body.startsWith('.') || body.endsWith('-') || body.endsWith('.')) return false
  return Array.from(body).every((char) => isSafeHostChar(char, bracketed))
}

function isSafeHostChar (char: string, bracketed: boolean): boolean {
  const code = char.charCodeAt(0)
  const isAlphanumeric = (code >= 48 && code <= 57) || (code >= 65 && code <= 90) || (code >= 97 && code <= 122)
  if (isAlphanumeric || char === '.' || char === '-' || char === '_') return true
  return bracketed && char === ':'
}

function isSsh (gitSpec: string): boolean {
  return gitSpec.slice(0, 10) === 'git+ssh://' ||
    gitSpec.slice(0, 4) === 'git@'
}
