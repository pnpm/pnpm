import {
  docsUrl,
  readDepNameCompletions,
  readProjectManifestOnly,
} from '@pnpm/cli-utils'
import { type CompletionFunc } from '@pnpm/command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { globalInfo } from '@pnpm/logger'
import { createMatcher } from '@pnpm/matcher'
import { outdatedDepsOfProjects } from '@pnpm/outdated'
import { PnpmError } from '@pnpm/error'
import { type IncludedDependencies, type ProjectRootDir } from '@pnpm/types'
import { prompt } from 'enquirer'
import chalk from 'chalk'
import pick from 'ramda/src/pick'
import pluck from 'ramda/src/pluck'
import unnest from 'ramda/src/unnest'
import renderHelp from 'render-help'
import { type InstallCommandOptions } from '../install'
import { installDeps } from '../installDeps'
import { type ChoiceRow, getUpdateChoices } from './getUpdateChoices'
import { parseUpdateParam } from '../recursive'
export function rcOptionsTypes (): Record<string, unknown> {
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
    'lockfile-include-tarball-url',
    'network-concurrency',
    'noproxy',
    'npm-path',
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
    'scripts-prepend-node-path',
    'shamefully-flatten',
    'shamefully-hoist',
    'shared-workspace-lockfile',
    'side-effects-cache-readonly',
    'side-effects-cache',
    'store-dir',
    'unsafe-perm',
    'use-running-store-server',
  ], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    interactive: Boolean,
    latest: Boolean,
    recursive: Boolean,
    workspace: Boolean,
  }
}

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['update', 'up', 'upgrade']

export const completion: CompletionFunc = async (cliOpts) => {
  return readDepNameCompletions(cliOpts.dir as string)
}

export function help (): string {
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
): Promise<string | undefined> {
  if (opts.interactive) {
    return interactiveUpdate(params, opts)
  }
  return update(params, opts) as Promise<undefined>
}

async function interactiveUpdate (
  input: string[],
  opts: UpdateCommandOptions
): Promise<string | undefined> {
  const include = makeIncludeDependenciesFromCLI(opts.cliOptions)
  const projects = (opts.selectedProjectsGraph != null)
    ? Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
    : [
      {
        rootDir: opts.dir as ProjectRootDir,
        manifest: await readProjectManifestOnly(opts.dir, opts),
      },
    ]
  const rootDir = opts.workspaceDir ?? opts.dir
  const rootProject = projects.find((project) => project.rootDir === rootDir)
  const outdatedPkgsOfProjects = await outdatedDepsOfProjects(projects, input, {
    ...opts,
    compatible: opts.latest !== true,
    ignoreDependencies: rootProject?.manifest?.pnpm?.updateConfig?.ignoreDependencies,
    include,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })
  const workspacesEnabled = !!opts.workspaceDir
  const choices = getUpdateChoices(unnest(outdatedPkgsOfProjects), workspacesEnabled)
  if (choices.length === 0) {
    if (opts.latest) {
      return 'All of your dependencies are already up to date'
    }
    return 'All of your dependencies are already up to date inside the specified ranges. Use the --latest option to update the ranges in package.json'
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
    result () {
      return this.selected
    },
    format () {
      if (!this.state.submitted || this.state.cancelled) return ''

      if (Array.isArray(this.selected)) {
        return this.selected
          // The custom format function is used to filter out "[dependencies]" or "[devDependencies]" from the output.
          // https://github.com/enquirer/enquirer/blob/master/lib/prompts/select.js#L98
          .filter((choice: ChoiceRow) => !/^\[.+\]$/.test(choice.name))
          .map((choice: ChoiceRow) => this.styles.primary(choice.name)).join(', ')
      }
      return this.styles.primary(this.selected.name)
    },
    styles: {
      dark: chalk.reset,
      em: chalk.bgBlack.whiteBright,
      success: chalk.reset,
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
    cancel () {
      // By default, canceling the prompt via Ctrl+c throws an empty string.
      // The custom cancel function prevents that behavior.
      // Otherwise, pnpm CLI would print an error and confuse users.
      // See related issue: https://github.com/enquirer/enquirer/issues/225
      globalInfo('Update canceled')
      process.exit(0)
    },
  } as any) as any // eslint-disable-line @typescript-eslint/no-explicit-any

  const updatePkgNames = pluck('value', updateDependencies as ChoiceRow[])
  return update(updatePkgNames, opts) as Promise<undefined>
}

async function update (
  dependencies: string[],
  opts: UpdateCommandOptions
): Promise<void> {
  if (opts.latest) {
    const dependenciesWithTags = dependencies.filter((name) => parseUpdateParam(name).versionSpec != null)
    if (dependenciesWithTags.length) {
      throw new PnpmError('LATEST_WITH_SPEC', `Specs are not allowed to be used with --latest (${dependenciesWithTags.join(', ')})`)
    }
  }
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
    ignoreCurrentPrefs: false,
    includeDirect,
    include,
    update: true,
    updateToLatest: opts.latest,
    updateMatching: (dependencies.length > 0) && dependencies.every(dep => !dep.substring(1).includes('@')) && depth > 0 && !opts.latest
      ? createMatcher(dependencies)
      : undefined,
    updatePackageManifest: opts.save !== false,
    resolutionMode: opts.save === false ? 'highest' : opts.resolutionMode,
  }, dependencies)
}

function makeIncludeDependenciesFromCLI (opts: {
  production?: boolean
  dev?: boolean
  optional?: boolean
}): IncludedDependencies {
  return {
    dependencies: opts.production === true || (opts.dev !== true && opts.optional !== true),
    devDependencies: opts.dev === true || (opts.production !== true && opts.optional !== true),
    optionalDependencies: opts.optional === true || (opts.production !== true && opts.dev !== true),
  }
}
