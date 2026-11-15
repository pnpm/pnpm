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

interface ExploratoryContext {
  opts: ParseCliArgsOptions
  baseTypes: Parameters<typeof nopt>[0]
  inputArgv: string[]
}

interface ResolvedCommand {
  commandName: string
  cmd: string
}

export function parseExploratory (
  opts: ParseCliArgsOptions,
  inputArgv: string[]
): ReturnType<typeof nopt> {
  const ctx: ExploratoryContext = {
    opts,
    baseTypes: buildBaseExploratoryTypes(opts),
    inputArgv,
  }
  const base = runExploratoryNopt(ctx, ctx.baseTypes)
  if (base.argv.remain.length === 0) return base
  const resolved = getResolvedCommand(opts, base.argv.remain)
  if (resolved != null) {
    return SPECIALLY_ESCAPED_CMDS.has(resolved.cmd)
      ? parseEscapedCommand(ctx, resolved, base)
      : base
  }
  return retryCandidateCommands(ctx, base)
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

function runExploratoryNopt (
  ctx: ExploratoryContext,
  exploratoryTypes: Parameters<typeof nopt>[0],
  shorthands?: Record<string, string | string[]>
): ReturnType<typeof nopt> {
  return nopt(
    { ...exploratoryTypes, ...configDotTypes(exploratoryTypes) },
    {
      r: '--recursive',
      ...ctx.opts.universalShorthands,
      ...shorthands,
    },
    ctx.inputArgv,
    0,
    { escapeArgs: ctx.opts.escapeArgs }
  )
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
  fallback: ReturnType<typeof nopt>
): ReturnType<typeof nopt> {
  const types = extendExploratoryTypes(ctx.baseTypes, ctx.opts, [resolved.cmd])
  const shorthands = ctx.opts.shorthandsByCommandName[resolved.commandName] ??
    ctx.opts.shorthandsByCommandName[resolved.cmd]
  const parsed = runExploratoryNopt(ctx, types, shorthands)
  return getResolvedCommand(ctx.opts, parsed.argv.remain)?.cmd === resolved.cmd
    ? parsed
    : fallback
}

function retryCandidateCommands (
  ctx: ExploratoryContext,
  fallback: ReturnType<typeof nopt>
): ReturnType<typeof nopt> {
  const candidates = collectCandidatesBeforeSeparator(ctx.opts, ctx.inputArgv)
  if (candidates.size === 0) return fallback
  const retry = runExploratoryNopt(ctx, extendExploratoryTypes(ctx.baseTypes, ctx.opts, candidates))
  if (getResolvedCommand(ctx.opts, retry.argv.remain) != null) {
    return retry
  }
  return Boolean(ctx.opts.fallbackCommand) && stripRecursivePrefix(retry.argv.remain).length > 0
    ? retry
    : fallback
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
