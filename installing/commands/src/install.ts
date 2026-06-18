import type { CommandHandlerMap } from '@pnpm/cli.command'
import { FILTERING, OPTIONS, OUTPUT_OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types as allTypes } from '@pnpm/config.reader'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { calcDedupeCheckIssues, countDedupeCheckIssues } from '@pnpm/installing.dedupe.check'
import { renderDedupeCheckIssues } from '@pnpm/installing.dedupe.issues-renderer'
import type { DryRunInstallResult } from '@pnpm/installing.deps-installer'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { getFetchFullMetadata } from './getFetchFullMetadata.js'
import { installDeps, type InstallDepsOptions } from './installDeps.js'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
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
  ], allTypes)
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

export function help (): string {
  return renderHelp({
    aliases: ['i'],
    description: 'Installs all dependencies of the project in the current working directory. \
When executed inside a workspace, installs all dependencies of all projects.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Run installation recursively in every package found in subdirectories. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          OPTIONS.ignoreScripts,
          OPTIONS.offline,
          OPTIONS.preferOffline,
          OPTIONS.globalDir,
          {
            description: "Packages in `devDependencies` won't be installed",
            name: '--prod',
            shortAlias: '-P',
          },
          {
            description: 'Only `devDependencies` are installed',
            name: '--dev',
            shortAlias: '-D',
          },
          {
            description: 'Skip reinstall if the workspace state is up-to-date',
            name: '--optimistic-repeat-install',
          },
          {
            description: 'Report what an install would change without writing anything to disk (no lockfile, no node_modules). Resolution still runs against the registry.',
            name: '--dry-run',
          },
          {
            description: '`optionalDependencies` are not installed',
            name: '--no-optional',
          },
          {
            description: 'Skip installing runtime entries (e.g. Node.js downloaded via `devEngines.runtime`). The lockfile is left untouched, so frozen installs still validate; only the runtime fetch and bin-linking are skipped. Useful in CI matrices where the runtime is provisioned externally.',
            name: '--no-runtime',
          },
          {
            description: `Don't read or generate a \`${WANTED_LOCKFILE}\` file`,
            name: '--no-lockfile',
          },
          {
            description: `Dependencies are not downloaded. Only \`${WANTED_LOCKFILE}\` is updated`,
            name: '--lockfile-only',
          },
          {
            description: "Don't generate a lockfile and fail if an update is needed. This setting is on by default in CI environments, so use --no-frozen-lockfile if you need to disable it for some reason",
            name: '--[no-]frozen-lockfile',
          },
          {
            description: `If the available \`${WANTED_LOCKFILE}\` satisfies the \`package.json\` then perform a headless installation`,
            name: '--prefer-frozen-lockfile',
          },
          {
            description: `The directory in which the ${WANTED_LOCKFILE} of the package will be created. Several projects may share a single lockfile.`,
            name: '--lockfile-dir <dir>',
          },
          {
            description: 'Fix broken lockfile entries automatically',
            name: '--fix-lockfile',
          },
          {
            description: 'Refresh integrity checksums recorded in the lockfile from the registry',
            name: '--update-checksums',
          },
          {
            description: 'Merge lockfiles were generated on git branch',
            name: '--merge-git-branch-lockfiles',
          },
          {
            description: 'The directory in which dependencies will be installed (instead of node_modules)',
            name: '--modules-dir <dir>',
          },
          {
            description: 'Dependencies inside the modules directory will have access only to their listed dependencies',
            name: '--no-hoist',
          },
          {
            description: 'All the subdeps will be hoisted into the root node_modules. Your code will have access to them',
            name: '--shamefully-hoist',
          },
          {
            description: 'Hoist all dependencies matching the pattern to `node_modules/.pnpm/node_modules`. \
The default pattern is * and matches everything. Hoisted packages can be required \
by any dependencies, so it is an emulation of a flat node_modules',
            name: '--hoist-pattern <pattern>',
          },
          {
            description: 'Hoist all dependencies matching the pattern to the root of the modules directory',
            name: '--public-hoist-pattern <pattern>',
          },
          OPTIONS.storeDir,
          OPTIONS.virtualStoreDir,
          {
            description: 'Maximum number of concurrent network requests',
            name: '--network-concurrency <number>',
          },
          {
            description: 'Controls the number of child processes run parallelly to build node modules',
            name: '--child-concurrency <number>',
          },
          {
            description: 'Disable pnpm hooks defined in .pnpmfile.cjs',
            name: '--ignore-pnpmfile',
          },
          {
            description: 'Ignore pnpm-workspace.yaml if exists in the parent directory, and treat the installation as normal non-workspace installation.',
            name: '--ignore-workspace',
          },
          {
            description: 'If false, skips store integrity checks. These checks detect accidental corruption, not tampering by untrusted users with write access to the store',
            name: '--[no-]verify-store-integrity',
          },
          {
            description: 'Open the package store read-only (immutable) and skip all store writes. For installs against a store on a read-only filesystem (e.g. a Nix store); pair with --offline --frozen-lockfile. Incompatible with --force',
            name: '--frozen-store',
          },
          {
            description: 'Fail on missing or invalid peer dependencies',
            name: '--strict-peer-dependencies',
          },
          {
            description: "Fail when a package's trust level is downgraded (e.g., from a trusted publisher to provenance only or no trust evidence)",
            name: '--trust-policy no-downgrade',
          },
          {
            description: 'Exclude specific packages from trust policy checks',
            name: '--trust-policy-exclude <package-spec>',
          },
          {
            description: 'Ignore trust downgrades for packages published more than specified minutes ago',
            name: '--trust-policy-ignore-after <minutes>',
          },
          {
            description: 'Trust the lockfile and skip the supply-chain verification step that re-applies minimumReleaseAge / trustPolicy to each lockfile entry. Use only when the lockfile is part of the trusted base (closed-source projects, CI runs against an already-verified lockfile)',
            name: '--trust-lockfile',
          },
          {
            description: 'Clones/hardlinks or copies packages. The selected method depends from the file system',
            name: '--package-import-method auto',
          },
          {
            description: 'Hardlink packages from the store',
            name: '--package-import-method hardlink',
          },
          {
            description: 'Copy packages from the store',
            name: '--package-import-method copy',
          },
          {
            description: 'Clone (aka copy-on-write) packages from the store',
            name: '--package-import-method clone',
          },
          {
            description: 'Force reinstall dependencies: refetch packages modified in store, \
recreate a lockfile and/or modules directory created by a non-compatible version of pnpm. \
Install all optionalDependencies even when they don\'t satisfy the current environment(cpu, os, arch)',
            name: '--force',
          },
          {
            description: 'Use or cache the results of (pre/post)install hooks',
            name: '--side-effects-cache',
          },
          {
            description: 'Only use the side effects cache if present, do not create it for new packages',
            name: '--side-effects-cache-readonly',
          },
          {
            description: 'Re-runs resolution: useful for printing out peer dependency issues',
            name: '--resolution-only',
          },
          {
            description: 'Override CPU architecture of native modules to install. Acceptable values are the same as the `cpu` field of `package.json` (from `process.arch`)',
            name: '--cpu <arch>',
          },
          {
            description: 'Override OS of native modules to install. Acceptable values are the same as the `os` field of `package.json` (from `process.platform`)',
            name: '--os <os>',
          },
          {
            description: 'Override libc of native modules to install. Acceptable values are the same as the `libc` field of `package.json`',
            name: '--libc <libc>',
          },
          ...UNIVERSAL_OPTIONS,
        ],
      },
      OUTPUT_OPTIONS,
      FILTERING,
    ],
    url: docsUrl('install'),
    usages: ['pnpm install [options]'],
  })
}

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
| 'modulesDir'
| 'nodeLinker'
| 'patchedDependencies'
| 'preferFrozenLockfile'
| 'preferWorkspacePackages'
| 'production'
| 'registries'
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
| 'sideEffectsCache'
| 'sideEffectsCacheReadonly'
| 'sort'
| 'sharedWorkspaceLockfile'
| 'tag'
| 'trustLockfile'
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
  workspace?: boolean
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
    frozenLockfileIfExists: opts.frozenLockfileIfExists ?? (
      opts.ci && !opts.lockfileOnly &&
      typeof opts.frozenLockfile === 'undefined' &&
      typeof opts.preferFrozenLockfile === 'undefined'
    ),
    include,
    includeDirect: include,
    fetchFullMetadata: getFetchFullMetadata(opts),
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
  // `dryRun` makes the installer resolve fully and return the before/after
  // wanted lockfile without writing anything. `lockfileOnly` keeps it from
  // materializing `node_modules` and skips the metadata cache (resolution
  // skips fetching). The optimistic fast path is disabled so resolution
  // always runs.
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
