import {
  buildBaseExploratoryTypes,
  collectCandidateCommands,
  collectCandidateShorthands,
  configDotTypes,
  extendExploratoryTypes,
  parseCliArgs as parseCliArgsLib,
  type ParseCliArgsOptions,
  type ParsedCliArgs,
} from '@pnpm/cli.parse-cli-args'
import { PnpmError } from '@pnpm/error'
import nopt from '@pnpm/nopt'

import {
  getCliOptionsTypes,
  getCommandFullName,
  GLOBAL_OPTIONS,
  shorthandsByCommandName,
} from './cmd/index.js'
import { shorthands as universalShorthands } from './shorthands.js'

const RENAMED_OPTIONS = {
  prefix: 'dir',
  store: 'store-dir',
}

type FlagTypeMap = Parameters<typeof nopt>[0]

const MARKER_PREFIX_RE = /^__pnpm_prefix_(\d+)_/
const OPTION_SEPARATOR_RE = /^-{2,}$/
const RECURSIVE_PREFIXES = new Set(['recursive', 'multi', 'm'])
const PREFIX_WORDS = new Set(['pm', 'with', 'current', ...RECURSIVE_PREFIXES])
const EXPLORATORY_SHORTHANDS = { r: '--recursive', ...universalShorthands }

interface BaseScanTypes {
  rawBase: FlagTypeMap
  baseWithConfigDot: FlagTypeMap
}

let libOpts: ParseCliArgsOptions | undefined
let cachedBaseScanTypes: BaseScanTypes | undefined

export type ParsedCliArgsWithBuiltIn = ParsedCliArgs & { builtInCommandForced: boolean }

export async function parseCliArgs (inputArgv: string[]): Promise<ParsedCliArgsWithBuiltIn> {
  const { argv, builtInCommandForced, withCurrent } = stripPreCommandPrefixes(inputArgv)
  if (withCurrent) {
    process.env.pnpm_config_pm_on_fail = 'ignore'
  }
  const result = await parseCliArgsLib(getLibOpts(), argv)
  return { ...result, builtInCommandForced }
}

interface ScannedPrefixes {
  pmIndex?: number
  withCurrentIndices?: [number, number]
  recursiveSeen?: boolean
}

interface StrippedPrefixes {
  argv: string[]
  builtInCommandForced: boolean
  withCurrent: boolean
}

function stripPreCommandPrefixes (inputArgv: string[]): StrippedPrefixes {
  if (!inputArgv.includes('pm') && !inputArgv.includes('with')) {
    return { argv: [...inputArgv], builtInCommandForced: false, withCurrent: false }
  }
  const { pmIndex, withCurrentIndices } = scanPreCommandPrefixes(inputArgv)
  const removed = new Set<number>([
    ...(pmIndex === undefined ? [] : [pmIndex]),
    ...(withCurrentIndices ?? []),
  ])
  if (withCurrentIndices != null) {
    const hasTrailingArg = inputArgv.some((_, idx) => idx > withCurrentIndices[1] && !removed.has(idx))
    if (!hasTrailingArg) {
      throw new PnpmError('MISSING_WITH_CURRENT_CMD',
        'Missing command after "current". Usage: pnpm with current <command> [args...]')
    }
  }
  return {
    argv: inputArgv.filter((_, idx) => !removed.has(idx)),
    builtInCommandForced: pmIndex !== undefined,
    withCurrent: withCurrentIndices !== undefined,
  }
}

function getLibOpts (): ParseCliArgsOptions {
  libOpts ??= {
    fallbackCommand: 'run',
    escapeArgs: ['create', 'exec', 'test'],
    getCommandLongName: getCommandFullName,
    getTypesByCommandName: getCliOptionsTypes,
    renamedOptions: RENAMED_OPTIONS,
    shorthandsByCommandName,
    universalOptionsTypes: GLOBAL_OPTIONS,
    universalShorthands,
  }
  return libOpts
}

function scanPreCommandPrefixes (argv: string[]): ScannedPrefixes {
  const positionals = extractRemainingPositionals(argv)
  return consumeLeadingPrefixes(argv, positionals)
}

function extractRemainingPositionals (argv: string[]): Array<number | undefined> {
  const sepIndex = argv.findIndex((token) => OPTION_SEPARATOR_RE.test(token))
  const head = sepIndex >= 0 ? argv.slice(0, sepIndex) : argv
  if (!head.some((token) => token.startsWith('-') && token.length > 1)) {
    return head.map((token, idx) => (PREFIX_WORDS.has(token) ? idx : undefined))
  }
  const tagged = tagPrefixCandidates(head)
  const baseRemain = nopt(
    getBaseScanTypes().baseWithConfigDot,
    EXPLORATORY_SHORTHANDS,
    tagged.taggedArgs,
    0
  ).argv.remain
  const finalRemain = retryScanIfPrefixBlocked(baseRemain, tagged)
  return finalRemain.map((token) => tagged.tagToIndex.get(token))
}

interface PrefixCursor {
  argv: string[]
  positionals: Array<number | undefined>
  scanned: ScannedPrefixes
}

function consumeLeadingPrefixes (
  argv: string[],
  positionals: Array<number | undefined>
): ScannedPrefixes {
  const state: PrefixCursor = { argv, positionals, scanned: {} }
  let cursor = 0
  while (cursor < positionals.length) {
    const nextCursor = advancePrefixCursor(state, cursor)
    if (nextCursor < 0) break
    cursor = nextCursor
  }
  return state.scanned
}

function advancePrefixCursor (state: PrefixCursor, cursor: number): number {
  const { argv, positionals, scanned } = state
  const index = positionals[cursor]
  if (index === undefined) return -1
  const word = argv[index]
  if (scanned.pmIndex === undefined && word === 'pm') {
    scanned.pmIndex = index
    return cursor + 1
  }
  if (scanned.withCurrentIndices === undefined && word === 'with') {
    const nextIndex = positionals[cursor + 1]
    if (nextIndex !== undefined && argv[nextIndex] === 'current') {
      scanned.withCurrentIndices = [index, nextIndex]
      return cursor + 2
    }
    return -1
  }
  if (!scanned.recursiveSeen && RECURSIVE_PREFIXES.has(word) && cursor + 1 < positionals.length) {
    scanned.recursiveSeen = true
    return cursor + 1
  }
  return -1
}

interface TaggedCandidates {
  taggedArgs: string[]
  tagToIndex: Map<string, number>
}

function tagPrefixCandidates (head: string[]): TaggedCandidates {
  const prefix = chooseMarkerPrefix(head)
  const tagToIndex = new Map<string, number>()
  const taggedArgs = head.map((token, idx) => {
    if (!PREFIX_WORDS.has(token)) return token
    const tag = `${prefix}${idx}__`
    tagToIndex.set(tag, idx)
    return tag
  })
  return { taggedArgs, tagToIndex }
}

function chooseMarkerPrefix (head: string[]): string {
  const occupied = new Set<string>()
  for (const token of head) {
    const match = MARKER_PREFIX_RE.exec(token)
    if (match != null) occupied.add(match[1])
  }
  let salt = 0
  while (occupied.has(String(salt))) {
    salt += 1
  }
  return `__pnpm_prefix_${salt}_`
}

function getBaseScanTypes (): BaseScanTypes {
  if (cachedBaseScanTypes == null) {
    const rawBase = buildBaseExploratoryTypes(getLibOpts())
    cachedBaseScanTypes = {
      rawBase,
      baseWithConfigDot: { ...rawBase, ...configDotTypes(rawBase) },
    }
  }
  return cachedBaseScanTypes
}

function retryScanIfPrefixBlocked (baseRemain: string[], tagged: TaggedCandidates): string[] {
  const candidates = findBlockedPrefixCommands(baseRemain, tagged.tagToIndex)
  if (candidates == null || candidates.size === 0) return baseRemain
  const extended = extendExploratoryTypes(getBaseScanTypes().rawBase, getLibOpts(), candidates)
  const shorthands = {
    ...EXPLORATORY_SHORTHANDS,
    ...collectCandidateShorthands(shorthandsByCommandName, candidates),
  }
  const retryRemain = nopt(
    { ...extended, ...configDotTypes(extended) },
    shorthands,
    tagged.taggedArgs,
    0
  ).argv.remain
  return retryRemain.some((token) => !tagged.tagToIndex.has(token)) ? retryRemain : baseRemain
}

function findBlockedPrefixCommands (
  remain: string[],
  tagToIndex: Map<string, number>
): Set<string> | undefined {
  const firstNonTagIdx = remain.findIndex((token) => !tagToIndex.has(token))
  if (firstNonTagIdx < 0 || getCommandFullName(remain[firstNonTagIdx]) != null) {
    return undefined
  }
  const tail = remain.slice(firstNonTagIdx + 1)
  const firstBlockedTagIdx = tail.findIndex((token) => tagToIndex.has(token))
  if (firstBlockedTagIdx < 0) {
    return undefined
  }
  return collectCandidateCommands(
    getLibOpts(),
    tail.slice(firstBlockedTagIdx + 1).filter((token) => !tagToIndex.has(token))
  )
}
