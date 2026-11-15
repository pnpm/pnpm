import { parseCliArgs as parseCliArgsLib, type ParsedCliArgs } from '@pnpm/cli.parse-cli-args'
import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import nopt from '@pnpm/nopt'

import {
  getCliOptionsTypes,
  getCommandFullName,
  GLOBAL_OPTIONS,
  pnpmCmds,
  shorthandsByCommandName,
} from './cmd/index.js'
import { shorthands as universalShorthands } from './shorthands.js'

const RENAMED_OPTIONS = {
  prefix: 'dir',
  store: 'store-dir',
}

type FlagTypeMap = Parameters<typeof nopt>[0]

const OPTION_SEPARATOR_RE = /^-{2,}$/
const CUSTOM_OPTION_PREFIX = '--config.'
const RECURSIVE_PREFIXES = new Set(['recursive', 'multi', 'm'])
let basePreCommandTypes: FlagTypeMap | undefined
let fullPreCommandTypes: FlagTypeMap | undefined

export type ParsedCliArgsWithBuiltIn = ParsedCliArgs & { builtInCommandForced: boolean }

export async function parseCliArgs (inputArgv: string[]): Promise<ParsedCliArgsWithBuiltIn> {
  const { argv, builtInCommandForced, withCurrent } = stripPreCommandPrefixes(inputArgv)
  if (withCurrent) {
    process.env.pnpm_config_pm_on_fail = 'ignore'
  }
  const result = await parseCliArgsLib({
    fallbackCommand: 'run',
    escapeArgs: ['create', 'exec', 'test'],
    getCommandLongName: getCommandFullName,
    getTypesByCommandName: getCliOptionsTypes,
    renamedOptions: RENAMED_OPTIONS,
    shorthandsByCommandName,
    universalOptionsTypes: GLOBAL_OPTIONS,
    universalShorthands,
  }, argv)
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

function scanPreCommandPrefixes (argv: string[]): ScannedPrefixes {
  const scanned: ScannedPrefixes = {}
  let resumeAt = 0
  while (true) {
    const pos = nextPositionalBeforeSeparator(argv, resumeAt)
    const nextResume = pos >= 0 ? consumePrefixAt(argv, pos, scanned) : -1
    if (nextResume < 0) break
    resumeAt = nextResume
  }
  return scanned
}

function consumePrefixAt (argv: string[], pos: number, scanned: ScannedPrefixes): number {
  if (scanned.pmIndex === undefined && argv[pos] === 'pm') {
    scanned.pmIndex = pos
    return pos + 1
  }
  const currentPos = scanned.withCurrentIndices === undefined ? matchWithCurrentAt(argv, pos) : -1
  if (currentPos >= 0) {
    scanned.withCurrentIndices = [pos, currentPos]
    return currentPos + 1
  }
  if (!scanned.recursiveSeen && RECURSIVE_PREFIXES.has(argv[pos]) && nextPositionalBeforeSeparator(argv, pos + 1) >= 0) {
    scanned.recursiveSeen = true
    return pos + 1
  }
  return -1
}

function matchWithCurrentAt (argv: string[], pos: number): number {
  if (argv[pos] !== 'with') return -1
  const currentPos = nextPositionalBeforeSeparator(argv, pos + 1)
  return currentPos >= 0 && argv[currentPos] === 'current' ? currentPos : -1
}

function isOptionSeparator (token: string): boolean {
  return OPTION_SEPARATOR_RE.test(token)
}

function nextPositionalBeforeSeparator (argv: string[], from: number): number {
  let index = from
  while (index < argv.length) {
    const token = argv[index]
    if (isOptionSeparator(token)) return -1
    if (!token.startsWith('-') || token === '-') return index
    index += optionConsumesNext(token, argv[index + 1]) ? 2 : 1
  }
  return -1
}

function optionConsumesNext (token: string, next: string | undefined): boolean {
  if (next === undefined || isOptionSeparator(next) || token.includes('=')) return false
  const normalized = normalizePreCommandOption(token)
  if (normalized == null) return false
  const { probeToken, shorthands, types } = normalized
  if (!next.startsWith('-') || next === '-') {
    return nopt(types, shorthands, [probeToken, next], 0).argv.remain.length === 0
  }
  if (nopt(types, shorthands, [probeToken, 'value'], 0).argv.remain.length > 0) {
    return false
  }
  return !('__probe__' in nopt(types, shorthands, [probeToken, '--__probe__'], 0))
}

interface NormalizedPreCommandOption {
  probeToken: string
  shorthands: Record<string, string>
  types: FlagTypeMap
}

function normalizePreCommandOption (token: string): NormalizedPreCommandOption | undefined {
  if (token.startsWith(CUSTOM_OPTION_PREFIX)) {
    const key = token.slice(CUSTOM_OPTION_PREFIX.length)
    if (key === '') return undefined
    const probeToken = `--${key}`
    return { probeToken, shorthands: {}, types: getPreCommandTypesFor(probeToken, false) }
  }
  return {
    probeToken: token,
    shorthands: universalShorthands,
    types: getPreCommandTypesFor(token, true),
  }
}

function getPreCommandTypesFor (probeToken: string, allowShorthands: boolean): FlagTypeMap {
  const base = getBasePreCommandTypes()
  if (!probeToken.startsWith('--')) return base
  const rawName = probeToken.slice(2)
  const name = rawName.startsWith('no-') ? rawName.slice(3) : rawName
  if (Object.hasOwn(base, name) || (allowShorthands && Object.hasOwn(universalShorthands, rawName))) {
    return base
  }
  return getFullPreCommandTypes()
}

function getBasePreCommandTypes (): FlagTypeMap {
  basePreCommandTypes ??= { ...allTypes, ...GLOBAL_OPTIONS } as FlagTypeMap
  return basePreCommandTypes
}

function getFullPreCommandTypes (): FlagTypeMap {
  if (fullPreCommandTypes != null) return fullPreCommandTypes
  const merged: Record<string, unknown> = { ...allTypes }
  const seenCommands = new Set<string>()
  for (const commandName of Object.keys(pnpmCmds)) {
    const canonical = getCommandFullName(commandName) ?? commandName
    if (seenCommands.has(canonical)) continue
    seenCommands.add(canonical)
    mergeCommandOptionTypes(merged, getCliOptionsTypes(canonical))
  }
  Object.assign(merged, GLOBAL_OPTIONS)
  fullPreCommandTypes = merged as FlagTypeMap
  return fullPreCommandTypes
}

function mergeCommandOptionTypes (
  merged: Record<string, unknown>,
  commandTypes: Record<string, unknown>
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
