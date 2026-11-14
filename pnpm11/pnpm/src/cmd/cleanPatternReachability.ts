import path from 'node:path'

const IS_CASE_INSENSITIVE_FS = process.platform === 'win32' || process.platform === 'darwin'
const MAX_BRACE_EXPANSIONS = 64

export function escapeGlobPath (posixPath: string): string {
  return posixPath.replace(/[\\*?[\]{}()!+@]/gu, '\\$&')
}

export function pathsMatchOnHost (left: string, right: string): boolean {
  return IS_CASE_INSENSITIVE_FS
    ? left.toLowerCase() === right.toLowerCase()
    : left === right
}

export interface RootModulesRecoveryPatternOptions {
  customLeaves: string[]
  hoistedSubpaths: string[]
  patterns?: string[]
  relModules: string
}

export function patternsEnteringRootModulesDir (
  opts: RootModulesRecoveryPatternOptions
): string[] | undefined {
  const { customLeaves, hoistedSubpaths, patterns, relModules } = opts
  if (patterns == null) return undefined
  const targetSegments = relModules.split('/')
  const includes: string[] = []
  const negations: string[] = []
  for (const pattern of patterns) {
    if (pattern.startsWith('!')) {
      negations.push(pattern)
    } else {
      includes.push(...matchingRecoveryBranches(pattern, targetSegments))
    }
  }
  if (includes.length === 0) return undefined
  const memberModulesIgnores = customLeaves.map((leaf) => `!*/**/${escapeGlobPath(leaf)}/**`)
  const hoistedIgnores = hoistedSubpaths.map((subpath) => `!${escapeGlobPath(subpath)}/**`)
  return [...includes, ...negations, ...memberModulesIgnores, ...hoistedIgnores]
}

function matchingRecoveryBranches (pattern: string, targetSegments: string[]): string[] {
  const matched: string[] = []
  for (const branch of expandBraces(pattern)) {
    const normalized = path.posix.normalize(branch.replaceAll('\\', '/')).replace(/\/+$/u, '')
    if (branchReachesTarget(normalized, targetSegments)) {
      matched.push(normalized)
    }
  }
  return matched
}

interface PrefixMatchContext {
  memo: Array<Array<boolean | undefined>>
  pattern: string[]
  target: string[]
}

function branchReachesTarget (branch: string, target: string[]): boolean {
  if (branch === '' || branch === '.' || branch.startsWith('/')) return false
  const segments = branch.split('/')
  if (segments.length === 0 || segments.includes('..') || segments[0].includes('**')) {
    return false
  }
  const memo: Array<Array<boolean | undefined>> = Array.from(
    { length: segments.length + 1 },
    () => new Array<boolean | undefined>(target.length + 1)
  )
  return matchesPrefix({ memo, pattern: segments, target }, 0, 0)
}

function matchesPrefix (
  ctx: PrefixMatchContext,
  pi: number,
  mi: number
): boolean {
  const { memo, pattern, target } = ctx
  const cached = memo[pi][mi]
  if (cached !== undefined) return cached
  let matched: boolean
  if (mi === target.length) {
    matched = true
  } else if (pi === pattern.length) {
    matched = false
  } else if (pattern[pi] === '**') {
    matched = matchesPrefix(ctx, pi + 1, mi) ||
      matchesPrefix(ctx, pi, mi + 1)
  } else {
    matched = segmentGlobMatches(pattern[pi], target[mi]) &&
      matchesPrefix(ctx, pi + 1, mi + 1)
  }
  memo[pi][mi] = matched
  return matched
}

function segmentGlobMatches (patternSeg: string, targetSeg: string): boolean {
  if (targetSeg.startsWith('.') && !patternSeg.startsWith('.')) return false
  if (isLiteralGlobSegment(patternSeg)) {
    return pathsMatchOnHost(patternSeg, targetSeg)
  }
  const pat = IS_CASE_INSENSITIVE_FS ? patternSeg.toLowerCase() : patternSeg
  const seg = IS_CASE_INSENSITIVE_FS ? targetSeg.toLowerCase() : targetSeg
  return path.posix.matchesGlob(seg, pat)
}

export function literalPatternSubpaths (patterns: string[] | undefined): string[] {
  if (patterns == null) return []
  const negations: string[] = []
  const literals: string[] = []
  for (const pattern of patterns) {
    if (pattern.startsWith('!')) {
      const trimmed = pattern.slice(1).replaceAll('\\', '/').replace(/\/+$/u, '')
      if (trimmed !== '') negations.push(trimmed)
    } else {
      literals.push(...expandLiteralBranches(pattern))
    }
  }
  return literals.filter((subpath) => !isNegatedLiteral(subpath, negations))
}

function expandLiteralBranches (raw: string): string[] {
  const out: string[] = []
  for (const branch of expandBraces(raw)) {
    const normalized = normalizeLiteralBranch(branch)
    if (normalized != null) out.push(normalized)
  }
  return out
}

function normalizeLiteralBranch (branch: string): string | undefined {
  const normalized = path.posix.normalize(branch.replaceAll('\\', '/')).replace(/\/+$/u, '')
  if (normalized === '' || normalized === '.' || normalized.startsWith('/')) {
    return undefined
  }
  return normalized.split('/').every(isLiteralGlobSegment) ? normalized : undefined
}

function isNegatedLiteral (subpath: string, negations: string[]): boolean {
  const candidate = IS_CASE_INSENSITIVE_FS ? subpath.toLowerCase() : subpath
  return negations.some((neg) => {
    const pat = IS_CASE_INSENSITIVE_FS ? neg.toLowerCase() : neg
    return path.posix.matchesGlob(candidate, pat)
  })
}

function isLiteralGlobSegment (segment: string): boolean {
  return segment !== '' &&
    segment !== '.' &&
    segment !== '..' &&
    !/[\\*?[\]{}()|@+!]/u.test(segment)
}

function expandBraces (pattern: string): string[] {
  const queue = [pattern]
  const expanded: string[] = []
  while (queue.length > 0) {
    const current = queue.pop()!
    const group = findFirstBraceGroup(current)
    if (group == null || queue.length + expanded.length + group.alts.length > MAX_BRACE_EXPANSIONS) {
      expanded.push(current)
      continue
    }
    const prefix = current.slice(0, group.open)
    const suffix = current.slice(group.close + 1)
    for (let idx = group.alts.length - 1; idx >= 0; idx--) {
      queue.push(`${prefix}${group.alts[idx]}${suffix}`)
    }
  }
  return expanded
}

interface BraceGroup {
  alts: string[]
  close: number
  open: number
}

interface BraceScanState {
  commas: number[]
  depth: number
  escaped: boolean
  inBrackets: boolean
  openIdx: number
}

function findFirstBraceGroup (input: string): BraceGroup | undefined {
  const state: BraceScanState = { commas: [], depth: 0, escaped: false, inBrackets: false, openIdx: -1 }
  for (let idx = 0; idx < input.length; idx++) {
    const found = stepBraceScan(state, input, idx)
    if (found != null) return found
  }
  return undefined
}

function stepBraceScan (state: BraceScanState, input: string, idx: number): BraceGroup | undefined {
  if (state.escaped) {
    state.escaped = false
    return undefined
  }
  const ch = input[idx]
  if (ch === '\\') {
    state.escaped = true
    return undefined
  }
  if (stepBracketState(state, ch)) return undefined
  return state.inBrackets ? undefined : stepBraceDelimiter(state, input, idx)
}

function stepBracketState (state: BraceScanState, ch: string): boolean {
  if (ch === '[' && !state.inBrackets) {
    state.inBrackets = true
    return true
  }
  if (ch === ']' && state.inBrackets) {
    state.inBrackets = false
    return true
  }
  return false
}

function stepBraceDelimiter (
  state: BraceScanState,
  input: string,
  idx: number
): BraceGroup | undefined {
  const ch = input[idx]
  if (ch === '{') {
    openBraceGroup(state, idx)
    return undefined
  }
  if (ch === ',' && state.depth === 1) {
    state.commas.push(idx)
    return undefined
  }
  return ch === '}' && state.depth > 0 ? closeBraceGroup(state, input, idx) : undefined
}

function openBraceGroup (state: BraceScanState, idx: number): void {
  if (state.depth === 0) {
    state.openIdx = idx
    state.commas = []
  }
  state.depth++
}

function closeBraceGroup (state: BraceScanState, input: string, closeIdx: number): BraceGroup | undefined {
  state.depth--
  if (state.depth > 0 || state.commas.length === 0) return undefined
  return {
    alts: sliceBraceAlts(input, state, closeIdx),
    close: closeIdx,
    open: state.openIdx,
  }
}

function sliceBraceAlts (input: string, state: BraceScanState, close: number): string[] {
  const alts: string[] = []
  let start = state.openIdx + 1
  for (const comma of state.commas) {
    alts.push(input.slice(start, comma))
    start = comma + 1
  }
  alts.push(input.slice(start, close))
  return alts
}
