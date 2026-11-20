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

const MAX_MERGE_DEPTH = 64
const YAML_MERGE_TAG = 'tag:yaml.org,2002:merge'

type LookupOutcome =
  | { kind: 'found', value: Node | null }
  | { kind: 'missing', lowlink: number | null }
  | { kind: 'cutoff' }

type CollectOutcome =
  | { kind: 'complete', lowlink: number | null }
  | { kind: 'cutoff' }

interface MapLookupContext {
  document: Document
  key: string
  confirmedMissing: Set<YAMLMap>
  activeIndex: Map<YAMLMap, number>
}

interface MapEntriesContext {
  document: Document
  seenKeys: Set<string>
  completed: Set<YAMLMap>
  activeIndex: Map<YAMLMap, number>
  values: Array<Node | null>
}

interface DirectMapScan {
  hasMerge: boolean
  mergeNode: Node | null
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
    completed: new Set<YAMLMap>(),
    activeIndex: new Map<YAMLMap, number>(),
    values: [],
  }
  visitMapEntries(context, rootMap, 0)
  return context.values
}

function visitMapEntries (context: MapEntriesContext, node: Node | null, depth: number): CollectOutcome {
  if (!isMap(node) || context.completed.has(node)) return { kind: 'complete', lowlink: null }
  const ancestorPos = context.activeIndex.get(node)
  if (ancestorPos !== undefined) return { kind: 'complete', lowlink: ancestorPos }
  const scan = collectDirectMapValues(context, node)
  if (!scan.hasMerge) {
    context.completed.add(node)
    return { kind: 'complete', lowlink: null }
  }
  if (depth >= MAX_MERGE_DEPTH) return { kind: 'cutoff' }
  return descendMapEntriesMerge(context, node, scan.mergeNode, depth + 1)
}

function descendMapEntriesMerge (
  context: MapEntriesContext,
  owner: YAMLMap,
  mergeNode: Node | null,
  depth: number
): CollectOutcome {
  if (mergeNode == null) {
    context.completed.add(owner)
    return { kind: 'complete', lowlink: null }
  }
  const myPos = context.activeIndex.size
  context.activeIndex.set(owner, myPos)
  const outcome = visitMergedMapEntries(context, mergeNode, depth)
  context.activeIndex.delete(owner)
  if (outcome.kind === 'complete' && (outcome.lowlink === null || outcome.lowlink >= myPos)) {
    context.completed.add(owner)
    return { kind: 'complete', lowlink: null }
  }
  return outcome
}

function collectDirectMapValues (context: MapEntriesContext, node: YAMLMap): DirectMapScan {
  let mergePairValue: unknown
  let hasMerge = false
  for (const pair of node.items) {
    if (isMergeKey(context.document, pair.key)) {
      if (!hasMerge) {
        mergePairValue = pair.value
        hasMerge = true
      }
      continue
    }
    recordDirectPair(context, pair.key, pair.value)
  }
  return {
    hasMerge,
    mergeNode: hasMerge ? derefNode(context.document, mergePairValue) : null,
  }
}

function recordDirectPair (context: MapEntriesContext, rawKey: unknown, rawValue: unknown): void {
  const key = stringKeyName(context.document, rawKey)
  if (key == null || context.seenKeys.has(key)) return
  context.seenKeys.add(key)
  context.values.push(derefNode(context.document, rawValue))
}

function visitMergedMapEntries (context: MapEntriesContext, merged: Node, depth: number): CollectOutcome {
  if (isMap(merged)) return visitMapEntries(context, merged, depth)
  if (!isSeq(merged)) return { kind: 'complete', lowlink: null }
  let minLowlink: number | null = null
  for (const entry of merged.items) {
    const outcome = visitMapEntries(context, derefNode(context.document, entry), depth)
    if (outcome.kind === 'cutoff') return outcome
    minLowlink = mergeLowlink(minLowlink, outcome.lowlink)
  }
  return { kind: 'complete', lowlink: minLowlink }
}

function isMergeKey (document: Document, key: unknown): boolean {
  const resolved = derefNode(document, key)
  if (!isScalar(resolved)) return false
  if (resolved.tag === YAML_MERGE_TAG) return true
  return resolved.tag === undefined && resolved.type === 'PLAIN' && resolved.value === '<<'
}

function stringKeyName (document: Document, key: unknown): string | null {
  if (isMergeKey(document, key)) return null
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
  const outcome = lookupMapValue(
    {
      document,
      key,
      confirmedMissing: new Set<YAMLMap>(),
      activeIndex: new Map<YAMLMap, number>(),
    },
    node,
    0
  )
  return outcome.kind === 'found' ? outcome.value : null
}

function lookupMapValue (context: MapLookupContext, node: Node, depth: number): LookupOutcome {
  if (!isMap(node) || context.confirmedMissing.has(node)) {
    return { kind: 'missing', lowlink: null }
  }
  const ancestorPos = context.activeIndex.get(node)
  if (ancestorPos !== undefined) return { kind: 'missing', lowlink: ancestorPos }
  const directPair = node.items.find((pair) => stringKeyName(context.document, pair.key) === context.key)
  if (directPair !== undefined) {
    return { kind: 'found', value: derefNode(context.document, directPair.value) }
  }
  const mergePair = node.items.find((pair) => isMergeKey(context.document, pair.key))
  if (mergePair === undefined) {
    context.confirmedMissing.add(node)
    return { kind: 'missing', lowlink: null }
  }
  if (depth >= MAX_MERGE_DEPTH) return { kind: 'cutoff' }
  return lookupMergedPair(context, node, mergePair.value, depth + 1)
}

function lookupMergedPair (
  context: MapLookupContext,
  owner: YAMLMap,
  mergeValue: unknown,
  depth: number
): LookupOutcome {
  const myPos = context.activeIndex.size
  context.activeIndex.set(owner, myPos)
  const outcome = lookupMergedTarget(context, derefNode(context.document, mergeValue), depth)
  context.activeIndex.delete(owner)
  if (outcome.kind === 'missing' && (outcome.lowlink === null || outcome.lowlink >= myPos)) {
    context.confirmedMissing.add(owner)
    return { kind: 'missing', lowlink: null }
  }
  return outcome
}

function lookupMergedTarget (context: MapLookupContext, merged: Node | null, depth: number): LookupOutcome {
  if (isMap(merged)) return lookupMapValue(context, merged, depth)
  if (!isSeq(merged)) return { kind: 'missing', lowlink: null }
  let minLowlink: number | null = null
  for (const entry of merged.items) {
    const entryMap = derefNode(context.document, entry)
    if (entryMap == null) continue
    const outcome = lookupMapValue(context, entryMap, depth)
    if (outcome.kind !== 'missing') return outcome
    minLowlink = mergeLowlink(minLowlink, outcome.lowlink)
  }
  return { kind: 'missing', lowlink: minLowlink }
}

function mergeLowlink (current: number | null, next: number | null): number | null {
  if (current === null) return next
  if (next === null) return current
  return Math.min(current, next)
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
