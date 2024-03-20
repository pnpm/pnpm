import {
  normalizeRegistries,
  DEFAULT_REGISTRIES,
} from '@pnpm/normalize-registries'
import { PnpmError } from '@pnpm/error'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { createReadPackageHook } from '@pnpm/hooks.read-package-hook'
import type { InstallOptions, ProcessedInstallOptions, StrictInstallOptions } from '@pnpm/types'

import { pnpmPkgJson } from '../pnpmPkgJson'

function defaults(opts: InstallOptions): StrictInstallOptions {
  const packageManager = opts.packageManager ?? {
    name: pnpmPkgJson?.name ?? '',
    version: pnpmPkgJson?.version ?? '',
  }

  return {
    allowedDeprecatedVersions: {},
    allowNonAppliedPatches: false,
    autoInstallPeers: true,
    autoInstallPeersFromHighestMatch: false,
    childConcurrency: 5,
    confirmModulesPurge: !opts.force,
    depth: 0,
    enablePnp: false,
    engineStrict: false,
    force: false,
    forceFullResolution: false,
    forceSharedLockfile: false,
    frozenLockfile: false,
    hoistPattern: undefined,
    publicHoistPattern: undefined,
    hooks: {},
    ignoreCurrentPrefs: false,
    ignoreDepScripts: false,
    ignoreScripts: false,
    include: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    includeDirect: {
      dependencies: true,
      devDependencies: true,
      optionalDependencies: true,
    },
    lockfileDir: opts.lockfileDir ?? opts.dir ?? process.cwd(),
    lockfileOnly: false,
    nodeVersion: process.version,
    nodeLinker: 'isolated',
    overrides: {},
    ownLifecycleHooksStdio: 'inherit',
    ignoreCompatibilityDb: false,
    ignorePackageManifest: false,
    packageExtensions: {},
    packageManager,
    preferFrozenLockfile: true,
    preferWorkspacePackages: false,
    preserveWorkspaceProtocol: true,
    pruneLockfileImporters: false,
    pruneStore: false,
    rawConfig: {},
    registries: DEFAULT_REGISTRIES,
    resolutionMode: 'lowest-direct',
    saveWorkspaceProtocol: 'rolling',
    lockfileIncludeTarballUrl: false,
    scriptsPrependNodePath: false,
    shamefullyHoist: false,
    shellEmulator: false,
    sideEffectsCacheRead: false,
    sideEffectsCacheWrite: false,
    symlink: true,
    storeController: opts.storeController,
    storeDir: opts.storeDir,
    strictPeerDependencies: true,
    tag: 'latest',
    unsafePerm: process.platform === 'win32' ||
      process.platform === 'cygwin' ||
      !process.setgid ||
      process.getuid?.() !== 0,
    useLockfile: true,
    saveLockfile: true,
    useGitBranchLockfile: false,
    mergeGitBranchLockfiles: false,
    userAgent: `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`,
    verifyStoreIntegrity: true,
    workspacePackages: {},
    enableModulesDir: true,
    modulesCacheMaxAge: 7 * 24 * 60,
    resolveSymlinksInInjectedDirs: false,
    dedupeDirectDeps: true,
    dedupePeerDependents: true,
    resolvePeersFromWorkspaceRoot: true,
    extendNodePath: true,
    ignoreWorkspaceCycles: false,
    disallowWorkspaceCycles: false,
    excludeLinksFromLockfile: false,
  }
}

export function extendOptions(opts: InstallOptions): ProcessedInstallOptions {
  if (opts) {
    for (const key in opts) {
      if (opts[key as keyof InstallOptions] === undefined) {
        delete opts[key as keyof InstallOptions]
      }
    }
  }

  if (opts.onlyBuiltDependencies && opts.neverBuiltDependencies) {
    throw new PnpmError(
      'CONFIG_CONFLICT_BUILT_DEPENDENCIES',
      'Cannot have both neverBuiltDependencies and onlyBuiltDependencies'
    )
  }

  const defaultOpts = defaults(opts)

  const extendedOpts: ProcessedInstallOptions = {
    ...defaultOpts,
    ...opts,
    storeDir: defaultOpts.storeDir,
  }

  extendedOpts.readPackageHook = createReadPackageHook({
    ignoreCompatibilityDb: extendedOpts.ignoreCompatibilityDb,
    readPackageHook: extendedOpts.hooks?.readPackage,
    overrides: extendedOpts.overrides,
    lockfileDir: extendedOpts.lockfileDir,
    packageExtensions: extendedOpts.packageExtensions,
    peerDependencyRules: extendedOpts.peerDependencyRules,
  })

  if (extendedOpts.lockfileOnly) {
    extendedOpts.ignoreScripts = true

    if (!extendedOpts.useLockfile) {
      throw new PnpmError(
        'CONFIG_CONFLICT_LOCKFILE_ONLY_WITH_NO_LOCKFILE',
        `Cannot generate a ${WANTED_LOCKFILE} because lockfile is set to false`
      )
    }
  }

  if (extendedOpts.userAgent.startsWith('npm/')) {
    extendedOpts.userAgent = `${extendedOpts.packageManager.name}/${extendedOpts.packageManager.version} ${extendedOpts.userAgent}`
  }

  extendedOpts.registries = normalizeRegistries(extendedOpts.registries)

  extendedOpts.rawConfig.registry = extendedOpts.registries.default

  return extendedOpts
}
