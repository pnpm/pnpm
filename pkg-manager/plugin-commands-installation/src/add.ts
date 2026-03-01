import { docsUrl } from '@pnpm/cli-utils'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/common-cli-options-help'
import { types as allTypes } from '@pnpm/config'
import { resolveConfigDeps } from '@pnpm/config.deps-installer'
import { PnpmError } from '@pnpm/error'
import { handleGlobalAdd } from '@pnpm/global.commands'
import { createStoreController } from '@pnpm/store-connection-manager'
import { pick } from 'ramda'
import renderHelp from 'render-help'
import { getFetchFullMetadata } from './getFetchFullMetadata.js'
import { type InstallCommandOptions } from './install.js'
import { installDeps } from './installDeps.js'
import { writeSettings } from '@pnpm/config.config-writer'

export const shorthands: Record<string, string> = {
  'save-catalog': '--save-catalog-name=default',
  d: '--save-dev',
  e: '--save-exact',
  o: '--save-optional',
  p: '--save-prod',
}

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'cache-dir',
    'cpu',
    'child-concurrency',
    'dangerously-allow-all-builds',
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
    'libc',
    'link-workspace-packages',
    'lockfile-dir',
    'lockfile-directory',
    'lockfile-only',
    'lockfile',
    'modules-dir',
    'network-concurrency',
    'node-linker',
    'noproxy',
    'npm-path',
    'os',
    'package-import-method',
    'pnpmfile',
    'prefer-offline',
    'production',
    'proxy',
    'public-hoist-pattern',
    'registry',
    'reporter',
    'save-catalog-name',
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
    'store-dir',
    'strict-peer-dependencies',
    'trust-policy',
    'trust-policy-exclude',
    'trust-policy-ignore-after',
    'unsafe-perm',
    'offline',
    'only',
    'optional',
    'verify-store-integrity',
    'virtual-store-dir',
  ], allTypes)
}

export function cliOptionsTypes (): Record<string, unknown> {
  return {
    ...rcOptionsTypes(),
    'allow-build': [String, Array],
    recursive: Boolean,
    save: Boolean,
    workspace: Boolean,
    config: Boolean,
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
            shortAlias: '-p',
          },
          {
            description: 'Save package to your `devDependencies`',
            name: '--save-dev',
            shortAlias: '-d',
          },
          {
            description: 'Save package to your `optionalDependencies`',
            name: '--save-optional',
            shortAlias: '-o',
          },
          {
            description: 'Save package to your `peerDependencies` and `devDependencies`',
            name: '--save-peer',
          },
          {
            description: 'Save package to the default catalog',
            name: '--save-catalog',
          },
          {
            description: 'Save package to the specified catalog',
            name: '--save-catalog-name=<name>',
          },
          {
            description: 'Install exact version',
            name: '--[no-]save-exact',
            shortAlias: '-e',
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
          {
            description: 'Save the dependency to configurational dependencies',
            name: '--config',
          },
          OPTIONS.ignoreScripts,
          OPTIONS.offline,
          OPTIONS.preferOffline,
          OPTIONS.storeDir,
          OPTIONS.virtualStoreDir,
          OPTIONS.globalDir,
          ...UNIVERSAL_OPTIONS,
          {
            description: 'A list of package names that are allowed to run postinstall scripts during installation',
            name: '--allow-build',
          },
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
  allowBuild?: string[]
  allowNew?: boolean
  ignoreWorkspaceRootCheck?: boolean
  save?: boolean
  update?: boolean
  useBetaCli?: boolean
  workspaceRoot?: boolean
  config?: boolean
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
  if (opts.config) {
    const store = await createStoreController(opts)
    await resolveConfigDeps(params, {
      ...opts,
      store: store.ctrl,
      rootDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
    })
    return
  }
  if (
    !opts.recursive &&
    opts.workspaceDir === opts.dir &&
    !opts.ignoreWorkspaceRootCheck &&
    !opts.workspaceRoot &&
    opts.workspacePackagePatterns &&
    opts.workspacePackagePatterns.length > 1
  ) {
    throw new PnpmError('ADDING_TO_ROOT',
      'Running this command will add the dependency to the workspace root, ' +
      'which might not be what you want - if you really meant it, ' +
      'make it explicit by running this command again with the -w flag (or --workspace-root). ' +
      'If you don\'t want to see this warning anymore, you may set the ignore-workspace-root-check setting to true.'
    )
  }
  if (opts.global) {
    if (!opts.bin) {
      throw new PnpmError('NO_GLOBAL_BIN_DIR', 'Unable to find the global bin directory', {
        hint: 'Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH.',
      })
    }
    if (params.includes('pnpm') || params.includes('@pnpm/exe')) {
      throw new PnpmError('GLOBAL_PNPM_INSTALL', 'Use the "pnpm self-update" command to install or update pnpm')
    }
    return handleGlobalAdd(opts, params)
  }

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  if (opts.allowBuild?.length) {
    if (opts.argv.original.includes('--allow-build')) {
      throw new PnpmError('ALLOW_BUILD_MISSING_PACKAGE', 'The --allow-build flag is missing a package name. Please specify the package name(s) that are allowed to run installation scripts.')
    }
    if (opts.rootProjectManifest?.pnpm?.allowBuilds) {
      const disallowedBuilds = Object.keys(opts.rootProjectManifest.pnpm.allowBuilds)
        .filter(pkg => opts.rootProjectManifest!.pnpm!.allowBuilds![pkg] === false)
      const overlapDependencies = disallowedBuilds.filter((dep) => opts.allowBuild?.includes(dep))
      if (overlapDependencies.length) {
        throw new PnpmError('OVERRIDING_IGNORED_BUILT_DEPENDENCIES', `The following dependencies are ignored by the root project, but are allowed to be built by the current command: ${overlapDependencies.join(', ')}`, {
          hint: 'If you are sure you want to allow those dependencies to run installation scripts, remove them from the pnpm.allowBuilds list (or change their value to true).',
        })
      }
    }
    const allowBuilds: Record<string, boolean> = {}
    for (const pkg of opts.allowBuild) {
      allowBuilds[pkg] = true
    }
    if (opts.rootProjectManifestDir) {
      opts.rootProjectManifest = opts.rootProjectManifest ?? {}
      await writeSettings({
        ...opts,
        workspaceDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
        updatedSettings: {
          allowBuilds,
        },
      })
    }
    // Pass the allowed packages to allowBuilds so they can build during this install
    const mergedAllowBuilds = { ...opts.allowBuilds }
    for (const pkg of opts.allowBuild) {
      mergedAllowBuilds[pkg] = true
    }
    return installDeps({
      ...opts,
      allowBuilds: mergedAllowBuilds,
      fetchFullMetadata: getFetchFullMetadata(opts),
      include,
      includeDirect: include,
    }, params)
  }
  return installDeps({
    ...opts,
    fetchFullMetadata: getFetchFullMetadata(opts),
    include,
    includeDirect: include,
  }, params)
}
