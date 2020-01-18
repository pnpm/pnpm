import { docsUrl, readProjectManifestOnly } from '@pnpm/cli-utils'
import colorizeSemverDiff from '@pnpm/colorize-semver-diff'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import PnpmError from '@pnpm/error'
import { outdatedDepsOfProjects, OutdatedPackage } from '@pnpm/outdated'
import semverDiff from '@pnpm/semver-diff'
import chalk = require('chalk')
import { oneLine } from 'common-tags'
import { prompt } from 'enquirer'
import R = require('ramda')
import renderHelp = require('render-help')
import { handler as install, InstallCommandOptions } from './install'

export function rcOptionsTypes () {
  return R.pick([
    'depth',
    'dev',
    'engine-strict',
    'force',
    'global-dir',
    'global-pnpmfile',
    'global',
    'ignore-pnpmfile',
    'ignore-scripts',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'npmPath',
    'offline',
    'only',
    'optional',
    'package-import-method',
    'pnpmfile',
    'prefer-offline',
    'production',
    'recursive',
    'registry',
    'reporter',
    'resolution-strategy',
    'save',
    'save-exact',
    'shamefully-flatten',
    'shamefully-hoist',
    'shared-workspace-lockfile',
    'side-effects-cache-readonly',
    'side-effects-cache',
    'store',
    'store-dir',
    'unsafe-perm',
    'use-running-store-server',
  ], allTypes)
}

export function cliOptionsTypes () {
  return {
    ...rcOptionsTypes(),
    interactive: Boolean,
    latest: Boolean,
    workspace: Boolean,
  }
}

export const commandNames = ['update', 'up', 'upgrade']

export function help () {
  return renderHelp({
    aliases: ['up', 'upgrade'],
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: oneLine`Update in every package found in subdirectories
              or every workspace package, when executed inside a workspace.
              For options that may be used with \`-r\`, see "pnpm help recursive"`,
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Update globally installed packages',
            name: '--global',
            shortAlias: '-g',
          },
          {
            description: 'How deep should levels of dependencies be inspected. 0 is default, which means top-level dependencies',
            name: '--depth <number>',
          },
          {
            description: 'Ignore version ranges in package.json',
            name: '--latest',
            shortAlias: '-L',
          },
          {
            description: 'Update packages only in "dependencies" and "optionalDependencies"',
            name: '--prod',
          },
          {
            description: 'Update packages only in "devDependencies"',
            name: '--dev',
          },
          {
            description: `Don't update packages in "optionalDependencies"`,
            name: '--no-optional',
          },
          {
            description:  oneLine`Tries to link all packages from the workspace.
              Versions are updated to match the versions of packages inside the workspace.
              If specific packages are updated, the command will fail if any of the updated
              dependencies is not found inside the workspace`,
            name: '--workspace',
          },
          {
            description: 'Show outdated dependencies and select which ones to update',
            name: '--interactive',
            shortAlias: '-i',
          },
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('update'),
    usages: ['pnpm update [-g] [<pkg>...]'],
  })
}

export async function handler (
  input: string[],
  opts: InstallCommandOptions & { interactive?: boolean },
) {
  if (opts.interactive) {
    const include = {
      dependencies: opts.production !== false,
      devDependencies: opts.dev !== false,
      optionalDependencies: opts.optional !== false,
    }
    const projects = opts.selectedProjectsGraph
      ? Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
      : [
        {
          dir: opts.dir,
          manifest: await readProjectManifestOnly(opts.dir, opts),
        },
      ]
    const outdatedPkgsOfProjects = await outdatedDepsOfProjects(projects, input, {
      ...opts,
      compatible: opts.latest !== true,
      include,
    })
    const allOutdatedPkgs: Record<string, OutdatedPackage> = {}
    outdatedPkgsOfProjects
      .map(({ outdatedPackages }) => outdatedPackages)
      .flat()
      .forEach((outdatedPkg) => {
        const key = JSON.stringify([
          outdatedPkg.packageName,
          outdatedPkg.latestManifest?.version,
          outdatedPkg.current,
        ])
        if (!allOutdatedPkgs[key]) {
          allOutdatedPkgs[key] = outdatedPkg
          return
        }
        if (allOutdatedPkgs[key].belongsTo === 'dependencies') return
        if (outdatedPkg.belongsTo !== 'devDependencies') {
          allOutdatedPkgs[key].belongsTo = outdatedPkg.belongsTo
        }
      })
    const outdatedPackages = Object.values(allOutdatedPkgs)

    if (outdatedPackages.length === 0) {
      if (opts.latest) {
        return 'All of your dependencies are already up-to-date'
      }
      return 'All of your dependencies are already up-to-date inside the specified ranges. Use the --latest option to update the ranges in package.json'
    }
    const outdatedPackagesByType = R.groupBy(R.prop('belongsTo'), outdatedPackages)
    const choices = Object.entries(outdatedPackagesByType)
      .map(([depType, outdatedPkgs]) => ({
        choices: Object.entries(R.groupBy(R.prop('packageName'), outdatedPkgs))
          .map(([packageName, outdatedPkgs]) => {
            const message = outdatedPkgs
              .map((outdatedPkg) => {
                const sdiff = semverDiff(outdatedPkg.wanted, outdatedPkg.latestManifest!.version)
                const nextVersion = sdiff.change === null
                  ? outdatedPkg.latestManifest!.version
                  : colorizeSemverDiff(sdiff as any) // tslint:disable-line:no-any
                return `${outdatedPkg.packageName} ${outdatedPkg.current} ❯ ${nextVersion}`
              }).join('\n    ')
            return {
              message,
              name: packageName,
            }
          }),
        name: depType,
      }))
    const { updateDependencies } = await prompt({
      choices,
      footer: '\nEnter to start updating. Ctrl-c to cancel.',
      message: `Choose which packages to update ` +
        `(Press ${chalk.cyan('<space>')} to select, ` +
        `${chalk.cyan('<a>')} to toggle all, ` +
        `${chalk.cyan('<i>')} to invert selection)`,
      name: 'updateDependencies',
      type: 'multiselect',
      validate (value: string[]) {
        if (value.length === 0) {
          return 'You must choose at least one package.'
        }
        return true
      },
    } as any) // tslint:disable-line:no-any
    return install(updateDependencies, { ...opts, update: true, allowNew: false })
  }
  return install(input, { ...opts, update: true, allowNew: false })
}
