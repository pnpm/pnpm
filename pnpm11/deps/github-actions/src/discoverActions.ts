import fs from 'node:fs/promises'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { isSubdir } from 'is-subdir'
import semver from 'semver'
import YAML, { isMap, isNode, isScalar, isSeq, type Node, type Scalar, type YAMLMap } from 'yaml'

import { isErrorCode, workflowError } from './errors.js'
import type { ActionFile, ActionReference } from './types.js'

interface ScanContext {
  actions: ActionReference[]
  dir: string
  realRoot: string
  visited: Set<string>
}

interface WorkflowReferences {
  actions: ActionReference[]
  localReferences: string[]
}

export async function discoverActions (dir: string): Promise<ActionReference[]> {
  const realRoot = await readRealPath(dir)
  const workflowFiles = await listWorkflowFiles(dir)
  const context: ScanContext = { actions: [], dir, realRoot, visited: new Set<string>() }
  await Promise.all(workflowFiles.map(async (filePath) => scanFile(context, filePath)))
  return context.actions
}

async function listWorkflowFiles (dir: string): Promise<string[]> {
  const workflowDir = path.join(dir, '.github', 'workflows')
  let entries: string[]
  try {
    entries = await fs.readdir(workflowDir)
  } catch (err: unknown) {
    if (isErrorCode(err, 'ENOENT')) return []
    throw workflowError('READ', workflowDir, err)
  }
  return entries
    .filter((entry) => entry.endsWith('.yml') || entry.endsWith('.yaml'))
    .map((entry) => path.join(workflowDir, entry))
}

async function scanFile (context: ScanContext, filePath: string): Promise<void> {
  const realFilePath = await readRealPath(filePath)
  if (!isSubdir(context.realRoot, realFilePath)) {
    throw new PnpmError('GITHUB_ACTIONS_WORKFLOW_OUTSIDE_ROOT', `GitHub Actions workflow is outside the project root: ${filePath}`)
  }
  if (context.visited.has(realFilePath)) return
  context.visited.add(realFilePath)
  let source: string
  try {
    source = await fs.readFile(realFilePath, 'utf8')
  } catch (err: unknown) {
    throw workflowError('READ', realFilePath, err)
  }
  const { actions, localReferences } = collectWorkflowReferences(realFilePath, source)
  context.actions.push(...actions)
  const localFiles = await Promise.all(localReferences.map(async (reference) => resolveLocalReference(context.dir, reference)))
  await Promise.all(localFiles.filter((local): local is string => local != null).map(async (local) => scanFile(context, local)))
}

async function readRealPath (filePath: string): Promise<string> {
  try {
    return await fs.realpath(filePath)
  } catch (err: unknown) {
    throw workflowError('READ', filePath, err)
  }
}

function collectWorkflowReferences (realFilePath: string, source: string): WorkflowReferences {
  const document = YAML.parseDocument(source)
  if (document.errors.length > 0) throw workflowError('PARSE', realFilePath, document.errors[0])
  const file = { path: realFilePath }
  const references: WorkflowReferences = { actions: [], localReferences: [] }
  for (const node of findUsesScalars(document.contents)) {
    const localReference = parseLocalReference(node.value)
    if (localReference != null) {
      references.localReferences.push(localReference)
      continue
    }
    const parsed = parseActionReference(node.value)
    if (parsed == null) continue
    references.actions.push(createActionReference({ file, node, parsed, source }))
  }
  return references
}

function createActionReference ({ file, node, parsed, source }: {
  file: ActionFile
  node: Scalar<string>
  parsed: Pick<ActionReference, 'name' | 'ref' | 'repo'>
  source: string
}): ActionReference {
  if (node.range == null) throw new Error(`Missing source range for GitHub Action in ${file.path}`)
  const end = trimLineBreak(source, node.range[2] ?? node.range[1])
  return {
    ...parsed,
    commentVersion: getCommentVersion(node),
    file,
    flowStyle: isFlowStyle(source, node.range[1]),
    indentation: getIndentation(source, node.range[0]),
    originalValue: source.slice(node.range[0], end),
    range: [node.range[0], end],
  }
}

function findUsesScalars (node: Node | null | undefined): Array<Scalar<string>> {
  if (!isMap(node)) return []
  const found: Array<Scalar<string>> = []
  const jobs = findMapValue(node, 'jobs')
  if (isMap(jobs)) {
    found.push(...findJobsUses(jobs))
  }
  const runs = findMapValue(node, 'runs')
  if (isMap(runs)) {
    found.push(...findStepUses(findMapValue(runs, 'steps')))
  }
  return found
}

function findJobsUses (jobs: YAMLMap): Array<Scalar<string>> {
  const found: Array<Scalar<string>> = []
  for (const job of jobs.items) {
    if (!isMap(job.value)) continue
    const jobUses = findStringScalar(job.value, 'uses')
    if (jobUses != null) found.push(jobUses)
    found.push(...findStepUses(findMapValue(job.value, 'steps')))
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

function getCommentVersion (node: Scalar<string>): string | undefined {
  const candidate = node.comment?.trimStart().split(/\s/, 1)[0]
  return candidate != null && semver.valid(candidate, { loose: true }) != null ? candidate : undefined
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
