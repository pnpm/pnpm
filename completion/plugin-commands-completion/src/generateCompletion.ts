import renderHelp from 'render-help'
import { docsUrl } from '@pnpm/cli-utils'
import { getCompletionScript, SUPPORTED_SHELLS } from '@pnpm/tabtab'
import { getShellFromParams } from './getShell'

export const commandNames = ['completion']

export const rcOptionsTypes = () => ({})

export const cliOptionsTypes = () => ({})

export function help () {
  return renderHelp({
    description: 'Print shell completion code to stdout',
    url: docsUrl('completion'),
    usages: SUPPORTED_SHELLS.map(shell => `pnpm completion ${shell}`),
  })
}

export interface Context {
  readonly log: (output: string) => void
}

export function createCompletionGenerator (ctx: Context) {
  return async function handler (_opts: unknown, params: string[]): Promise<void> {
    const shell = getShellFromParams(params)
    const output = await getCompletionScript({ name: 'pnpm', completer: 'pnpm', shell })
    ctx.log(output)
  }
}

export const handler = createCompletionGenerator({
  log: console.log,
})
