import {
  getOptionCompletions,
  optionTypesToCompletions,
} from '@pnpm/cli-utils'
import { Completion, CompletionFunc } from '@pnpm/command'
import findWorkspaceDir from '@pnpm/find-workspace-dir'
import findWorkspacePackages from '@pnpm/find-workspace-packages'
import shortHands from '../shorthands'

export default async function complete (
  ctx: {
    cliOptionsTypesByCommandName: Record<string, () => Object>,
    completionByCommandName: Record<string, CompletionFunc>,
    globalOptionTypes: Record<string, Object>,
    initialCompletion: () => Completion[],
  },
  input: {
    args: string[],
    cmd: string | null,
    currentTypedWordType: 'option' | 'value' | null,
    lastOption: string | null,
    options: Record<string, unknown>,
  },
) {
  if (input.options.version) return []
  const optionTypes = {
    ...ctx.globalOptionTypes,
    ...((input.cmd && ctx.cliOptionsTypesByCommandName[input.cmd]?.()) ?? {}),
  }

  // Autocompleting option values
  if (input.currentTypedWordType !== 'option') {
    if (input.lastOption === '--filter') {
      const workspaceDir = await findWorkspaceDir(process.cwd()) ?? process.cwd()
      const allProjects = await findWorkspacePackages(workspaceDir, {})
      return allProjects
        .filter(({ manifest }) => manifest.name)
        .map(({ manifest }) => ({ name: manifest.name }))
    } else if (input.lastOption) {
      const optionCompletions = getOptionCompletions(
        optionTypes as any, // tslint:disable-line
        shortHands,
        input.lastOption,
      )
      if (optionCompletions !== undefined) {
        return optionCompletions.map((name) => ({ name }))
      }
    }
  }
  let completions: Completion[] = []
  if (input.currentTypedWordType !== 'option') {
    if (!input.cmd || input.currentTypedWordType === 'value' && !ctx.completionByCommandName[input.cmd]) {
      completions = ctx.initialCompletion()
    } else if (ctx.completionByCommandName[input.cmd]) {
      try {
        completions = await ctx.completionByCommandName[input.cmd](input.args, input.options)
      } catch (err) {
        // Ignore
      }
    }
  }
  if (input.currentTypedWordType === 'value') {
    return completions
  }
  if (!input.cmd) {
    return [
      ...completions,
      ...optionTypesToCompletions(optionTypes),
      { name: '--version' },
    ]
  }
  return [
    ...completions,
    ...optionTypesToCompletions(optionTypes as any), // tslint:disable-line
  ]
}
