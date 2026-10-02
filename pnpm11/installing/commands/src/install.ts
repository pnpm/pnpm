import type { CommandHandlerMap } from '@pnpm/cli.command'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { calcDedupeCheckIssues, countDedupeCheckIssues } from '@pnpm/installing.dedupe.check'
import { renderDedupeCheckIssues } from '@pnpm/installing.dedupe.issues-renderer'
import type { DryRunInstallResult } from '@pnpm/installing.deps-installer'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import { pick } from 'ramda'

import { installDeps, type InstallDepsOptions } from './installDeps.js'

const RC_OPTION_NAMES = [
  'cache-dir',
  'child-concurrency',
  'cpu',
  'dangerously-allow-all-builds',
  'dev',
  'engine-strict',
  'fetch-retries',
  'fetch-retry-factor',
  'fetch-retry-maxtimeout',
  'fetch-retry-mintimeout',
  'fetch-timeout',
  'frozen-lockfile',
  'global-dir',
  'global-pnpmfile',
  'global',
  'hoist',
  'hoist-pattern',
  'hoisting-limits',
  'https-proxy',
  'ignore-pnpmfile',
  'ignore-scripts',
  'optimistic-repeat-install',
  'os',
  'libc',
  'link-workspace-packages',
  'lockfile-dir',
  'lockfile-only',
  'lockfile',
  'merge-git-branch-lockfiles',
  'merge-git-branch-lockfiles-branch-pattern',
  'modules-dir',
  'network-concurrency',
  'node-experimental-package-map',
  'node-package-map-type',
  'node-linker',
  'noproxy',
  'package-import-method',
  'pnpmfile',
  'pnpr-server',
  'prefer-frozen-lockfile',
  'prefer-offline',
  'production',
  'proxy',
  'public-hoist-pattern',
  'registry',
  'reporter',
  'runtime',
  'save-workspace-protocol',
  'scripts-prepend-node-path',
  'shamefully-hoist',
  'shared-workspace-lockfile',
  'side-effects-cache-readonly',
  'side-effects-cache',
  'store-dir',
  'strict-peer-dependencies',
  'trust-lockfile',
  'trust-policy',
  'trust-policy-exclude',
  'trust-policy-ignore-after',
  'offline',
  'only',
  'optional',
  'unsafe-perm',
  'verify-store-integrity',
  'frozen-store',
  'virtual-store-dir',
  'virtual-store-only',
] as const

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(RC_OPTION_NAMES, allTypes)
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  ...pick(['force'], allTypes),
  'dry-run': Boolean,
  'fix-lockfile': Boolean,
  'update-checksums': Boolean,
  'resolution-only': Boolean,
  recursive: Boolean,
  // `--no-save` lets `pnpm install` skip writing to package.json /
  // pnpm-workspace.yaml. Without registering it here, nopt drops the
  // flag, `opts.save` stays undefined, and the auto-add path treats
  // it as "save enabled".
  save: Boolean,
})

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['install', 'i']

export const recursiveByDefault = true

export { help } from './installHelp.js'

export type InstallCommandOptions = Pick<Config,
| 'autoInstallPeers'
| 'bail'
| 'bin'
| 'catalogs'
| 'configDependencies'
| 'dedupeInjectedDeps'
| 'dedupeDirectDeps'
| 'dedupePeerDependents'
| 'dedupePeers'
| 'deployAllFiles'
| 'depth'
| 'dev'
| 'dryRun'
| 'enableGlobalVirtualStore'
| 'engineStrict'
| 'excludeLinksFromLockfile'
| 'forceIgnoresPlatform'
| 'frozenLockfile'
| 'global'
| 'globalPnpmfile'
| 'hoistPattern'
| 'hoistingLimits'
| 'publicHoistPattern'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'injectWorkspacePackages'
| 'linkWorkspacePackages'
| 'lockfileDir'
| 'lockfileOnly'
| 'optimisticRepeatInstall'
| 'minimumReleaseAgeExcludePrune'
| 'modulesDir'
| 'nodeLinker'
| 'patchedDependencies'
| 'preferFrozenLockfile'
| 'preferWorkspacePackages'
| 'production'
| 'registriesByScope'
| 'save'
| 'saveDev'
| 'saveExact'
| 'saveOptional'
| 'savePeer'
| 'savePrefix'
| 'saveProd'
| 'saveCatalogName'
| 'saveWorkspaceProtocol'
| 'lockfileIncludeTarballUrl'
| 'sideEffectsCacheRead'
| 'sideEffectsCacheWrite'
| 'sort'
| 'sharedWorkspaceLockfile'
| 'tag'
| 'trustLockfile'
| 'trustPolicyExcludePrune'
| 'tryLoadDefaultPnpmfile'
| 'allowBuilds'
| 'optional'
| 'virtualStoreDir'
| 'workspaceConcurrency'
| 'workspaceDir'
| 'workspacePackagePatterns'
| 'extraEnv'
| 'resolutionMode'
| 'ignoreWorkspaceCycles'
| 'disallowWorkspaceCycles'
| 'updateConfig'
| 'overrides'
| 'packageExtensions'
| 'pnprServer'
| 'supportedArchitectures'
| 'packageConfigs'
> & Pick<ConfigContext,
| 'allProjects'
| 'cliOptions'
| 'hooks'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
| 'allProjectsGraph'
| 'selectedProjectsGraph'
> & CreateStoreControllerOptions & Partial<Pick<Config, 'globalPkgDir'>> & {
  argv: {
    cooked?: string[]
    original: string[]
    remain?: string[]
  }
  /** See {@link InstallDepsOptions.excludeWorkspaceRootProject}. */
  excludeWorkspaceRootProject?: boolean
  deploy?: boolean
  fixLockfile?: boolean
  updateChecksums?: boolean
  frozenLockfileIfExists?: boolean
  useBetaCli?: boolean
  pruneDirectDependencies?: boolean
  pruneLockfileImporters?: boolean
  pruneStore?: boolean
  recursive?: boolean
  resolutionOnly?: boolean
  saveLockfile?: boolean
  /** See {@link InstallDepsOptions.saveWorkspaceState}. */
  saveWorkspaceState?: boolean
  workspace?: boolean
  interactiveUpdate?: boolean
  includeOnlyPackageFiles?: boolean
  confirmModulesPurge?: boolean
  pnpmfile: string[]
} & Partial<Pick<Config, 'ci' | 'modulesCacheMaxAge' | 'pnpmHomeDir' | 'preferWorkspacePackages' | 'strictDepBuilds' | 'useLockfile' | 'symlink'>>

export async function handler (opts: InstallCommandOptions & { _calledFromLink?: boolean }, _params?: string[], commands?: CommandHandlerMap): Promise<void | string> {
  if (opts.global && !opts._calledFromLink) {
    throw new PnpmError('GLOBAL_INSTALL_NOT_SUPPORTED',
      '"pnpm install -g" is not supported. Use "pnpm add -g <pkg>" to install global packages.')
  }
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  const installDepsOptions: InstallDepsOptions = {
    ...opts,
    rebuildHandler: commands?.rebuild,
    frozenLockfileIfExists: shouldFreezeLockfileIfExists(opts),
    include,
    includeDirect: include,
    isInstallCommand: true,
  }
  if (opts.resolutionOnly) {
    installDepsOptions.lockfileOnly = true
    installDepsOptions.forceFullResolution = true
  }
  if (opts.dryRun) {
    return dryRunInstall(installDepsOptions, opts)
  }
  await installDeps(installDepsOptions, [])
}

/**
 * Runs a full resolution but writes nothing to disk (no lockfile, no
 * `node_modules`), then reports what a real install would change. Exits
 * successfully regardless of whether changes were found — mirroring the
 * preview semantics of `npm install --dry-run`.
 */
async function dryRunInstall (installDepsOptions: InstallDepsOptions, opts: InstallCommandOptions): Promise<string> {
  if (opts.pnprServer) {
    throw new PnpmError('CONFIG_CONFLICT_DRY_RUN_WITH_PNPR_SERVER',
      'Cannot use --dry-run with a configured pnpr server because the pnpr install path resolves and links through the server')
  }
  // `lockfileOnly` keeps the installer from materializing `node_modules`
  // and skips the metadata cache (resolution skips fetching). The
  // optimistic fast path is disabled so resolution always runs.
  installDepsOptions.optimisticRepeatInstall = false
  installDepsOptions.lockfileOnly = true
  installDepsOptions.dryRun = true
  const dryRunResult = await installDeps(installDepsOptions, [])
  if (dryRunResult == null) {
    // No comparison was produced — this install configuration's resolve path
    // doesn't surface the dry-run lockfiles (e.g. a workspace without a
    // shared lockfile). Report that explicitly instead of claiming "up to
    // date", but keep `--dry-run`'s exit-0 contract.
    return 'Dry run complete. Could not compute the changes for this install configuration (no shared lockfile to compare).'
  }
  return renderDryRunReport(dryRunResult)
}

function renderDryRunReport (dryRunResult: DryRunInstallResult): string {
  const issues = calcDedupeCheckIssues(dryRunResult.originalLockfile, dryRunResult.wantedLockfile, { includeImporterSpecifiers: true })
  if (countDedupeCheckIssues(issues) === 0) {
    return `Dry run complete. ${WANTED_LOCKFILE} is up to date; a real install would make no changes.`
  }
  return [
    'Dry run complete. A real install would make the following changes (nothing was written to disk):',
    '',
    renderDedupeCheckIssues(issues),
  ].join('\n')
}

export function shouldFreezeLockfileIfExists (opts: InstallCommandOptions): boolean {
  return opts.frozenLockfileIfExists ?? (
    opts.ci === true &&
    !opts.lockfileOnly &&
    !opts.resolutionOnly &&
    opts.frozenLockfile !== false &&
    opts.preferFrozenLockfile !== false
  )
}
