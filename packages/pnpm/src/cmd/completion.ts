import {
  getLastOption,
  getOptionCompletions,
  optionTypesToCompletions,
} from '@pnpm/cli-utils'
import { CompletionFunc } from '@pnpm/command'
import { split as splitCmd } from 'split-cmd'
import tabtab = require('tabtab')
import handlerByCommandName from '.'
import { parseCliArgs, shortHands } from '../main'

export default function (
  completionByCommandName: Record<string, CompletionFunc>,
  cliOptionsTypesByCommandName: Record<string, () => Object>,
) {
  return async () => {
    const env = tabtab.parseEnv(process.env)
    if (!env.complete) return

    const inputArgv = splitCmd(env.line).slice(1)
    const { argv, cliArgs, cliConf, cmd, subCmd, unknownOptions, workspaceDir } = await parseCliArgs(inputArgv)
    const optionTypes = cliOptionsTypesByCommandName[cmd]?.()
    const option = getLastOption(env)
    if (optionTypes && (env.partial.endsWith(' ') || !env.lastPartial.startsWith('-')) && option) {
      const optionCompletions = getOptionCompletions(
        optionTypes as any, // tslint:disable-line
        shortHands,
        option,
      )
      if (optionCompletions !== undefined) {
        return tabtab.log(optionCompletions)
      }
    }
    if (completionByCommandName[cmd]) {
      return tabtab.log(
        await completionByCommandName[cmd](env, cliArgs, cliConf),
      )
    }

    if (optionTypes) {
      return tabtab.log(
        optionTypesToCompletions(optionTypes as any), // tslint:disable-line
      )
    }

    return tabtab.log([
      '--version',
      ...Object.keys(handlerByCommandName),
    ])
  }
}
