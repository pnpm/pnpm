import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { PnpmError } from '@pnpm/error'
import pick from 'ramda/src/pick'
import renderHelp from 'render-help'
import { type InstallCommandOptions } from './install'
import { installDeps } from './installDeps'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'cache-dir',
    'child-concurrency',
    'engine-strict',
    'fetch-retries',
    'fetch-retry-factor',
    'fetch-retry-maxtimeout',
    'fetch-retry-mintimeout',
    'fetch-timeout',
    'force',
    'global-bin-dir',
    'global-dir',
    'global-pnpmfile',
    'global',
    'hoist',
    'hoist-pattern',
    'https-proxy',
    'ignore-pnpmfile',
    'ignore-scripts',
    'ignore-workspace-root-check',
    'link-workspace-packages',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'modules-dir',
    'network-concurrency',
    'node-linker',
    'noproxy',
    'npmPath',
    'package-import-method',
    'pnpmfile',
    'prefer-offline',
    'production',
    'proxy',
    'public-hoist-pattern',
    'registry',
    'reporter',
    'save-dev',
    'save-exact',
    'save-optional',
    'save-peer',
    'save-prefix',
    'save-prod',
    'save-workspace-protocol',
    'shamefully-flatten',
    'shamefully-hoist',
    'shared-workspace-lockfile',
    'side-effects-cache-readonly',
    'side-effects-cache',
    'store',
    'store-dir',
    'strict-peer-dependencies',
    'unsafe-perm',
    'offline',
    'only',
    'optional',
    'use-running-store-server',
    'use-store-server',
    'verify-store-integrity',
    'virtual-store-dir',
  ], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    recursive: Boolean,
    save: Boolean,
    workspace: Boolean,
  }
}

export const commandNames = ['add']

export function help (): string {
  return renderHelp({
    description: 'Installs a package and any packages that it depends on.',
    descriptionLists: [
      {
        title: 'Options',

        list: [
          {
            description: 'Save package to your `dependencies`. The default behavior',
            name: '--save-prod',
            shortAlias: '-P',
          },
          {
            description: 'Save package to your `devDependencies`',
            name: '--save-dev',
            shortAlias: '-D',
          },
          {
            description: 'Save package to your `optionalDependencies`',
            name: '--save-optional',
            shortAlias: '-O',
          },
          {
            description: 'Save package to your `peerDependencies` and `devDependencies`',
            name: '--save-peer',
          },
          {
            description: 'Install exact version',
            name: '--[no-]save-exact',
            shortAlias: '-E',
          },
          {
            description: 'Save packages from the workspace with a "workspace:" protocol. True by default',
            name: '--[no-]save-workspace-protocol',
          },
          {
            description: 'Install as a global package',
            name: '--global',
            shortAlias: '-g',
          },
          {
            description: 'Run installation recursively in every package found in subdirectories \
or in every workspace package, when executed inside a workspace. \
For options that may be used with `-r`, see "pnpm help recursive"',
            name: '--recursive',
            shortAlias: '-r',
          },
          {
            description: 'Only adds the new dependency if it is found in the workspace',
            name: '--workspace',
          },
          OPTIONS.ignoreScripts,
          OPTIONS.offline,
          OPTIONS.preferOffline,
          OPTIONS.storeDir,
          OPTIONS.virtualStoreDir,
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
        ],
      },
      FILTERING,
    ],
    url: docsUrl('add'),
    usages: [
      'pnpm add <name>',
      'pnpm add <name>@<tag>',
      'pnpm add <name>@<version>',
      'pnpm add <name>@<version range>',
      'pnpm add <git host>:<git user>/<repo name>',
      'pnpm add <git repo url>',
      'pnpm add <tarball file>',
      'pnpm add <tarball url>',
      'pnpm add <dir>',
    ],
  })
}

export type AddCommandOptions = InstallCommandOptions & {
  allowNew?: boolean
  ignoreWorkspaceRootCheck?: boolean
  save?: boolean
  update?: boolean
  useBetaCli?: boolean
  workspaceRoot?: boolean
}

export async function handler (
  opts: AddCommandOptions,
  params: string[]
): Promise<void> {
  if (opts.cliOptions['save'] === false) {
    throw new PnpmError('OPTION_NOT_SUPPORTED', 'The "add" command currently does not support the no-save option')
  }
  if (!params || (params.length === 0)) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm add` requires the package name')
  }
  if (
    !opts.recursive &&
    opts.workspaceDir === opts.dir &&
    !opts.ignoreWorkspaceRootCheck &&
    !opts.workspaceRoot
  ) {
    throw new PnpmError('ADDING_TO_ROOT',
      'Running this command will add the dependency to the workspace root, ' +
      'which might not be what you want - if you really meant it, ' +
      'make it explicit by running this command again with the -w flag (or --workspace-root). ' +
      'If you don\'t want to see this warning anymore, you may set the ignore-workspace-root-check setting to true.'
    )
  }
  if (opts.global && !opts.bin) {
    throw new PnpmError('NO_GLOBAL_BIN_DIR', 'Unable to find the global bin directory', {
      hint: 'Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH.',
    })
  }

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  return installDeps({
    ...opts,
    include,
    includeDirect: include,
  }, params)
}
