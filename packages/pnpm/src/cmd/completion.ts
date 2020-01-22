import { optionTypesToCompletions } from '@pnpm/cli-utils'
import { CompletionFunc } from '@pnpm/command'
import { split as splitCmd } from 'split-cmd'
import tabtab = require('tabtab')
import handlerByCommandName from '.'
import { parseCliArgs } from '../main'

export default function (
  completionByCommandName: Record<string, CompletionFunc>,
  cliOptionsTypesByCommandName: Record<string, () => Object>,
) {
  return async () => {
    const env = tabtab.parseEnv(process.env)
    if (!env.complete) return

    const inputArgv = splitCmd(env.line).slice(1)
    const { argv, cliArgs, cliConf, cmd, subCmd, unknownOptions, workspaceDir } = await parseCliArgs(inputArgv)

    if (completionByCommandName[cmd]) {
      return tabtab.log(
        await completionByCommandName[cmd](env, cliArgs, cliConf),
      )
    }

    if (cliOptionsTypesByCommandName[cmd]) {
      return tabtab.log(
        optionTypesToCompletions(cliOptionsTypesByCommandName[cmd]() as any), // tslint:disable-line
      )
    }

    return tabtab.log([
      '--version',
      ...Object.keys(handlerByCommandName),
    ])
  }
}
