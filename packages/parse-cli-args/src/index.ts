import findWorkspaceDir from '@pnpm/find-workspace-dir'
import nopt = require('nopt')

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
  const noptExploratoryResults = nopt({ recursive: Boolean, filter: [String] }, { 'r': ['--recursive'] }, inputArgv, 0)

  const types = (() => {
    if (opts.getCommandLongName(noptExploratoryResults.argv.remain[0]) === 'recursive') {
      return {
        ...opts.globalOptionsTypes,
        ...opts.getTypesByCommandName('recursive'),
        ...opts.getTypesByCommandName(getCommandName(noptExploratoryResults.argv.remain.slice(1))),
      }
    }
    if (noptExploratoryResults['filter'] || noptExploratoryResults['recursive'] === true) {
      return {
        ...opts.globalOptionsTypes,
        ...opts.getTypesByCommandName('recursive'),
        ...opts.getTypesByCommandName(getCommandName(noptExploratoryResults.argv.remain)),
      }
    }
    return {
      ...opts.globalOptionsTypes,
      ...opts.getTypesByCommandName(getCommandName(noptExploratoryResults.argv.remain)),
    }
  })() as any // tslint:disable-line:no-any

  function getCommandName (cliArgs: string[]) {
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
    || 'help'
  if (!opts.isKnownCommand(cmd)) {
    cmd = 'help'
  }

  let subCmd: string | null = argv.remain[1] && opts.getCommandLongName(argv.remain[1])

  // `pnpm install ""` is going to be just `pnpm install`
  const cliArgs = argv.remain.slice(1).filter(Boolean)

  if (cmd !== 'recursive' && (cliConf['filter'] || cliConf['recursive'] === true)) {
    subCmd = cmd
    cmd = 'recursive'
    cliArgs.unshift(subCmd)
  } else if (subCmd && !opts.isKnownCommand(subCmd)) {
    subCmd = null
  }
  const dir = cliArgs['dir'] ?? process.cwd()
  const workspaceDir = cliArgs['global'] // tslint:disable-line
    ? undefined
    : await findWorkspaceDir(dir)

  if (
    (cmd === 'add' || cmd === 'install') &&
    typeof workspaceDir === 'string' &&
    cliArgs.length === 0
  ) {
    subCmd = cmd
    cmd = 'recursive'
    cliArgs.unshift(subCmd)
  }

  if (cmd === 'install' && cliArgs.length > 0) {
    cmd = 'add'
  } else if (subCmd === 'install' && cliArgs.length > 1) {
    subCmd = 'add'
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
    dir,
    subCmd,
    unknownOptions,
    workspaceDir,
  }
}
