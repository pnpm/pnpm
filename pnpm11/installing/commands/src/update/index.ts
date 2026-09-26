import util from 'node:util'

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
import { writeSettings } from '@pnpm/config.writer'
import { findOutdatedGitHubActions, isGitHubActionSelector, normalizeGitHubActionSelector, shouldCheckGitHubActions, updateGitHubActions } from '@pnpm/deps.github-actions'
import { outdatedDepsOfProjects } from '@pnpm/deps.inspection.outdated'
import { PnpmError } from '@pnpm/error'
import { handleGlobalUpdate, hasPnpmCliDependency, selectsPnpmCli } from '@pnpm/global.commands'
import { scanGlobalPackages } from '@pnpm/global.packages'
import type { UpdateMatchingFunction } from '@pnpm/installing.deps-installer'
import { globalInfo, globalWarn } from '@pnpm/logger'
import { calcVersionRange, getRangeOfSpecifier, guessDependencyType, inferRangeSpecStyle } from '@pnpm/pkg-manifest.utils'
import { sanitizeInline } from '@pnpm/text.sanitize'
import type { IncludedDependencies, PackageVulnerabilityAudit, ProjectRootDir, RangeSpecStyle } from '@pnpm/types'
import chalk from 'chalk'
import { pick, unnest } from 'ramda'
import { renderHelp } from 'render-help'
import semver from 'semver'

import type { InstallCommandOptions } from '../install.js'
import { createVulnerabilityUpdateMatching, installDeps } from '../installDeps.js'
import { createUpdateMatching, expandUpdateSelectorsForMatching, parseUpdateParam } from '../recursive.js'
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
    'store-dir',
    'trust-lockfile',
    'trust-policy',
    'trust-policy-exclude',
    'trust-policy-ignore-after',
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
    patches: Boolean,
    peer: Boolean,
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
            description: 'Refresh registry revisions without changing package versions',
            name: '--patches',
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
            description: 'Also update packages in "peerDependencies"',
            name: '--peer',
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
  patches?: boolean
  packageVulnerabilityAudit?: PackageVulnerabilityAudit
}

export async function handler (
  opts: UpdateCommandOptions,
  params: string[] = [],
  commands?: CommandHandlerMap
): Promise<string | undefined> {
  assertPatchesOptions(params, opts)
  if (opts.global) {
    if (!opts.bin) {
      throw new PnpmError('NO_GLOBAL_BIN_DIR', 'Unable to find the global bin directory', {
        hint: 'Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH.',
      })
    }
    if (selectsPnpmCli(params)) {
      throw new PnpmError('GLOBAL_PNPM_INSTALL', 'Use the "pnpm self-update" command to install or update pnpm')
    }
    const selection = opts.interactive
      ? await selectGlobalPackageGroups(params, opts)
      : undefined
    if (typeof selection === 'string') return selection
    if (selection?.size === 0) return undefined
    return handleGlobalUpdate({
      ...opts,
      ...createGlobalPolicyCallbacks(opts),
      selectedPackageHashes: selection,
    }, params, commands ?? {})
  }
  const rebuildHandler = commands?.rebuild
  if (opts.interactive) {
    return interactiveUpdate(params, opts, rebuildHandler)
  }
  return update(params, opts, rebuildHandler) as Promise<undefined>
}

async function selectGlobalPackageGroups (
  input: string[],
  opts: UpdateCommandOptions
): Promise<Set<string> | string> {
  const scannedPackages = scanGlobalPackages(opts.globalPkgDir!)
  if (scannedPackages.length === 0) return 'No global packages found'
  // The pnpm CLI's own global install belongs to `pnpm self-update`, so it is
  // never offered as a choice. See `hasPnpmCliDependency`.
  const globalPackages = scannedPackages.filter((pkg) => !hasPnpmCliDependency(pkg))
  if (globalPackages.length === 0) {
    return 'No global packages to update. Run "pnpm self-update" to update pnpm itself.'
  }
  // A global group is always updated as a whole, so the params select groups
  // rather than dependencies, the same way `handleGlobalUpdate()` reads them.
  const matchedPackages = input.length === 0
    ? globalPackages
    : globalPackages.filter((pkg) => input.some((param) => Object.hasOwn(pkg.dependencies, param)))
  if (matchedPackages.length === 0) return 'No matching global packages found'
  const outdatedPerGroup = await Promise.all(matchedPackages.map(async (pkg) => {
    const project = {
      rootDir: pkg.installDir as ProjectRootDir,
      manifest: await readProjectManifestOnly(pkg.installDir, opts),
    }
    const [outdated] = await outdatedDepsOfProjects([project], [], {
      ...opts,
      compatible: opts.latest !== true,
      ignoreDependencies: opts.updateConfig?.ignoreDependencies,
      include: {
        dependencies: true,
        devDependencies: false,
        optionalDependencies: true,
      },
      retry: {
        factor: opts.fetchRetryFactor,
        maxTimeout: opts.fetchRetryMaxtimeout,
        minTimeout: opts.fetchRetryMintimeout,
        retries: opts.fetchRetries,
      },
      timeout: opts.fetchTimeout,
    })
    return { pkg, outdated }
  }))
  const choices = outdatedPerGroup
    .filter(({ outdated }) => outdated.length > 0)
    .map(({ pkg, outdated }) => ({
      name: outdated
        .map(({ alias, current, wanted, latestManifest }) =>
          [alias, current ?? 'missing', '→', opts.latest ? latestManifest?.version ?? wanted : wanted]
            .map(sanitizeInline)
            .join(' ')
        )
        .join(', '),
      value: pkg.hash,
    }))
  if (choices.length === 0) {
    return opts.latest
      ? 'All of your dependencies are already up to date'
      : 'All of your dependencies are already up to date inside the specified ranges. Use the --latest option to update the ranges in package.json'
  }
  return new Set(await runUpdatePrompt(() => checkbox({
    choices,
    message: 'Choose which global package groups to update (space to select, enter to confirm)',
    pageSize: Math.min(choices.length, interactivePromptPageSize()),
  })))
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
    shouldUpdateGitHubActions(opts, include)
      ? findOutdatedGitHubActions({
        compatible: opts.latest !== true,
        dir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
        match: input.length > 0 ? createMatcher(input.map(normalizeGitHubActionSelector)) : undefined,
        minimumReleaseAge: opts.minimumReleaseAge,
        minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
        serverUrl: opts.updateConfig?.githubActionsServer,
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
          short: choice.short,
        })
      }
    }
  }

  const message = 'Choose which dependencies to update ' +
    `(Press ${chalk.cyan('<space>')} to select, ` +
    `${chalk.cyan('<a>')} to toggle all, ` +
    `${chalk.cyan('<i>')} to invert selection)\n\nEnter to start updating. Ctrl-c to cancel.`
  const updatePkgNames = await runUpdatePrompt(() => checkbox({
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
  }))

  return update(updatePkgNames, { ...opts, interactive: false, interactiveUpdate: true }, rebuildHandler) as Promise<undefined>
}

/**
 * Cancelling a prompt with Ctrl-c is how the user declines to update, not an
 * error: report it and leave with a success status.
 */
async function runUpdatePrompt<T> (prompt: () => Promise<T>): Promise<T> {
  try {
    return await prompt()
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && err.name === 'ExitPromptError') {
      globalInfo('Update canceled')
      process.exit(0)
    }
    throw err
  }
}

async function update (
  dependencies: string[],
  opts: UpdateCommandOptions,
  rebuildHandler?: CommandHandler
): Promise<void> {
  assertPatchesOptions(dependencies, opts)
  const includeDirect = makeIncludeDependenciesFromCLI(opts.cliOptions)
  const updateActions = shouldUpdateGitHubActions(opts, includeDirect)
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
  const include = opts.include
  const depth = opts.depth ?? Infinity
  let updateMatching: UpdateMatchingFunction | undefined
  if (opts.packageVulnerabilityAudit != null) {
    updateMatching = createVulnerabilityUpdateMatching(opts.packageVulnerabilityAudit)
  } else if ((packageDependencies.length > 0) && depth > 0 && !opts.latest) {
    updateMatching = createUpdateMatching(packageDependencies.flatMap(expandUpdateSelectorsForMatching))
  }
  const generateChangeset = opts.changeset ?? opts.updateConfig?.changeset ?? false
  const changesetContext = generateChangeset ? await captureUpdateChangesetContext(opts) : undefined
  if (dependencies.length === 0 || packageDependencies.length > 0) {
    const overrideMove = await planOverrideMove(packageDependencies, opts, includeDirect)
    await installDeps({
      ...opts,
      ...(overrideMove != null ? { overrides: overrideMove.overrides } : {}),
      rebuildHandler,
      allowNew: false,
      peer: opts.cliOptions.peer === true,
      depth,
      ignoreCurrentSpecifiers: false,
      include,
      includeDirect,
      update: true,
      updatePatches: opts.patches,
      updateToLatest: opts.latest,
      updateMatching,
      updatePackageManifest: opts.patches ? false : opts.save !== false,
      resolutionMode: opts.save === false ? 'highest' : opts.resolutionMode,
      // `--dry-run` is an `install`-only preview; never let a config-level
      // `dry-run` turn `update` into a no-op check.
      dryRun: false,
    }, packageDependencies)
    if (overrideMove?.updatedOverrides != null) {
      await writeSettings({
        ...opts,
        workspaceDir: opts.workspaceDir ?? (opts.rootProjectManifestDir || opts.dir),
        updatedOverrides: overrideMove.updatedOverrides,
      })
    }
  }
  if (updateActions) {
    await updateGitHubActions({
      dir: opts.workspaceDir ?? opts.lockfileDir ?? opts.dir,
      latest: opts.latest,
      match: dependencies.length > 0 ? createMatcher(dependencies.map(normalizeGitHubActionSelector)) : undefined,
      minimumReleaseAge: opts.minimumReleaseAge,
      minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
      serverUrl: opts.updateConfig?.githubActionsServer,
    })
  }
  if (changesetContext != null) {
    await generateUpdateChangeset(changesetContext)
  }
}

interface OverrideMove {
  /**
   * The overrides the install runs under: the configured ones with the
   * entries this update moves replaced, so the resolution this run writes
   * already answers to the moved pins.
   */
  overrides: Record<string, string>
  /**
   * The moved entries, for the workspace-manifest write the update owes
   * after a successful install. `undefined` when the update only warned.
   */
  updatedOverrides?: Record<string, string>
}

/**
 * What an update that names dependencies an override governs does about it
 * (pnpm/pnpm#8701): the override, not the manifest declaration, owns the
 * resolution, so a targeted `--latest` moves the override entry with the
 * update — keeping the entry's own range shape — and reports the overrides
 * it cannot move instead of failing silently. A compatible update cannot
 * move an override that pins one version, and says so.
 *
 * `undefined` when nothing the update names is governed by an override.
 */
async function planOverrideMove (
  dependencies: string[],
  opts: UpdateCommandOptions,
  includeDirect: IncludedDependencies
): Promise<OverrideMove | undefined> {
  const rawOverrides = opts.overrides
  if (rawOverrides == null || dependencies.length === 0) return undefined
  const projects = (opts.selectedProjectsGraph != null)
    ? Object.values(opts.selectedProjectsGraph).map((wsPkg) => wsPkg.package)
    : [
      {
        rootDir: opts.dir as ProjectRootDir,
        manifest: await readProjectManifestOnly(opts.dir, opts),
      },
    ]
  const governed = dependencies.filter((name) =>
    rawOverrides[name] != null && projects.some((project) => guessDependencyType(name, project.manifest) != null))
  if (governed.length === 0) return undefined
  if (!opts.latest) {
    for (const name of governed) {
      const value = rawOverrides[name]
      const style = movableOverrideRangeStyle(value)
      if (style === 'patch' || style === 'exact') {
        globalWarn(`Skipping "${name}": it is pinned to "${value}" by an override, which a compatible update cannot move. Use --latest or update the override in pnpm-workspace.yaml.`)
      }
    }
    return undefined
  }
  if (opts.save === false) return undefined
  // Only bare range values are looked up in one batch: a value that names
  // another dependency (`npm:`, `link:`, …) has no version of its own, and
  // asking for one can fail resolution, so each of those is asked on its
  // own below.
  const movableNames = governed.filter((name) => movableOverrideRangeStyle(rawOverrides[name]) != null)
  const outdatedOpts = {
    ...opts,
    compatible: false,
    include: includeDirect,
    ignoreDependencies: opts.updateConfig?.ignoreDependencies,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  } as const
  const latest = new Map<string, string>()
  if (movableNames.length > 0) {
    for (const outdated of await outdatedDepsOfProjects(projects, movableNames, outdatedOpts)) {
      for (const pkg of outdated) {
        const version = pkg.latestManifest?.version
        if (version != null && !latest.has(pkg.alias)) {
          latest.set(pkg.alias, version)
        }
      }
    }
  }
  const referencedLatest = new Map<string, string | undefined>()
  const unresolvable = new Set<string>()
  await Promise.all(governed.filter((name) => namesAnotherDependency(rawOverrides[name])).map(async (name) => {
    try {
      const [outdated] = await outdatedDepsOfProjects(projects, [name], outdatedOpts)
      referencedLatest.set(name, outdated.find((pkg) => pkg.alias === name)?.latestManifest?.version)
    } catch {
      unresolvable.add(name)
    }
  }))
  const updatedOverrides: Record<string, string> = {}
  for (const name of governed) {
    const value = rawOverrides[name]
    // A `catalog:`-valued override tracks the catalog entry it points at —
    // the catalog update path owns that entry — and a bare value with no
    // recoverable operator (a dist tag, a partial version) tracks a moving
    // target the way a tag-tracking declaration does. Neither is the
    // update's to rewrite.
    if (value.startsWith('catalog:') || (!namesAnotherDependency(value) && movableOverrideRangeStyle(value) == null)) continue
    if (unresolvable.has(name)) {
      warnUnmovableOverride(name, value)
      continue
    }
    const nextVersion = latest.get(name) ?? referencedLatest.get(name)
    if (nextVersion == null) continue // already up to date
    const range = getRangeOfSpecifier(value)
    if (range != null && semver.validRange(range) != null && semver.satisfies(nextVersion, range)) {
      // The override already admits the version the update picked, so the
      // resolution moves within it and the entry stands.
      continue
    }
    if (namesAnotherDependency(value)) {
      warnUnmovableOverride(name, value)
    } else {
      const next = calcVersionRange(nextVersion, { prevSpecifier: value, isUpdate: true })
      if (next !== value) {
        updatedOverrides[name] = next
      }
    }
  }
  if (Object.keys(updatedOverrides).length === 0) return undefined
  return {
    overrides: { ...rawOverrides, ...updatedOverrides },
    updatedOverrides,
  }
}

/**
 * Whether an override value names a dependency of its own rather than
 * pinning a version of the one it overrides: a protocol reference (`npm:`,
 * `link:`, a named registry, ...) or a `$` reference to another dependency's
 * specifier. A `catalog:` reference is the catalog update path's to move.
 */
function namesAnotherDependency (value: string): boolean {
  return !value.startsWith('catalog:') && (value.includes(':') || value.startsWith('$'))
}

function warnUnmovableOverride (name: string, value: string): void {
  globalWarn(`Skipping "${name}": it is controlled by an override ("${name}" => "${value}") that pnpm cannot update automatically. Update the override in pnpm-workspace.yaml to update this dependency.`)
}

/**
 * The range style an update can preserve when it moves an override entry.
 * A protocol reference (`npm:`, `catalog:`, `link:`, …) or a `$` reference
 * names a dependency of its own that the update must not rewrite; a value
 * with no single recoverable operator (`*`, a compound range) has no shape
 * to keep.
 */
function movableOverrideRangeStyle (value: string): RangeSpecStyle | undefined {
  if (value.includes(':') || value.startsWith('$')) return undefined
  const style = inferRangeSpecStyle(value)
  return style === 'none' ? undefined : style
}

function assertPatchesOptions (dependencies: string[], opts: UpdateCommandOptions): void {
  if (opts.patches && (dependencies.length > 0 || opts.latest || opts.interactive || opts.global)) {
    throw new PnpmError('PATCHES_WITH_SELECTOR', '--patches cannot be combined with package selectors, --latest, --interactive, or --global')
  }
}

function shouldUpdateGitHubActions (opts: UpdateCommandOptions, include: IncludedDependencies): boolean {
  return include.devDependencies &&
    opts.save !== false &&
    !opts.lockfileOnly &&
    shouldCheckGitHubActions(opts)
}

function makeIncludeDependenciesFromCLI (opts: {
  production?: boolean
  dev?: boolean
  optional?: boolean
  peer?: boolean
}): IncludedDependencies {
  return {
    dependencies: opts.production === true || (opts.dev !== true && opts.optional !== true),
    devDependencies: opts.dev === true || (opts.production !== true && opts.optional !== true),
    optionalDependencies: opts.optional === true || (opts.optional !== false && opts.dev !== true),
    ...(opts.peer === true ? { peerDependencies: true } : {}),
  }
}
