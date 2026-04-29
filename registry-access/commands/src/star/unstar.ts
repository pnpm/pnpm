import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { renderHelp } from 'render-help'

import { cliOptionsTypes, performStarAction, rcOptionsTypes, type StarOptions } from './common.js'

export { cliOptionsTypes, rcOptionsTypes }

export const commandNames = ['unstar']

export function help (): string {
  return renderHelp({
    description: 'Removes a package from your favorites.',
    url: docsUrl('unstar'),
    usages: ['pnpm unstar <package>'],
  })
}

export async function handler (opts: StarOptions, params: string[]): Promise<void> {
  if (params.length === 0) {
    throw new PnpmError('UNSTAR_PACKAGE_REQUIRED', 'Package name is required')
  }
  await performStarAction(opts, { packageName: params[0], star: false })
}
