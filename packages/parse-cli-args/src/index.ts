import findWorkspaceDir from '@pnpm/find-workspace-dir'
import nopt = require('nopt')

const RECURSIVE_CMDS = new Set(['recursive', 'multi', 'm'])

export default async function parseCliArgs (
  opts: {
    getCommandLongName: (commandName: string) => string,
    getTypesByCommandName: (commandName: string) => object,
    globalOptionsTypes: Record<string, unknown>,
    isKnownCommand: (commandName: string) => boolean,
    renamedOptions?: Record<string, string>,
    shortHands: Record<string, string[]>,
  },
  inputArgv: string[],
) {
  const noptExploratoryResults = nopt(
    {
      filter: [String],
      help: Boolean,
      recursive: Boolean,
      ...opts.globalOptionsTypes,
      ...opts.getTypesByCommandName('add'),
      ...opts.getTypesByCommandName('install'),
    },
    {
      'r': ['--recursive'],
      ...opts.shortHands,
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
      dir: process.cwd(),
      subCmd: null,
      unknownOptions: [] as string[],
    }
  }

  const types = {
    'recursive': Boolean,
    ...opts.globalOptionsTypes,
    ...opts.getTypesByCommandName(getCommandName(noptExploratoryResults.argv.remain)),
  } as any // tslint:disable-line:no-any

  function getCommandName (cliArgs: string[]) {
    if (RECURSIVE_CMDS.has(cliArgs[0])) {
      cliArgs = cliArgs.slice(1)
    }
    if (opts.getCommandLongName(cliArgs[0]) !== 'install' || cliArgs.length === 1) return cliArgs[0]
    return 'add'
  }

  const { argv, ...cliConf } = nopt(types, opts.shortHands, inputArgv, 0)

  if (opts.renamedOptions) {
    for (const cliOption of Object.keys(cliConf)) {
      if (opts.renamedOptions[cliOption]) {
        cliConf[opts.renamedOptions[cliOption]] = cliConf[cliOption]
        delete cliConf[cliOption]
      }
    }
  }

  let cmd = opts.getCommandLongName(argv.remain[0])
  if (cmd && !opts.isKnownCommand(cmd) && !RECURSIVE_CMDS.has(cmd)) {
    cmd = 'help'
  }

  let subCmd: string | null = argv.remain[1] && opts.getCommandLongName(argv.remain[1])

  // `pnpm install ""` is going to be just `pnpm install`
  const cliArgs = argv.remain.slice(1).filter(Boolean)

  if (cliConf['recursive'] !== true && (cliConf['filter'] || RECURSIVE_CMDS.has(cmd))) {
    cliConf['recursive'] = true
    if (subCmd && RECURSIVE_CMDS.has(cmd)) {
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
    subCmd,
    unknownOptions,
    workspaceDir,
  }
}
