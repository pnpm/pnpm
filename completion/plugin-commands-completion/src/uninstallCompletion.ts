import renderHelp from 'render-help'
import { docsUrl } from '@pnpm/cli-utils'
import { PnpmError } from '@pnpm/error'
import { uninstall } from '@pnpm/tabtab'

export const commandNames = ['uninstall-completion']

export const rcOptionsTypes = () => ({})

export const cliOptionsTypes = () => ({})

export function help () {
  return renderHelp({
    description: 'Uninstall the completion scripts from your system, from all shells that is supported by pnpm',
    url: docsUrl('uninstall-completion'),
    usages: ['pnpm uninstall-completion'],
  })
}

export async function handler (_opts: unknown, params: string[]): Promise<void> {
  if (params.length) {
    throw new PnpmError('REDUNDANT_PARAMETERS', '`pnpm uninstall-completion` does not take any parameter.')
  }

  await uninstall({ name: 'pnpm' })
}
