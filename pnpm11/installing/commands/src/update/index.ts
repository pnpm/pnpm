import { checkbox, Separator } from '@inquirer/prompts'
import type { CommandHandler, CommandHandlerMap, CompletionFunc } from '@pnpm/cli.command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import {
  docsUrl,
  interactivePromptPageSize,
  readDepNameCompletions,
  readProjectManifestOnly,
} from '@pnpm/cli.utils'
import { createMatcher } from '@pnpm/config.matcher'
import { types as allTypes } from '@pnpm/config.reader'
import { findOutdatedGitHubActions, isGitHubActionSelector, normalizeGitHubActionSelector, updateGitHubActions } from '@pnpm/deps.github-actions'
import { outdatedDepsOfProjects } from '@pnpm/deps.inspection.outdated'
import { PnpmError } from '@pnpm/error'
import { handleGlobalUpdate } from '@pnpm/global.commands'
import type { UpdateMatchingFunction } from '@pnpm/installing.deps-installer'
import { globalInfo } from '@pnpm/logger'
import type { IncludedDependencies, PackageVulnerabilityAudit, ProjectRootDir } from '@pnpm/types'
import chalk from 'chalk'
import { pick, unnest } from 'ramda'
import { renderHelp } from 'render-help'

import type { InstallCommandOptions } from '../install.js'
import { createVulnerabilityUpdateMatching, installDeps } from '../installDeps.js'
import { parseUpdateParam } from '../recursive.js'
import { createGlobalPolicyCallbacks } from '../resolutionPolicyManifest.js'
import { captureUpdateChangesetContext, generateUpdateChangeset } from './generateUpdateChangeset.js'
import { getUpdateChoices } from './getUpdateChoices.js'
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
    'node-experimental-package-map',
    'node-package-map-type',
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
    'minimum-release-age',
    'minimum-release-age-exclude',
    'store-dir',
    'unsafe-perm',
  ], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    changeset: Boolean,
    'include-github-actions': Boolean,
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
    description: 'Updates package dependencies to their latest version based on the specified range. GitHub Actions dependencies can be included with --include-github-actions. You can use "*" in a dependency name to update all dependencies with the same pattern.',
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
          {
            description: 'Generate a changeset file declaring a patch bump for every workspace package whose production dependencies were changed by the update',
            name: '--changeset',
          },
          {
            description: 'Also update GitHub Actions dependencies in workflow and action files',
            name: '--include-github-actions',
          },
          {
            description: 'Don\'t update the ranges in package.json.',
            name: '--no-save',
          },
          OPTIONS.globalDir,
          OPTIONS.minimumReleaseAge,
          OPTIONS.minimumReleaseAgeExclude,
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
  changeset?: boolean
  include?: IncludedDependencies
  includeGithubActions?: boolean
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
    return handleGlobalUpdate({
      ...opts,
      ...createGlobalPolicyCallbacks(opts),
    }, params, commands ?? {})
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
  const packageInput = input.filter((selector) => !isGitHubActionSelector(selector))
  const [outdatedPkgsOfProjects, outdatedActions] = await Promise.all([
    input.length === 0 || packageInput.length > 0
      ? outdatedDepsOfProjects(projects, packageInput, {
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
      : projects.map(() => []),
    include.devDependencies && opts.save !== false && !opts.lockfileOnly
      ? findOutdatedGitHubActions({
        compatible: opts.latest !== true,
        dir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
        match: input.length > 0 ? createMatcher(input.map(normalizeGitHubActionSelector)) : undefined,
      })
      : [],
  ])
  const workspacesEnabled = !!opts.workspaceDir
  const choiceGroups = getUpdateChoices([
    ...unnest(outdatedPkgsOfProjects),
    ...outdatedActions.map((action) => ({
      alias: action.name,
      belongsTo: 'devDependencies' as const,
      current: action.current,
      dependencyType: 'githubAction' as const,
      latestManifest: { name: action.name, version: action.latest, homepage: action.homepage },
      packageName: action.name,
      wanted: action.wanted,
    })),
  ], workspacesEnabled)
  if (choiceGroups.length === 0) {
    if (opts.latest) {
      return 'All of your dependencies are already up to date'
    }
    return 'All of your dependencies are already up to date inside the specified ranges. Use the --latest option to update the ranges in package.json'
  }

  const flatChoices: Array<Separator | { name: string; value: string; short: string; disabled?: boolean | string }> = []
  for (const group of choiceGroups) {
    flatChoices.push(new Separator(chalk.bold(`── ${group.message} ──`)))
    for (const choice of group.choices) {
      if (choice.disabled) {
        flatChoices.push(new Separator(`  ${choice.message ?? choice.name}`))
      } else {
        flatChoices.push({
          name: choice.message,
          value: choice.value,
          // `name` is the rendered table row (label + versions + workspace + url)
          // that lays out a single choice during selection. After submission
          // @inquirer/prompts comma-joins each choice's `short`, which without
          // this defaults to `name` and dumps the whole table back to stdout.
          short: choice.value,
        })
      }
    }
  }

  const message = 'Choose which dependencies to update ' +
    `(Press ${chalk.cyan('<space>')} to select, ` +
    `${chalk.cyan('<a>')} to toggle all, ` +
    `${chalk.cyan('<i>')} to invert selection)\n\nEnter to start updating. Ctrl-c to cancel.`
  let updatePkgNames: string[]
  try {
    updatePkgNames = await checkbox({
      choices: flatChoices,
      pageSize: interactivePromptPageSize(),
      message,
      required: true,
      validate: (values) => {
        if (values.length === 0) {
          return 'You must choose at least one dependency.'
        }
        return true
      },
      theme: {
        icon: { checked: '●', unchecked: '○', cursor: '❯' },
        style: {
          highlight: (text: string) => text,
        },
        keybindings: ['vim'],
      },
    })
  } catch (err) {
    if (err instanceof Error && err.name === 'ExitPromptError') {
      globalInfo('Update canceled')
      process.exit(0)
    }
    throw err
  }

  return update(updatePkgNames, { ...opts, includeGithubActions: true }, rebuildHandler) as Promise<undefined>
}

async function update (
  dependencies: string[],
  opts: UpdateCommandOptions,
  rebuildHandler?: CommandHandler
): Promise<void> {
  const includeDirect = makeIncludeDependenciesFromCLI(opts.cliOptions)
  const updateActions = includeDirect.devDependencies &&
    opts.save !== false &&
    !opts.lockfileOnly &&
    (opts.includeGithubActions === true || opts.updateConfig?.githubActions === true)
  if (opts.latest) {
    const dependenciesWithTags = dependencies.filter((name) =>
      (!updateActions || !isGitHubActionSelector(name)) && parseUpdateParam(name).versionSpec != null)
    if (dependenciesWithTags.length) {
      throw new PnpmError('LATEST_WITH_SPEC', `Specs are not allowed to be used with --latest (${dependenciesWithTags.join(', ')})`)
    }
  }
  const packageDependencies = updateActions
    ? dependencies.filter((dependency) => !isGitHubActionSelector(dependency))
    : dependencies
  // include is always all-true for updates: updates should not change which
  // dep types the modules directory supports. The filtering of which deps to
  // actually resolve/update is handled by includeDirect (from CLI flags).
  // This matches the original behavior where rawConfig didn't have derived
  // values like dev=false from --prod, so include defaulted to all-true.
  const include = {
    dependencies: true,
    devDependencies: true,
    optionalDependencies: true,
  }
  const depth = opts.depth ?? Infinity
  let updateMatching: UpdateMatchingFunction | undefined
  if (opts.packageVulnerabilityAudit != null) {
    updateMatching = createVulnerabilityUpdateMatching(opts.packageVulnerabilityAudit)
  } else if (
    (packageDependencies.length > 0) && packageDependencies.every(dep => !dep.substring(1).includes('@')) && depth > 0 && !opts.latest
  ) {
    updateMatching = createMatcher(packageDependencies)
  }
  const generateChangeset = opts.changeset ?? opts.updateConfig?.changeset ?? false
  const changesetContext = generateChangeset ? await captureUpdateChangesetContext(opts) : undefined
  if (dependencies.length === 0 || packageDependencies.length > 0) {
    await installDeps({
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
      // `--dry-run` is an `install`-only preview; never let a config-level
      // `dry-run` turn `update` into a no-op check.
      dryRun: false,
    }, packageDependencies)
  }
  if (updateActions) {
    await updateGitHubActions({
      dir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
      latest: opts.latest,
      match: dependencies.length > 0 ? createMatcher(dependencies.map(normalizeGitHubActionSelector)) : undefined,
    })
  }
  if (changesetContext != null) {
    await generateUpdateChangeset(changesetContext)
  }
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
