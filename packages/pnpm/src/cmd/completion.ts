import {
  currentTypedWordType,
  getLastOption,
} from '@pnpm/cli-utils'
import { Completion, CompletionFunc } from '@pnpm/command'
import { split as splitCmd } from 'split-cmd'
import tabtab = require('tabtab')
import parseCliArgs from '../parseCliArgs'
import complete from './complete'

export default function (
  opts: {
    cliOptionsTypesByCommandName: Record<string, () => Object>,
    completionByCommandName: Record<string, CompletionFunc>,
    globalOptionTypes: Record<string, Object>,
    initialCompletion: () => Completion[],
  },
) {
  return async () => {
    const env = tabtab.parseEnv(process.env)
    if (!env.complete) return

    // Parse only words that are before the pointer and finished.
    // Finished means that there's at least one space between the word and pointer
    const finishedArgv = env.partial.substr(0, env.partial.length - env.lastPartial.length)
    const inputArgv = splitCmd(finishedArgv).slice(1)
    // We cannot autocomplete what a user types after "pnpm test --"
    if (inputArgv.includes('--')) return
    const { cliArgs, cliConf, cmd } = await parseCliArgs(inputArgv)
    return tabtab.log(
      await complete(
        opts,
        {
          args: cliArgs,
          cmd,
          currentTypedWordType: currentTypedWordType(env),
          lastOption: getLastOption(env),
          options: cliConf,
        },
      ),
    )
  }
}
