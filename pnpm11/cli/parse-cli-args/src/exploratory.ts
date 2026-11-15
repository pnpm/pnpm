import nopt from '@pnpm/nopt'

import type { ParseCliArgsOptions } from './index.js'

const CUSTOM_OPTION_PREFIX = 'config.'
const OPTION_SEPARATOR_RE = /^-{2,}$/
const RECURSIVE_CMDS = new Set(['recursive', 'multi', 'm'])
const SPECIALLY_ESCAPED_CMDS = new Set(['run', 'dlx', 'with'])

const VERSION_BOOTSTRAP_TYPES = {
  'frozen-lockfile': Boolean,
  global: Boolean,
  'global-dir': String,
  lockfile: Boolean,
  'lockfile-dir': String,
  'npmrc-auth-file': String,
  offline: Boolean,
  'prefer-offline': Boolean,
  registry: String,
  'state-dir': String,
  'store-dir': String,
}

export const VERSION_BOOTSTRAP_KEYS = Object.keys(VERSION_BOOTSTRAP_TYPES)

export const BASE_EXPLORATORY_TYPES = {
  filter: [String],
  help: Boolean,
  recursive: Boolean,
  ...VERSION_BOOTSTRAP_TYPES,
}

export type ExploratoryOptions = Pick<
  ParseCliArgsOptions,
  'fallbackCommand' | 'getCommandLongName' | 'getTypesByCommandName' | 'universalOptionsTypes'
>

export interface PreservedCommandToken {
  token: string
  inputIndex: number
}

export interface ExploratoryResult {
  parsed: ReturnType<typeof nopt>
  preservedTokens: PreservedCommandToken[]
}

export interface RestoreCommandTokensTarget {
  filteredArgv: string[]
  mapInputIndex: (inputIndex: number) => number
  preservedTokens: PreservedCommandToken[]
}

interface ExploratoryContext {
  opts: ParseCliArgsOptions
  baseTypes: Parameters<typeof nopt>[0]
  inputArgv: string[]
}

interface ExploratorySpec {
  types: Parameters<typeof nopt>[0]
  shorthands?: Record<string, string | string[]>
}

interface ExploratoryWin extends ExploratorySpec {
  parsed: ReturnType<typeof nopt>
}

interface ResolvedCommand {
  commandName: string
  cmd: string
}

interface RemainRankTarget {
  rank: number
  minIndex: number
}

interface PrefixSearchBounds {
  minIndex: number
  totalLength: number
  targetCount: number
}

export function parseExploratory (
  opts: ParseCliArgsOptions,
  inputArgv: string[]
): ExploratoryResult {
  const ctx: ExploratoryContext = {
    opts,
    baseTypes: buildBaseExploratoryTypes(opts),
    inputArgv,
  }
  const win = selectExploratoryParse(ctx)
  return {
    parsed: win.parsed,
    preservedTokens: collectPreservedCommandTokens(ctx, win),
  }
}

export function restoreConsumedCommandTokens (
  remain: string[],
  countRemainInPrefix: (end: number) => number,
  target: RestoreCommandTokensTarget
): void {
  let restoredCount = 0
  for (const item of target.preservedTokens) {
    const filteredIndex = target.mapInputIndex(item.inputIndex)
    if (!canBeConsumedByPrecedingOption(target.filteredArgv, filteredIndex)) continue
    const beforeCount = countRemainInPrefix(filteredIndex)
    if (countRemainInPrefix(filteredIndex + 1) === beforeCount) {
      remain.splice(beforeCount + restoredCount, 0, item.token)
      restoredCount += 1
    }
  }
}

export function buildBaseExploratoryTypes (
  opts: Pick<ExploratoryOptions, 'getTypesByCommandName' | 'universalOptionsTypes'>
): Parameters<typeof nopt>[0] {
  return {
    ...BASE_EXPLORATORY_TYPES,
    ...opts.universalOptionsTypes,
    ...opts.getTypesByCommandName('add'),
    ...opts.getTypesByCommandName('install'),
  }
}

export function extendExploratoryTypes (
  baseTypes: Parameters<typeof nopt>[0],
  opts: Pick<ExploratoryOptions, 'getTypesByCommandName'>,
  commandNames: Iterable<string>
): Parameters<typeof nopt>[0] {
  const merged: Record<string, unknown> = { ...baseTypes }
  for (const commandName of commandNames) {
    mergeCommandOptionTypes(merged, opts.getTypesByCommandName(commandName))
  }
  return merged as Parameters<typeof nopt>[0]
}

export function collectCandidateCommands (
  opts: Pick<ExploratoryOptions, 'fallbackCommand' | 'getCommandLongName'>,
  tokens: Iterable<string>
): Set<string> {
  const names = new Set<string>()
  for (const token of tokens) {
    const canonical = opts.getCommandLongName(token) ?? opts.fallbackCommand
    if (canonical && canonical !== 'add' && canonical !== 'install') {
      names.add(canonical)
    }
  }
  return names
}

/**
 * The types of `types` under their `--config.<key>` spellings, so a
 * `--config.<key>` value takes the type the command declares for `<key>`: a
 * list setting given once is a one-item list and a number is a number, as
 * the bare `--<key>` flag would give. A key the command does not declare
 * keeps its raw value.
 */
export function configDotTypes (types: Parameters<typeof nopt>[0]): Parameters<typeof nopt>[0] {
  return Object.fromEntries(
    Object.entries(types).map(([name, type]) => [`${CUSTOM_OPTION_PREFIX}${name}`, type])
  )
}

function selectExploratoryParse (ctx: ExploratoryContext): ExploratoryWin {
  const base = runExploratoryWin(ctx, { types: ctx.baseTypes })
  if (base.parsed.argv.remain.length === 0) return base
  const resolved = getResolvedCommand(ctx.opts, base.parsed.argv.remain)
  if (resolved != null) {
    return SPECIALLY_ESCAPED_CMDS.has(resolved.cmd)
      ? parseEscapedCommand(ctx, resolved, base)
      : base
  }
  return retryCandidateCommands(ctx, base)
}

function runExploratoryWin (ctx: ExploratoryContext, spec: ExploratorySpec): ExploratoryWin {
  const fullTypes = { ...spec.types, ...configDotTypes(spec.types) }
  const fullShorthands = {
    r: '--recursive',
    ...ctx.opts.universalShorthands,
    ...spec.shorthands,
  }
  return {
    types: fullTypes,
    shorthands: fullShorthands,
    parsed: runPreparedExploratoryNopt(ctx, { types: fullTypes, shorthands: fullShorthands }, ctx.inputArgv),
  }
}

function runPreparedExploratoryNopt (
  ctx: ExploratoryContext,
  prepared: ExploratorySpec,
  args: string[]
): ReturnType<typeof nopt> {
  return nopt(prepared.types, prepared.shorthands, args, 0, { escapeArgs: ctx.opts.escapeArgs })
}

function getResolvedCommand (
  opts: Pick<ParseCliArgsOptions, 'getCommandLongName'>,
  remain: string[]
): ResolvedCommand | null {
  const effectiveArgs = stripRecursivePrefix(remain)
  const commandName = effectiveArgs[0]
  if (!commandName) return null
  const cmd = opts.getCommandLongName(commandName)
  return cmd ? { commandName, cmd } : null
}

function parseEscapedCommand (
  ctx: ExploratoryContext,
  resolved: ResolvedCommand,
  fallback: ExploratoryWin
): ExploratoryWin {
  const types = extendExploratoryTypes(ctx.baseTypes, ctx.opts, [resolved.cmd])
  const shorthands = ctx.opts.shorthandsByCommandName[resolved.commandName] ??
    ctx.opts.shorthandsByCommandName[resolved.cmd]
  const candidate = runExploratoryWin(ctx, { types, shorthands })
  return getResolvedCommand(ctx.opts, candidate.parsed.argv.remain)?.cmd === resolved.cmd
    ? candidate
    : fallback
}

function retryCandidateCommands (
  ctx: ExploratoryContext,
  fallback: ExploratoryWin
): ExploratoryWin {
  const candidates = collectCandidatesBeforeSeparator(ctx.opts, ctx.inputArgv)
  if (candidates.size === 0) return fallback
  const retry = runExploratoryWin(ctx, {
    types: extendExploratoryTypes(ctx.baseTypes, ctx.opts, candidates),
  })
  if (getResolvedCommand(ctx.opts, retry.parsed.argv.remain) != null) {
    return retry
  }
  return Boolean(ctx.opts.fallbackCommand) && stripRecursivePrefix(retry.parsed.argv.remain).length > 0
    ? retry
    : fallback
}

function collectPreservedCommandTokens (
  ctx: ExploratoryContext,
  win: ExploratoryWin
): PreservedCommandToken[] {
  const remain = win.parsed.argv.remain
  if (remain.length === 0) return []
  const count = RECURSIVE_CMDS.has(remain[0]) && remain.length > 1 && Boolean(ctx.opts.getCommandLongName(remain[1]))
    ? 2
    : 1
  const preserved: PreservedCommandToken[] = []
  let minIndex = 0
  for (let rank = 0; rank < count; rank += 1) {
    const inputIndex = locateRemainTokenIndex(ctx, win, { rank, minIndex })
    if (ctx.inputArgv[inputIndex] === remain[rank]) {
      preserved.push({ token: remain[rank], inputIndex })
    }
    minIndex = inputIndex + 1
  }
  return preserved
}

function locateRemainTokenIndex (
  ctx: ExploratoryContext,
  win: ExploratoryWin,
  target: RemainRankTarget
): number {
  if (target.rank === 0 && !isOptionToken(ctx.inputArgv[0])) return 0
  if (target.rank === 1 && target.minIndex === 1 && !isOptionToken(ctx.inputArgv[1])) return 1
  const countRemainInPrefix = (end: number): number =>
    runPreparedExploratoryNopt(ctx, win, ctx.inputArgv.slice(0, end)).argv.remain.length
  return searchFirstPrefixReachingCount(countRemainInPrefix, {
    minIndex: target.minIndex,
    totalLength: ctx.inputArgv.length,
    targetCount: target.rank + 1,
  })
}

function searchFirstPrefixReachingCount (
  countRemainInPrefix: (end: number) => number,
  bounds: PrefixSearchBounds
): number {
  let lo = bounds.minIndex + 1
  let step = 1
  let hi = lo
  while (hi < bounds.totalLength && countRemainInPrefix(hi) < bounds.targetCount) {
    lo = hi + 1
    step *= 2
    hi = Math.min(bounds.totalLength, lo + step - 1)
  }
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2)
    if (countRemainInPrefix(mid) >= bounds.targetCount) {
      hi = mid
    } else {
      lo = mid + 1
    }
  }
  return lo - 1
}

function stripRecursivePrefix (remain: string[]): string[] {
  return RECURSIVE_CMDS.has(remain[0]) ? remain.slice(1) : remain
}

function collectCandidatesBeforeSeparator (
  opts: Pick<ExploratoryOptions, 'fallbackCommand' | 'getCommandLongName'>,
  inputArgv: string[]
): Set<string> {
  const preSep = sliceBeforeSeparator(inputArgv)
  if (!preSep.some((arg) => arg.startsWith('-') && arg.length > 1)) {
    return new Set()
  }
  const nonFlagTokens = preSep.filter((arg) => !arg.startsWith('-') && !RECURSIVE_CMDS.has(arg))
  return collectCandidateCommands(opts, nonFlagTokens)
}

function sliceBeforeSeparator (inputArgv: string[]): string[] {
  const sepIndex = inputArgv.findIndex((arg) => OPTION_SEPARATOR_RE.test(arg))
  return sepIndex >= 0 ? inputArgv.slice(0, sepIndex) : inputArgv
}

function mergeCommandOptionTypes (
  merged: Record<string, unknown>,
  commandTypes: object
): void {
  for (const [key, type] of Object.entries(commandTypes)) {
    merged[key] = Object.hasOwn(merged, key) && isBooleanType(merged[key]) !== isBooleanType(type)
      ? Boolean
      : (merged[key] ?? type)
  }
}

function isBooleanType (type: unknown): boolean {
  return type === Boolean || (Array.isArray(type) && type.includes(Boolean))
}

function isOptionToken (arg: string | undefined): boolean {
  return arg != null && arg.startsWith('-') && arg.length > 1
}

function canBeConsumedByPrecedingOption (args: string[], index: number): boolean {
  if (index <= 0) return false
  const prev = args[index - 1]
  return isOptionToken(prev) && !OPTION_SEPARATOR_RE.test(prev)
}
