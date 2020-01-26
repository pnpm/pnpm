import {
  currentTypedWordType,
  getLastOption,
  getOptionCompletions,
  optionTypesToCompletions,
} from '@pnpm/cli-utils'
import { Completion, CompletionFunc } from '@pnpm/command'
import findWorkspaceDir from '@pnpm/find-workspace-dir'
import findWorkspacePackages from '@pnpm/find-workspace-packages'
import { split as splitCmd } from 'split-cmd'
import tabtab = require('tabtab')
import handlerByCommandName from '.'
import { parseCliArgs, shortHands } from '../main'

export default function (
  completionByCommandName: Record<string, CompletionFunc>,
  cliOptionsTypesByCommandName: Record<string, () => Object>,
  globalOptionTypes: Record<string, Object>,
) {
  return async () => {
    const env = tabtab.parseEnv(process.env)
    if (!env.complete) return

    // Parse only words that are before the pointer and finished.
    // Finished means that there's at least one space between the word and pointer
    const finishedArgv = env.partial.substr(0, env.partial.length - env.lastPartial.length)
    const inputArgv = splitCmd(finishedArgv).slice(1)
    const { cliArgs, cliConf, cmd } = await parseCliArgs(inputArgv)
    const optionTypes = {
      ...globalOptionTypes,
      ...(cliOptionsTypesByCommandName[cmd]?.() ?? {}),
    }
    const currTypedWordType = currentTypedWordType(env)

    // Autocompleting option values
    if (currTypedWordType !== 'option') {
      const option = getLastOption(env)
      if (option === '--filter') {
        const workspaceDir = await findWorkspaceDir(process.cwd()) ?? process.cwd()
        const allProjects = await findWorkspacePackages(workspaceDir, {})
        return tabtab.log(
          allProjects
            .filter(({ manifest }) => manifest.name)
            .map(({ manifest }) => ({ name: manifest.name })),
        )
      } else if (option) {
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
      if (!cmd) {
        completions = [
          ...completions,
          ...optionTypesToCompletions(optionTypes),
          { name: '--version' },
        ]
      } else {
        completions = [
          ...completions,
          ...optionTypesToCompletions(optionTypes as any), // tslint:disable-line
        ]
      }
    }

    return tabtab.log(completions)
  }

  function defaultCompletions () {
    return Object.keys(handlerByCommandName).map((name) => ({ name }))
  }
}
