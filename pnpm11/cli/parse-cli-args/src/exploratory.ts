import nopt from '@pnpm/nopt'

import type { ParseCliArgsOptions } from './index.js'

const CUSTOM_OPTION_PREFIX = 'config.'
const OPTION_SEPARATOR_RE = /^-{2,}$/
const RECURSIVE_CMDS = new Set(['recursive', 'multi', 'm'])

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
  'getCommandLongName' | 'getTypesByCommandName' | 'universalOptionsTypes'
>

export function parseExploratory (
  opts: ParseCliArgsOptions,
  inputArgv: string[]
): ReturnType<typeof nopt> {
  const baseTypes = buildBaseExploratoryTypes(opts)
  const base = runExploratoryNopt(opts, baseTypes, inputArgv)
  if (base.argv.remain.length === 0 || resolvesKnownCommand(opts, base.argv.remain)) {
    return base
  }
  const candidates = collectKnownCommandsBeforeSeparator(opts, inputArgv)
  if (candidates.size === 0) return base
  const retry = runExploratoryNopt(opts, extendExploratoryTypes(baseTypes, opts, candidates), inputArgv)
  return resolvesKnownCommand(opts, retry.argv.remain) ? retry : base
}

export function buildBaseExploratoryTypes (
  opts: ExploratoryOptions
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
  opts: ExploratoryOptions,
  commandNames: Iterable<string>
): Parameters<typeof nopt>[0] {
  const merged: Record<string, unknown> = { ...baseTypes }
  for (const commandName of commandNames) {
    mergeCommandOptionTypes(merged, opts.getTypesByCommandName(commandName))
  }
  return merged as Parameters<typeof nopt>[0]
}

export function collectKnownCommands (
  opts: Pick<ExploratoryOptions, 'getCommandLongName'>,
  tokens: Iterable<string>
): Set<string> {
  const names = new Set<string>()
  for (const token of tokens) {
    const canonical = opts.getCommandLongName(token)
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
  opts: ParseCliArgsOptions,
  exploratoryTypes: Parameters<typeof nopt>[0],
  inputArgv: string[]
): ReturnType<typeof nopt> {
  return nopt(
    { ...exploratoryTypes, ...configDotTypes(exploratoryTypes) },
    { r: '--recursive', ...opts.universalShorthands },
    inputArgv,
    0,
    { escapeArgs: opts.escapeArgs }
  )
}

function resolvesKnownCommand (opts: ParseCliArgsOptions, remain: string[]): boolean {
  const effectiveArgs = RECURSIVE_CMDS.has(remain[0]) ? remain.slice(1) : remain
  return Boolean(effectiveArgs[0] && opts.getCommandLongName(effectiveArgs[0]))
}

function collectKnownCommandsBeforeSeparator (
  opts: Pick<ExploratoryOptions, 'getCommandLongName'>,
  inputArgv: string[]
): Set<string> {
  const tokens: string[] = []
  for (const arg of inputArgv) {
    if (OPTION_SEPARATOR_RE.test(arg)) break
    if (!arg.startsWith('-') && !RECURSIVE_CMDS.has(arg)) {
      tokens.push(arg)
    }
  }
  return collectKnownCommands(opts, tokens)
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
