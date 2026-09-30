import type { CompletionItem } from '@pnpm/tabtab'

export type CompletionFunc = (
  options: Record<string, unknown>,
  params: string[]
) => Promise<CompletionItem[]>

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- each command declares its own option and result types, and the map holds them all
export type CommandHandler = (opts: any, params: string[], commands?: CommandHandlerMap) => any

export type CommandHandlerMap = Record<string, CommandHandler>
