import { parseAllowBuildSelector } from '@pnpm/building.policy'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import { FILTERING, OPTIONS, UNIVERSAL_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl } from '@pnpm/cli.utils'
import { types as allTypes } from '@pnpm/config.reader'
import { writeSettings } from '@pnpm/config.writer'
import { PnpmError } from '@pnpm/error'
import { handleGlobalAdd, selectsPnpmCli } from '@pnpm/global.commands'
import { resolveConfigDeps } from '@pnpm/installing.env-installer'
import { linkedDirectoryPath } from '@pnpm/resolving.local-resolver'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import { createStoreController } from '@pnpm/store.connection-manager'
import { safeReadProjectManifestOnly } from '@pnpm/workspace.project-manifest-reader'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import type { InstallCommandOptions } from './install.js'
import { installDeps } from './installDeps.js'
import { createGlobalPolicyCallbacks } from './resolutionPolicyManifest.js'
import { warnAboutLinkedPeerDependencies } from './warnAboutLinkedPeerDependencies.js'

export const shorthands: Record<string, string> = {
  'save-catalog': '--save-catalog-name=default',
  d: '--save-dev',
  e: '--save-exact',
  o: '--save-optional',
  p: '--save-prod',
}

const RC_OPTION_NAMES = [
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
  'hoisting-limits',
  'https-proxy',
  'ignore-pnpmfile',
  'ignore-scripts',
  'ignore-workspace-root-check',
  'libc',
  'link-workspace-packages',
  'lockfile-dir',
  'lockfile-only',
  'lockfile',
  'modules-dir',
  'network-concurrency',
  'node-experimental-package-map',
  'node-package-map-type',
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
  'unsafe-perm',
  'offline',
  'only',
  'optional',
  'verify-store-integrity',
  'virtual-store-dir',
] as const

export function rcOptionsTypes (): Record<string, unknown> {
  return pick(RC_OPTION_NAMES, allTypes)
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

const OPTIONS_HELP_LIST = [
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
  {
    description: 'The registry to use for the installation',
    name: '--registry <url>',
  },
  OPTIONS.storeDir,
  OPTIONS.virtualStoreDir,
  OPTIONS.globalDir,
  ...UNIVERSAL_OPTIONS,
  {
    description: 'A list of package names that are allowed to run postinstall scripts during installation. Prefix a name with ! to deny its scripts instead',
    name: '--allow-build',
  },
]

export function help (): string {
  return renderHelp({
    description: 'Installs a package and any packages that it depends on.',
    descriptionLists: [
      {
        title: 'Options',

        list: OPTIONS_HELP_LIST,
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
  params: string[],
  commands?: CommandHandlerMap
): Promise<void> {
  assertAddParams(opts, params)
  if (opts.config) {
    await addConfigDependencies(opts, params)
    return
  }
  assertNotAddingToWorkspaceRoot(opts)
  const allowBuildSelectors = parseAllowBuildSelectors(opts)
  if (opts.global) {
    return addGlobally({ allowBuildSelectors, commands, opts, params })
  }

  const include = {
    dependencies: opts.production !== false,
    devDependencies: opts.dev !== false,
    optionalDependencies: opts.optional !== false,
  }
  if (allowBuildSelectors.length) {
    const allowBuilds = await saveAllowedBuilds(opts, allowBuildSelectors)
    await installDeps({
      ...opts,
      allowBuilds,
      rebuildHandler: commands?.rebuild,
      include,
      includeDirect: include,
      // `--dry-run` is an `install`-only preview; never let a config-level
      // `dry-run` turn `add` into a no-op check.
      dryRun: false,
    }, params)
  } else {
    await installDeps({
      ...opts,
      rebuildHandler: commands?.rebuild,
      include,
      includeDirect: include,
      dryRun: false,
    }, params)
  }
  await Promise.all(params.map(async (param) => warnIfLinkedWithPeers(param, opts.dir)))
}

type AllowBuildSelector = ReturnType<typeof parseAllowBuildSelector>

function assertAddParams (opts: AddCommandOptions, params: string[]): void {
  if (opts.cliOptions['save'] === false) {
    throw new PnpmError('OPTION_NOT_SUPPORTED', 'The "add" command currently does not support the no-save option')
  }
  if (!params || (params.length === 0)) {
    throw new PnpmError('MISSING_PACKAGE_NAME', '`pnpm add` requires the package name')
  }
}

async function addConfigDependencies (opts: AddCommandOptions, params: string[]): Promise<void> {
  const store = await createStoreController(opts)
  await resolveConfigDeps(params, {
    ...opts,
    store: store.ctrl,
    storeDir: store.dir,
    rootDir: opts.workspaceDir ?? opts.rootProjectManifestDir,
  })
}

function assertNotAddingToWorkspaceRoot (opts: AddCommandOptions): void {
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
}

function parseAllowBuildSelectors (opts: AddCommandOptions): AllowBuildSelector[] {
  const allowBuildSelectors = opts.allowBuild?.map(parseAllowBuildSelector) ?? []
  if (
    allowBuildSelectors.length &&
    (opts.argv.original.includes('--allow-build') || allowBuildSelectors.some(({ name }) => name === ''))
  ) {
    throw new PnpmError('ALLOW_BUILD_MISSING_PACKAGE', 'The --allow-build flag is missing a package name. Please specify the package name(s) that are allowed to run installation scripts.')
  }
  return allowBuildSelectors
}

interface AddGloballyOptions {
  allowBuildSelectors: AllowBuildSelector[]
  commands?: CommandHandlerMap
  opts: AddCommandOptions
  params: string[]
}

async function addGlobally ({ allowBuildSelectors, commands, opts, params }: AddGloballyOptions): Promise<void> {
  if (!opts.bin) {
    throw new PnpmError('NO_GLOBAL_BIN_DIR', 'Unable to find the global bin directory', {
      hint: 'Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH.',
    })
  }
  if (selectsPnpmCli(params)) {
    throw new PnpmError('GLOBAL_PNPM_INSTALL', 'Use the "pnpm self-update" command to install or update pnpm')
  }
  return handleGlobalAdd({
    ...opts,
    allowBuilds: applyAllowBuildSelectors(opts.allowBuilds, allowBuildSelectors),
    ...createGlobalPolicyCallbacks(opts),
  }, params, commands ?? {})
}

async function saveAllowedBuilds (
  opts: AddCommandOptions,
  allowBuildSelectors: AllowBuildSelector[]
): Promise<Record<string, boolean | string>> {
  if (opts.allowBuilds) {
    assertNoIgnoredBuildIsAllowed(opts.allowBuilds, allowBuildSelectors)
  }
  const allowBuilds = applyAllowBuildSelectors(opts.allowBuilds, allowBuildSelectors)
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
  return allowBuilds
}

function assertNoIgnoredBuildIsAllowed (
  allowBuilds: Record<string, boolean | string>,
  allowBuildSelectors: AllowBuildSelector[]
): void {
  const disallowedBuilds = Object.entries(allowBuilds)
    .filter(([, value]) => value === false)
    .map(([pkg]) => pkg)
  const allowedOnly = allowBuildSelectors.filter(({ allowed }) => allowed).map(({ name }) => name)
  const overlapDependencies = disallowedBuilds.filter((dep) => allowedOnly.includes(dep))
  if (overlapDependencies.length) {
    throw new PnpmError('OVERRIDING_IGNORED_BUILT_DEPENDENCIES', `The following dependencies are ignored by the root project, but are allowed to be built by the current command: ${overlapDependencies.join(', ')}`, {
      hint: 'If you are sure you want to allow those dependencies to run installation scripts, remove them from the allowBuilds list (or change their value to true).',
    })
  }
}

/**
 * A directory added without an alias (`pnpm add ../pkg`) is saved as a
 * `link:` dependency, so it gets the same peer dependency warning as
 * `pnpm link`. Selectors of any other kind, and a directory without a
 * manifest, produce no warning.
 */
async function warnIfLinkedWithPeers (param: string, projectDir: string): Promise<void> {
  const { alias, bareSpecifier } = parseWantedDependency(param)
  if (alias != null || bareSpecifier == null) return
  const pkgDir = linkedDirectoryPath(bareSpecifier, projectDir)
  if (pkgDir == null) return
  warnAboutLinkedPeerDependencies(await safeReadProjectManifestOnly(pkgDir), { pkgDir, prefix: projectDir })
}

function applyAllowBuildSelectors (
  allowBuilds: Record<string, boolean | string> | undefined,
  selectors: AllowBuildSelector[]
): Record<string, boolean | string> {
  const updated = { ...allowBuilds }
  for (const { name, allowed } of selectors) {
    updated[name] = allowed
  }
  return updated
}
