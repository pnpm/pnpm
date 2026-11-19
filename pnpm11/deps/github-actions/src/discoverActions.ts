import fs from 'node:fs/promises'
import path from 'node:path'

import { PnpmError } from '@pnpm/error'
import { isSubdir } from 'is-subdir'
import YAML, { type Document, isAlias, isMap, isNode, isScalar, isSeq, type Node, type Scalar, type YAMLMap } from 'yaml'

import { getCommentVersionToken } from './commentVersion.js'
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
  for (const node of findUsesScalars(document, document.contents)) {
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

const MAX_MERGE_DEPTH = 8

interface MapLookupContext {
  document: Document
  key: string
  visitedDepth: Map<YAMLMap, number>
}

interface MapEntriesContext {
  document: Document
  seenKeys: Set<string>
  visitedDepth: Map<YAMLMap, number>
  values: Array<Node | null>
}

function findUsesScalars (document: Document, root: Node | null | undefined): Array<Scalar<string>> {
  const resolvedRoot = derefNode(document, root)
  if (!isMap(resolvedRoot)) return []
  const found: Array<Scalar<string>> = []
  const jobs = findMapValue(document, resolvedRoot, 'jobs')
  if (isMap(jobs)) {
    found.push(...findJobsUses(document, jobs))
  }
  const runs = findMapValue(document, resolvedRoot, 'runs')
  if (isMap(runs)) {
    found.push(...findStepUses(document, findMapValue(document, runs, 'steps')))
  }
  return [...new Set(found)]
}

function findJobsUses (document: Document, jobs: YAMLMap): Array<Scalar<string>> {
  const found: Array<Scalar<string>> = []
  for (const jobValue of collectMapValues(document, jobs)) {
    if (!isMap(jobValue)) continue
    const jobUses = findStringScalar(document, jobValue, 'uses')
    if (jobUses != null) found.push(jobUses)
    found.push(...findStepUses(document, findMapValue(document, jobValue, 'steps')))
  }
  return found
}

function collectMapValues (document: Document, rootMap: YAMLMap): Array<Node | null> {
  const context: MapEntriesContext = {
    document,
    seenKeys: new Set<string>(),
    visitedDepth: new Map<YAMLMap, number>(),
    values: [],
  }
  visitMapEntries(context, rootMap, 0)
  return context.values
}

function visitMapEntries (context: MapEntriesContext, node: Node | null, depth: number): void {
  if (!isMap(node)) return
  const previousDepth = context.visitedDepth.get(node)
  if (previousDepth !== undefined && previousDepth <= depth) return
  context.visitedDepth.set(node, depth)

  const mergeNode = collectDirectMapValues(context, node)
  if (depth < MAX_MERGE_DEPTH && mergeNode != null) {
    visitMergedMapEntries(context, mergeNode, depth + 1)
  }
}

function collectDirectMapValues (context: MapEntriesContext, node: YAMLMap): Node | null {
  let mergePairValue: unknown
  let hasMerge = false
  for (const pair of node.items) {
    const key = pairKeyName(context.document, pair.key)
    if (key === '<<') {
      if (!hasMerge) {
        mergePairValue = pair.value
        hasMerge = true
      }
    } else if (key != null && !context.seenKeys.has(key)) {
      context.seenKeys.add(key)
      context.values.push(derefNode(context.document, pair.value))
    }
  }
  return hasMerge ? derefNode(context.document, mergePairValue) : null
}

function visitMergedMapEntries (context: MapEntriesContext, merged: Node, depth: number): void {
  if (isMap(merged)) {
    visitMapEntries(context, merged, depth)
    return
  }
  if (!isSeq(merged)) return
  for (const entry of merged.items) {
    visitMapEntries(context, derefNode(context.document, entry), depth)
  }
}

function pairKeyName (document: Document, key: unknown): string | null {
  const resolved = derefNode(document, key)
  return isScalar(resolved) && typeof resolved.value === 'string' ? resolved.value : null
}

function findStepUses (document: Document, node: Node | null): Array<Scalar<string>> {
  const resolved = derefNode(document, node)
  if (!isSeq(resolved)) return []
  return resolved.items.flatMap((item) => {
    const step = derefNode(document, isNode(item) ? item : null)
    if (!isMap(step)) return []
    const uses = findStringScalar(document, step, 'uses')
    return uses == null ? [] : [uses]
  })
}

function derefNode (document: Document, node: unknown): Node | null {
  let current: unknown = node
  for (let hop = 0; hop < MAX_MERGE_DEPTH && isAlias(current); hop++) {
    current = current.resolve(document)
  }
  return isNode(current) && !isAlias(current) ? current : null
}

function findStringScalar (document: Document, node: Node, key: string): Scalar<string> | null {
  const value = findMapValue(document, node, key)
  return isScalar(value) && typeof value.value === 'string' ? value as Scalar<string> : null
}

function findMapValue (document: Document, node: Node, key: string): Node | null {
  return lookupMapValue({ document, key, visitedDepth: new Map<YAMLMap, number>() }, node, 0)
}

function lookupMapValue (context: MapLookupContext, node: Node, depth: number): Node | null {
  if (!isMap(node)) return null
  const previousDepth = context.visitedDepth.get(node)
  if (previousDepth !== undefined && previousDepth <= depth) return null
  context.visitedDepth.set(node, depth)
  const directPair = node.items.find((pair) => pairKeyName(context.document, pair.key) === context.key)
  if (directPair !== undefined) return derefNode(context.document, directPair.value)
  if (depth >= MAX_MERGE_DEPTH) return null
  return lookupMergedValue(context, node, depth + 1)
}

function lookupMergedValue (context: MapLookupContext, node: YAMLMap, depth: number): Node | null {
  const merged = derefNode(
    context.document,
    node.items.find((pair) => pairKeyName(context.document, pair.key) === '<<')?.value
  )
  if (isMap(merged)) return lookupMapValue(context, merged, depth)
  if (!isSeq(merged)) return null
  for (const entry of merged.items) {
    const entryMap = derefNode(context.document, entry)
    const found = entryMap != null ? lookupMapValue(context, entryMap, depth) : null
    if (found != null) return found
  }
  return null
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
  return getCommentVersionToken(node.comment)
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
