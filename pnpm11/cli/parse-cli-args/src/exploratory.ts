import nopt from '@pnpm/nopt'

import type { ParseCliArgsOptions } from './index.js'

export const OPTION_SEPARATOR_RE = /^-{2,}$/
export const RECURSIVE_CMDS = new Set(['recursive', 'multi', 'm'])
export const SPECIALLY_ESCAPED_CMDS = new Set(['run', 'dlx', 'with'])

const CUSTOM_OPTION_PREFIX = 'config.'
const MARKER_PREFIX_RE = /^__pnpm_prefix_(\d+)_/

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

export interface ResolvedCommand {
  commandName: string
  cmd: string
}

export interface CandidateCommand extends ResolvedCommand {
  isFallback: boolean
}

export interface CommandTokenRecovery {
  candidateTokens: string[]
  locateInputIndex: (rank: number) => number
}

export interface ExploratoryResult {
  parsed: ReturnType<typeof nopt>
  recovery: CommandTokenRecovery
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
    recovery: createCommandTokenRecovery(ctx, win),
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
  const fullShorthands = buildExploratoryShorthands(ctx.opts.universalShorthands, spec.shorthands)
  return {
    types: fullTypes,
    shorthands: fullShorthands,
    parsed: runPreparedExploratoryNopt(
      { types: fullTypes, shorthands: fullShorthands },
      ctx.inputArgv,
      ctx.opts.escapeArgs
    ),
  }
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

export function buildExploratoryShorthands (
  universalShorthands: Record<string, string | string[]>,
  commandShorthands?: Record<string, string | string[]>
): Record<string, string | string[]> {
  return {
    r: '--recursive',
    ...universalShorthands,
    ...commandShorthands,
  }
}

function runPreparedExploratoryNopt (
  prepared: ExploratorySpec,
  args: string[],
  escapeArgs: string[] | undefined
): ReturnType<typeof nopt> {
  return nopt(prepared.types, { ...prepared.shorthands }, args, 0, { escapeArgs })
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

function stripRecursivePrefix (remain: string[]): string[] {
  return RECURSIVE_CMDS.has(remain[0]) ? remain.slice(1) : remain
}

function parseEscapedCommand (
  ctx: ExploratoryContext,
  resolved: ResolvedCommand,
  fallback: ExploratoryWin
): ExploratoryWin {
  const candidate = runCandidateWin(ctx, resolved)
  return getResolvedCommand(ctx.opts, candidate.parsed.argv.remain)?.cmd === resolved.cmd
    ? candidate
    : fallback
}

function runCandidateWin (
  ctx: ExploratoryContext,
  target: ResolvedCommand
): ExploratoryWin {
  const types = extendExploratoryTypes(ctx.baseTypes, ctx.opts, [target.cmd])
  const shorthands = ctx.opts.shorthandsByCommandName[target.commandName] ??
    ctx.opts.shorthandsByCommandName[target.cmd]
  return runExploratoryWin(ctx, { types, shorthands })
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

function retryCandidateCommands (
  ctx: ExploratoryContext,
  fallback: ExploratoryWin
): ExploratoryWin {
  const candidates = collectCandidatesBeforeSeparator(ctx.opts, ctx.inputArgv)
  for (const candidate of candidates) {
    const matched = tryCandidateCommand(ctx, candidate)
    if (matched != null) return matched
  }
  return fallback
}

function tryCandidateCommand (
  ctx: ExploratoryContext,
  candidate: CandidateCommand
): ExploratoryWin | undefined {
  const win = runCandidateWin(ctx, candidate)
  const resolved = getResolvedCommand(ctx.opts, win.parsed.argv.remain)
  if (resolved?.cmd === candidate.cmd) return win
  if (candidate.isFallback && resolved == null && stripRecursivePrefix(win.parsed.argv.remain).length > 0) {
    return win
  }
  return undefined
}

function collectCandidatesBeforeSeparator (
  opts: Pick<ExploratoryOptions, 'fallbackCommand' | 'getCommandLongName'>,
  inputArgv: string[]
): CandidateCommand[] {
  const preSep = sliceBeforeSeparator(inputArgv)
  if (!preSep.some((arg) => isOptionToken(arg))) {
    return []
  }
  const nonFlagTokens = preSep.filter((arg) => !arg.startsWith('-') && !RECURSIVE_CMDS.has(arg))
  return collectCandidateCommands(opts, nonFlagTokens)
}

function sliceBeforeSeparator (inputArgv: string[]): string[] {
  const sepIndex = inputArgv.findIndex((arg) => OPTION_SEPARATOR_RE.test(arg))
  return sepIndex >= 0 ? inputArgv.slice(0, sepIndex) : inputArgv
}

export function isOptionToken (arg: string | undefined): boolean {
  return arg != null && arg.startsWith('-') && arg.length > 1
}

export function collectCandidateCommands (
  opts: Pick<ExploratoryOptions, 'fallbackCommand' | 'getCommandLongName'>,
  tokens: Iterable<string>
): CandidateCommand[] {
  const candidates: CandidateCommand[] = []
  const seen = new Set<string>()
  let sawUnknown = false
  for (const token of tokens) {
    const canonical = opts.getCommandLongName(token)
    if (canonical == null) {
      sawUnknown = true
    } else {
      pushCandidateCommand(candidates, seen, { commandName: token, cmd: canonical, isFallback: false })
    }
  }
  if (sawUnknown && opts.fallbackCommand) {
    pushCandidateCommand(candidates, seen, {
      commandName: opts.fallbackCommand,
      cmd: opts.fallbackCommand,
      isFallback: true,
    })
  }
  return candidates
}

function pushCandidateCommand (
  candidates: CandidateCommand[],
  seen: Set<string>,
  candidate: CandidateCommand
): void {
  if (candidate.cmd === 'add' || candidate.cmd === 'install' || seen.has(candidate.cmd)) {
    return
  }
  seen.add(candidate.cmd)
  candidates.push(candidate)
}

function createCommandTokenRecovery (
  ctx: ExploratoryContext,
  win: ExploratoryWin
): CommandTokenRecovery {
  const remain = win.parsed.argv.remain
  const count = countRecoverableHeadTokens(ctx.opts, remain)
  return {
    candidateTokens: remain.slice(0, count),
    locateInputIndex: (rank) => locateRemainTokenIndex(ctx, win, rank),
  }
}

function countRecoverableHeadTokens (
  opts: Pick<ParseCliArgsOptions, 'getCommandLongName'>,
  remain: string[]
): number {
  if (remain.length === 0) return 0
  return RECURSIVE_CMDS.has(remain[0]) && remain.length > 1 && Boolean(opts.getCommandLongName(remain[1]))
    ? 2
    : 1
}

function locateRemainTokenIndex (
  ctx: ExploratoryContext,
  win: ExploratoryWin,
  rank: number
): number {
  const token = win.parsed.argv.remain[rank]
  if (token == null) return -1
  const prefix = chooseMarkerPrefix(ctx.inputArgv)
  const tagToIndex = new Map<string, number>()
  const taggedArgs = ctx.inputArgv.map((arg, idx) => {
    if (arg !== token) return arg
    const tag = `${prefix}${idx}__`
    tagToIndex.set(tag, idx)
    return tag
  })
  const escapeArgs = ctx.opts.escapeArgs?.includes(token)
    ? [...ctx.opts.escapeArgs, ...tagToIndex.keys()]
    : ctx.opts.escapeArgs
  const taggedRemain = runPreparedExploratoryNopt(win, taggedArgs, escapeArgs).argv.remain
  return tagToIndex.get(taggedRemain[rank]) ?? -1
}

export function chooseMarkerPrefix (args: string[]): string {
  const occupied = new Set<string>()
  for (const token of args) {
    const match = MARKER_PREFIX_RE.exec(token)
    if (match != null) occupied.add(match[1])
  }
  let salt = 0
  while (occupied.has(String(salt))) {
    salt += 1
  }
  return `__pnpm_prefix_${salt}_`
}
