import {
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import outdated, {
  forPackages as outdatedForPackages,
} from '@pnpm/outdated'
import { Registries } from '@pnpm/types'
import { pickRegistryForPackage } from '@pnpm/utils'
import chalk from 'chalk'
import stripColor = require('strip-color')
import table = require('text-table')
import createResolver from '../createResolver'
import { readImporterManifestOnly } from '../readImporterManifest'
import { PnpmOptions } from '../types'

export default async function (
  args: string[],
  opts: PnpmOptions & {
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
    store: string,
    strictSsl: boolean,
    tag: string,
    userAgent: string,
  },
  command: string,
) {
  const lockfileDirectory = opts.lockfileDirectory || opts.prefix
  const currentLockfile = await readCurrentLockfile(lockfileDirectory, { ignoreIncompatible: false })
  const wantedLockfile = await readWantedLockfile(lockfileDirectory, { ignoreIncompatible: false }) || currentLockfile
  if (!wantedLockfile) {
    const err = new Error('No lockfile in this directory. Run `pnpm install` to generate one.')
    err['code'] = 'ERR_PNPM_OUTDATED_NO_LOCKFILE'
    throw err
  }
  const resolve = createResolver(opts)
  async function getLatestVersion (packageName: string) {
    const resolution = await resolve({ alias: packageName, pref: 'latest' }, {
      lockfileDirectory,
      preferredVersions: {},
      prefix: opts.prefix,
      registry: pickRegistryForPackage(opts.registries, packageName),
    })
    return resolution && resolution.latest || null
  }
  const manifest = await readImporterManifestOnly(opts.prefix)
  const optsForOutdated = {
    currentLockfile,
    getLatestVersion,
    lockfileDirectory,
    manifest,
    prefix: opts.prefix,
    wantedLockfile,
  }
  const outdatedPkgs = args.length
    ? await outdatedForPackages(args, optsForOutdated)
    : await outdated(optsForOutdated)

  if (!outdatedPkgs.length) return

  const columnNames = ['Package', 'Current', 'Wanted', 'Latest'].map((txt) => chalk.underline(txt))
  console.log(
    table([columnNames].concat(
      outdatedPkgs.map((outdatedPkg) => [
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
