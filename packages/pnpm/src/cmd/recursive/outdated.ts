import { getLockfileImporterId } from '@pnpm/lockfile-file'
import { OutdatedPackage } from '@pnpm/outdated'
import { DependenciesField, PackageJson, Registries } from '@pnpm/types'
import chalk from 'chalk'
import { stripIndent } from 'common-tags'
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

interface OutdatedInWorkspace extends OutdatedPackage {
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
  let output: string

  if (opts.table !== false) {
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
      dependentPackages,
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
    output = sortOutdatedPackages(Object.values(outdatedByNameAndType))
      .map((outdatedPkg) => {
        let info = stripIndent`
          ${chalk.bold(renderPackageName(outdatedPkg))}
          ${renderCurrent(outdatedPkg)} ${chalk.grey('=>')} ${renderLatest(outdatedPkg)}`

        const dependents = dependentPackages(outdatedPkg)

        if (dependents) {
          info += `\n${chalk.bold(
              outdatedPkg.dependentPkgs.length > 1
                ? 'Dependents:'
                : 'Dependent:'
            )} ${dependents}`
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

function dependentPackages ({ dependentPkgs }: OutdatedInWorkspace) {
  return dependentPkgs
    .map(({ manifest, location }) => manifest.name || location)
    .sort()
    .join(', ')
}

function sortOutdatedPackages (outdatedPackages: ReadonlyArray<OutdatedInWorkspace>) {
  return R.sortWith(
    COMPARATORS,
    outdatedPackages.map(toOutdatedWithVersionDiff),
  )
}
