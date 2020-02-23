import {
  docsUrl,
  optionTypesToCompletions,
  readDepNameCompletions,
  readProjectManifestOnly,
  TABLE_OPTIONS,
} from '@pnpm/cli-utils'
import colorizeSemverDiff from '@pnpm/colorize-semver-diff'
import { CompletionFunc } from '@pnpm/command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import {
  outdatedDepsOfProjects,
  OutdatedPackage,
} from '@pnpm/outdated'
import semverDiff from '@pnpm/semver-diff'
import chalk = require('chalk')
import { oneLine, stripIndent } from 'common-tags'
import R = require('ramda')
import renderHelp = require('render-help')
import stripAnsi = require('strip-ansi')
import { table } from 'table'
import wrapAnsi = require('wrap-ansi')
import outdatedRecursive from './recursive'
import {
  DEFAULT_COMPARATORS,
  OutdatedWithVersionDiff,
} from './utils'

export const rcOptionsTypes = cliOptionsTypes

export function cliOptionsTypes () {
  return {
    ...R.pick([
      'depth',
      'dev',
      'global-dir',
      'global',
      'long',
      'optional',
      'production',
      'recursive',
    ], allTypes),
    'compatible': Boolean,
    'table': Boolean,
  }
}

export const shorthands = {
  'D': '--dev',
  'P': '--production',
}

export const commandNames = ['outdated']

export function help () {
  return renderHelp({
    description: stripIndent`
      Check for outdated packages. The check can be limited to a subset of the installed packages by providing arguments (patterns are supported).

      Examples:
      pnpm outdated
      pnpm outdated --long
      pnpm outdated gulp-* @babel/core`,
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Print only versions that satisfy specs in package.json',
            name: '--compatible',
          },
          {
            description: oneLine`
            By default, details about the outdated packages (such as a link to the repo) are not displayed.
            To display the details, pass this option.`,
            name: '--long',
          },
          {
            description: oneLine`
              Check for outdated dependencies in every package found in subdirectories
              or in every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Prints the outdated packages in a list. Good for small consoles',
            name: '--no-table',
          },
          {
            description: 'Check only "dependencies" and "optionalDependencies"',
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Check only "devDependencies"',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: `Don't check "optionalDependencies"`,
            name: '--no-optional',
          },
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('outdated'),
    usages: ['pnpm outdated [<pkg> ...]'],
  })
}

export const completion: CompletionFunc = (args, cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export type OutdatedCommandOptions = {
  compatible?: boolean,
  long?: boolean
  recursive?: boolean,
  table?: boolean,
} & Pick<Config, 'allProjects' |
  'alwaysAuth' |
  'ca' |
  'cert' |
  'dev' |
  'dir' |
  'engineStrict' |
  'fetchRetries' |
  'fetchRetryFactor' |
  'fetchRetryMaxtimeout' |
  'fetchRetryMintimeout' |
  'global' |
  'httpsProxy' |
  'independentLeaves' |
  'key' |
  'localAddress' |
  'lockfileDir' |
  'networkConcurrency' |
  'offline' |
  'optional' |
  'production' |
  'proxy' |
  'rawConfig' |
  'registries' |
  'selectedProjectsGraph' |
  'storeDir' |
  'strictSsl' |
  'tag' |
  'userAgent'
>

export async function handler (
  args: string[],
  opts: OutdatedCommandOptions,
) {
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  if (opts.recursive && opts.selectedProjectsGraph) {
    const pkgs = Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
    return outdatedRecursive(pkgs, args, { ...opts, include })
  }
  const packages = [
    {
      dir: opts.dir,
      manifest: await readProjectManifestOnly(opts.dir, opts),
    },
  ]
  const [ outdatedPackages ] = (await outdatedDepsOfProjects(packages, args, { ...opts, include }))

  if (!outdatedPackages.length) return ''

  if (opts.table !== false) {
    return renderOutdatedTable(outdatedPackages, opts)
  } else {
    return renderOutdatedList(outdatedPackages, opts)
  }
}

function renderOutdatedTable (outdatedPackages: ReadonlyArray<OutdatedPackage>, opts: { long?: boolean }) {
  let columnNames = [
    'Package',
    'Current',
    'Latest',
  ]

  let columnFns = [
    renderPackageName,
    renderCurrent,
    renderLatest,
  ]

  if (opts.long) {
    columnNames.push('Details')
    columnFns.push(renderDetails)
  }

  // Avoid the overhead of allocating a new array caused by calling `array.map()`
  for (let i = 0; i < columnNames.length; i++)
    columnNames[i] = chalk.blueBright(columnNames[i])

  return table([
    columnNames,
    ...sortOutdatedPackages(outdatedPackages)
      .map((outdatedPkg) => columnFns.map((fn) => fn(outdatedPkg))),
  ], TABLE_OPTIONS)
}

function renderOutdatedList (outdatedPackages: ReadonlyArray<OutdatedPackage>, opts: { long?: boolean }) {
  return sortOutdatedPackages(outdatedPackages)
    .map((outdatedPkg) => {
      let info = stripIndent`
        ${chalk.bold(renderPackageName(outdatedPkg))}
        ${renderCurrent(outdatedPkg)} ${chalk.grey('=>')} ${renderLatest(outdatedPkg)}`

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

function sortOutdatedPackages (outdatedPackages: ReadonlyArray<OutdatedPackage>) {
  return R.sortWith(
    DEFAULT_COMPARATORS,
    outdatedPackages.map(toOutdatedWithVersionDiff),
  )
}

export function getCellWidth (data: string[][], columnNumber: number, maxWidth: number) {
  const maxCellWidth = data.reduce((cellWidth, row) => {
    const cellLines = stripAnsi(row[columnNumber]).split('\n')
    const currentCellWidth = cellLines.reduce((lineWidth, line) => {
      return Math.max(lineWidth, line.length)
    }, 0)
    return Math.max(cellWidth, currentCellWidth)
  }, 0)
  return Math.min(maxWidth, maxCellWidth)
}

export function toOutdatedWithVersionDiff<T> (outdated: T & OutdatedPackage): T & OutdatedWithVersionDiff {
  if (outdated.latestManifest) {
    return {
      ...outdated,
      ...semverDiff(outdated.wanted, outdated.latestManifest.version),
    }
  }
  return {
    ...outdated,
    change: 'unknown',
  }
}

export function renderPackageName ({ belongsTo, packageName }: OutdatedPackage) {
  switch (belongsTo) {
    case 'devDependencies': return `${packageName} ${chalk.dim('(dev)')}`
    case 'optionalDependencies': return `${packageName} ${chalk.dim('(optional)')}`
    default: return packageName
  }
}

export function renderCurrent ({ current, wanted }: OutdatedPackage) {
  let output = current || 'missing'
  if (current === wanted) return output
  return `${output} (wanted ${wanted})`
}

export function renderLatest (outdatedPkg: OutdatedWithVersionDiff): string {
  const { latestManifest, change, diff } = outdatedPkg
  if (!latestManifest) return ''
  if (change === null || !diff) {
    return latestManifest.deprecated
      ? chalk.redBright.bold('Deprecated')
      : latestManifest.version
  }

  return colorizeSemverDiff({ change, diff })
}

export function renderDetails ({ latestManifest }: OutdatedPackage) {
  if (!latestManifest) return ''
  const outputs = []
  if (latestManifest.deprecated) {
    outputs.push(wrapAnsi(chalk.redBright(latestManifest.deprecated), 40))
  }
  if (latestManifest.homepage) {
    outputs.push(chalk.underline(latestManifest.homepage))
  }
  return outputs.join('\n')
}
