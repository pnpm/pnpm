import { type CompletionItem } from '@pnpm/tabtab'

export type CompletionFunc = (
  options: Record<string, unknown>,
  params: string[]
) => Promise<CompletionItem[]>
