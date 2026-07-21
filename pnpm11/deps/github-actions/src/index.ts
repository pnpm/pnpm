import fs from 'node:fs/promises'
import path from 'node:path'
import util from 'node:util'

import { getRepoRefs } from '@pnpm/resolving.git-resolver'
import semver from 'semver'
import YAML, { type Document, isMap, isNode, isScalar, isSeq, type Node, type Scalar } from 'yaml'

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
  name: string
  node: Scalar<string>
  ref: string
  repo: string
}

interface ActionFile {
  document: Document
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

export function isGitHubActionSelector (selector: string): boolean {
  const pattern = selector.startsWith('!') ? selector.slice(1) : selector
  return !pattern.startsWith('@') && pattern.includes('/')
}

export async function findOutdatedGitHubActions (
  opts: FindOutdatedGitHubActionsOptions
): Promise<OutdatedGitHubAction[]> {
  const plans = await createUpdatePlan(opts)
  const target = (plan: PlannedUpdate) => opts.compatible ? plan.wanted : plan.latest
  return dedupeOutdated(plans
    .filter((plan) => semver.lt(plan.current.version, target(plan).version))
    .map((plan) => ({
      current: plan.current.version.version,
      latest: target(plan).version.version,
      name: plan.action.name,
      wanted: plan.wanted.version.version,
      homepage: `https://github.com/${plan.action.repo}`,
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
  const changedFiles = new Set<ActionFile>()
  for (const plan of updates) {
    const target = opts.latest ? plan.latest : plan.wanted
    plan.action.node.value = `${plan.action.name}@${renderTargetRef(target)}`
    updateVersionComment(plan.action.node, plan.action.commentVersion, target)
    changedFiles.add(plan.action.file)
  }
  await Promise.all([...changedFiles].map(async (file) => fs.writeFile(file.path, file.document.toString())))
  return dedupeOutdated(updates.map((plan) => {
    const target = opts.latest ? plan.latest : plan.wanted
    return {
      current: plan.current.version.version,
      latest: target.version.version,
      name: plan.action.name,
      wanted: plan.wanted.version.version,
      homepage: `https://github.com/${plan.action.repo}`,
    }
  }))
}

async function createUpdatePlan (opts: GitHubActionsOptions): Promise<PlannedUpdate[]> {
  const actions = await discoverActions(opts.dir)
  const selected = opts.match == null ? actions : actions.filter((action) => opts.match!(action.name) || opts.match!(action.repo))
  const readRepoRefs = opts.readRepoRefs ?? readRefsWithGit
  const refsByRepo = new Map<string, Promise<RepoVersion[]>>()
  return (await Promise.all(selected.map(async (action): Promise<PlannedUpdate | null> => {
    let versionsPromise = refsByRepo.get(action.repo)
    if (versionsPromise == null) {
      versionsPromise = readRepoRefs(action.repo).then(parseRepoVersions)
      refsByRepo.set(action.repo, versionsPromise)
    }
    const versions = await versionsPromise
    const current = findCurrentVersion(action, versions)
    if (current == null) return null
    const stable = versions.filter(({ version }) => version.prerelease.length === 0)
    const candidates = current.version.prerelease.length === 0 ? stable : versions
    const latest = candidates.at(-1)
    const wanted = candidates.filter(({ version }) => version.major === current.version.major).at(-1)
    if (latest == null || wanted == null) return null
    return { action, current, latest, wanted }
  }))).filter((plan): plan is PlannedUpdate => plan != null)
}

async function discoverActions (dir: string): Promise<ActionReference[]> {
  const workflowDir = path.join(dir, '.github', 'workflows')
  let entries: string[]
  try {
    entries = await fs.readdir(workflowDir)
  } catch (err: unknown) {
    if (isErrorCode(err, 'ENOENT')) return []
    throw err
  }
  const workflowFiles = entries
    .filter((entry) => entry.endsWith('.yml') || entry.endsWith('.yaml'))
    .map((entry) => path.join(workflowDir, entry))
  const visited = new Set<string>()
  const actions: ActionReference[] = []
  await Promise.all(workflowFiles.map(scanFile))
  return actions

  async function scanFile (filePath: string): Promise<void> {
    if (visited.has(filePath)) return
    visited.add(filePath)
    const source = await fs.readFile(filePath, 'utf8')
    const file = { document: YAML.parseDocument(source), path: filePath }
    if (file.document.errors.length > 0) throw file.document.errors[0]
    const localReferences: string[] = []
    for (const node of findUsesScalars(file.document.contents)) {
      const value = node.value
      if (value.startsWith('./')) {
        localReferences.push(value)
        continue
      }
      const parsed = parseActionReference(value)
      if (parsed == null) continue
      actions.push({
        ...parsed,
        commentVersion: getCommentVersion(node),
        file,
        node,
      })
    }
    const localFiles = await Promise.all(localReferences.map(async (reference) => resolveLocalReference(dir, reference)))
    await Promise.all(localFiles.filter((local): local is string => local != null).map(scanFile))
  }
}

function findUsesScalars (node: Node | null | undefined): Array<Scalar<string>> {
  if (node == null) return []
  if (isMap(node)) {
    const found: Array<Scalar<string>> = []
    for (const pair of node.items) {
      if (isScalar(pair.key) && pair.key.value === 'uses' && isScalar(pair.value) && typeof pair.value.value === 'string') {
        found.push(pair.value as Scalar<string>)
      } else if (isNode(pair.value)) {
        found.push(...findUsesScalars(pair.value))
      }
    }
    return found
  }
  if (isSeq(node)) {
    return node.items.flatMap((item) => isNode(item) ? findUsesScalars(item) : [])
  }
  return []
}

async function resolveLocalReference (rootDir: string, reference: string): Promise<string | null> {
  const target = path.resolve(rootDir, reference)
  const candidate = target.endsWith('.yml') || target.endsWith('.yaml')
    ? await existingPath(target)
    : (await Promise.all(['action.yml', 'action.yaml'].map(async (filename) => existingPath(path.join(target, filename)))))
      .find((candidate): candidate is string => candidate != null) ?? null
  if (candidate == null) return null
  const [realRoot, realCandidate] = await Promise.all([fs.realpath(rootDir), fs.realpath(candidate)])
  const relative = path.relative(realRoot, realCandidate)
  return relative === '' || (!path.isAbsolute(relative) && relative !== '..' && !relative.startsWith(`..${path.sep}`))
    ? realCandidate
    : null
}

async function existingPath (candidate: string): Promise<string | null> {
  try {
    await fs.access(candidate)
    return candidate
  } catch (err: unknown) {
    if (!isErrorCode(err, 'ENOENT')) throw err
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
  if (parsed != null && parsed.raw.replace(/^v/, '').split('.').length === 3) {
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

function renderTargetRef (target: RepoVersion): string {
  return target.commit
}

function getCommentVersion (node: Scalar<string>): string | undefined {
  const candidate = node.comment?.trimStart().split(/\s/, 1)[0]
  return candidate != null && semver.valid(candidate, { loose: true }) != null ? candidate : undefined
}

function updateVersionComment (node: Scalar<string>, previousVersion: string | undefined, target: RepoVersion): void {
  if (previousVersion != null && node.comment != null) {
    node.comment = node.comment.replace(previousVersion, target.tag)
  } else {
    node.comment = ` ${target.tag}${node.comment == null ? '' : ` ${node.comment.trimStart()}`}`
  }
}

function dedupeOutdated (actions: OutdatedGitHubAction[]): OutdatedGitHubAction[] {
  return [...new Map(actions.map((action) => [action.name, action])).values()]
    .sort((left, right) => left.name.localeCompare(right.name))
}

async function readRefsWithGit (repo: string): Promise<Record<string, string>> {
  return getRepoRefs(`https://github.com/${repo}.git`, null)
}

function isErrorCode (err: unknown, code: string): err is NodeJS.ErrnoException {
  return util.types.isNativeError(err) && 'code' in err && err.code === code
}
