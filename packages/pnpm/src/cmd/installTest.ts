import { docsUrl } from '@pnpm/cli-utils'
import renderHelp = require('render-help')
import { PnpmOptions } from '../types'
import { handler as install, types } from './install'
import { handler as test } from './test'

export { types }

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
  await install(input, opts)
  await test(input, opts)
}
