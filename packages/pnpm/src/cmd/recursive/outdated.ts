import logger from '@pnpm/logger'
import outdated, {
  forPackages as outdatedForPackages,
} from '@pnpm/outdated'
import {PackageJson} from '@pnpm/types'
import chalk from 'chalk'
import path = require('path')
import stripColor = require('strip-color')
import table = require('text-table')

export default async (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  cmd: string,
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
    rawNpmConfig: object,
    alwaysAuth: boolean,
  },
) => {
  const outdatedPkgs = [] as Array<{
    packageName: string,
    current?: string,
    wanted: string,
    latest?: string,
    prefix: string,
  }>
  const getOutdated = args.length ? outdatedForPackages.bind(null, args) : outdated
  for (const pkg of pkgs) {
    let outdatedPackagesOfProject
    try {
      outdatedPackagesOfProject = await getOutdated(pkg.path, opts)
    } catch (err) {
      logger.info(err)
      err['prefix'] = pkg.path // tslint:disable-line:no-string-literal
      throw err
    }
    const prefix = path.relative(opts.prefix, pkg.path)
    outdatedPackagesOfProject.forEach((outdatedPkg: any) => outdatedPkgs.push({...outdatedPkg, prefix})) // tslint:disable-line:no-any
  }

  const columnNames = ['', 'Package', 'Current', 'Wanted', 'Latest'].map((txt) => chalk.underline(txt))
  console.log(
    table([columnNames].concat(
      outdatedPkgs.map((outdatedPkg) => [
        outdatedPkg.prefix,
        chalk.yellow(outdatedPkg.packageName),
        outdatedPkg.current || 'missing',
        chalk.green(outdatedPkg.wanted),
        chalk.magenta(outdatedPkg.latest || ''),
      ]),
    ), {
      stringLength: (s: string) => stripColor(s).length,
    }),
  )
}
