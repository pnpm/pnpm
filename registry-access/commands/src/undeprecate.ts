import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { renderHelp } from 'render-help'

import { cliOptionsTypes, type DeprecateOptions, parsePackageSpec, rcOptionsTypes, updateDeprecation } from './common.js'

export { cliOptionsTypes, rcOptionsTypes }

export const commandNames = ['undeprecate']

export function help (): string {
  return renderHelp({
    description: 'Removes deprecation from a version of a package in the registry. Only works on already deprecated versions.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'The base URL of the npm registry.',
            name: '--registry <url>',
          },
          {
            description: 'When publishing packages that require two-factor authentication, this option can specify a one-time password.',
            name: '--otp',
          },
        ],
      },
    ],
    url: docsUrl('undeprecate'),
    usages: [
      'pnpm undeprecate <package>[@<version>]',
    ],
  })
}

export async function handler (
  opts: DeprecateOptions,
  params: string[]
): Promise<void> {
  if (params.length === 0) {
    throw new PnpmError('UNDEPRECATE_REQUIRED', 'Package name is required')
  }

  if (params.length > 1) {
    throw new PnpmError('UNDEPRECATE_NO_MESSAGE', 'The undeprecate command does not accept a message.')
  }

  const { name, version } = parsePackageSpec(params[0])

  await updateDeprecation(name, version, '', opts, true)
}
