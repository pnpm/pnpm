import findWorkspaceDir from '@pnpm/find-workspace-dir'
import nopt = require('nopt')

const RECURSIVE_CMDS = new Set(['recursive', 'multi', 'm'])

export interface ParsedCliArgs {
  argv: {
    remain: string[],
    cooked: string[],
    original: string[],
  }
  cliArgs: string[]
  cliConf: {
    // tslint:disable-next-line: no-any
    [option: string]: any,
  }
  cmd: string | null
  subCmd: string | null
  isKnownCommand: boolean
  unknownOptions: string[]
  workspaceDir?: string
}

export default async function parseCliArgs (
  opts: {
    getCommandLongName: (commandName: string) => string | null,
    getTypesByCommandName: (commandName: string) => object,
    isKnownCommand: (commandName: string) => boolean,
    renamedOptions?: Record<string, string>,
    shorthandsByCommandName: Record<string, Record<string, string>>,
    universalOptionsTypes: Record<string, unknown>,
    universalShorthands: Record<string, string>,
  },
  inputArgv: string[],
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
      'r': '--recursive',
      ...opts.universalShorthands,
    },
    inputArgv,
    0,
  )
  if (noptExploratoryResults['help']) {
    return {
      argv: noptExploratoryResults.argv,
      cliArgs: noptExploratoryResults.argv.remain,
      cliConf: {},
      cmd: 'help',
      isKnownCommand: true,
      subCmd: null,
      unknownOptions: [] as string[],
    }
  }

  const recursiveCommandUsed = RECURSIVE_CMDS.has(noptExploratoryResults.argv.remain[0])
  const commandName = getCommandName(noptExploratoryResults.argv.remain)
  let cmd = commandName ? opts.getCommandLongName(commandName) : null
  const types = {
    'recursive': Boolean,
    ...opts.universalOptionsTypes,
    ...opts.getTypesByCommandName(commandName),
  } as any // tslint:disable-line:no-any

  function getCommandName (cliArgs: string[]) {
    if (recursiveCommandUsed) {
      cliArgs = cliArgs.slice(1)
    }
    if (opts.getCommandLongName(cliArgs[0]) !== 'install' || cliArgs.length === 1) {
      return cliArgs[0]
    }
    return 'add'
  }

  const { argv, ...cliConf } = nopt(
    types,
    {
      ...opts.universalShorthands,
      ...opts.shorthandsByCommandName[commandName],
    },
    inputArgv,
    0,
  )

  if (opts.renamedOptions) {
    for (const cliOption of Object.keys(cliConf)) {
      if (opts.renamedOptions[cliOption]) {
        cliConf[opts.renamedOptions[cliOption]] = cliConf[cliOption]
        delete cliConf[cliOption]
      }
    }
  }

  let subCmd: string | null = argv.remain[1] && opts.getCommandLongName(argv.remain[1])

  // `pnpm install ""` is going to be just `pnpm install`
  const cliArgs = argv.remain.slice(1).filter(Boolean)

  if (cliConf['recursive'] !== true && (cliConf['filter'] || recursiveCommandUsed)) {
    cliConf['recursive'] = true
    if (subCmd && recursiveCommandUsed) {
      cliArgs.shift()
      argv.remain.shift()
      cmd = subCmd
      subCmd = null
    }
  } else if (subCmd && !opts.isKnownCommand(subCmd)) {
    subCmd = null
  }
  const dir = cliConf['dir'] ?? process.cwd()
  const workspaceDir = cliConf['global'] // tslint:disable-line
    ? undefined
    : await findWorkspaceDir(dir)

  if (
    (cmd === 'add' || cmd === 'install') &&
    typeof workspaceDir === 'string' &&
    cliArgs.length === 0
  ) {
    cliConf['recursive'] = true
  }

  if (cmd === 'install' && cliArgs.length > 0) {
    cmd = 'add'
  } else if (subCmd === 'install' && cliArgs.length > 1) {
    cmd = 'add'
  }
  if (!cmd && cliConf['recursive']) {
    cmd = 'recursive'
  }

  const allowedOptions = new Set(Object.keys(types))
  const unknownOptions = [] as string[]
  for (const cliOption of Object.keys(cliConf)) {
    if (!allowedOptions.has(cliOption) && !cliOption.startsWith('//')) {
      unknownOptions.push(cliOption)
    }
  }
  return {
    argv,
    cliArgs,
    cliConf,
    cmd,
    isKnownCommand: opts.isKnownCommand(commandName),
    subCmd,
    unknownOptions,
    workspaceDir,
  }
}
