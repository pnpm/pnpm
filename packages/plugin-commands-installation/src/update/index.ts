import {
  docsUrl,
  readDepNameCompletions,
  readProjectManifestOnly,
} from '@pnpm/cli-utils'
import { CompletionFunc } from '@pnpm/command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import matcher from '@pnpm/matcher'
import { outdatedDepsOfProjects } from '@pnpm/outdated'
import { prompt } from 'enquirer'
import chalk from 'chalk'
import pick from 'ramda/src/pick'
import unnest from 'ramda/src/unnest'
import renderHelp from 'render-help'
import { InstallCommandOptions } from '../install'
import installDeps from '../installDeps'
import getUpdateChoices from './getUpdateChoices'

export function rcOptionsTypes () {
  return pick([
    'cache-dir',
    'depth',
    'dev',
    'engine-strict',
    'fetch-retries',
    'fetch-retry-factor',
    'fetch-retry-maxtimeout',
    'fetch-retry-mintimeout',
    'fetch-timeout',
    'force',
    'global-dir',
    'global-pnpmfile',
    'global',
    'https-proxy',
    'ignore-pnpmfile',
    'ignore-scripts',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'network-concurrency',
    'noproxy',
    'npmPath',
    'offline',
    'only',
    'optional',
    'package-import-method',
    'pnpmfile',
    'prefer-offline',
    'production',
    'proxy',
    'registry',
    'reporter',
    'save',
    'save-exact',
    'save-prefix',
    'save-workspace-protocol',
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
    recursive: Boolean,
    workspace: Boolean,
  }
}

export const shorthands = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['update', 'up', 'upgrade']

export const completion: CompletionFunc = async (cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export function help () {
  return renderHelp({
    aliases: ['up', 'upgrade'],
    description: 'Updates packages to their latest version based on the specified range. You can use "*" in package name to update all packages with the same pattern.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Update in every package found in subdirectories \
or every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Update globally installed packages',
            name: '--global',
            shortAlias: '-g',
          },
          {
            description: 'How deep should levels of dependencies be inspected. Infinity is default. 0 would mean top-level dependencies only',
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
            shortAlias: '-P',
          },
          {
            description: 'Update packages only in "devDependencies"',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: 'Don\'t update packages in "optionalDependencies"',
            name: '--no-optional',
          },
          {
            description: 'Tries to link all packages from the workspace. \
Versions are updated to match the versions of packages inside the workspace. \
If specific packages are updated, the command will fail if any of the updated \
dependencies is not found inside the workspace',
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

export type UpdateCommandOptions = InstallCommandOptions & {
  interactive?: boolean
  latest?: boolean
}

export async function handler (
  opts: UpdateCommandOptions,
  params: string[] = []
) {
  if (opts.interactive) {
    return interactiveUpdate(params, opts)
  }
  return update(params, opts)
}

async function interactiveUpdate (
  input: string[],
  opts: UpdateCommandOptions
) {
  const include = makeIncludeDependenciesFromCLI(opts.cliOptions)
  const projects = (opts.selectedProjectsGraph != null)
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
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })
  const choices = getUpdateChoices(unnest(outdatedPkgsOfProjects))
  if (choices.length === 0) {
    if (opts.latest) {
      return 'All of your dependencies are already up-to-date'
    }
    return 'All of your dependencies are already up-to-date inside the specified ranges. Use the --latest option to update the ranges in package.json'
  }
  const { updateDependencies } = await prompt({
    choices,
    footer: '\nEnter to start updating. Ctrl-c to cancel.',
    indicator (state: any, choice: any) { // eslint-disable-line @typescript-eslint/no-explicit-any
      return ` ${choice.enabled ? '●' : '○'}`
    },
    message: 'Choose which packages to update ' +
      `(Press ${chalk.cyan('<space>')} to select, ` +
      `${chalk.cyan('<a>')} to toggle all, ` +
      `${chalk.cyan('<i>')} to invert selection)`,
    name: 'updateDependencies',
    pointer: '❯',
    styles: {
      dark: chalk.white,
      em: chalk.bgBlack.whiteBright,
      success: chalk.white,
    },
    type: 'multiselect',
    validate (value: string[]) {
      if (value.length === 0) {
        return 'You must choose at least one package.'
      }
      return true
    },

    // For Vim users (related: https://github.com/enquirer/enquirer/pull/163)
    j () {
      return this.down()
    },
    k () {
      return this.up()
    },
  } as any) as any // eslint-disable-line @typescript-eslint/no-explicit-any
  return update(updateDependencies, opts)
}

async function update (
  dependencies: string[],
  opts: UpdateCommandOptions
) {
  const includeDirect = makeIncludeDependenciesFromCLI(opts.cliOptions)
  const include = {
    dependencies: opts.rawConfig.production !== false,
    devDependencies: opts.rawConfig.dev !== false,
    optionalDependencies: opts.rawConfig.optional !== false,
  }
  const depth = opts.depth ?? Infinity
  return installDeps({
    ...opts,
    allowNew: false,
    depth,
    includeDirect,
    include,
    update: true,
    updateMatching: (dependencies.length > 0) && dependencies.every(dep => !dep.substring(1).includes('@')) && depth > 0 && !opts.latest
      ? matcher(dependencies)
      : undefined,
    updatePackageManifest: opts.save !== false,
  }, dependencies)
}

function makeIncludeDependenciesFromCLI (opts: {
  production?: boolean
  dev?: boolean
  optional?: boolean
}) {
  return {
    dependencies: opts.production === true || (opts.dev !== true && opts.optional !== true),
    devDependencies: opts.dev === true || (opts.production !== true && opts.optional !== true),
    optionalDependencies: opts.optional === true || (opts.production !== true && opts.dev !== true),
  }
}
