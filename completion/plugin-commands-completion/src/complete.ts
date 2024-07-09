import { type CompletionItem } from '@pnpm/tabtab'
import { type CompletionFunc } from '@pnpm/command'
import { findWorkspaceDir } from '@pnpm/find-workspace-dir'
import { findWorkspacePackages } from '@pnpm/workspace.find-packages'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { getOptionCompletions } from './getOptionType'
import { optionTypesToCompletions } from './optionTypesToCompletions'

export async function complete (
  ctx: {
    cliOptionsTypesByCommandName: Record<string, () => Record<string, unknown>>
    completionByCommandName: Record<string, CompletionFunc>
    initialCompletion: () => CompletionItem[]
    shorthandsByCommandName: Record<string, Record<string, string | string[]>>
    universalOptionsTypes: Record<string, unknown>
    universalShorthands: Record<string, string>
  },
  input: {
    params: string[]
    cmd: string | null
    currentTypedWordType: 'option' | 'value' | null
    lastOption: string | null
    options: Record<string, unknown>
  }
): Promise<CompletionItem[]> {
  if (input.options.version) return []
  const optionTypes = {
    ...ctx.universalOptionsTypes,
    ...((input.cmd && ctx.cliOptionsTypesByCommandName[input.cmd]?.()) ?? {}),
  }

  // Autocompleting option values
  if (input.currentTypedWordType !== 'option') {
    if (input.lastOption === '--filter') {
      const workspaceDir = await findWorkspaceDir(process.cwd()) ?? process.cwd()
      const workspaceManifest = await readWorkspaceManifest(workspaceDir)
      const allProjects = await findWorkspacePackages(workspaceDir, {
        patterns: workspaceManifest?.packages,
        supportedArchitectures: {
          os: ['current'],
          cpu: ['current'],
          libc: ['current'],
        },
      })
      return allProjects
        .map(({ manifest }) => ({ name: manifest.name }))
        .filter((item): item is CompletionItem => !!item.name)
    } else if (input.lastOption) {
      const optionCompletions = getOptionCompletions(
        optionTypes as any, // eslint-disable-line
        {
          ...ctx.universalShorthands,
          ...(input.cmd ? ctx.shorthandsByCommandName[input.cmd] : {}),
        },
        input.lastOption
      )
      if (optionCompletions !== undefined) {
        return optionCompletions.map((name) => ({ name }))
      }
    }
  }
  let completions: CompletionItem[] = []
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
