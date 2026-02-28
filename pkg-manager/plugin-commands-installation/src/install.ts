import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, OUTPUT_OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types as allTypes } from '@pnpm/config'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { type CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import { pick } from 'ramda'
import renderHelp from 'render-help'
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
    'https-proxy',
    'ignore-pnpmfile',
    'ignore-scripts',
    'optimistic-repeat-install',
    'os',
    'libc',
    'link-workspace-packages',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'merge-git-branch-lockfiles',
    'merge-git-branch-lockfiles-branch-pattern',
    'modules-dir',
    'network-concurrency',
    'node-linker',
    'noproxy',
    'package-import-method',
    'pnpmfile',
    'prefer-frozen-lockfile',
    'prefer-offline',
    'production',
    'proxy',
    'public-hoist-pattern',
    'registry',
    'reporter',
    'save-workspace-protocol',
    'scripts-prepend-node-path',
    'shamefully-flatten',
    'shamefully-hoist',
    'shared-workspace-lockfile',
    'side-effects-cache-readonly',
    'side-effects-cache',
    'store-dir',
    'strict-peer-dependencies',
    'trust-policy',
    'trust-policy-exclude',
    'trust-policy-ignore-after',
    'offline',
    'only',
    'optional',
    'unsafe-perm',
    'verify-store-integrity',
    'virtual-store-dir',
  ], allTypes)
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  ...pick(['force'], allTypes),
  'fix-lockfile': Boolean,
  'resolution-only': Boolean,
  recursive: Boolean,
})

export const shorthands: Record<string, string> = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['install', 'i']

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
            description: '`optionalDependencies` are not installed',
            name: '--no-optional',
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
            description: "If false, doesn't check whether packages in the store were mutated",
            name: '--[no-]verify-store-integrity',
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
Install all optionalDependencies even they don\'t satisfy the current environment(cpu, os, arch)',
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
| 'allProjects'
| 'autoInstallPeers'
| 'bail'
| 'bin'
| 'catalogs'
| 'cliOptions'
| 'configDependencies'
| 'dedupeInjectedDeps'
| 'dedupeDirectDeps'
| 'dedupePeerDependents'
| 'deployAllFiles'
| 'depth'
| 'dev'
| 'enableGlobalVirtualStore'
| 'engineStrict'
| 'excludeLinksFromLockfile'
| 'frozenLockfile'
| 'global'
| 'globalPnpmfile'
| 'hooks'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'injectWorkspacePackages'
| 'linkWorkspacePackages'
| 'rawLocalConfig'
| 'lockfileDir'
| 'lockfileOnly'
| 'modulesDir'
| 'nodeLinker'
| 'patchedDependencies'
| 'preferFrozenLockfile'
| 'preferWorkspacePackages'
| 'production'
| 'registries'
| 'rootProjectManifest'
| 'rootProjectManifestDir'
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
| 'allProjectsGraph'
| 'selectedProjectsGraph'
| 'sideEffectsCache'
| 'sideEffectsCacheReadonly'
| 'sort'
| 'sharedWorkspaceLockfile'
| 'tag'
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
| 'supportedArchitectures'
| 'packageConfigs'
> & CreateStoreControllerOptions & Partial<Pick<Config, 'globalPkgDir'>> & {
  argv: {
    original: string[]
  }
  fixLockfile?: boolean
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
} & Partial<Pick<Config, 'ci' | 'modulesCacheMaxAge' | 'pnpmHomeDir' | 'preferWorkspacePackages' | 'useLockfile' | 'symlink'>>

export async function handler (opts: InstallCommandOptions & { _calledFromLink?: boolean }): Promise<void> {
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
    frozenLockfileIfExists: opts.frozenLockfileIfExists ?? (
      opts.ci && !opts.lockfileOnly &&
      typeof opts.frozenLockfile === 'undefined' &&
      typeof opts.preferFrozenLockfile === 'undefined'
    ),
    include,
    includeDirect: include,
    fetchFullMetadata: getFetchFullMetadata(opts),
  }
  if (opts.resolutionOnly) {
    installDepsOptions.lockfileOnly = true
    installDepsOptions.forceFullResolution = true
  }
  return installDeps(installDepsOptions, [])
}
