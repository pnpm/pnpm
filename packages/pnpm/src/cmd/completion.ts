import { Completion, CompletionFunc } from '@pnpm/command'
import { split as splitCmd } from 'split-cmd'
import {
  currentTypedWordType,
  getLastOption,
} from '../getOptionType'
import parseCliArgs from '../parseCliArgs'
import complete from './complete'
import tabtab = require('@pnpm/tabtab')

export default function (
  opts: {
    cliOptionsTypesByCommandName: Record<string, () => Object>
    completionByCommandName: Record<string, CompletionFunc>
    initialCompletion: () => Completion[]
    shorthandsByCommandName: Record<string, Record<string, string | string[]>>
    universalOptionsTypes: Record<string, Object>
  }
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
    const { params, options, cmd } = await parseCliArgs(inputArgv)
    return tabtab.log(
      await complete(
        opts,
        {
          cmd,
          currentTypedWordType: currentTypedWordType(env),
          lastOption: getLastOption(env),
          options,
          params,
        }
      )
    )
  }
}
