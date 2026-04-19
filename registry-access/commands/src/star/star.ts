import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { renderHelp } from 'render-help'

import { cliOptionsTypes, performStarAction, rcOptionsTypes, type StarOptions } from './common.js'

export { cliOptionsTypes, rcOptionsTypes }

export const commandNames = ['star']

export function help (): string {
  return renderHelp({
    description: 'Marks a package as a favorite.',
    url: docsUrl('star'),
    usages: ['pnpm star <package>'],
  })
}

export async function handler (opts: StarOptions, params: string[]): Promise<void> {
  if (params.length === 0) {
    throw new PnpmError('STAR_PACKAGE_REQUIRED', 'Package name is required')
  }
  await performStarAction(opts, { packageName: params[0], star: true })
}
