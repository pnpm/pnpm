import { docsUrl } from '@pnpm/cli.utils'
import { PnpmError } from '@pnpm/error'
import { renderHelp } from 'render-help'

import { cliOptionsTypes, type DeprecateOptions, parsePackageSpec, rcOptionsTypes, updateDeprecation } from './common.js'

export { cliOptionsTypes, rcOptionsTypes }

export const commandNames = ['deprecate']

export function help (): string {
  return renderHelp({
    description: 'Deprecates a version of a package in the registry.',
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
    url: docsUrl('deprecate'),
    usages: [
      'pnpm deprecate <package>[@<version>] <message>',
    ],
  })
}

export async function handler (
  opts: DeprecateOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0) {
    throw new PnpmError('DEPRECATE_REQUIRED', 'Package name is required')
  }

  const packageSpec = params[0]
  const message = params.slice(1).join(' ')

  if (message === '') {
    throw new PnpmError('DEPRECATE_MESSAGE_REQUIRED', 'Deprecation message is required. To un-deprecate, use the undeprecate command.')
  }

  const { name, versionRange } = parsePackageSpec(packageSpec)

  return updateDeprecation(opts, { deprecate: true, message, packageName: name, versionRange })
}
