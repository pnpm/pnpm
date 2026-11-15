import type nopt from '@pnpm/nopt'

import type { ParseCliArgsOptions } from './index.js'

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
  'fallbackCommand' | 'getCommandLongName' | 'getTypesByCommandName' | 'universalOptionsTypes'
>

export function buildExploratoryTypes (
  opts: ExploratoryOptions,
  inputArgv: string[]
): Parameters<typeof nopt>[0] {
  const merged: Record<string, unknown> = {
    ...BASE_EXPLORATORY_TYPES,
    ...opts.universalOptionsTypes,
    ...opts.getTypesByCommandName('add'),
    ...opts.getTypesByCommandName('install'),
  }
  if (inputArgv.some((arg) => arg.startsWith('-'))) {
    mergeArgvCommandTypes(merged, opts, inputArgv)
  }
  return merged as Parameters<typeof nopt>[0]
}

function mergeArgvCommandTypes (
  merged: Record<string, unknown>,
  opts: ExploratoryOptions,
  inputArgv: string[]
): void {
  for (const commandName of collectArgvCommandNames(opts, inputArgv)) {
    mergeCommandOptionTypes(merged, opts.getTypesByCommandName(commandName))
  }
}

function collectArgvCommandNames (opts: ExploratoryOptions, inputArgv: string[]): Set<string> {
  const names = new Set<string>()
  for (const arg of inputArgv) {
    if (OPTION_SEPARATOR_RE.test(arg)) break
    if (arg.startsWith('-') || RECURSIVE_CMDS.has(arg)) continue
    const canonical = opts.getCommandLongName(arg) ?? opts.fallbackCommand
    if (canonical && canonical !== 'add' && canonical !== 'install') {
      names.add(canonical)
    }
  }
  return names
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
