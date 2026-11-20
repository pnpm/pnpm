import { PnpmError } from '@pnpm/error'
import type nopt from '@pnpm/nopt'
import { findWorkspaceDir } from '@pnpm/workspace.root-finder'
import didYouMean, { ReturnTypeEnums } from 'didyoumean2'

import { parseResolvedCommandArgs } from './commandParse.js'
import {
  type CommandTokenRecovery,
  type ExploratoryResult,
  parseExploratory,
  RECURSIVE_CMDS,
  SPECIALLY_ESCAPED_CMDS,
  VERSION_BOOTSTRAP_KEYS,
} from './exploratory.js'
import { attributeUnknownOptionsToShorthands, type ShorthandContext } from './shorthandAttribution.js'

export {
  BASE_EXPLORATORY_TYPES,
  buildBaseExploratoryTypes,
  buildExploratoryShorthands,
  type CandidateCommand,
  chooseMarkerPrefix,
  collectCandidateCommands,
  configDotTypes,
  extendExploratoryTypes,
  OPTION_SEPARATOR_RE,
  RECURSIVE_CMDS,
} from './exploratory.js'

const CUSTOM_OPTION_PREFIX = 'config.'

export interface ParsedCliArgs {
  argv: {
    remain: string[]
    cooked: string[]
    original: string[]
  }
  params: string[]
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- option values have command-specific types
  options: Record<string, any>
  cmd: string | null
  unknownOptions: Map<string, string[]>
  /**
   * The unknown options that were passed via a shorthand, mapped to the
   * shorthand as it was typed. For instance, `-l` expands to `--long`, so if
   * `long` is unknown to the command, this maps `long` to `-l`.
   */
  unknownOptionShorthands?: Map<string, string>
  fallbackCommandUsed: boolean
  workspaceDir: string | undefined
  /** The `--config.<key>=<value>` options as given, keyed without the prefix. */
  rawCliConfig?: Record<string, unknown>
}

export interface ParseCliArgsOptions {
  escapeArgs?: string[]
  fallbackCommand?: string
  getCommandLongName: (commandName: string) => string | null
  getTypesByCommandName: (commandName: string) => object
  renamedOptions?: Record<string, string>
  shorthandsByCommandName: Record<string, Record<string, string | string[]>>
  universalOptionsTypes: Record<string, unknown>
  universalShorthands: Record<string, string | string[]>
}

interface EarlyResolution {
  commandName: string
  cmd: string | null
  fallbackCommandUsed: boolean
  recovery: CommandTokenRecovery | undefined
  recursiveCommandUsed: boolean
}

export async function parseCliArgs (
  opts: ParseCliArgsOptions,
  inputArgv: string[]
): Promise<ParsedCliArgs> {
  const exploratoryResult = parseExploratory(opts, inputArgv)
  const resolution = resolveCommand(opts, exploratoryResult, inputArgv)
  const earlyExit = await checkEarlyExit(opts, exploratoryResult.parsed, resolution)
  if (earlyExit != null) return earlyExit

  const types = {
    ...opts.universalOptionsTypes,
    ...opts.getTypesByCommandName(resolution.commandName),
  } as any // eslint-disable-line @typescript-eslint/no-explicit-any -- option types are declared as unknown values
  const escapeArgs = getEscapeArgs(opts, resolution, exploratoryResult.parsed)
  const { argv, rawOptions, types: parsedTypes } = parseResolvedCommandArgs(opts, inputArgv, {
    commandName: resolution.commandName,
    types,
    escapeArgs,
    recovery: resolution.recovery,
  })

  applyRenamedOptions(rawOptions, opts.renamedOptions)
  const workspaceDir = await getWorkspaceDir(rawOptions)
  if (SPECIALLY_ESCAPED_CMDS.has(resolution.cmd!) && rawOptions['help']) {
    return buildEarlyHelpResult(opts, exploratoryResult.parsed, workspaceDir)
  }

  const { cmd, params } = adjustPostParseCommands(rawOptions, argv, resolution, opts)
  validateWorkspaceOptions(rawOptions, workspaceDir)
  return {
    argv,
    cmd,
    params,
    workspaceDir,
    fallbackCommandUsed: resolution.fallbackCommandUsed,
    ...normalizeOptions(rawOptions, new Set(Object.keys(types)), {
      originalArgv: argv.original,
      types: parsedTypes,
      escapeArgs,
      shorthands: {
        ...opts.universalShorthands,
        ...opts.shorthandsByCommandName[resolution.commandName],
      },
    }),
  }
}

function resolveCommand (
  opts: ParseCliArgsOptions,
  exploratoryResult: ExploratoryResult,
  inputArgv: string[]
): EarlyResolution {
  const remain = exploratoryResult.parsed.argv.remain
  const recursiveCommandUsed = RECURSIVE_CMDS.has(remain[0])
  let commandName = getCommandName(remain, recursiveCommandUsed, opts)
  let cmd = commandName ? opts.getCommandLongName(commandName) : null
  const fallbackCommandUsed = Boolean(commandName && !cmd && opts.fallbackCommand)
  if (fallbackCommandUsed) {
    cmd = opts.fallbackCommand!
    commandName = opts.fallbackCommand!
    inputArgv.unshift(opts.fallbackCommand!)
  }
  return {
    commandName,
    cmd,
    fallbackCommandUsed,
    recovery: fallbackCommandUsed ? undefined : exploratoryResult.recovery,
    recursiveCommandUsed,
  }
}

function getCommandName (args: string[], recursiveCommandUsed: boolean, opts: ParseCliArgsOptions): string {
  const effectiveArgs = recursiveCommandUsed ? args.slice(1) : args
  if (opts.getCommandLongName(effectiveArgs[0]) !== 'install' || effectiveArgs.length === 1) {
    return effectiveArgs[0]
  }
  return 'add'
}

async function checkEarlyExit (
  opts: ParseCliArgsOptions,
  exploratory: ReturnType<typeof nopt>,
  resolution: EarlyResolution
): Promise<ParsedCliArgs | undefined> {
  if (resolution.fallbackCommandUsed || SPECIALLY_ESCAPED_CMDS.has(resolution.cmd!)) {
    return undefined
  }
  if (exploratory['help']) {
    const { options, rawCliConfig } = pickEarlyExitOptions(opts, exploratory, false)
    const workspaceDir = await getWorkspaceDir(options, opts.renamedOptions)
    return buildHelpResult(exploratory, options, rawCliConfig, workspaceDir)
  }
  if (exploratory['version'] || exploratory['v']) {
    const { options, rawCliConfig } = pickEarlyExitOptions(opts, exploratory, true)
    return {
      argv: exploratory.argv,
      cmd: null,
      options: {
        ...options,
        version: true,
      },
      rawCliConfig,
      params: exploratory.argv.remain,
      unknownOptions: new Map(),
      fallbackCommandUsed: false,
      workspaceDir: await getWorkspaceDir(options, opts.renamedOptions),
    }
  }
  return undefined
}

function buildEarlyHelpResult (
  opts: ParseCliArgsOptions,
  exploratory: ReturnType<typeof nopt>,
  workspaceDir: string | undefined
): ParsedCliArgs {
  const { options, rawCliConfig } = pickEarlyExitOptions(opts, exploratory, false)
  return buildHelpResult(exploratory, options, rawCliConfig, workspaceDir)
}

function buildHelpResult (
  exploratory: ReturnType<typeof nopt>,
  options: Record<string, unknown>,
  rawCliConfig: Record<string, unknown>,
  workspaceDir: string | undefined
): ParsedCliArgs {
  return {
    argv: exploratory.argv,
    cmd: 'help',
    options,
    rawCliConfig,
    params: exploratory.argv.remain,
    unknownOptions: new Map(),
    fallbackCommandUsed: false,
    workspaceDir,
  }
}

function pickEarlyExitOptions (
  opts: ParseCliArgsOptions,
  exploratory: ReturnType<typeof nopt>,
  includeVersionBootstrapKeys: boolean
): { options: Record<string, unknown>, rawCliConfig: Record<string, unknown> } {
  const keys = includeVersionBootstrapKeys
    ? [...Object.keys(opts.universalOptionsTypes), ...VERSION_BOOTSTRAP_KEYS]
    : Object.keys(opts.universalOptionsTypes)
  const options = pickKnownEarlyExitKeys(exploratory, keys, opts.renamedOptions)
  const rawCliConfig = pickConfigDotOptions(exploratory, options, opts.renamedOptions)
  return { options, rawCliConfig }
}

function pickKnownEarlyExitKeys (
  exploratory: Record<string, unknown>,
  keys: string[],
  renamedOptions?: Record<string, string>
): Record<string, unknown> {
  const options: Record<string, unknown> = {}
  for (const key of keys) {
    if (key in exploratory) {
      options[renamedOptions?.[key] ?? key] = exploratory[key]
    }
  }
  return options
}

function pickConfigDotOptions (
  exploratory: Record<string, unknown>,
  options: Record<string, unknown>,
  renamedOptions?: Record<string, string>
): Record<string, unknown> {
  const rawCliConfig: Record<string, unknown> = {}
  for (const [optionName, optionValue] of Object.entries(exploratory)) {
    if (!optionName.startsWith(CUSTOM_OPTION_PREFIX)) continue
    const rawKey = optionName.substring(CUSTOM_OPTION_PREFIX.length)
    options[renamedOptions?.[rawKey] ?? rawKey] = optionValue
    rawCliConfig[rawKey] = optionValue
  }
  return rawCliConfig
}

function getEscapeArgs (
  opts: ParseCliArgsOptions,
  resolution: EarlyResolution,
  exploratory: ReturnType<typeof nopt>
): string[] | undefined {
  if (!SPECIALLY_ESCAPED_CMDS.has(resolution.cmd!)) {
    return opts.escapeArgs?.includes(resolution.commandName) ? opts.escapeArgs : undefined
  }
  const indexOfRunScriptName = 1 +
    (resolution.recursiveCommandUsed ? 1 : 0) +
    (resolution.fallbackCommandUsed && opts.fallbackCommand === 'run' ? -1 : 0)
  return [exploratory.argv.remain[indexOfRunScriptName]]
}

function applyRenamedOptions (options: Record<string, unknown>, renamedOptions?: Record<string, string>): void {
  if (renamedOptions == null) return
  for (const [cliOption, optionValue] of Object.entries(options)) {
    const target = renamedOptions[cliOption]
    if (target) {
      if (!(target in options)) {
        options[target] = optionValue
      }
      delete options[cliOption]
    }
  }
}

interface AdjustedCommands {
  cmd: string | null
  params: string[]
}

function adjustPostParseCommands (
  options: Record<string, unknown>,
  argv: { remain: string[] },
  resolution: EarlyResolution,
  opts: ParseCliArgsOptions
): AdjustedCommands {
  let cmd = resolution.cmd
  const params = argv.remain.slice(1)
  cmd = handleRecursiveInvocation(options, argv, resolution, opts, params, cmd)
  if (cmd === 'install' && params.length > 0) {
    cmd = 'add'
  } else if (!cmd && options['recursive']) {
    cmd = 'recursive'
  }
  return { cmd, params }
}

function handleRecursiveInvocation (
  options: Record<string, unknown>,
  argv: { remain: string[] },
  resolution: EarlyResolution,
  opts: ParseCliArgsOptions,
  params: string[],
  cmd: string | null
): string | null {
  if (options['recursive'] === true || (!options['filter'] && !options['filter-prod'] && !resolution.recursiveCommandUsed)) {
    return cmd
  }
  options['recursive'] = true
  const subCmd: string | null = argv.remain[1] && opts.getCommandLongName(argv.remain[1])
  if (subCmd && resolution.recursiveCommandUsed) {
    params.shift()
    argv.remain.shift()
    return subCmd
  }
  return cmd
}

function validateWorkspaceOptions (options: Record<string, unknown>, workspaceDir: string | undefined): void {
  if (!options['workspace-root']) return
  if (isGlobalScope(options)) {
    throw new PnpmError('OPTIONS_CONFLICT', '--workspace-root may not be used with --global')
  }
  if (!workspaceDir) {
    throw new PnpmError('NOT_IN_WORKSPACE', '--workspace-root may only be used inside a workspace')
  }
  options['dir'] = workspaceDir
}

interface NormalizeOptionsResult {
  options: Record<string, unknown>
  unknownOptions: Map<string, string[]>
  unknownOptionShorthands: Map<string, string>
  rawCliConfig: Record<string, unknown>
}

function normalizeOptions (
  options: Record<string, unknown>,
  knownOptions: Set<string>,
  shorthandCtx: ShorthandContext
): NormalizeOptionsResult {
  const standardOptionNames = []
  const normalizedOptions: Record<string, unknown> = {}
  const rawCliConfig: Record<string, unknown> = {}
  for (const [optionName, optionValue] of Object.entries(options)) {
    if (optionName.startsWith(CUSTOM_OPTION_PREFIX)) {
      const key = optionName.substring(CUSTOM_OPTION_PREFIX.length)
      normalizedOptions[key] = optionValue
      rawCliConfig[key] = optionValue
      continue
    }
    normalizedOptions[optionName] = optionValue
    standardOptionNames.push(optionName)
  }
  const unknownOptions = getUnknownOptions(standardOptionNames, knownOptions)
  const unknownOptionShorthands = attributeUnknownOptionsToShorthands(unknownOptions, knownOptions, shorthandCtx)
  return { options: normalizedOptions, unknownOptions, unknownOptionShorthands, rawCliConfig }
}

function getUnknownOptions (usedOptions: string[], knownOptions: Set<string>): Map<string, string[]> {
  const unknownOptions = new Map<string, string[]>()
  const closestMatches = getClosestOptionMatches.bind(null, Array.from(knownOptions))
  for (const usedOption of usedOptions) {
    if (knownOptions.has(usedOption) || usedOption.startsWith('//') || isScopeRegistryOption(usedOption)) continue

    unknownOptions.set(usedOption, closestMatches(usedOption))
  }
  return unknownOptions
}

function isScopeRegistryOption (optionName: string): boolean {
  return /^@[a-z0-9][\w.-]*:registry$/.test(optionName)
}

function getClosestOptionMatches (knownOptions: string[], option: string): string[] {
  return didYouMean(option, knownOptions, {
    returnType: ReturnTypeEnums.ALL_CLOSEST_MATCHES,
  })
}

async function getWorkspaceDir (
  parsedOpts: Record<string, unknown>,
  renamedOptions?: Record<string, string>
): Promise<string | undefined> {
  if (isGlobalScope(parsedOpts) || parsedOpts['ignore-workspace']) return undefined
  let dir = parsedOpts['dir']
  if (dir == null && renamedOptions != null) {
    for (const [from, to] of Object.entries(renamedOptions)) {
      if (to === 'dir' && parsedOpts[from] != null) {
        dir = parsedOpts[from]
        break
      }
    }
  }
  return findWorkspaceDir((dir ?? process.cwd()) as string)
}

/** `--location`, which only config commands accept, takes precedence over `--global`. */
function isGlobalScope (parsedOpts: Record<string, unknown>): boolean {
  const location = parsedOpts['location'] ?? parsedOpts['config.location']
  if (location != null) return location === 'global'
  return Boolean(parsedOpts['global'] ?? parsedOpts['config.global'])
}
