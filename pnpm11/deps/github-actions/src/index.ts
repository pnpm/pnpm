import fs from 'node:fs/promises'
import { isIP } from 'node:net'
import os from 'node:os'
import path from 'node:path'
import util from 'node:util'

import { getPublishedByPolicy } from '@pnpm/config.version-policy'
import { PnpmError, redactAndSanitize, redactUrlForDisplay } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { nonInteractiveGitEnv } from '@pnpm/network.git-utils'
import { getRepoRefs } from '@pnpm/resolving.git-resolver'
import type { PackageVersionPolicy } from '@pnpm/types'
import { safeExeca as execa } from 'execa'
import { isSubdir } from 'is-subdir'
import pLimit from 'p-limit'
import semver from 'semver'
import writeFileAtomic from 'write-file-atomic'
import YAML, { isMap, isNode, isScalar, isSeq, type Node, type Scalar } from 'yaml'

export interface OutdatedGitHubAction {
  current: string
  latest: string
  name: string
  wanted: string
  homepage: string
}

export interface GitHubActionsOptions {
  dir: string
  match?: (name: string) => boolean
  readRepoRefs?: (repo: string) => Promise<Record<string, string>>
  /**
   * Reads the creation dates of tags of a repository. Defaults to fetching
   * the tags without their trees.
   */
  readTagDates?: (repo: string, tags: string[]) => Promise<Record<string, Date>>
  /**
   * Versions tagged fewer than this many minutes ago are not offered.
   */
  minimumReleaseAge?: number
  /**
   * Actions exempt from `minimumReleaseAge`, matched against the action and
   * repository names.
   */
  minimumReleaseAgeExclude?: string[]
  /**
   * The base URL of the GitHub server hosting the action repositories.
   * Defaults to the `GITHUB_SERVER_URL` environment variable, or
   * https://github.com.
   */
  serverUrl?: string
}

export interface FindOutdatedGitHubActionsOptions extends GitHubActionsOptions {
  compatible?: boolean
}

export interface UpdateGitHubActionsOptions extends GitHubActionsOptions {
  latest?: boolean
}

interface ActionReference {
  commentVersion?: string
  file: ActionFile
  flowStyle: boolean
  indentation: string
  name: string
  originalValue: string
  range: readonly [number, number]
  ref: string
  repo: string
}

interface ActionFile {
  path: string
}

interface RepoVersion {
  commit: string
  tag: string
  version: semver.SemVer
}

interface PlannedUpdate {
  action: ActionReference
  current: RepoVersion
  latest: RepoVersion
  wanted: RepoVersion
}

const SHA_PATTERN = /^[0-9a-f]{40}$/
const limitRepoReads = pLimit(8)

export interface GitHubActionsOptInOptions {
  includeGithubActions?: boolean
  updateConfig?: { githubActions?: boolean }
}

/**
 * GitHub Actions dependencies are opt-in. Reading them means running
 * `git ls-remote` against every referenced repository, so `pnpm outdated` and
 * `pnpm update` only look at workflow files when asked to, either with
 * `--include-github-actions` or with `update.githubActions: true`.
 */
export function shouldCheckGitHubActions (opts: GitHubActionsOptInOptions): boolean {
  return opts.includeGithubActions === true || opts.updateConfig?.githubActions === true
}

export function isGitHubActionSelector (selector: string): boolean {
  const pattern = selector.startsWith('!') ? selector.slice(1) : selector
  return !pattern.startsWith('@') && pattern.includes('/')
}

export function normalizeGitHubActionSelector (selector: string): string {
  if (!isGitHubActionSelector(selector)) return selector
  const refSeparator = selector.lastIndexOf('@')
  return refSeparator === -1 ? selector : selector.slice(0, refSeparator)
}

export async function findOutdatedGitHubActions (
  opts: FindOutdatedGitHubActionsOptions
): Promise<OutdatedGitHubAction[]> {
  const plans = await createUpdatePlan(opts)
  const serverUrl = resolveServerUrl(opts.serverUrl)
  const target = (plan: PlannedUpdate) => opts.compatible ? plan.wanted : plan.latest
  return dedupeOutdated(plans
    .filter((plan) => semver.lt(plan.current.version, target(plan).version))
    .map((plan) => ({
      current: plan.current.version.version,
      latest: target(plan).version.version,
      name: plan.action.name,
      wanted: plan.wanted.version.version,
      homepage: redactUrlForDisplay(`${serverUrl}/${plan.action.repo}`),
    })))
}

export async function updateGitHubActions (
  opts: UpdateGitHubActionsOptions
): Promise<OutdatedGitHubAction[]> {
  const plans = await createUpdatePlan(opts)
  const updates = plans.filter((plan) => {
    const target = opts.latest ? plan.latest : plan.wanted
    return semver.lte(plan.current.version, target.version) &&
      (plan.action.ref !== target.commit || plan.action.commentVersion !== target.tag)
  })
  const edits = new Map<ActionFile, Array<{ originalValue: string, range: readonly [number, number], value: string }>>()
  for (const plan of updates) {
    const target = opts.latest ? plan.latest : plan.wanted
    const replacements = edits.get(plan.action.file) ?? []
    replacements.push({
      originalValue: plan.action.originalValue,
      range: plan.action.range,
      value: renderTargetValue(plan.action, target),
    })
    edits.set(plan.action.file, replacements)
  }
  await Promise.all([...edits].map(async ([file, replacements]) => {
    let source: string
    try {
      source = await fs.readFile(file.path, 'utf8')
    } catch (err: unknown) {
      throw workflowError('READ', file.path, err)
    }
    for (const { originalValue, range: [start, end] } of replacements) {
      if (start < 0 || start > end || end > source.length || source.slice(start, end) !== originalValue) {
        throw new PnpmError('GITHUB_ACTIONS_WORKFLOW_CHANGED', `GitHub Actions workflow ${file.path} changed while resolving updates; retry the command`)
      }
    }
    replacements.sort((left, right) => right.range[0] - left.range[0])
    for (const replacement of replacements) {
      source = source.slice(0, replacement.range[0]) + replacement.value + source.slice(replacement.range[1])
    }
    try {
      await writeFileAtomic(file.path, source)
    } catch (err: unknown) {
      throw workflowError('WRITE', file.path, err)
    }
  }))
  const serverUrl = resolveServerUrl(opts.serverUrl)
  return dedupeOutdated(updates.map((plan) => {
    const target = opts.latest ? plan.latest : plan.wanted
    return {
      current: plan.current.version.version,
      latest: target.version.version,
      name: plan.action.name,
      wanted: plan.wanted.version.version,
      homepage: redactUrlForDisplay(`${serverUrl}/${plan.action.repo}`),
    }
  }))
}

async function createUpdatePlan (opts: GitHubActionsOptions): Promise<PlannedUpdate[]> {
  const actions = await discoverActions(opts.dir)
  const selected = opts.match == null ? actions : actions.filter((action) => opts.match!(action.name) || opts.match!(action.repo))
  const serverUrl = resolveServerUrl(opts.serverUrl)
  const readRepoRefs = opts.readRepoRefs ?? (async (repo: string) => getRepoRefs(`${serverUrl}/${repo}.git`, null))
  const { publishedBy, publishedByExclude } = getPublishedByPolicy(opts)
  const refsByRepo = new Map<string, Promise<RepoVersion[]>>()
  const resolved = await Promise.all(selected.map(async (action) => {
    let versionsPromise = refsByRepo.get(action.repo)
    if (versionsPromise == null) {
      versionsPromise = limitRepoReads(async () => {
        try {
          return parseRepoVersions(await readRepoRefs(action.repo))
        } catch (err: unknown) {
          // The git error may echo a credentialed URL or raw stderr back, so
          // it is redacted and stripped of control characters before logging.
          globalWarn(redactAndSanitize(`Skipping the GitHub Actions from "${action.repo}": ${util.types.isNativeError(err) ? err.message : String(err)}`))
          return []
        }
      })
      refsByRepo.set(action.repo, versionsPromise)
    }
    const versions = await versionsPromise
    return { action, versions, current: findCurrentVersion(action, versions) }
  }))
  const exempt = (action: ActionReference, candidate: RepoVersion) =>
    publishedByExclude != null && isExempt(publishedByExclude, action, candidate.version)
  const datesByRepo = publishedBy == null
    ? new Map<string, Record<string, Date> | null>()
    : await readTagDatesByRepo(resolved, exempt, opts.readTagDates ?? (async (repo, tags) => readTagDates(`${serverUrl}/${repo}.git`, tags)))
  return resolved.map(({ action, versions, current }): PlannedUpdate | null => {
    if (current == null) return null
    const dates = datesByRepo.get(action.repo)
    if (dates === null) return null
    const admits = (candidate: RepoVersion) => publishedBy == null ||
      semver.lte(candidate.version, current.version) ||
      exempt(action, candidate) ||
      (dates?.[candidate.tag] != null && dates[candidate.tag] <= publishedBy)
    const stable = versions.filter(({ version }) => version.prerelease.length === 0)
    const candidates = (current.version.prerelease.length === 0 ? stable : versions).filter(admits)
    const latest = candidates.at(-1)
    const wanted = candidates
      .filter(({ version }) => semver.satisfies(version, `^${current.version.version}`))
      .at(-1)
    if (latest == null || wanted == null) return null
    return { action, current, latest, wanted }
  }).filter((plan): plan is PlannedUpdate => plan != null)
}

/**
 * The creation dates of the tags newer than the version an action is on, per
 * repository. A repository whose dates cannot be read maps to `null` and is
 * skipped with a warning, so none of its versions is offered without its age
 * being known.
 */
async function readTagDatesByRepo (
  resolved: Array<{ action: ActionReference, versions: RepoVersion[], current: RepoVersion | null }>,
  exempt: (action: ActionReference, candidate: RepoVersion) => boolean,
  read: (repo: string, tags: string[]) => Promise<Record<string, Date>>
): Promise<Map<string, Record<string, Date> | null>> {
  const tagsByRepo = new Map<string, Set<string>>()
  for (const { action, versions, current } of resolved) {
    if (current == null) continue
    const tags = tagsByRepo.get(action.repo) ?? new Set<string>()
    for (const candidate of versions) {
      if (
        semver.gt(candidate.version, current.version) &&
        (current.version.prerelease.length > 0 || candidate.version.prerelease.length === 0) &&
        !exempt(action, candidate)
      ) tags.add(candidate.tag)
    }
    tagsByRepo.set(action.repo, tags)
  }
  const entries = await Promise.all([...tagsByRepo].filter(([, tags]) => tags.size > 0).map(async ([repo, tags]) => limitRepoReads(async (): Promise<[string, Record<string, Date> | null]> => {
    try {
      return [repo, await read(repo, [...tags].sort())]
    } catch (err: unknown) {
      globalWarn(redactAndSanitize(`Skipping the GitHub Actions from "${repo}": cannot read the release dates that minimumReleaseAge needs: ${util.types.isNativeError(err) ? err.message : String(err)}`))
      return [repo, null]
    }
  })))
  return new Map(entries)
}

function isExempt (exclude: PackageVersionPolicy, action: ActionReference, version: semver.SemVer): boolean {
  return [action.name, action.repo].some((name) => {
    const match = exclude(name)
    return Array.isArray(match) ? match.some((excluded) => semver.eq(excluded, version, { loose: true })) : match
  })
}

/**
 * `git ls-remote` lists tags without dates, so the tags are fetched shallowly
 * and without trees into a scratch repository, where each one's creation
 * date can be read: the tagger date of an annotated tag, the committer date of
 * a lightweight one.
 *
 * Whoever creates a tag or commit sets these dates, and the server does not
 * check them, so a backdated tag passes. Unlike a registry's publish time,
 * they hold back only releases that carry their real date.
 */
async function readTagDates (repoUrl: string, tags: string[]): Promise<Record<string, Date>> {
  const scratch = await fs.mkdtemp(path.join(os.tmpdir(), 'pnpm-github-actions-'))
  try {
    const env = await nonInteractiveGitEnv()
    await execa('git', ['init', '--quiet', '--bare'], { cwd: scratch, env })
    const input = tags.map((tag) => `refs/tags/${tag}:refs/tags/${tag}\n`).join('')
    const fetchArgs = ['fetch', '--quiet', '--depth=1', '--filter=tree:0', '--no-tags', '--no-write-fetch-head', '--stdin', repoUrl]
    try {
      await execa('git', fetchArgs, { cwd: scratch, env, input })
    } catch {
      // One retry, like `git ls-remote`.
      await execa('git', fetchArgs, { cwd: scratch, env, input })
    }
    const { stdout } = await execa('git', ['for-each-ref', '--format=%(creatordate:unix) %(refname:strip=2)', 'refs/tags'], { cwd: scratch, env })
    const dates: Record<string, Date> = {}
    for (const line of (stdout as string).split('\n')) {
      const separator = line.indexOf(' ')
      const seconds = Number(line.slice(0, separator))
      if (separator > 0 && Number.isInteger(seconds)) dates[line.slice(separator + 1)] = new Date(seconds * 1000)
    }
    return dates
  } finally {
    await fs.rm(scratch, { force: true, recursive: true })
  }
}

async function discoverActions (dir: string): Promise<ActionReference[]> {
  let realRoot: string
  try {
    realRoot = await fs.realpath(dir)
  } catch (err: unknown) {
    throw workflowError('READ', dir, err)
  }
  const workflowDir = path.join(dir, '.github', 'workflows')
  let entries: string[]
  try {
    entries = await fs.readdir(workflowDir)
  } catch (err: unknown) {
    if (isErrorCode(err, 'ENOENT')) return []
    throw workflowError('READ', workflowDir, err)
  }
  const workflowFiles = entries
    .filter((entry) => entry.endsWith('.yml') || entry.endsWith('.yaml'))
    .map((entry) => path.join(workflowDir, entry))
  const visited = new Set<string>()
  const actions: ActionReference[] = []
  await Promise.all(workflowFiles.map(scanFile))
  return actions

  async function scanFile (filePath: string): Promise<void> {
    let realFilePath: string
    try {
      realFilePath = await fs.realpath(filePath)
    } catch (err: unknown) {
      throw workflowError('READ', filePath, err)
    }
    if (!isSubdir(realRoot, realFilePath)) {
      throw new PnpmError('GITHUB_ACTIONS_WORKFLOW_OUTSIDE_ROOT', `GitHub Actions workflow is outside the project root: ${filePath}`)
    }
    if (visited.has(realFilePath)) return
    visited.add(realFilePath)
    let source: string
    try {
      source = await fs.readFile(realFilePath, 'utf8')
    } catch (err: unknown) {
      throw workflowError('READ', realFilePath, err)
    }
    const document = YAML.parseDocument(source)
    if (document.errors.length > 0) throw workflowError('PARSE', realFilePath, document.errors[0])
    const file = { path: realFilePath }
    const localReferences: string[] = []
    for (const node of findUsesScalars(document.contents)) {
      const value = node.value
      const localReference = parseLocalReference(value)
      if (localReference != null) {
        localReferences.push(localReference)
        continue
      }
      const parsed = parseActionReference(value)
      if (parsed == null) continue
      if (node.range == null) throw new Error(`Missing source range for GitHub Action in ${realFilePath}`)
      const end = trimLineBreak(source, node.range[2] ?? node.range[1])
      actions.push({
        ...parsed,
        commentVersion: getCommentVersion(node),
        file,
        flowStyle: isFlowStyle(source, node.range[1]),
        indentation: getIndentation(source, node.range[0]),
        originalValue: source.slice(node.range[0], end),
        range: [node.range[0], end],
      })
    }
    const localFiles = await Promise.all(localReferences.map(async (reference) => resolveLocalReference(dir, reference)))
    await Promise.all(localFiles.filter((local): local is string => local != null).map(scanFile))
  }
}

function findUsesScalars (node: Node | null | undefined): Array<Scalar<string>> {
  if (!isMap(node)) return []
  const found: Array<Scalar<string>> = []
  const jobs = findMapValue(node, 'jobs')
  if (isMap(jobs)) {
    for (const job of jobs.items) {
      if (!isMap(job.value)) continue
      const jobUses = findStringScalar(job.value, 'uses')
      if (jobUses != null) found.push(jobUses)
      found.push(...findStepUses(findMapValue(job.value, 'steps')))
    }
  }
  const runs = findMapValue(node, 'runs')
  if (isMap(runs)) {
    found.push(...findStepUses(findMapValue(runs, 'steps')))
  }
  return found
}

function findStepUses (node: Node | null): Array<Scalar<string>> {
  if (!isSeq(node)) return []
  return node.items.flatMap((item) => {
    if (!isMap(item)) return []
    const uses = findStringScalar(item, 'uses')
    return uses == null ? [] : [uses]
  })
}

function findMapValue (node: Node, key: string): Node | null {
  if (!isMap(node)) return null
  const value = node.items.find((pair) => isScalar(pair.key) && pair.key.value === key)?.value
  return isNode(value) ? value : null
}

function findStringScalar (node: Node, key: string): Scalar<string> | null {
  const value = findMapValue(node, key)
  return isScalar(value) && typeof value.value === 'string' ? value as Scalar<string> : null
}

/**
 * Returns the repository-relative path of a `uses:` value that points into the
 * same repository, either the workspace-relative `./` form or GitHub's
 * self-repository `$/` form. Returns `null` for every other value, such as an
 * `owner/repo@ref` or `docker://` reference, which is left to
 * `parseActionReference`.
 */
function parseLocalReference (value: string): string | null {
  return value.startsWith('./') || value.startsWith('$/') ? value.slice(2) : null
}

async function resolveLocalReference (rootDir: string, reference: string): Promise<string | null> {
  const target = path.resolve(rootDir, reference)
  const candidate = target.endsWith('.yml') || target.endsWith('.yaml')
    ? await existingPath(target)
    : (await Promise.all(['action.yml', 'action.yaml'].map(async (filename) => existingPath(path.join(target, filename)))))
      .find((candidate): candidate is string => candidate != null) ?? null
  if (candidate == null) return null
  let realRoot: string
  let realCandidate: string
  try {
    [realRoot, realCandidate] = await Promise.all([fs.realpath(rootDir), fs.realpath(candidate)])
  } catch (err: unknown) {
    throw workflowError('READ', candidate, err)
  }
  return isSubdir(realRoot, realCandidate) ? realCandidate : null
}

async function existingPath (candidate: string): Promise<string | null> {
  try {
    await fs.access(candidate)
    return candidate
  } catch (err: unknown) {
    if (!isErrorCode(err, 'ENOENT')) throw workflowError('READ', candidate, err)
    return null
  }
}

function parseActionReference (value: string): Pick<ActionReference, 'name' | 'ref' | 'repo'> | null {
  if (value.startsWith('docker://')) return null
  const at = value.lastIndexOf('@')
  if (at <= 0 || at === value.length - 1) return null
  const name = value.slice(0, at)
  const parts = name.split('/')
  if (parts.length < 2 || parts[0] === '' || parts[1] === '') return null
  return { name, ref: value.slice(at + 1), repo: `${parts[0]}/${parts[1]}` }
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
  if (SHA_PATTERN.test(action.ref) && action.commentVersion != null) {
    const parsed = semver.parse(action.commentVersion, { loose: true })
    if (parsed != null) {
      const annotated = versions.find(({ commit, version }) => commit === action.ref && semver.eq(version, parsed))
      if (annotated != null) return annotated
    }
  }
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

function getCommentVersion (node: Scalar<string>): string | undefined {
  const candidate = node.comment?.trimStart().split(/\s/, 1)[0]
  return candidate != null && semver.valid(candidate, { loose: true }) != null ? candidate : undefined
}

function renderTargetValue (action: ActionReference, target: RepoVersion): string {
  const oldReference = `${action.name}@${action.ref}`
  const newReference = `${action.name}@${target.commit}`
  let value = action.originalValue.replace(oldReference, newReference)
  if (action.commentVersion != null) return value.replace(action.commentVersion, target.tag)
  const comment = value.indexOf(' #')
  if (comment === -1) {
    return action.flowStyle
      ? `${value.trimEnd()} # ${target.tag}\n${action.indentation}`
      : `${value} # ${target.tag}`
  }
  return `${value.slice(0, comment + 2)}${target.tag} ${value.slice(comment + 2).trimStart()}`
}

function isFlowStyle (source: string, end: number): boolean {
  const lineEnd = source.indexOf('\n', end)
  const following = source.slice(end, lineEnd === -1 ? source.length : lineEnd).trimStart()
  return following.startsWith('}') || following.startsWith(']') || following.startsWith(',')
}

function getIndentation (source: string, start: number): string {
  const lineStart = source.lastIndexOf('\n', start - 1) + 1
  return ' '.repeat(start - lineStart)
}

function trimLineBreak (source: string, end: number): number {
  while (end > 0 && (source[end - 1] === '\n' || source[end - 1] === '\r')) end--
  return end
}

function dedupeOutdated (actions: OutdatedGitHubAction[]): OutdatedGitHubAction[] {
  return [...new Map(actions.map((action) => [action.name, action])).values()]
    .sort((left, right) => left.name.localeCompare(right.name))
}

function resolveServerUrl (serverUrl: string | undefined): string {
  const parsed = URL.parse(serverUrl || process.env.GITHUB_SERVER_URL || 'https://github.com')
  const loopback = parsed != null && (parsed.hostname === 'localhost' || parsed.hostname === '[::1]' || (isIP(parsed.hostname) === 4 && parsed.hostname.startsWith('127.')))
  if (parsed == null || (parsed.protocol !== 'https:' && !(parsed.protocol === 'http:' && loopback))) {
    throw new PnpmError('GITHUB_ACTIONS_SERVER_PROTOCOL', 'The GitHub Actions server URL must use HTTPS, except for HTTP on loopback hosts')
  }
  let url = parsed.href
  while (url.endsWith('/')) url = url.slice(0, -1)
  return url
}

function workflowError (operation: 'PARSE' | 'READ' | 'WRITE', filePath: string, cause: unknown): PnpmError {
  const detail = util.types.isNativeError(cause) ? cause.message : String(cause)
  return new PnpmError(`GITHUB_ACTIONS_WORKFLOW_${operation}`, `Failed to ${operation.toLowerCase()} GitHub Actions workflow ${filePath}: ${detail}`, { cause })
}

function isErrorCode (err: unknown, code: string): err is NodeJS.ErrnoException {
  return util.types.isNativeError(err) && 'code' in err && err.code === code
}
