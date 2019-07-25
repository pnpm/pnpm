import {
  getLockfileImporterId,
  readCurrentLockfile,
  readWantedLockfile,
} from '@pnpm/lockfile-file'
import outdated, {
  forPackages as outdatedForPackages,
} from '@pnpm/outdated'
import storePath from '@pnpm/store-path'
import { PackageJson, Registries } from '@pnpm/types'
import chalk from 'chalk'
import stripColor = require('strip-color')
import table = require('text-table')
import createLatestVersionGetter from '../createLatestVersionGetter'
import { readImporterManifestOnly } from '../readImporterManifest'

export default async function (
  args: string[],
  opts: {
    alwaysAuth: boolean,
    ca?: string,
    cert?: string,
    engineStrict?: boolean,
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
  command: string,
) {
  const packages = [
    {
      manifest: await readImporterManifestOnly(opts.prefix, opts),
      path: opts.prefix,
    },
  ]
  const { outdatedPackages } = (await outdatedDependenciesOfWorkspacePackages(packages, args, opts))[0]

  if (!outdatedPackages.length) return

  const columnNames = ['Package', 'Current', 'Wanted', 'Latest'].map((txt) => chalk.underline(txt))
  console.log(
    table([columnNames].concat(
      outdatedPackages.map((outdatedPkg) => [
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

export async function outdatedDependenciesOfWorkspacePackages (
  pkgs: Array<{path: string, manifest: PackageJson}>,
  args: string[],
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
) {
  const lockfileDirectory = opts.lockfileDirectory || opts.prefix
  const currentLockfile = await readCurrentLockfile(lockfileDirectory, { ignoreIncompatible: false })
  const wantedLockfile = await readWantedLockfile(lockfileDirectory, { ignoreIncompatible: false }) || currentLockfile
  if (!wantedLockfile) {
    const err = new Error('No lockfile in this directory. Run `pnpm install` to generate one.')
    err['code'] = 'ERR_PNPM_OUTDATED_NO_LOCKFILE'
    throw err
  }
  const store = await storePath(opts.prefix, opts.store)
  const getLatestVersion = createLatestVersionGetter({
    ...opts,
    lockfileDirectory,
    store,
  })
  return Promise.all(pkgs.map(async ({ manifest, path }) => {
    const optsForOutdated = {
      currentLockfile,
      getLatestVersion,
      lockfileDirectory,
      manifest,
      prefix: path,
      wantedLockfile,
    }
    return {
      outdatedPackages: args.length
        ? await outdatedForPackages(args, optsForOutdated)
        : await outdated(optsForOutdated),
      prefix: getLockfileImporterId(lockfileDirectory, path),
    }
  }))
}
