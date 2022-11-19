export interface Completion { name: string, description?: string }

export type CompletionFunc = (
  options: Record<string, unknown>,
  params: string[]
) => Promise<Completion[]>
