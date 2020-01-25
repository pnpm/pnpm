export type Completion = { name: string, description?: string }

export type CompletionFunc = {
  (
    args: string[],
    cliOpts: Record<string, unknown>,
  ): Promise<Completion[]>,
}
