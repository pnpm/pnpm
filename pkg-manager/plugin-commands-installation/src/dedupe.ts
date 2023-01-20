import { docsUrl } from '@pnpm/cli-utils'
import { UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import renderHelp from 'render-help'
import * as install from './install'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {}
}

export const commandNames = ['dedupe']

export function help () {
  return renderHelp({
    description: 'Perform an install removing older dependencies in the lockfile if a newer version can be used.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          ...UNIVERSAL_OPTIONS,
        ],
      },
    ],
    url: docsUrl('dedupe'),
    usages: ['pnpm dedupe'],
  })
}

export async function handler (
  opts: install.InstallCommandOptions
) {
  return install.handler({
    ...opts,
    dedupe: true,
  })
}
