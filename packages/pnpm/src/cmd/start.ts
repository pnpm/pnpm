import { docsUrl } from '@pnpm/cli-utils'
import { oneLine } from 'common-tags'
import renderHelp = require('render-help')
import { handler as run } from './run'

export function types () {
  return {}
}

export const commandNames = ['start']

export function help () {
  return renderHelp({
    description: oneLine`
      Runs an arbitrary command specified in the package's "start" property of its "scripts" object.
      If no "start" property is specified on the "scripts" object, it will run node server.js.`,
    url: docsUrl('start'),
    usages: ['pnpm start [-- <args>...]'],
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
  return run(['start', ...args], opts)
}
