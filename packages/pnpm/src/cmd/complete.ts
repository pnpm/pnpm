import { Completion, CompletionFunc } from '@pnpm/command'
import findWorkspaceDir from '@pnpm/find-workspace-dir'
import findWorkspacePackages from '@pnpm/find-workspace-packages'
import { getOptionCompletions } from '../getOptionType'
import optionTypesToCompletions from '../optionTypesToCompletions'
import universalShorthands from '../shorthands'

export default async function complete (
  ctx: {
    cliOptionsTypesByCommandName: Record<string, () => Object>
    completionByCommandName: Record<string, CompletionFunc>
    initialCompletion: () => Completion[]
    shorthandsByCommandName: Record<string, Record<string, string | string[]>>
    universalOptionsTypes: Record<string, Object>
  },
  input: {
    params: string[]
    cmd: string | null
    currentTypedWordType: 'option' | 'value' | null
    lastOption: string | null
    options: Record<string, unknown>
  }
) {
  if (input.options.version) return []
  const optionTypes = {
    ...ctx.universalOptionsTypes,
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
        optionTypes as any, // eslint-disable-line
        {
          ...universalShorthands,
          ...(input.cmd ? ctx.shorthandsByCommandName[input.cmd] : {}),
        },
        input.lastOption
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
        completions = await ctx.completionByCommandName[input.cmd](input.options, input.params)
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
    ...optionTypesToCompletions(optionTypes as any), // eslint-disable-line
  ]
}
