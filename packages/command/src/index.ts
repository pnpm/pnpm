export type CompletionCtx = {
  last: string,
  lastPartial: string,
  line: string,
  partial: string,
  point: number,
  prev: string,
  words: number,
}

export type CompletionFunc = {
  (
    ctx: CompletionCtx,
    args: string[],
    cliOpts: Record<string, unknown>,
  ): Promise<Array<{ name: string, description?: string }>>,
}
