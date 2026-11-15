import nopt from '@pnpm/nopt'

import {
  configDotTypes,
  type PreservedCommandToken,
  restoreConsumedCommandTokens,
} from './exploratory.js'
import type { ParseCliArgsOptions, ParsedCliArgs } from './index.js'

export interface CommandParseRequest {
  commandName: string
  types: Parameters<typeof nopt>[0]
  escapeArgs?: string[]
  preservedTokens: PreservedCommandToken[]
}

export interface CommandParseOutput {
  argv: ParsedCliArgs['argv']
  rawOptions: Record<string, unknown>
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

export function parseResolvedCommandArgs (
  opts: ParseCliArgsOptions,
  inputArgv: string[],
  request: CommandParseRequest
): CommandParseOutput {
  const separated = separateConfigDotArgs(inputArgv, 'config' in request.types)
  const spec = prepareCommandParseSpec(opts, request)
  const { argv, ...rawOptions } = runCommandNopt(spec, separated.filteredArgv)
  restoreConsumedCommandTokens(
    argv.remain,
    (end) => runCommandNopt(spec, separated.filteredArgv.slice(0, end)).argv.remain.length,
    {
      filteredArgv: separated.filteredArgv,
      mapInputIndex: separated.mapInputIndex,
      preservedTokens: request.preservedTokens,
    }
  )
  mergeExtractedConfigArgs(rawOptions, separated.configDotArgs, request.types)
  return { argv, rawOptions }
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

function mergeExtractedConfigArgs (
  options: Record<string, unknown>,
  configDotArgs: string[],
  types: Parameters<typeof nopt>[0]
): void {
  if (configDotArgs.length === 0) return
  const { argv: _, ...configOptions } = nopt(configDotTypes(types), {}, configDotArgs, 0)
  Object.assign(options, configOptions)
}
