import { PackageJson, Registries } from '@pnpm/types'
import chalk from 'chalk'
import stripColor = require('strip-color')
import table = require('text-table')
import { outdatedDependenciesOfWorkspacePackages } from '../outdated'

export default async (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
  cmd: string,
  opts: {
    alwaysAuth: boolean,
    ca?: string,
    cert?: string,
    fetchRetries: number,
    fetchRetryFactor: number,
    fetchRetryMaxtimeout: number,
    fetchRetryMintimeout: number,
    global: boolean,
    httpsProxy?: string,
    independentLeaves: boolean,
    key?: string,
    localAddress?: string,
    networkConcurrency: number,
    offline: boolean,
    prefix: string,
    proxy?: string,
    rawNpmConfig: object,
    registries: Registries,
    lockfileDirectory?: string,
    store?: string,
    strictSsl: boolean,
    tag: string,
    userAgent: string,
  },
) => {
  const outdatedPkgs = [] as Array<{
    packageName: string,
    current?: string,
    wanted: string,
    latest?: string,
    prefix: string,
  }>
  if (opts.lockfileDirectory) {
    const outdatedPackagesByProject = await outdatedDependenciesOfWorkspacePackages(pkgs, args, opts)
    for (let { prefix, outdatedPackages } of outdatedPackagesByProject) {
      outdatedPackages.forEach((outdatedPkg: any) => outdatedPkgs.push({ ...outdatedPkg, prefix })) // tslint:disable-line:no-any
    }
  } else {
    await Promise.all(pkgs.map(async ({ manifest, path }) => {
      const { outdatedPackages } = (
        await outdatedDependenciesOfWorkspacePackages([{ manifest, path }], args, { ...opts, lockfileDirectory: path })
      )[0]
      outdatedPackages.forEach((outdatedPkg: any) => outdatedPkgs.push({ ...outdatedPkg, prefix: path })) // tslint:disable-line:no-any
    }))
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
