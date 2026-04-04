import type { CommandHandler, CommandHandlerMap, CompletionFunc } from '@pnpm/cli.command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import {
  docsUrl,
  readDepNameCompletions,
  readProjectManifestOnly,
} from '@pnpm/cli.utils'
import { createMatcher } from '@pnpm/config.matcher'
import { types as allTypes } from '@pnpm/config.reader'
import { outdatedDepsOfProjects } from '@pnpm/deps.inspection.outdated'
import { PnpmError } from '@pnpm/error'
import { handleGlobalUpdate } from '@pnpm/global.commands'
import type { UpdateMatchingFunction } from '@pnpm/installing.deps-installer'
import { globalInfo } from '@pnpm/logger'
import type { IncludedDependencies, PackageVulnerabilityAudit, ProjectRootDir } from '@pnpm/types'
import chalk from 'chalk'
import enquirer from 'enquirer'
import { pick, pluck, unnest } from 'ramda'
import { renderHelp } from 'render-help'

import type { InstallCommandOptions } from '../install.js'
import { installDeps } from '../installDeps.js'
import { parseUpdateParam } from '../recursive.js'
import { type ChoiceRow, getUpdateChoices } from './getUpdateChoices.js'
export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'cache-dir',
    'dangerously-allow-all-builds',
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
    'shamefully-hoist',
    'shared-workspace-lockfile',
    'side-effects-cache-readonly',
    'side-effects-cache',
    'store-dir',
    'unsafe-perm',
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
  packageVulnerabilityAudit?: PackageVulnerabilityAudit
}

export async function handler (
  opts: UpdateCommandOptions,
  params: string[] = [],
  commands?: CommandHandlerMap
): Promise<string | undefined> {
  if (opts.global) {
    if (!opts.bin) {
      throw new PnpmError('NO_GLOBAL_BIN_DIR', 'Unable to find the global bin directory', {
        hint: 'Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH.',
      })
    }
    return handleGlobalUpdate(opts, params, commands ?? {})
  }
  const rebuildHandler = commands?.rebuild
  if (opts.interactive) {
    return interactiveUpdate(params, opts, rebuildHandler)
  }
  return update(params, opts, rebuildHandler) as Promise<undefined>
}

async function interactiveUpdate (
  input: string[],
  opts: UpdateCommandOptions,
  rebuildHandler?: CommandHandler
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
  const outdatedPkgsOfProjects = await outdatedDepsOfProjects(projects, input, {
    ...opts,
    compatible: opts.latest !== true,
    ignoreDependencies: opts.updateConfig?.ignoreDependencies,
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
  const { updateDependencies } = await enquirer.prompt({
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
  return update(updatePkgNames, opts, rebuildHandler) as Promise<undefined>
}

async function update (
  dependencies: string[],
  opts: UpdateCommandOptions,
  rebuildHandler?: CommandHandler
): Promise<void> {
  if (opts.latest) {
    const dependenciesWithTags = dependencies.filter((name) => parseUpdateParam(name).versionSpec != null)
    if (dependenciesWithTags.length) {
      throw new PnpmError('LATEST_WITH_SPEC', `Specs are not allowed to be used with --latest (${dependenciesWithTags.join(', ')})`)
    }
  }
  const includeDirect = makeIncludeDependenciesFromCLI(opts.cliOptions)
  // Use cliOptions for include: only exclude dep types the user explicitly
  // passed via CLI (e.g., --no-optional). Derived flags like dev=false from
  // --prod should NOT change include, as that would conflict with the
  // modules directory state (which was installed with all dep types).
  const include = {
    dependencies: opts.cliOptions.production !== false,
    devDependencies: opts.cliOptions.dev !== false,
    optionalDependencies: opts.cliOptions.optional !== false,
  }
  const depth = opts.depth ?? Infinity
  let updateMatching: UpdateMatchingFunction | undefined
  if (opts.packageVulnerabilityAudit != null) {
    const { packageVulnerabilityAudit } = opts
    updateMatching = (pkgName: string, version?: string) => version != null && packageVulnerabilityAudit.isVulnerable(pkgName, version)
  } else if (
    (dependencies.length > 0) && dependencies.every(dep => !dep.substring(1).includes('@')) && depth > 0 && !opts.latest
  ) {
    updateMatching = createMatcher(dependencies)
  }
  return installDeps({
    ...opts,
    rebuildHandler,
    allowNew: false,
    depth,
    ignoreCurrentSpecifiers: false,
    include,
    includeDirect,
    update: true,
    updateToLatest: opts.latest,
    updateMatching,
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
