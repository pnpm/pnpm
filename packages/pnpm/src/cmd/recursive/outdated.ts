import { getLockfileImporterId } from '@pnpm/lockfile-file'
import { OutdatedPackage } from '@pnpm/outdated'
import { DependenciesField, PackageJson, Registries } from '@pnpm/types'
import chalk from 'chalk'
import R = require('ramda')
import { table } from 'table'
import {
  DEFAULT_COMPARATORS,
  getCellWidth,
  outdatedDependenciesOfWorkspacePackages,
  OutdatedOptions,
  renderCurrent,
  renderDetails,
  renderLatest,
  renderPackageName,
  TABLE_OPTIONS,
  toOutdatedWithVersionDiff,
} from '../outdated'

const DEP_PRIORITY: Record<DependenciesField, number> = {
  dependencies: 1,
  devDependencies: 2,
  optionalDependencies: 0,
}

const COMPARATORS = [
  ...DEFAULT_COMPARATORS,
  (o1: OutdatedInWorkspace, o2: OutdatedInWorkspace) => DEP_PRIORITY[o1.belongsTo] - DEP_PRIORITY[o2.belongsTo],
]

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
  opts: OutdatedOptions,
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

  // TODO: Try and de-duplicate the following code into ../outdated.ts

  let columnNames = [
    'Package',
    'Current',
    'Latest',
    'Dependents'
  ]

  let columnFns = [
    renderPackageName,
    renderCurrent,
    renderLatest,
    (outdatedPkg: OutdatedInWorkspace) => outdatedPkg.dependentPkgs
      .map(({ manifest, location }) => manifest.name || location)
      .sort()
      .join(', '),
  ]

  if (opts.long) {
    columnNames.push('Details')
    columnFns.push(renderDetails)
  }

  // Avoid the overhead of allocating a new array caused by calling `array.map()`
  for (let i = 0; i < columnNames.length; i++)
    columnNames[i] = chalk.blueBright(columnNames[i])

  const data = [
    columnNames,
    ...sortOutdatedPackages(Object.values(outdatedByNameAndType))
      .map((outdatedPkg) => columnFns.map((fn) => fn(outdatedPkg))),
  ]
  process.stdout.write(
    table(data, {
      ...TABLE_OPTIONS,
      columns: {
        ...TABLE_OPTIONS.columns,
        // Dependents column:
        3: {
          width: getCellWidth(data, 3, 30),
          wrapWord: true,
        },
      },
    }),
  )
}

function sortOutdatedPackages (outdatedPackages: ReadonlyArray<OutdatedInWorkspace>) {
  return R.sortWith(
    COMPARATORS,
    outdatedPackages.map(toOutdatedWithVersionDiff),
  )
}
