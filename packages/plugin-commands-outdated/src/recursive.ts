import { TABLE_OPTIONS } from '@pnpm/cli-utils'
import PnpmError from '@pnpm/error'
import { getLockfileImporterId } from '@pnpm/lockfile-file'
import matcher from '@pnpm/matcher'
import {
  outdatedDepsOfProjects,
  OutdatedPackage,
} from '@pnpm/outdated'
import {
  DependenciesField,
  IncludedDependencies,
  ProjectManifest,
} from '@pnpm/types'
import { filterDependenciesByType } from '@pnpm/utils'
import chalk = require('chalk')
import { stripIndent } from 'common-tags'
import R = require('ramda')
import { table } from 'table'
import {
  getCellWidth,
  OutdatedCommandOptions,
  renderCurrent,
  renderDetails,
  renderLatest,
  renderPackageName,
  toOutdatedWithVersionDiff,
} from './outdated'
import { DEFAULT_COMPARATORS } from './utils'

const DEP_PRIORITY: Record<DependenciesField, number> = {
  dependencies: 1,
  devDependencies: 2,
  optionalDependencies: 0,
}

const COMPARATORS = [
  ...DEFAULT_COMPARATORS,
  (o1: OutdatedInWorkspace, o2: OutdatedInWorkspace) =>
    DEP_PRIORITY[o1.belongsTo] - DEP_PRIORITY[o2.belongsTo],
]

interface OutdatedInWorkspace extends OutdatedPackage {
  belongsTo: DependenciesField,
  current?: string,
  dependentPkgs: Array<{ location: string, manifest: ProjectManifest }>,
  latest?: string,
  packageName: string,
  wanted: string,
}

export default async (
  pkgs: Array<{ dir: string, manifest: ProjectManifest }>,
  args: string[],
  opts: OutdatedCommandOptions & { include: IncludedDependencies },
) => {
  for (let input of args) {
    input = input.indexOf('@', 1) !== -1 ? input.substr(0, input.indexOf('@', 1)) : input
    let dependencies: string[] = []
    for (const { manifest } of pkgs) {
      dependencies = [...matchDependencies(matcher(input), manifest, opts.include), ...dependencies]
    }
    if (dependencies.length === 0) {
      throw new PnpmError('NO_PACKAGE_IN_DEPENDENCY', `No ${input} package found in dependencies of the project`)
    }
  }
  const outdatedMap = {} as Record<string, OutdatedInWorkspace>
  const outdatedPackagesByProject = await outdatedDepsOfProjects(pkgs, args, opts)
  for (let i = 0; i < outdatedPackagesByProject.length; i++) {
    const { dir, manifest } = pkgs[i]
    outdatedPackagesByProject[i].forEach((outdatedPkg) => {
      const key = JSON.stringify([outdatedPkg.packageName, outdatedPkg.current, outdatedPkg.belongsTo])
      if (!outdatedMap[key]) {
        outdatedMap[key] = { ...outdatedPkg, dependentPkgs: [] }
      }
      outdatedMap[key].dependentPkgs.push({ location: dir, manifest })
    })
  }

  if (R.isEmpty(outdatedMap)) return ''

  if (opts.table !== false) {
    return renderOutdatedTable(outdatedMap, opts)
  }
  return renderOutdatedList(outdatedMap, opts)
}

function renderOutdatedTable (outdatedMap: Record<string, OutdatedInWorkspace>, opts: { long?: boolean }) {
  let columnNames = [
    'Package',
    'Current',
    'Latest',
    'Dependents',
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
    ...sortOutdatedPackages(Object.values(outdatedMap))
      .map((outdatedPkg) => columnFns.map((fn) => fn(outdatedPkg))),
  ]
  return table(data, {
    ...TABLE_OPTIONS,
    columns: {
      ...TABLE_OPTIONS.columns,
      // Dependents column:
      3: {
        width: getCellWidth(data, 3, 30),
        wrapWord: true,
      },
    },
  })
}

function renderOutdatedList (outdatedMap: Record<string, OutdatedInWorkspace>, opts: { long?: boolean }) {
  return sortOutdatedPackages(Object.values(outdatedMap))
    .map((outdatedPkg) => {
      let info = stripIndent`
        ${chalk.bold(renderPackageName(outdatedPkg))}
        ${renderCurrent(outdatedPkg)} ${chalk.grey('=>')} ${renderLatest(outdatedPkg)}`

      const dependents = dependentPackages(outdatedPkg)

      if (dependents) {
        info += `\n${chalk.bold(
            outdatedPkg.dependentPkgs.length > 1
              ? 'Dependents:'
              : 'Dependent:',
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

export function matchDependencies (
  match: (input: string) => boolean,
  manifest: ProjectManifest,
  include: IncludedDependencies,
) {
  return Object.keys(filterDependenciesByType(manifest, include)).filter(match)
}

export function checkInputInDependencies (
  inputs: string[],
  manifest: ProjectManifest,
  include: IncludedDependencies,
) {
  for (const input of inputs) {
    if (!matchDependencies(matcher(inputs), manifest, include).length) {
      throw new PnpmError('NO_PACKAGE_IN_DEPENDENCY', `No ${input} package found in dependencies of the project`)
    }
  }
}
