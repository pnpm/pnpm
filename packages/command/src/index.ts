export type CompletionCtx = {
  last: string,
  lastPartial: string,
  line: string,
  partial: string,
  point: number,
  prev: string,
  words: number,
}

export type Completion = { name: string, description?: string }

export type CompletionFunc = {
  (
    ctx: CompletionCtx,
    args: string[],
    cliOpts: Record<string, unknown>,
  ): Promise<Completion[]>,
}
