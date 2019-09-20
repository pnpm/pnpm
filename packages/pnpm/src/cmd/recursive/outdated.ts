import { getLockfileImporterId } from '@pnpm/lockfile-file'
import { OutdatedPackage } from '@pnpm/outdated'
import { DependenciesField, PackageJson, Registries } from '@pnpm/types'
import chalk from 'chalk'
import { stripIndent } from 'common-tags'
import R = require('ramda')
import { table } from 'table'
import {
  getCellWidth,
  outdatedDependenciesOfWorkspacePackages,
  renderCurrent,
  renderDetails,
  renderLatest,
  renderPackageName,
  sortBySemverChange,
  TABLE_OPTIONS,
  toOutdatedWithVersionDiff,
} from '../outdated'

const DEP_PRIORITY: Record<DependenciesField, number> = {
  dependencies: 1,
  devDependencies: 2,
  optionalDependencies: 0,
}

type OutdatedInWorkspace = OutdatedPackage & {
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
    long?: boolean,
    networkConcurrency: number,
    offline: boolean,
    prefix: string,
    proxy?: string,
    rawNpmConfig: object,
    registries: Registries,
    lockfileDirectory?: string,
    store?: string,
    strictSsl: boolean,
    table?: boolean,
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

  let output

  if (opts.table !== false) {
    let columnNames = [
      'Package',
      'Current',
      'Latest',
      'Dependents'
    ]

    if (opts.long) {
      columnNames.push('Details')
    }

    columnNames = columnNames.map((name: string) => chalk.blueBright(name))
    const data = [
      columnNames,
      ...sortedPackages(outdatedByNameAndType)
        .map((outdatedPkg) => [
          renderPackageName(outdatedPkg),
          renderCurrent(outdatedPkg),
          renderLatest(outdatedPkg),
          dependentPackages(outdatedPkg)
        ].concat(opts.long ? [renderDetails(outdatedPkg)] : [])),
    ]
    output = table(data, {
      ...TABLE_OPTIONS,
      columns: {
        ...TABLE_OPTIONS.columns,
        // Dependents column:
        3: {
          width: getCellWidth(data, 3, 30)
        },
      },
    })
  } else {
    output = sortedPackages(outdatedByNameAndType)
      .map((outdatedPkg) => {
        let info = stripIndent`
          ${renderPackageName(outdatedPkg)}
          ${renderCurrent(outdatedPkg)} => ${renderLatest(outdatedPkg)}`

        const dependents = dependentPackages(outdatedPkg)

        if (dependents) {
          info += `\n${dependents}`
        }

        if (opts.long) {
          const details = renderDetails(outdatedPkg)

          if (details) {
            info += `\n${details}`
          }
        }

        return info
      })
      .join('\n\n') + '\n'
  }

  process.stdout.write(output)
}

function dependentPackages (outdatedPkg: OutdatedInWorkspace) {
  return outdatedPkg.dependentPkgs
    .map(({ manifest, location }) => manifest.name || location)
    .sort()
    .join('\n')
}

function sortedPackages (outdatedByNameAndType: Record<string, OutdatedInWorkspace>) {
  return R.sortWith(
    [
      sortBySemverChange,
      (o1, o2) => o1.packageName.localeCompare(o2.packageName),
      (o1, o2) => DEP_PRIORITY[o1.belongsTo] - DEP_PRIORITY[o2.belongsTo],
    ],
    (
      Object.values(outdatedByNameAndType).map(toOutdatedWithVersionDiff)
    )
  )
}
