import path = require('path')
import outdated, {
  forPackages as outdatedForPackages
} from '@pnpm/outdated'
import {PnpmOptions} from 'supi'
import table = require('text-table')
import chalk = require('chalk')
import stripColor = require('strip-color')

const LAYOUT_VERSION = '1'

export default async function (
  args: string[],
  opts: {
    prefix: string,
    global: boolean,
    independentLeaves: boolean,
    offline: boolean,
    store: string,
    proxy?: string,
    httpsProxy?: string,
    localAddress?: string,
    cert?: string,
    key?: string,
    ca?: string,
    strictSsl: boolean,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMintimeout: number,
    fetchRetryMaxtimeout: number,
    userAgent: string,
    tag: string,
    networkConcurrency: number,
    rawNpmConfig: Object,
    alwaysAuth: boolean,
  },
  command: string
) {
  let prefix: string
  if (opts.global) {
    prefix = path.join(opts.prefix, LAYOUT_VERSION)
    if (opts.independentLeaves) {
      prefix += '_independent_leaves'
    }
  } else {
    prefix = opts.prefix
  }

  const outdatedPkgs = args.length
    ? await outdatedForPackages(args, prefix, opts)
    : await outdated(prefix, opts)

  const columnNames = ['Package', 'Current', 'Wanted', 'Latest'].map(txt => chalk.underline(txt))
  console.log(
    table([columnNames].concat(
      outdatedPkgs.map(outdated => [
        chalk.yellow(outdated.packageName),
        outdated.current || 'missing',
        chalk.green(outdated.wanted),
        chalk.magenta(outdated.latest),
      ])
    ), {
      stringLength: (s: string) => stripColor(s).length,
    })
  )
}
