import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { Config, types as allTypes } from '@pnpm/config'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { CreateStoreControllerOptions } from '@pnpm/store-connection-manager'
import installDeps from './installDeps'
import isCI = require('is-ci')
import R = require('ramda')
import renderHelp = require('render-help')

export function rcOptionsTypes () {
  return R.pick([
    'child-concurrency',
    'dev',
    'engine-strict',
    'fetch-retries',
    'fetch-retry-factor',
    'fetch-retry-maxtimeout',
    'fetch-retry-mintimeout',
    'frozen-lockfile',
    'global-dir',
    'global-pnpmfile',
    'global',
    'hoist',
    'hoist-pattern',
    'ignore-pnpmfile',
    'ignore-scripts',
    'link-workspace-packages',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'modules-dir',
    'network-concurrency',
    'package-import-method',
    'pnpmfile',
    'prefer-frozen-lockfile',
    'prefer-offline',
    'production',
    'public-hoist-pattern',
    'registry',
    'reporter',
    'save-workspace-protocol',
    'shamefully-flatten',
    'shamefully-hoist',
    'shared-workspace-lockfile',
    'side-effects-cache-readonly',
    'side-effects-cache',
    'store',
    'store-dir',
    'strict-peer-dependencies',
    'offline',
    'only',
    'optional',
    'unsafe-perm',
    'use-running-store-server',
    'use-store-server',
    'verify-store-integrity',
    'virtual-store-dir',
  ], allTypes)
}

export const cliOptionsTypes = () => ({
  ...rcOptionsTypes(),
  ...R.pick(['force'], allTypes),
  recursive: Boolean,
})

export const shorthands = {
  D: '--dev',
  P: '--production',
}

export const commandNames = ['install', 'i']

export function help () {
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
            description: 'Only `devDependencies` are installed regardless of the `NODE_ENV`',
            name: '--dev',
            shortAlias: '-D',
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
            description: "Don't generate a lockfile and fail if an update is needed",
            name: '--frozen-lockfile',
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
            description: 'Disable pnpm hooks defined in pnpmfile.js',
            name: '--ignore-pnpmfile',
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
            description: 'Starts a store server in the background. The store server will keep running after installation is done. To stop the store server, run `pnpm server stop`',
            name: '--use-store-server',
          },
          {
            description: 'Only allows installation with a store server. If no store server is running, installation will fail',
            name: '--use-running-store-server',
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
          ...UNIVERSAL_OPTIONS,
        ],
      },
      {
        title: 'Output',

        list: [
          {
            description: 'No output is logged to the console, except fatal errors',
            name: '--silent, --reporter silent',
            shortAlias: '-s',
          },
          {
            description: 'The default reporter when the stdout is TTY',
            name: '--reporter default',
          },
          {
            description: 'The output is always appended to the end. No cursor manipulations are performed',
            name: '--reporter append-only',
          },
          {
            description: 'The most verbose reporter. Prints all logs in ndjson format',
            name: '--reporter ndjson',
          },
        ],
      },
      FILTERING,
      {
        title: 'Experimental options',

        list: [
          {
            description: 'Use or cache the results of (pre/post)install hooks',
            name: '--side-effects-cache',
          },
          {
            description: 'Only use the side effects cache if present, do not create it for new packages',
            name: '--side-effects-cache-readonly',
          },
        ],
      },
    ],
    url: docsUrl('install'),
    usages: ['pnpm install [options]'],
  })
}

export type InstallCommandOptions = Pick<Config,
| 'allProjects'
| 'bail'
| 'bin'
| 'cliOptions'
| 'depth'
| 'dev'
| 'engineStrict'
| 'global'
| 'globalPnpmfile'
| 'ignorePnpmfile'
| 'ignoreScripts'
| 'linkWorkspacePackages'
| 'rawLocalConfig'
| 'lockfileDir'
| 'lockfileOnly'
| 'pnpmfile'
| 'production'
| 'rawLocalConfig'
| 'registries'
| 'save'
| 'saveDev'
| 'saveExact'
| 'saveOptional'
| 'savePeer'
| 'savePrefix'
| 'saveProd'
| 'saveWorkspaceProtocol'
| 'selectedProjectsGraph'
| 'sideEffectsCache'
| 'sideEffectsCacheReadonly'
| 'sort'
| 'sharedWorkspaceLockfile'
| 'tag'
| 'optional'
| 'workspaceConcurrency'
| 'workspaceDir'
> & CreateStoreControllerOptions & {
  argv: {
    original: string[]
  }
  useBetaCli?: boolean
  recursive?: boolean
  workspace?: boolean
} & Partial<Pick<Config, 'preferWorkspacePackages'>>

export function handler (
  opts: InstallCommandOptions
) {
  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  return installDeps({
    ...opts,
    frozenLockfileIfExists: isCI &&
      typeof opts.rawLocalConfig['frozen-lockfile'] === 'undefined' &&
      typeof opts.rawLocalConfig['prefer-frozen-lockfile'] === 'undefined',
    include,
    includeDirect: include,
  }, [])
}
