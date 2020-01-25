import {
  currentTypedWordType,
  getLastOption,
  getOptionCompletions,
  optionTypesToCompletions,
} from '@pnpm/cli-utils'
import { Completion, CompletionFunc } from '@pnpm/command'
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
    const { cliArgs, cliConf, cmd } = await parseCliArgs(inputArgv)
    const optionTypes = cliOptionsTypesByCommandName[cmd]?.()
    const currTypedWordType = currentTypedWordType(env)

    // Autocompleting option values
    if (optionTypes && currTypedWordType !== 'option') {
      const option = getLastOption(env)
      if (option) {
        const optionCompletions = getOptionCompletions(
          optionTypes as any, // tslint:disable-line
          shortHands,
          option,
        )
        if (optionCompletions !== undefined) {
          return tabtab.log(optionCompletions)
        }
      }
    }
    let completions: Completion[] = []
    if (currTypedWordType !== 'option') {
      if (!cmd || currTypedWordType === 'value' && !completionByCommandName[cmd]) {
        completions = defaultCompletions()
      } else if (completionByCommandName[cmd]) {
        completions = await completionByCommandName[cmd](cliArgs, cliConf)
      }
    }
    if (currTypedWordType !== 'value') {
      if (optionTypes) {
        completions = [
          ...completions,
          ...optionTypesToCompletions(optionTypes as any), // tslint:disable-line
        ]
      } else if (!cmd) {
        completions = [
          ...completions,
          { name: '--version' },
        ]
      }
    }

    return tabtab.log(completions)
  }

  function defaultCompletions () {
    return Object.keys(handlerByCommandName).map((name) => ({ name }))
  }
}
