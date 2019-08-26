import { getLockfileImporterId } from '@pnpm/lockfile-file'
import semverDiff from '@pnpm/semver-diff'
import { DependenciesField, PackageJson, Registries } from '@pnpm/types'
import chalk from 'chalk'
import R = require('ramda')
import stripColor = require('strip-color')
import table = require('text-table')
import {
  outdatedDependenciesOfWorkspacePackages,
  OutdatedPackageWithVersionDiff,
  renderCurrent,
  renderLatest,
  renderPackageName,
  sortBySemverChange,
} from '../outdated'

const DEP_PRIORITY: Record<DependenciesField, number> = {
  dependencies: 1,
  devDependencies: 2,
  optionalDependencies: 0,
}

type OutdatedInWorkspace = {
  belongsTo: DependenciesField,
  current?: string,
  dependentPkgs: Array<{ location: string, manifest: PackageJson }>,
  latest?: string,
  packageName: string,
  wanted: string,
}

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
  const outdatedByNameAndType = {} as Record<string, OutdatedInWorkspace>
  if (opts.lockfileDirectory) {
    const outdatedPackagesByProject = await outdatedDependenciesOfWorkspacePackages(pkgs, args, opts)
    for (let { prefix, outdatedPackages, manifest } of outdatedPackagesByProject) {
      outdatedPackages.forEach((outdatedPkg) => {
        const key = JSON.stringify([outdatedPkg.packageName, outdatedPkg.belongsTo])
        if (!outdatedByNameAndType[key]) {
          outdatedByNameAndType[key] = { ...outdatedPkg, dependentPkgs: [] }
        }
        outdatedByNameAndType[key].dependentPkgs.push({ location: prefix, manifest })
      })
    }
  } else {
    await Promise.all(pkgs.map(async ({ manifest, path }) => {
      const { outdatedPackages } = (
        await outdatedDependenciesOfWorkspacePackages([{ manifest, path }], args, { ...opts, lockfileDirectory: path })
      )[0]
      outdatedPackages.forEach((outdatedPkg) => {
        const key = JSON.stringify([outdatedPkg.packageName, outdatedPkg.belongsTo])
        if (!outdatedByNameAndType[key]) {
          outdatedByNameAndType[key] = { ...outdatedPkg, dependentPkgs: [] }
        }
        outdatedByNameAndType[key].dependentPkgs.push({ location: getLockfileImporterId(opts.prefix, path), manifest })
      })
    }))
  }

  const columnNames = ['Package', 'Current', 'Latest', 'Dependents'].map((txt) => chalk.underline(txt))
  console.log(
    table([
      columnNames,
      ...R.sortWith(
        [
          (o1, o2) => sortBySemverChange(o1, o2),
          (o1, o2) => o1.packageName.localeCompare(o2.packageName),
          (o1, o2) => DEP_PRIORITY[o1.belongsTo] - DEP_PRIORITY[o2.belongsTo],
        ],
        (
          Object.values(outdatedByNameAndType)
            .map((outdatedPkg) => outdatedPkg.latest
              ? {
                ...outdatedPkg,
                ...semverDiff(outdatedPkg.wanted, outdatedPkg.latest)}
              : outdatedPkg,
            ) as Array<OutdatedInWorkspace & OutdatedPackageWithVersionDiff>
        ),
      )
        .map((outdatedPkg) => [
          renderPackageName(outdatedPkg),
          renderCurrent(outdatedPkg),
          renderLatest(outdatedPkg),
          outdatedPkg.dependentPkgs
            .map(({ manifest, location }) => manifest.name || location)
            .sort()
            .join(', '),
        ]),
    ], {
      stringLength: (s: string) => stripColor(s).length,
    }),
  )
}
