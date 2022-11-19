import { TABLE_OPTIONS } from '@pnpm/cli-utils'
import { PnpmError } from '@pnpm/error'
import {
  outdatedDepsOfProjects,
  OutdatedPackage,
} from '@pnpm/outdated'
import {
  DependenciesField,
  IncludedDependencies,
  ProjectManifest,
} from '@pnpm/types'
import { table } from '@zkochan/table'
import chalk from 'chalk'
import isEmpty from 'ramda/src/isEmpty'
import sortWith from 'ramda/src/sortWith'
import {
  getCellWidth,
  OutdatedCommandOptions,
  OutdatedPackageJSONOutput,
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
  belongsTo: DependenciesField
  current?: string
  dependentPkgs: Array<{ location: string, manifest: ProjectManifest }>
  latest?: string
  packageName: string
  wanted: string
}

export async function outdatedRecursive (
  pkgs: Array<{ dir: string, manifest: ProjectManifest }>,
  params: string[],
  opts: OutdatedCommandOptions & { include: IncludedDependencies }
) {
  const outdatedMap = {} as Record<string, OutdatedInWorkspace>
  const rootManifest = pkgs.find(({ dir }) => dir === opts.lockfileDir ?? opts.dir)
  const outdatedPackagesByProject = await outdatedDepsOfProjects(pkgs, params, {
    ...opts,
    fullMetadata: opts.long,
    ignoreDependencies: new Set(rootManifest?.manifest?.pnpm?.updateConfig?.ignoreDependencies ?? []),
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })
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

  let output!: string
  switch (opts.format ?? 'table') {
  case 'table': {
    output = renderOutdatedTable(outdatedMap, opts)
    break
  }
  case 'list': {
    output = renderOutdatedList(outdatedMap, opts)
    break
  }
  case 'json': {
    output = renderOutdatedJSON(outdatedMap, opts)
    break
  }
  default: {
    throw new PnpmError('BAD_OUTDATED_FORMAT', `Unsupported format: ${opts.format?.toString() ?? 'undefined'}`)
  }
  }
  return {
    output,
    exitCode: isEmpty(outdatedMap) ? 0 : 1,
  }
}

function renderOutdatedTable (outdatedMap: Record<string, OutdatedInWorkspace>, opts: { long?: boolean }) {
  if (isEmpty(outdatedMap)) return ''
  const columnNames = [
    'Package',
    'Current',
    'Latest',
    'Dependents',
  ]

  const columnFns = [
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
  if (isEmpty(outdatedMap)) return ''
  return sortOutdatedPackages(Object.values(outdatedMap))
    .map((outdatedPkg) => {
      let info = `${chalk.bold(renderPackageName(outdatedPkg))}
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

export interface OutdatedPackageInWorkspaceJSONOutput extends OutdatedPackageJSONOutput {
  dependentPackages: Array<{ name: string, location: string }>
}

function renderOutdatedJSON (
  outdatedMap: Record<string, OutdatedInWorkspace>,
  opts: { long?: boolean }
): string {
  const outdatedPackagesJSON: Record<string, OutdatedPackageInWorkspaceJSONOutput> = sortOutdatedPackages(Object.values(outdatedMap))
    .reduce((acc, outdatedPkg) => {
      acc[outdatedPkg.packageName] = {
        current: outdatedPkg.current,
        latest: outdatedPkg.latestManifest?.version,
        wanted: outdatedPkg.wanted,
        isDeprecated: Boolean(outdatedPkg.latestManifest?.deprecated),
        dependencyType: outdatedPkg.belongsTo,
        dependentPackages: outdatedPkg.dependentPkgs.map(({ manifest, location }) => ({ name: manifest.name, location })),
      }
      if (opts.long) {
        acc[outdatedPkg.packageName].latestManifest = outdatedPkg.latestManifest
      }
      return acc
    }, {})
  return JSON.stringify(outdatedPackagesJSON, null, 2)
}

function dependentPackages ({ dependentPkgs }: OutdatedInWorkspace) {
  return dependentPkgs
    .map(({ manifest, location }) => manifest.name ?? location)
    .sort()
    .join(', ')
}

function sortOutdatedPackages (outdatedPackages: readonly OutdatedInWorkspace[]) {
  return sortWith(
    COMPARATORS,
    outdatedPackages.map(toOutdatedWithVersionDiff)
  )
}
