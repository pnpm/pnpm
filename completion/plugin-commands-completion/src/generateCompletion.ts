import renderHelp from 'render-help'
import { docsUrl } from '@pnpm/cli-utils'
import { getCompletionScript, SUPPORTED_SHELLS } from '@pnpm/tabtab'
import { getShellFromParams } from './getShell'

export const commandNames = ['completion']

export const rcOptionsTypes = (): Record<string, unknown> => ({})

export const cliOptionsTypes = (): Record<string, unknown> => ({})

export function help (): string {
  return renderHelp({
    description: 'Print shell completion code to stdout',
    url: docsUrl('completion'),
    usages: SUPPORTED_SHELLS.map(shell => `pnpm completion ${shell}`),
  })
}

export interface Context {
  readonly log: (output: string) => void
}

export type CompletionGenerator = (_opts: unknown, params: string[]) => Promise<void>

export function createCompletionGenerator (ctx: Context): CompletionGenerator {
  return async function handler (_opts: unknown, params: string[]): Promise<void> {
    const shell = getShellFromParams(params)
    const output = await getCompletionScript({ name: 'pnpm', completer: 'pnpm', shell })
    ctx.log(output)
  }
}

export const handler: CompletionGenerator = createCompletionGenerator({
  log: console.log,
})
