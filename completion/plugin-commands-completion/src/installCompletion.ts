import renderHelp from 'render-help'
import { docsUrl } from '@pnpm/cli-utils'
import { install, SUPPORTED_SHELLS } from '@pnpm/tabtab'
import { getShellFromParams } from './getShell'

export const commandNames = ['install-completion']

export const rcOptionsTypes = () => ({})

export const cliOptionsTypes = () => ({})

export function help () {
  return renderHelp({
    description: 'Install shell completion into your system',
    url: docsUrl('install-completion'),
    usages: SUPPORTED_SHELLS.map(shell => `pnpm install-completion ${shell}`),
  })
}

export async function handler (_opts: unknown, params: string[]): Promise<void> {
  const shell = getShellFromParams(params)
  await install({ name: 'pnpm', completer: 'pnpm', shell })
}
