import { docsUrl } from '@pnpm/cli-utils'
import renderHelp = require('render-help')
import { handler as run } from './run'

export function types () {
  return {}
}

export const commandNames = ['stop']

export function help () {
  return renderHelp({
    description: `Runs a package's "stop" script, if one was provided.`,
    url: docsUrl('stop'),
    usages: ['pnpm stop [-- <args>...]'],
  })
}

export async function handler (
  args: string[],
  opts: {
    extraBinPaths: string[],
    dir: string,
    rawConfig: object,
    argv: {
      cooked: string[],
      original: string[],
      remain: string[],
    },
  },
) {
  return run(['stop', ...args], opts)
}
