import { getCompletionScript, SUPPORTED_SHELLS, type SupportedShell } from '@pnpm/tabtab'
import { renderHelp } from 'render-help'

import { getShellFromParams } from './getShell.js'

export const commandNames = ['completion']

export const skipPackageManagerCheck = true

export const rcOptionsTypes = (): Record<string, unknown> => ({})

export const cliOptionsTypes = (): Record<string, unknown> => ({})

export function help (): string {
  return renderHelp({
    description: 'Print shell completion code to stdout',
    url: 'https://pnpm.io/completion',
    usages: SUPPORTED_SHELLS.map(shell => `pnpm completion ${shell}`),
  })
}

export interface Context {
  readonly log: (output: string) => void
}

export type CompletionGenerator = (_opts: unknown, params: string[]) => Promise<void>

const PNPM_COMMAND = 'pnpm'
const PNPM_SHORT_ALIAS = 'pn'

function registerShortAlias (output: string, shell: SupportedShell): string {
  switch (shell) {
    case 'bash':
      return output.replace(
        `complete -o default -F _${PNPM_COMMAND}_completion ${PNPM_COMMAND}`,
        `complete -o default -F _${PNPM_COMMAND}_completion ${PNPM_COMMAND} ${PNPM_SHORT_ALIAS}`
      )
    case 'fish':
      return output.replace(
        `complete -f -d '${PNPM_COMMAND}' -c ${PNPM_COMMAND} -a "(_${PNPM_COMMAND}_completion)"`,
        `complete -f -d '${PNPM_COMMAND}' -c ${PNPM_COMMAND} -a "(_${PNPM_COMMAND}_completion)"\ncomplete -f -d '${PNPM_COMMAND}' -c ${PNPM_SHORT_ALIAS} -a "(_${PNPM_COMMAND}_completion)"`
      )
    case 'pwsh':
      return output.replace(
        `Register-ArgumentCompleter -CommandName '${PNPM_COMMAND}' -ScriptBlock`,
        `Register-ArgumentCompleter -CommandName '${PNPM_COMMAND}','${PNPM_SHORT_ALIAS}' -ScriptBlock`
      )
    case 'zsh':
      return output
        .replace(`#compdef ${PNPM_COMMAND}`, `#compdef ${PNPM_COMMAND} ${PNPM_SHORT_ALIAS}`)
        .replace(
          `compdef _${PNPM_COMMAND}_completion ${PNPM_COMMAND}`,
          `compdef _${PNPM_COMMAND}_completion ${PNPM_COMMAND} ${PNPM_SHORT_ALIAS}`
        )
  }
}

export function createCompletionGenerator (ctx: Context): CompletionGenerator {
  return async function handler (_opts: unknown, params: string[]): Promise<void> {
    const shell = getShellFromParams(params)
    const output = await getCompletionScript({ name: PNPM_COMMAND, completer: PNPM_COMMAND, shell })
    ctx.log(registerShortAlias(output, shell))
  }
}

export const handler: CompletionGenerator = createCompletionGenerator({
  log: console.log,
})
