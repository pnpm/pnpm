import {
  BASE_EXPLORATORY_TYPES,
  buildExploratoryTypes,
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

const OPTION_SEPARATOR_RE = /^-{2,}$/
const CUSTOM_OPTION_PREFIX = '--config.'
const RECURSIVE_PREFIXES = new Set(['recursive', 'multi', 'm'])
const EXPLORATORY_SHORTHANDS = { r: '--recursive', ...universalShorthands }
let libOpts: ParseCliArgsOptions | undefined
let basePreCommandTypes: FlagTypeMap | undefined

export type ParsedCliArgsWithBuiltIn = ParsedCliArgs & { builtInCommandForced: boolean }

export async function parseCliArgs (inputArgv: string[]): Promise<ParsedCliArgsWithBuiltIn> {
  const { argv, builtInCommandForced, withCurrent } = stripPreCommandPrefixes(inputArgv)
  if (withCurrent) {
    process.env.pnpm_config_pm_on_fail = 'ignore'
  }
  const result = await parseCliArgsLib(getLibOpts(), argv)
  return { ...result, builtInCommandForced }
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
  let exploratoryTypes: FlagTypeMap | undefined
  const getExploratoryTypes = (): FlagTypeMap => {
    exploratoryTypes ??= buildExploratoryTypes(getLibOpts(), argv)
    return exploratoryTypes
  }
  let resumeAt = 0
  while (true) {
    const pos = nextPositionalBeforeSeparator(argv, resumeAt, getExploratoryTypes)
    const nextResume = pos >= 0 ? consumePrefixAt(argv, pos, scanned, getExploratoryTypes) : -1
    if (nextResume < 0) break
    resumeAt = nextResume
  }
  return scanned
}

function nextPositionalBeforeSeparator (
  argv: string[],
  from: number,
  getExploratoryTypes: () => FlagTypeMap
): number {
  let index = from
  while (index < argv.length) {
    const token = argv[index]
    if (isOptionSeparator(token)) return -1
    if (!token.startsWith('-') || token === '-') return index
    index += optionConsumesNext(token, argv[index + 1], getExploratoryTypes) ? 2 : 1
  }
  return -1
}

function consumePrefixAt (
  argv: string[],
  pos: number,
  scanned: ScannedPrefixes,
  getExploratoryTypes: () => FlagTypeMap
): number {
  if (scanned.pmIndex === undefined && argv[pos] === 'pm') {
    scanned.pmIndex = pos
    return pos + 1
  }
  const currentPos = scanned.withCurrentIndices === undefined
    ? matchWithCurrentAt(argv, pos, getExploratoryTypes)
    : -1
  if (currentPos >= 0) {
    scanned.withCurrentIndices = [pos, currentPos]
    return currentPos + 1
  }
  if (
    !scanned.recursiveSeen &&
    RECURSIVE_PREFIXES.has(argv[pos]) &&
    nextPositionalBeforeSeparator(argv, pos + 1, getExploratoryTypes) >= 0
  ) {
    scanned.recursiveSeen = true
    return pos + 1
  }
  return -1
}

function matchWithCurrentAt (
  argv: string[],
  pos: number,
  getExploratoryTypes: () => FlagTypeMap
): number {
  if (argv[pos] !== 'with') return -1
  const currentPos = nextPositionalBeforeSeparator(argv, pos + 1, getExploratoryTypes)
  return currentPos >= 0 && argv[currentPos] === 'current' ? currentPos : -1
}

function isOptionSeparator (token: string): boolean {
  return OPTION_SEPARATOR_RE.test(token)
}

function optionConsumesNext (
  token: string,
  next: string | undefined,
  getExploratoryTypes: () => FlagTypeMap
): boolean {
  if (next === undefined || isOptionSeparator(next) || token.includes('=')) return false
  const normalized = normalizePreCommandOption(token, getExploratoryTypes)
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

function normalizePreCommandOption (
  token: string,
  getExploratoryTypes: () => FlagTypeMap
): NormalizedPreCommandOption | undefined {
  if (token.startsWith(CUSTOM_OPTION_PREFIX)) {
    const key = token.slice(CUSTOM_OPTION_PREFIX.length)
    if (key === '') return undefined
    const probeToken = `--${key}`
    return {
      probeToken,
      shorthands: {},
      types: resolvePreCommandTypes(probeToken, false, getExploratoryTypes),
    }
  }
  return {
    probeToken: token,
    shorthands: EXPLORATORY_SHORTHANDS,
    types: resolvePreCommandTypes(token, true, getExploratoryTypes),
  }
}

function resolvePreCommandTypes (
  probeToken: string,
  allowShorthands: boolean,
  getExploratoryTypes: () => FlagTypeMap
): FlagTypeMap {
  const base = getBasePreCommandTypes()
  return isKnownBaseOption(probeToken, base, allowShorthands) ? base : getExploratoryTypes()
}

function getBasePreCommandTypes (): FlagTypeMap {
  basePreCommandTypes ??= { ...BASE_EXPLORATORY_TYPES, ...GLOBAL_OPTIONS } as FlagTypeMap
  return basePreCommandTypes
}

function isKnownBaseOption (probeToken: string, base: FlagTypeMap, allowShorthands: boolean): boolean {
  if (!probeToken.startsWith('--')) {
    return allowShorthands && probeToken.length === 2 && Object.hasOwn(EXPLORATORY_SHORTHANDS, probeToken[1])
  }
  const rawName = probeToken.slice(2)
  const name = rawName.startsWith('no-') ? rawName.slice(3) : rawName
  return Object.hasOwn(base, name) || (allowShorthands && Object.hasOwn(EXPLORATORY_SHORTHANDS, rawName))
}
