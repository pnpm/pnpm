import type { CompletionItem } from '@pnpm/tabtab'

export type CompletionFunc = (
  options: Record<string, unknown>,
  params: string[]
) => Promise<CompletionItem[]>

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type CommandHandler = (opts: any, params: string[], commands?: CommandHandlerMap) => any

export type CommandHandlerMap = Record<string, CommandHandler>
