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

const PRE_COMMAND_SHORTHANDS = { r: '--recursive', ...universalShorthands }
const RECURSIVE_PREFIXES = new Set(['recursive', 'multi', 'm'])
let preCommandTypes: FlagTypeMap | undefined

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

function nextPositionalBeforeSeparator (argv: string[], from: number): number {
  let index = from
  while (index < argv.length) {
    const token = argv[index]
    if (token === '--') return -1
    if (!token.startsWith('-') || token === '-') return index
    index += optionConsumesNext(token, argv[index + 1]) ? 2 : 1
  }
  return -1
}

function optionConsumesNext (token: string, next: string | undefined): boolean {
  if (next === undefined || next === '--' || token.startsWith('--config.')) return false
  const types = getPreCommandTypes()
  if (!next.startsWith('-') || next === '-') {
    return nopt(types, PRE_COMMAND_SHORTHANDS, [token, next], 0).argv.remain.length === 0
  }
  if (nopt(types, PRE_COMMAND_SHORTHANDS, [token, 'value'], 0).argv.remain.length > 0) {
    return false
  }
  return !('__probe__' in nopt(types, PRE_COMMAND_SHORTHANDS, [token, '--__probe__'], 0))
}

function getPreCommandTypes (): FlagTypeMap {
  if (preCommandTypes != null) return preCommandTypes
  const merged: Record<string, unknown> = { ...allTypes }
  for (const commandName of Object.keys(pnpmCmds)) {
    mergeCommandOptionTypes(merged, getCliOptionsTypes(commandName))
  }
  Object.assign(merged, GLOBAL_OPTIONS)
  preCommandTypes = merged as FlagTypeMap
  return preCommandTypes
}

function mergeCommandOptionTypes (
  merged: Record<string, unknown>,
  commandTypes: Record<string, unknown>
): void {
  for (const [key, type] of Object.entries(commandTypes)) {
    merged[key] = key in merged && isBooleanType(merged[key]) !== isBooleanType(type)
      ? Boolean
      : (merged[key] ?? type)
  }
}

function isBooleanType (type: unknown): boolean {
  return type === Boolean || (Array.isArray(type) && type.includes(Boolean))
}
