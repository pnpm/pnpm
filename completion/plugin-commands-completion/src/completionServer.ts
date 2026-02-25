import { type CompletionItem, getShellFromEnv } from '@pnpm/tabtab'
import { type CompletionFunc } from '@pnpm/command'
import { split as splitCmd } from 'split-cmd/index.modern.mjs'
import tabtab from '@pnpm/tabtab'
import {
  currentTypedWordType,
  getLastOption,
} from './getOptionType.js'
import { type ParsedCliArgs } from '@pnpm/parse-cli-args'
import { complete } from './complete.js'

export function createCompletionServer (
  opts: {
    cliOptionsTypesByCommandName: Record<string, () => Record<string, unknown>>
    completionByCommandName: Record<string, CompletionFunc>
    initialCompletion: () => CompletionItem[]
    shorthandsByCommandName: Record<string, Record<string, string | string[]>>
    parseCliArgs: (args: string[]) => Promise<ParsedCliArgs>
    universalOptionsTypes: Record<string, unknown>
    universalShorthands: Record<string, string>
  }
): () => Promise<void> {
  return async () => {
    const shell = getShellFromEnv(process.env)

    const env = tabtab.parseEnv(process.env)
    if (!env.complete) return

    const inputArgv = splitCmd(stripPartialWord(env)).slice(1)
    // We cannot autocomplete what a user types after "pnpm test --"
    if (inputArgv.includes('--')) return
    const { params, options, cmd } = await opts.parseCliArgs(inputArgv)
    tabtab.log(
      await complete(
        opts,
        {
          cmd,
          currentTypedWordType: currentTypedWordType(env),
          lastOption: getLastOption(env),
          options,
          params,
        }
      ),
      shell
    )
  }
}

/**
 * Returns the portion of the command line that consists of fully typed words,
 */
function stripPartialWord (env: { partial: string, lastPartial: string }): string {
  if (env.lastPartial.length > 0) {
    // stripping any word the user is currently typing.
    return env.partial.slice(0, -env.lastPartial.length)
  }
  return env.partial
}