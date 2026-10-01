import { getPublishedByPolicy } from '@pnpm/config.version-policy'
import { isError, redactAndSanitize } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { getRepoRefs } from '@pnpm/resolving.git-resolver'
import type { PackageVersionPolicy } from '@pnpm/types'
import pLimit from 'p-limit'
import semver from 'semver'

import { discoverActions } from './discoverActions.js'
import { readTagDates } from './readTagDates.js'
import { resolveServerUrl } from './resolveServerUrl.js'
import type { ActionReference, GitHubActionsOptions, PlannedUpdate, RepoVersion } from './types.js'

const SHA_PATTERN = /^[0-9a-f]{40}$/
const limitRepoReads = pLimit(8)

interface ResolvedAction {
  action: ActionReference
  versions: RepoVersion[]
  current: RepoVersion | null
}

type IsExempt = (action: ActionReference, candidate: RepoVersion) => boolean

type ReadRepoRefs = (repo: string) => Promise<Record<string, string>>

type ReadTagDates = (repo: string, tags: string[]) => Promise<Record<string, Date>>

type TagDatesByRepo = Map<string, Record<string, Date> | null>

interface ReleaseAgePolicy {
  datesByRepo: TagDatesByRepo
  exempt: IsExempt
  publishedBy: Date | undefined
}

export async function createUpdatePlan (opts: GitHubActionsOptions): Promise<PlannedUpdate[]> {
  const selected = selectActions(await discoverActions(opts.dir), opts.match)
  const serverUrl = resolveServerUrl(opts.serverUrl)
  const readRepoRefs = opts.readRepoRefs ?? (async (repo: string) => getRepoRefs(`${serverUrl}/${repo}.git`, null))
  const { publishedBy, publishedByExclude } = getPublishedByPolicy(opts)
  const resolved = await resolveCurrentVersions(selected, readRepoRefs)
  const policy = await readReleaseAgePolicy({
    publishedBy,
    publishedByExclude,
    read: opts.readTagDates ?? (async (repo, tags) => readTagDates(`${serverUrl}/${repo}.git`, tags)),
    resolved,
  })
  return resolved
    .map((resolvedAction) => planUpdate(resolvedAction, policy))
    .filter((plan): plan is PlannedUpdate => plan != null)
}

function selectActions (actions: ActionReference[], match: ((name: string) => boolean) | undefined): ActionReference[] {
  return match == null ? actions : actions.filter((action) => match(action.name) || match(action.repo))
}

async function readReleaseAgePolicy ({ publishedBy, publishedByExclude, read, resolved }: {
  publishedBy: Date | undefined
  publishedByExclude: PackageVersionPolicy | undefined
  read: ReadTagDates
  resolved: ResolvedAction[]
}): Promise<ReleaseAgePolicy> {
  const exempt: IsExempt = (action, candidate) =>
    publishedByExclude != null && isExempt(publishedByExclude, action, candidate.version)
  const datesByRepo: TagDatesByRepo = publishedBy == null
    ? new Map<string, Record<string, Date> | null>()
    : await readTagDatesByRepo(resolved, exempt, read)
  return { datesByRepo, exempt, publishedBy }
}

async function resolveCurrentVersions (selected: ActionReference[], readRepoRefs: ReadRepoRefs): Promise<ResolvedAction[]> {
  const refsByRepo = new Map<string, Promise<RepoVersion[]>>()
  return Promise.all(selected.map(async (action) => {
    let versionsPromise = refsByRepo.get(action.repo)
    if (versionsPromise == null) {
      versionsPromise = readRepoVersions(action.repo, readRepoRefs)
      refsByRepo.set(action.repo, versionsPromise)
    }
    const versions = await versionsPromise
    return { action, versions, current: findCurrentVersion(action, versions) }
  }))
}

async function readRepoVersions (repo: string, readRepoRefs: ReadRepoRefs): Promise<RepoVersion[]> {
  return limitRepoReads(async () => {
    try {
      return parseRepoVersions(await readRepoRefs(repo))
    } catch (err: unknown) {
      // The git error may echo a credentialed URL or raw stderr back, so
      // it is redacted and stripped of control characters before logging.
      globalWarn(redactAndSanitize(`Skipping the GitHub Actions from "${repo}": ${isError(err) ? err.message : String(err)}`))
      return []
    }
  })
}

function planUpdate ({ action, versions, current }: ResolvedAction, policy: ReleaseAgePolicy): PlannedUpdate | null {
  if (current == null) return null
  const dates = policy.datesByRepo.get(action.repo)
  if (dates === null) return null
  const { publishedBy } = policy
  const admits = (candidate: RepoVersion) => publishedBy == null ||
    semver.lte(candidate.version, current.version) ||
    policy.exempt(action, candidate) ||
    (dates?.[candidate.tag] != null && dates[candidate.tag] <= publishedBy)
  const stable = versions.filter(({ version }) => version.prerelease.length === 0)
  const candidates = (current.version.prerelease.length === 0 ? stable : versions).filter(admits)
  const latest = candidates.at(-1)
  const wanted = candidates
    .filter(({ version }) => semver.satisfies(version, `^${current.version.version}`))
    .at(-1)
  if (latest == null || wanted == null) return null
  return { action, current, latest, wanted }
}

/**
 * The creation dates of the tags newer than the version an action is on, per
 * repository. A repository whose dates cannot be read maps to `null` and is
 * skipped with a warning, so none of its versions is offered without its age
 * being known.
 */
async function readTagDatesByRepo (
  resolved: ResolvedAction[],
  exempt: IsExempt,
  read: ReadTagDates
): Promise<TagDatesByRepo> {
  const tagsByRepo = collectNewerTagsByRepo(resolved, exempt)
  const entries = await Promise.all([...tagsByRepo]
    .filter(([, tags]) => tags.size > 0)
    .map(async ([repo, tags]) => readRepoTagDates(repo, tags, read)))
  return new Map(entries)
}

function collectNewerTagsByRepo (resolved: ResolvedAction[], exempt: IsExempt): Map<string, Set<string>> {
  const tagsByRepo = new Map<string, Set<string>>()
  for (const { action, versions, current } of resolved) {
    if (current == null) continue
    const tags = tagsByRepo.get(action.repo) ?? new Set<string>()
    for (const candidate of versions) {
      if (isNewerCandidate(current, candidate) && !exempt(action, candidate)) tags.add(candidate.tag)
    }
    tagsByRepo.set(action.repo, tags)
  }
  return tagsByRepo
}

function isNewerCandidate (current: RepoVersion, candidate: RepoVersion): boolean {
  return semver.gt(candidate.version, current.version) &&
    (current.version.prerelease.length > 0 || candidate.version.prerelease.length === 0)
}

async function readRepoTagDates (repo: string, tags: Set<string>, read: ReadTagDates): Promise<[string, Record<string, Date> | null]> {
  return limitRepoReads(async (): Promise<[string, Record<string, Date> | null]> => {
    try {
      return [repo, await read(repo, [...tags].sort())]
    } catch (err: unknown) {
      globalWarn(redactAndSanitize(`Skipping the GitHub Actions from "${repo}": cannot read the release dates that minimumReleaseAge needs: ${isError(err) ? err.message : String(err)}`))
      return [repo, null]
    }
  })
}

function isExempt (exclude: PackageVersionPolicy, action: ActionReference, version: semver.SemVer): boolean {
  return [action.name, action.repo].some((name) => {
    const match = exclude(name)
    return Array.isArray(match) ? match.some((excluded) => semver.eq(excluded, version, { loose: true })) : match
  })
}

function parseRepoVersions (refs: Record<string, string>): RepoVersion[] {
  const versions: RepoVersion[] = []
  for (const [ref, commit] of Object.entries(refs)) {
    const match = /^refs\/tags\/(v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/.exec(ref)
    if (match == null) continue
    const version = semver.parse(match[1], { loose: true })
    if (version == null) continue
    versions.push({
      commit: refs[`${ref}^{}`] ?? commit,
      tag: match[1],
      version,
    })
  }
  return versions.sort((left, right) => semver.compare(left.version, right.version))
}

function findCurrentVersion (action: ActionReference, versions: RepoVersion[]): RepoVersion | null {
  const annotated = findCommentAnnotatedVersion(action, versions)
  if (annotated != null) return annotated
  const parsed = semver.parse(action.ref, { loose: true })
  if (parsed != null) {
    return versions.find(({ version }) => semver.eq(version, parsed)) ?? null
  }
  if (/^v?\d+$/.test(action.ref)) {
    const major = Number(action.ref.replace(/^v/, ''))
    return versions.filter(({ version }) => version.major === major && version.prerelease.length === 0).at(-1) ?? null
  }
  if (SHA_PATTERN.test(action.ref)) {
    return versions.filter(({ commit }) => commit === action.ref).at(-1) ?? null
  }
  return null
}

function findCommentAnnotatedVersion (action: ActionReference, versions: RepoVersion[]): RepoVersion | undefined {
  if (!SHA_PATTERN.test(action.ref) || action.commentVersion == null) return undefined
  const parsed = semver.parse(action.commentVersion, { loose: true })
  if (parsed == null) return undefined
  return versions.find(({ commit, version }) => commit === action.ref && semver.eq(version, parsed))
}
