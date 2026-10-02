import type { CompletionFunc } from '@pnpm/cli.command'
import type { CompletionItem } from '@pnpm/tabtab'
import { findWorkspaceProjects } from '@pnpm/workspace.projects-reader'
import { findWorkspaceDir } from '@pnpm/workspace.root-finder'
import { readWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

import { getOptionCompletions } from './getOptionType.js'
import { optionTypesToCompletions } from './optionTypesToCompletions.js'

interface CompletionContext {
  cliOptionsTypesByCommandName: Record<string, () => Record<string, unknown>>
  completionByCommandName: Record<string, CompletionFunc>
  initialCompletion: () => CompletionItem[]
  shorthandsByCommandName: Record<string, Record<string, string | string[]>>
  universalOptionsTypes: Record<string, unknown>
  universalShorthands: Record<string, string>
}

interface CompletionInput {
  params: string[]
  cmd: string | null
  currentTypedWordType: 'option' | 'value' | null
  lastOption: string | null
  options: Record<string, unknown>
}

export async function complete (
  ctx: CompletionContext,
  input: CompletionInput
): Promise<CompletionItem[]> {
  if (input.options.version) return []
  const optionTypes = {
    ...ctx.universalOptionsTypes,
    ...((input.cmd && ctx.cliOptionsTypesByCommandName[input.cmd]?.()) ?? {}),
  }

  if (input.currentTypedWordType !== 'option') {
    const optionValueCompletions = await completeOptionValue(ctx, input, optionTypes)
    if (optionValueCompletions != null) return optionValueCompletions
  }
  const completions = input.currentTypedWordType === 'option' ? [] : await completeCommandParams(ctx, input)
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
    ...optionTypesToCompletions(optionTypes),
  ]
}

async function completeOptionValue (
  ctx: CompletionContext,
  input: CompletionInput,
  optionTypes: Record<string, unknown>
): Promise<CompletionItem[] | undefined> {
  if (input.lastOption === '--filter' || input.lastOption === '-F') {
    return completeWorkspaceProjectNames()
  }
  if (!input.lastOption) return undefined
  const optionCompletions = getOptionCompletions(
    optionTypes,
    {
      ...ctx.universalShorthands,
      ...(input.cmd ? ctx.shorthandsByCommandName[input.cmd] : {}),
    },
    input.lastOption
  )
  return optionCompletions?.map((name) => ({ name }))
}

async function completeWorkspaceProjectNames (): Promise<CompletionItem[]> {
  const workspaceDir = await findWorkspaceDir(process.cwd()) ?? process.cwd()
  const workspaceManifest = await readWorkspaceManifest(workspaceDir)
  const allProjects = await findWorkspaceProjects(workspaceDir, {
    patterns: workspaceManifest == null ? undefined : workspaceManifest.packages ?? ['.'],
    supportedArchitectures: {
      os: ['current'],
      cpu: ['current'],
      libc: ['current'],
    },
  })
  return allProjects
    .map(({ manifest }) => ({ name: manifest.name }))
    .filter((item): item is CompletionItem => !!item.name)
}

async function completeCommandParams (ctx: CompletionContext, input: CompletionInput): Promise<CompletionItem[]> {
  const commandCompletion = input.cmd ? ctx.completionByCommandName[input.cmd] : undefined
  if (!input.cmd || input.currentTypedWordType === 'value' && !commandCompletion) {
    return ctx.initialCompletion()
  }
  if (!commandCompletion) return []
  try {
    return await commandCompletion(input.options, input.params)
  } catch {
    return []
  }
}
