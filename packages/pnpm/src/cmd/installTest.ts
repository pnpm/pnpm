import { docsUrl } from '@pnpm/cli-utils'
import { install } from '@pnpm/plugin-commands-installation'
import { test } from '@pnpm/plugin-commands-script-runners'
import renderHelp = require('render-help')
import { PnpmOptions } from '../types'

export const cliOptionsTypes = install.cliOptionsTypes

export const rcOptionsTypes = install.rcOptionsTypes

export const commandNames = ['install-test', 'it']

export function help () {
  return renderHelp({
    aliases: ['it'],
    description: 'Runs a \`pnpm install\` followed immediately by a \`pnpm test\`. It takes exactly the same arguments as \`pnpm install\`.',
    url: docsUrl('install-test'),
    usages: ['pnpm install-test'],
  })
}

export async function handler (input: string[], opts: PnpmOptions) {
  await install.handler(input, opts)
  await test.handler(input, opts as any) // tslint:disable-line:no-any
}
