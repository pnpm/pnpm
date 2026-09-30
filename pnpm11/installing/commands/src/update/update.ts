import type { CommandHandler } from '@pnpm/cli.command'
import { createMatcher } from '@pnpm/config.matcher'
import { isGitHubActionSelector, normalizeGitHubActionSelector, shouldCheckGitHubActions, updateGitHubActions } from '@pnpm/deps.github-actions'
import { PnpmError } from '@pnpm/error'
import type { UpdateMatchingFunction } from '@pnpm/installing.deps-installer'
import type { IncludedDependencies, PackageVulnerabilityAudit } from '@pnpm/types'

import type { InstallCommandOptions } from '../install.js'
import { createVulnerabilityUpdateMatching, installDeps } from '../installDeps.js'
import { createUpdateMatching, expandUpdateSelectorsForMatching, parseUpdateParam } from '../recursive.js'
import { captureUpdateChangesetContext, generateUpdateChangeset } from './generateUpdateChangeset.js'

export type UpdateCommandOptions = InstallCommandOptions & {
  changeset?: boolean
  include?: IncludedDependencies
  includeGithubActions?: boolean
  interactive?: boolean
  latest?: boolean
  patches?: boolean
  packageVulnerabilityAudit?: PackageVulnerabilityAudit
}

export async function update (
  dependencies: string[],
  opts: UpdateCommandOptions,
  rebuildHandler?: CommandHandler
): Promise<void> {
  assertPatchesOptions(dependencies, opts)
  const includeDirect = makeIncludeDependenciesFromCLI(opts.cliOptions)
  const updateActions = shouldUpdateGitHubActions(opts, includeDirect)
  if (opts.latest) {
    assertNoVersionSpecsWithLatest(dependencies, updateActions)
  }
  const packageDependencies = updateActions
    ? dependencies.filter((dependency) => !isGitHubActionSelector(dependency))
    : dependencies
  const depth = opts.depth ?? Infinity
  const updateMatching = selectUpdateMatching(packageDependencies, opts, depth)
  const generateChangeset = opts.changeset ?? opts.updateConfig?.changeset ?? false
  const changesetContext = generateChangeset ? await captureUpdateChangesetContext(opts) : undefined
  if (dependencies.length === 0 || packageDependencies.length > 0) {
    await installUpdates(packageDependencies, { depth, includeDirect, opts, rebuildHandler, updateMatching })
  }
  if (updateActions) {
    await updateGitHubActions({
      dir: getGitHubActionsDir(opts),
      latest: opts.latest,
      match: createGitHubActionsMatcher(dependencies),
      minimumReleaseAge: opts.minimumReleaseAge,
      minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
      serverUrl: opts.updateConfig?.githubActionsServer,
    })
  }
  if (changesetContext != null) {
    await generateUpdateChangeset(changesetContext)
  }
}

function assertNoVersionSpecsWithLatest (dependencies: string[], updateActions: boolean): void {
  const dependenciesWithTags = dependencies.filter((name) =>
    (!updateActions || !isGitHubActionSelector(name)) && parseUpdateParam(name).versionSpec != null)
  if (dependenciesWithTags.length) {
    throw new PnpmError('LATEST_WITH_SPEC', `Specs are not allowed to be used with --latest (${dependenciesWithTags.join(', ')})`)
  }
}

function selectUpdateMatching (
  packageDependencies: string[],
  opts: UpdateCommandOptions,
  depth: number
): UpdateMatchingFunction | undefined {
  if (opts.packageVulnerabilityAudit != null) {
    return createVulnerabilityUpdateMatching(opts.packageVulnerabilityAudit)
  }
  if ((packageDependencies.length > 0) && depth > 0 && !opts.latest) {
    return createUpdateMatching(packageDependencies.flatMap(expandUpdateSelectorsForMatching))
  }
  return undefined
}

interface InstallUpdatesOptions {
  depth: number
  includeDirect: IncludedDependencies
  opts: UpdateCommandOptions
  rebuildHandler?: CommandHandler
  updateMatching?: UpdateMatchingFunction
}

async function installUpdates (
  packageDependencies: string[],
  { depth, includeDirect, opts, rebuildHandler, updateMatching }: InstallUpdatesOptions
): Promise<void> {
  await installDeps({
    ...opts,
    rebuildHandler,
    allowNew: false,
    peer: opts.cliOptions.peer === true,
    depth,
    ignoreCurrentSpecifiers: false,
    include: opts.include,
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
}

export function getGitHubActionsDir (opts: UpdateCommandOptions): string {
  return opts.workspaceDir ?? opts.lockfileDir ?? opts.dir
}

export function createGitHubActionsMatcher (selectors: string[]): ReturnType<typeof createMatcher> | undefined {
  return selectors.length > 0 ? createMatcher(selectors.map(normalizeGitHubActionSelector)) : undefined
}

export function assertPatchesOptions (dependencies: string[], opts: UpdateCommandOptions): void {
  if (opts.patches && (dependencies.length > 0 || opts.latest || opts.interactive || opts.global)) {
    throw new PnpmError('PATCHES_WITH_SELECTOR', '--patches cannot be combined with package selectors, --latest, --interactive, or --global')
  }
}

export function shouldUpdateGitHubActions (opts: UpdateCommandOptions, include: IncludedDependencies): boolean {
  return include.devDependencies &&
    opts.save !== false &&
    !opts.lockfileOnly &&
    shouldCheckGitHubActions(opts)
}

export function makeIncludeDependenciesFromCLI (opts: {
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
