import PnpmError from '@pnpm/error'
import findWorkspaceDir from '@pnpm/find-workspace-dir'
import didYouMean, { ReturnTypeEnums } from 'didyoumean2'
import nopt = require('nopt')

const RECURSIVE_CMDS = new Set(['recursive', 'multi', 'm'])

export interface ParsedCliArgs {
  argv: {
    remain: string[]
    cooked: string[]
    original: string[]
  }
  params: string[]
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  options: Record<string, any>
  cmd: string | null
  unknownOptions: Map<string, string[]>
  workspaceDir?: string
}

export default async function parseCliArgs (
  opts: {
    fallbackCommand?: string
    getCommandLongName: (commandName: string) => string | null
    getTypesByCommandName: (commandName: string) => object
    renamedOptions?: Record<string, string>
    shorthandsByCommandName: Record<string, Record<string, string | string[]>>
    universalOptionsTypes: Record<string, unknown>
    universalShorthands: Record<string, string | string[]>
  },
  inputArgv: string[]
): Promise<ParsedCliArgs> {
  const noptExploratoryResults = nopt(
    {
      filter: [String],
      help: Boolean,
      recursive: Boolean,
      ...opts.universalOptionsTypes,
      ...opts.getTypesByCommandName('add'),
      ...opts.getTypesByCommandName('install'),
    },
    {
      r: '--recursive',
      ...opts.universalShorthands,
    },
    inputArgv,
    0
  )
  if (noptExploratoryResults['help']) {
    return {
      argv: noptExploratoryResults.argv,
      cmd: 'help',
      options: {},
      params: noptExploratoryResults.argv.remain,
      unknownOptions: new Map(),
    }
  }

  const recursiveCommandUsed = RECURSIVE_CMDS.has(noptExploratoryResults.argv.remain[0])
  let commandName = getCommandName(noptExploratoryResults.argv.remain)
  let cmd = commandName ? opts.getCommandLongName(commandName) : null
  if (commandName && !cmd && opts.fallbackCommand) {
    cmd = opts.fallbackCommand
    commandName = opts.fallbackCommand
    inputArgv.unshift(opts.fallbackCommand)
  }
  const types = {
    ...opts.universalOptionsTypes,
    ...opts.getTypesByCommandName(commandName),
  } as any // eslint-disable-line @typescript-eslint/no-explicit-any

  function getCommandName (args: string[]) {
    if (recursiveCommandUsed) {
      args = args.slice(1)
    }
    if (opts.getCommandLongName(args[0]) !== 'install' || args.length === 1) {
      return args[0]
    }
    return 'add'
  }

  const { argv, ...options } = nopt(
    {
      recursive: Boolean,
      ...types,
    },
    {
      ...opts.universalShorthands,
      ...opts.shorthandsByCommandName[commandName],
    },
    inputArgv,
    0
  )

  if (opts.renamedOptions) {
    for (const cliOption of Object.keys(options)) {
      if (opts.renamedOptions[cliOption]) {
        options[opts.renamedOptions[cliOption]] = options[cliOption]
        delete options[cliOption]
      }
    }
  }

  // `pnpm install ""` is going to be just `pnpm install`
  const params = argv.remain.slice(1).filter(Boolean)

  if (options['recursive'] !== true && (options['filter'] || recursiveCommandUsed)) {
    options['recursive'] = true
    const subCmd: string | null = argv.remain[1] && opts.getCommandLongName(argv.remain[1])
    if (subCmd && recursiveCommandUsed) {
      params.shift()
      argv.remain.shift()
      cmd = subCmd
    }
  }
  const dir = options['dir'] ?? process.cwd()
  const workspaceDir = options['global'] // eslint-disable-line
    ? undefined
    : await findWorkspaceDir(dir)
  if (options['workspace-root']) {
    if (options['global']) {
      throw new PnpmError('OPTIONS_CONFLICT', '--workspace-root may not be used with --global')
    }
    if (!workspaceDir) {
      throw new PnpmError('NOT_IN_WORKSPACE', '--workspace-root may only be used inside a workspace')
    }
    options['dir'] = workspaceDir
  }

  if (cmd === 'install' && params.length > 0) {
    cmd = 'add'
  }
  if (!cmd && options['recursive']) {
    cmd = 'recursive'
  }

  const knownOptions = new Set(Object.keys(types))
  return {
    argv,
    cmd,
    params,
    workspaceDir,
    ...normalizeOptions(options, knownOptions),
  }
}

const CUSTOM_OPTION_PREFIX = 'config.'

function normalizeOptions (options: Record<string, unknown>, knownOptions: Set<string>) {
  const standardOptionNames = []
  const normalizedOptions = {}
  for (const [optionName, optionValue] of Object.entries(options)) {
    if (optionName.startsWith(CUSTOM_OPTION_PREFIX)) {
      normalizedOptions[optionName.substring(CUSTOM_OPTION_PREFIX.length)] = optionValue
      continue
    }
    normalizedOptions[optionName] = optionValue
    standardOptionNames.push(optionName)
  }
  const unknownOptions = getUnknownOptions(standardOptionNames, knownOptions)
  return { options: normalizedOptions, unknownOptions }
}

function getUnknownOptions (usedOptions: string[], knownOptions: Set<string>) {
  const unknownOptions = new Map<string, string[]>()
  const closestMatches = getClosestOptionMatches.bind(null, Array.from(knownOptions))
  for (const usedOption of usedOptions) {
    if (knownOptions.has(usedOption) || usedOption.startsWith('//')) continue

    unknownOptions.set(usedOption, closestMatches(usedOption))
  }
  return unknownOptions
}

function getClosestOptionMatches (knownOptions: string[], option: string) {
  return didYouMean(option, knownOptions, {
    returnType: ReturnTypeEnums.ALL_CLOSEST_MATCHES,
  })
}
