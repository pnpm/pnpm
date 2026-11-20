import { type Document, isAlias, isMap, isNode, isScalar, isSeq, type Node, type Scalar, type YAMLMap, type YAMLSeq } from 'yaml'

const MAX_MERGE_DEPTH = 64
const MAX_MERGE_STEPS = 4096
const YAML_MERGE_TAG = 'tag:yaml.org,2002:merge'

type LookupOutcome =
  | { kind: 'found', value: Node | null }
  | { kind: 'missing', depMask: bigint }
  | { kind: 'cutoff' }

type CollectOutcome =
  | { kind: 'complete', depMask: bigint }
  | { kind: 'cutoff' }

interface CycleTracker {
  confirmed: Set<YAMLMap>
  activeIndex: Map<YAMLMap, number>
  cycleMasks: Map<YAMLMap, bigint>
  evictStack: YAMLMap[][]
  steps: number
}

interface MapLookupContext {
  document: Document
  key: string
  tracker: CycleTracker
}

interface MapEntriesContext {
  document: Document
  seenKeys: Set<string>
  tracker: CycleTracker
  values: Array<Node | null>
}

interface DirectMapScan {
  hasMerge: boolean
  mergeNode: Node | null
}

function createCycleTracker (): CycleTracker {
  return {
    confirmed: new Set<YAMLMap>(),
    activeIndex: new Map<YAMLMap, number>(),
    cycleMasks: new Map<YAMLMap, bigint>(),
    evictStack: [],
    steps: 0,
  }
}

function stepBudgetExceeded (tracker: CycleTracker): boolean {
  tracker.steps += 1
  return tracker.steps > MAX_MERGE_STEPS
}

function checkCachedMask (tracker: CycleTracker, node: YAMLMap): bigint | undefined {
  if (tracker.confirmed.has(node)) return 0n
  const ancestorPos = tracker.activeIndex.get(node)
  if (ancestorPos !== undefined) return 1n << BigInt(ancestorPos)
  return tracker.cycleMasks.get(node)
}

function enterCycleFrame (tracker: CycleTracker, node: YAMLMap): number {
  const myPos = tracker.activeIndex.size
  tracker.activeIndex.set(node, myPos)
  tracker.evictStack.push([])
  return myPos
}

function evictCycleFrame (tracker: CycleTracker, node: YAMLMap): void {
  tracker.activeIndex.delete(node)
  const evicted = tracker.evictStack.pop()
  if (evicted === undefined) return
  for (const cached of evicted) {
    tracker.cycleMasks.delete(cached)
  }
}

function exitCycleComplete (tracker: CycleTracker, node: YAMLMap, myPos: number, depMask: bigint): bigint {
  evictCycleFrame(tracker, node)
  const externalMask = depMask & ((1n << BigInt(myPos)) - 1n)
  if (externalMask === 0n) {
    tracker.confirmed.add(node)
    return 0n
  }
  const deepestDep = externalMask.toString(2).length - 1
  tracker.cycleMasks.set(node, externalMask)
  tracker.evictStack[deepestDep].push(node)
  return externalMask
}

export function findUsesScalars (
  document: Document,
  root: Node | null | undefined,
  source: string
): Array<Scalar<string>> {
  const resolvedRoot = derefNode(document, root)
  if (!isMap(resolvedRoot)) return []
  const found = new Set<Scalar<string>>()
  const seenStepSequences = new Set<YAMLSeq>()
  const jobs = findMapValue(document, resolvedRoot, 'jobs')
  if (isMap(jobs)) {
    addJobsUses(document, source, jobs, found, seenStepSequences)
  }
  const runs = findMapValue(document, resolvedRoot, 'runs')
  if (isMap(runs)) {
    addStepUses(document, source, findMapValue(document, runs, 'steps'), found, seenStepSequences)
  }
  return [...found]
}

function addJobsUses (
  document: Document,
  source: string,
  jobs: YAMLMap,
  found: Set<Scalar<string>>,
  seenStepSequences: Set<YAMLSeq>
): void {
  for (const jobValue of collectMapValues(document, jobs)) {
    if (!isMap(jobValue)) continue
    const jobUses = findStringScalar(document, source, jobValue, 'uses')
    if (jobUses != null) found.add(jobUses)
    addStepUses(document, source, findMapValue(document, jobValue, 'steps'), found, seenStepSequences)
  }
}

function collectMapValues (document: Document, rootMap: YAMLMap): Array<Node | null> {
  const context: MapEntriesContext = {
    document,
    seenKeys: new Set<string>(),
    tracker: createCycleTracker(),
    values: [],
  }
  visitMapEntries(context, rootMap, 0)
  return context.values
}

function visitMapEntries (context: MapEntriesContext, node: Node | null, depth: number): CollectOutcome {
  if (stepBudgetExceeded(context.tracker)) return { kind: 'cutoff' }
  if (!isMap(node)) return { kind: 'complete', depMask: 0n }
  const cached = checkCachedMask(context.tracker, node)
  if (cached !== undefined) return { kind: 'complete', depMask: cached }
  const scan = collectDirectMapValues(context, node)
  if (!scan.hasMerge) {
    context.tracker.confirmed.add(node)
    return { kind: 'complete', depMask: 0n }
  }
  if (depth >= MAX_MERGE_DEPTH) return { kind: 'cutoff' }
  return descendEntriesMerge(context, node, scan.mergeNode, depth + 1)
}

function descendEntriesMerge (
  context: MapEntriesContext,
  owner: YAMLMap,
  mergeNode: Node | null,
  depth: number
): CollectOutcome {
  if (mergeNode == null) {
    context.tracker.confirmed.add(owner)
    return { kind: 'complete', depMask: 0n }
  }
  const myPos = enterCycleFrame(context.tracker, owner)
  const outcome = visitMergedMapEntries(context, mergeNode, depth)
  if (outcome.kind === 'cutoff') {
    evictCycleFrame(context.tracker, owner)
    return outcome
  }
  return { kind: 'complete', depMask: exitCycleComplete(context.tracker, owner, myPos, outcome.depMask) }
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
  if (!isSeq(merged)) return { kind: 'complete', depMask: 0n }
  let combinedMask = 0n
  for (const entry of merged.items) {
    const outcome = visitMapEntries(context, derefNode(context.document, entry), depth)
    if (outcome.kind === 'cutoff') return outcome
    combinedMask |= outcome.depMask
  }
  return { kind: 'complete', depMask: combinedMask }
}

function isMergeKey (document: Document, key: unknown): boolean {
  const resolved = derefNode(document, key)
  if (!isScalar(resolved)) return false
  if (resolved.tag === YAML_MERGE_TAG) return true
  if (resolved.tag !== undefined || resolved.type !== 'PLAIN') return false
  return resolved.value === '<<' || (typeof resolved.value === 'symbol' && resolved.value.description === '<<')
}

function stringKeyName (document: Document, key: unknown): string | null {
  if (isMergeKey(document, key)) return null
  const resolved = derefNode(document, key)
  return isScalar(resolved) && typeof resolved.value === 'string' ? resolved.value : null
}

function addStepUses (
  document: Document,
  source: string,
  node: Node | null,
  found: Set<Scalar<string>>,
  seenStepSequences: Set<YAMLSeq>
): void {
  const resolved = derefNode(document, node)
  if (!isSeq(resolved) || seenStepSequences.has(resolved)) return
  seenStepSequences.add(resolved)
  for (const item of resolved.items) {
    const step = derefNode(document, isNode(item) ? item : null)
    if (!isMap(step)) continue
    const uses = findStringScalar(document, source, step, 'uses')
    if (uses != null) found.add(uses)
  }
}

function derefNode (document: Document, node: unknown): Node | null {
  let current: unknown = node
  for (let hop = 0; hop < MAX_MERGE_DEPTH && isAlias(current); hop++) {
    current = current.resolve(document)
  }
  return isNode(current) && !isAlias(current) ? current : null
}

function findStringScalar (document: Document, source: string, node: Node, key: string): Scalar<string> | null {
  const value = findMapValue(document, node, key)
  return isInlineStringScalar(value, source) ? value : null
}

function isInlineStringScalar (node: unknown, source: string): node is Scalar<string> {
  if (!isScalar(node) || typeof node.value !== 'string') return false
  if (node.type !== 'PLAIN' && node.type !== 'QUOTE_SINGLE' && node.type !== 'QUOTE_DOUBLE') {
    return false
  }
  if (node.range == null || node.range[0] >= node.range[1]) return false
  const raw = source.slice(node.range[0], node.range[1])
  return !raw.includes('\n') && !raw.includes('\r')
}

function findMapValue (document: Document, node: Node, key: string): Node | null {
  const outcome = lookupMapValue({ document, key, tracker: createCycleTracker() }, node, 0)
  return outcome.kind === 'found' ? outcome.value : null
}

function lookupMapValue (context: MapLookupContext, node: Node, depth: number): LookupOutcome {
  if (stepBudgetExceeded(context.tracker)) return { kind: 'cutoff' }
  if (!isMap(node)) return { kind: 'missing', depMask: 0n }
  const cached = checkCachedMask(context.tracker, node)
  if (cached !== undefined) return { kind: 'missing', depMask: cached }
  const directPair = node.items.find((pair) => stringKeyName(context.document, pair.key) === context.key)
  if (directPair !== undefined) {
    return { kind: 'found', value: derefNode(context.document, directPair.value) }
  }
  const mergePair = node.items.find((pair) => isMergeKey(context.document, pair.key))
  if (mergePair === undefined) {
    context.tracker.confirmed.add(node)
    return { kind: 'missing', depMask: 0n }
  }
  if (depth >= MAX_MERGE_DEPTH) return { kind: 'cutoff' }
  return descendLookupMerge(context, node, mergePair.value, depth + 1)
}

function descendLookupMerge (
  context: MapLookupContext,
  owner: YAMLMap,
  mergeValue: unknown,
  depth: number
): LookupOutcome {
  const myPos = enterCycleFrame(context.tracker, owner)
  const outcome = lookupMergedTarget(context, derefNode(context.document, mergeValue), depth)
  if (outcome.kind !== 'missing') {
    evictCycleFrame(context.tracker, owner)
    return outcome
  }
  return { kind: 'missing', depMask: exitCycleComplete(context.tracker, owner, myPos, outcome.depMask) }
}

function lookupMergedTarget (context: MapLookupContext, merged: Node | null, depth: number): LookupOutcome {
  if (isMap(merged)) return lookupMapValue(context, merged, depth)
  if (!isSeq(merged)) return { kind: 'missing', depMask: 0n }
  let combinedMask = 0n
  for (const entry of merged.items) {
    const entryMap = derefNode(context.document, entry)
    if (entryMap == null) continue
    const outcome = lookupMapValue(context, entryMap, depth)
    if (outcome.kind !== 'missing') return outcome
    combinedMask |= outcome.depMask
  }
  return { kind: 'missing', depMask: combinedMask }
}
