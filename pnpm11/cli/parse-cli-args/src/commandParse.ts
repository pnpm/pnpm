import nopt from '@pnpm/nopt'

import {
  type CommandTokenRecovery,
  configDotTypes,
  isOptionToken,
  OPTION_SEPARATOR_RE,
} from './exploratory.js'
import type { ParseCliArgsOptions, ParsedCliArgs } from './index.js'

export interface CommandParseRequest {
  commandName: string
  types: Parameters<typeof nopt>[0]
  escapeArgs?: string[]
  recovery?: CommandTokenRecovery
}

export interface CommandParseOutput {
  argv: ParsedCliArgs['argv']
  rawOptions: Record<string, unknown>
  /** The option types the arguments were parsed with. */
  types: Parameters<typeof nopt>[0]
}

interface SeparatedConfigArgs {
  filteredArgv: string[]
  configDotArgs: string[]
  mapInputIndex: (inputIndex: number) => number
}

interface CommandParseSpec {
  types: Parameters<typeof nopt>[0]
  shorthands: Record<string, string | string[]>
  escapeArgs?: string[]
}

interface RecoverySession {
  inputArgv: string[]
  separated: SeparatedConfigArgs
  spec: CommandParseSpec
  remain: string[]
  recovery: CommandTokenRecovery
  minInputIndex: number
  restoredCount: number
}

export function parseResolvedCommandArgs (
  opts: ParseCliArgsOptions,
  inputArgv: string[],
  request: CommandParseRequest
): CommandParseOutput {
  const separated = separateConfigDotArgs(inputArgv, 'config' in request.types)
  const spec = prepareCommandParseSpec(opts, request)
  const { argv, ...rawOptions } = runCommandNopt(spec, separated.filteredArgv)
  if (request.recovery != null && request.recovery.candidateTokens.length > 0) {
    restoreConsumedCommandTokens({
      inputArgv,
      separated,
      spec,
      remain: argv.remain,
      recovery: request.recovery,
      minInputIndex: 0,
      restoredCount: 0,
    })
  }
  mergeExtractedConfigArgs(rawOptions, separated.configDotArgs, request.types)
  return { argv, rawOptions, types: spec.types }
}

function separateConfigDotArgs (
  inputArgv: string[],
  hasConfigOption: boolean
): SeparatedConfigArgs {
  if (!hasConfigOption) {
    return { filteredArgv: inputArgv, configDotArgs: [], mapInputIndex: (idx) => idx }
  }
  const configDotArgs: string[] = []
  const filteredArgv: string[] = []
  const indexMap: number[] = []
  for (const arg of inputArgv) {
    indexMap.push(filteredArgv.length)
    if (arg.startsWith('--config.')) {
      configDotArgs.push(arg)
    } else {
      filteredArgv.push(arg)
    }
  }
  return { filteredArgv, configDotArgs, mapInputIndex: (idx) => indexMap[idx] }
}

function prepareCommandParseSpec (
  opts: ParseCliArgsOptions,
  request: CommandParseRequest
): CommandParseSpec {
  return {
    types: { recursive: Boolean, ...request.types, ...configDotTypes(request.types) },
    shorthands: {
      ...opts.universalShorthands,
      ...opts.shorthandsByCommandName[request.commandName],
    },
    escapeArgs: request.escapeArgs,
  }
}

function runCommandNopt (
  spec: CommandParseSpec,
  filteredArgv: string[]
): ReturnType<typeof nopt> {
  return nopt(spec.types, spec.shorthands, filteredArgv, 0, { escapeArgs: spec.escapeArgs })
}

function restoreConsumedCommandTokens (session: RecoverySession): void {
  for (let rank = 0; rank < session.recovery.candidateTokens.length; rank += 1) {
    restoreTokenAtRank(session, rank)
  }
}

function restoreTokenAtRank (session: RecoverySession, rank: number): void {
  const token = session.recovery.candidateTokens[rank]
  const occurrences = findTokenIndices(session.inputArgv, token, session.minInputIndex)
  if (occurrences.length === 0) return
  if (occurrences.length === 1) {
    restoreUniqueOccurrence(session, token, occurrences[0])
    return
  }
  restoreRepeatedOccurrence(session, token, {
    rank,
    occurrences,
    hasNextRank: rank + 1 < session.recovery.candidateTokens.length,
  })
}

function restoreUniqueOccurrence (
  session: RecoverySession,
  token: string,
  inputIndex: number
): void {
  session.minInputIndex = inputIndex + 1
  const filteredIndex = session.separated.mapInputIndex(inputIndex)
  if (!canBeConsumedByPrecedingOption(session.separated.filteredArgv, filteredIndex)) return
  if (areAllOccurrencesInRemain(session, token)) return
  insertIfConsumedInPrefix(session, token, filteredIndex)
}

function restoreRepeatedOccurrence (
  session: RecoverySession,
  token: string,
  context: { rank: number, occurrences: number[], hasNextRank: boolean }
): void {
  const anyFollowsOption = context.occurrences.some((idx) =>
    canBeConsumedByPrecedingOption(session.separated.filteredArgv, session.separated.mapInputIndex(idx))
  )
  if (!context.hasNextRank && (!anyFollowsOption || areAllOccurrencesInRemain(session, token))) return
  const inputIndex = session.recovery.locateInputIndex(context.rank)
  if (inputIndex < session.minInputIndex || session.inputArgv[inputIndex] !== token) return
  session.minInputIndex = inputIndex + 1
  const filteredIndex = session.separated.mapInputIndex(inputIndex)
  if (!canBeConsumedByPrecedingOption(session.separated.filteredArgv, filteredIndex)) return
  insertIfConsumedInPrefix(session, token, filteredIndex)
}

function areAllOccurrencesInRemain (session: RecoverySession, token: string): boolean {
  const { filteredArgv } = session.separated
  if (canSynthesizeToken(filteredArgv, session.spec.shorthands, token)) return false
  return countTokenOccurrences(session.remain, token) === countTokenOccurrences(filteredArgv, token)
}

function insertIfConsumedInPrefix (
  session: RecoverySession,
  token: string,
  filteredIndex: number
): void {
  const beforeCount = countCommandRemainInPrefix(session, filteredIndex)
  if (countCommandRemainInPrefix(session, filteredIndex + 1) === beforeCount) {
    session.remain.splice(beforeCount + session.restoredCount, 0, token)
    session.restoredCount += 1
  }
}

function countCommandRemainInPrefix (session: RecoverySession, end: number): number {
  return runCommandNopt(session.spec, session.separated.filteredArgv.slice(0, end)).argv.remain.length
}

function findTokenIndices (args: string[], token: string, startIndex: number): number[] {
  const indices: number[] = []
  for (let idx = startIndex; idx < args.length; idx += 1) {
    if (args[idx] === token) indices.push(idx)
  }
  return indices
}

function countTokenOccurrences (args: string[], token: string): number {
  let count = 0
  for (const arg of args) {
    if (arg === token) count += 1
  }
  return count
}

function canBeConsumedByPrecedingOption (args: string[], index: number): boolean {
  if (index <= 0) return false
  const prev = args[index - 1]
  return isOptionToken(prev) && !OPTION_SEPARATOR_RE.test(prev)
}

function canSynthesizeToken (
  args: string[],
  shorthands: Record<string, unknown>,
  token: string
): boolean {
  return args.some((arg) => hasAttachedOptionValue(arg, token)) ||
    shorthandsContainToken(shorthands, token)
}

function shorthandsContainToken (
  shorthands: Record<string, unknown>,
  token: string
): boolean {
  for (const value of Object.values(shorthands)) {
    const parts = typeof value === 'string'
      ? value.split(/\s+/)
      : (Array.isArray(value) ? value : [])
    if (parts.some((part) => part === token || hasAttachedOptionValue(part, token))) {
      return true
    }
  }
  return false
}

function hasAttachedOptionValue (arg: string, token: string): boolean {
  if (!isOptionToken(arg)) return false
  const eqIndex = arg.indexOf('=')
  return eqIndex >= 0 && arg.slice(eqIndex + 1) === token
}

function mergeExtractedConfigArgs (
  options: Record<string, unknown>,
  configDotArgs: string[],
  types: Parameters<typeof nopt>[0]
): void {
  if (configDotArgs.length === 0) return
  const { argv: _, ...configOptions } = nopt(configDotTypes(types), {}, configDotArgs, 0)
  Object.assign(options, configOptions)
}
