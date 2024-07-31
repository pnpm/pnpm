import path from 'path'
import fs from 'fs'
import os from 'os'
import { getCatalogsFromWorkspaceManifest } from '@pnpm/catalogs.config'
import { LAYOUT_VERSION } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import loadNpmConf from '@pnpm/npm-conf'
import type npmTypes from '@pnpm/npm-conf/lib/types'
import { requireHooks } from '@pnpm/pnpmfile'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import { getCurrentBranch } from '@pnpm/git-utils'
import { createMatcher } from '@pnpm/matcher'
import betterPathResolve from 'better-path-resolve'
import camelcase from 'camelcase'
import isWindows from 'is-windows'
import normalizeRegistryUrl from 'normalize-registry-url'
import realpathMissing from 'realpath-missing'
import pathAbsolute from 'path-absolute'
import which from 'which'
import { inheritAuthConfig } from './auth'
import { checkGlobalBinDir } from './checkGlobalBinDir'
import { getNetworkConfigs } from './getNetworkConfigs'
import { getCacheDir, getConfigDir, getDataDir, getStateDir } from './dirs'
import {
  type Config,
  type ConfigWithDeprecatedSettings,
  type UniversalOptions,
} from './Config'
import { getWorkspaceConcurrency } from './concurrency'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'

import { types } from './types'
export { types }

export { getOptionsFromRootManifest, type OptionsFromRootManifest } from './getOptionsFromRootManifest'
export * from './readLocalConfig'

export type { Config, UniversalOptions }

type CamelToKebabCase<S extends string> = S extends `${infer T}${infer U}`
  ? `${T extends Capitalize<T> ? '-' : ''}${Lowercase<T>}${CamelToKebabCase<U>}`
  : S

type KebabCaseConfig = {
  [K in keyof ConfigWithDeprecatedSettings as CamelToKebabCase<K>]: ConfigWithDeprecatedSettings[K];
} | typeof npmTypes.types

const npmDefaults = loadNpmConf.defaults

export type CliOptions = Record<string, unknown> & { dir?: string, json?: boolean }

export async function getConfig (opts: {
  globalDirShouldAllowWrite?: boolean
  cliOptions: CliOptions
  packageManager: {
    name: string
    version: string
  }
  rcOptionsTypes?: Record<string, unknown>
  workspaceDir?: string | undefined
  checkUnknownSetting?: boolean
  env?: Record<string, string | undefined>
  ignoreNonAuthSettingsFromLocal?: boolean
}): Promise<{ config: Config, warnings: string[] }> {
  if (opts.ignoreNonAuthSettingsFromLocal) {
    const { ignoreNonAuthSettingsFromLocal: _, ...authOpts } = opts
    const globalCfgOpts: typeof authOpts = {
      ...authOpts,
      cliOptions: {
        ...authOpts.cliOptions,
        dir: os.homedir(),
      },
    }
    const [final, authSrc] = await Promise.all([getConfig(globalCfgOpts), getConfig(authOpts)])
    inheritAuthConfig(final.config, authSrc.config)
    final.warnings.push(...authSrc.warnings)
    return final
  }

  const env = opts.env ?? process.env
  const packageManager = opts.packageManager ?? { name: 'pnpm', version: 'undefined' }
  const cliOptions = opts.cliOptions ?? {}

  if (cliOptions['hoist'] === false) {
    if (cliOptions['shamefully-hoist'] === true) {
      throw new PnpmError('CONFIG_CONFLICT_HOIST', '--shamefully-hoist cannot be used with --no-hoist')
    }
    if (cliOptions['shamefully-flatten'] === true) {
      throw new PnpmError('CONFIG_CONFLICT_HOIST', '--shamefully-flatten cannot be used with --no-hoist')
    }
    if (cliOptions['hoist-pattern']) {
      throw new PnpmError('CONFIG_CONFLICT_HOIST', '--hoist-pattern cannot be used with --no-hoist')
    }
  }

  // This is what npm does as well, overriding process.execPath with the resolved location of Node.
  // The value of process.execPath is changed only for the duration of config initialization.
  // Otherwise, npmConfig.globalPrefix would sometimes have the bad location.
  //
  // TODO: use this workaround only during global installation
  const originalExecPath = process.execPath
  try {
    const node = await which(process.argv[0])
    if (node.toUpperCase() !== process.execPath.toUpperCase()) {
      process.execPath = node
    }
  } catch (err) { } // eslint-disable-line:no-empty

  if (cliOptions.dir) {
    cliOptions.dir = await realpathMissing(cliOptions.dir)
    cliOptions['prefix'] = cliOptions.dir // the npm config system still expects `prefix`
  }
  const rcOptionsTypes = { ...types, ...opts.rcOptionsTypes }
  const defaultOptions: Partial<KebabCaseConfig> | typeof npmTypes.types = {
    'auto-install-peers': true,
    bail: true,
    color: 'auto',
    'deploy-all-files': false,
    'dedupe-peer-dependents': true,
    'dedupe-direct-deps': false,
    'dedupe-injected-deps': true,
    'disallow-workspace-cycles': false,
    'enable-modules-dir': true,
    'enable-pre-post-scripts': true,
    'exclude-links-from-lockfile': false,
    'extend-node-path': true,
    'fail-if-no-match': false,
    'fetch-retries': 2,
    'fetch-retry-factor': 10,
    'fetch-retry-maxtimeout': 60000,
    'fetch-retry-mintimeout': 10000,
    'fetch-timeout': 60000,
    'git-shallow-hosts': [
      // Follow https://github.com/npm/git/blob/1e1dbd26bd5b87ca055defecc3679777cb480e2a/lib/clone.js#L13-L19
      'github.com',
      'gist.github.com',
      'gitlab.com',
      'bitbucket.com',
      'bitbucket.org',
    ],
    globalconfig: npmDefaults.globalconfig,
    'git-branch-lockfile': false,
    hoist: true,
    'hoist-pattern': ['*'],
    'hoist-workspace-packages': true,
    'ignore-workspace-cycles': false,
    'ignore-workspace-root-check': false,
    'link-workspace-packages': false,
    'lockfile-include-tarball-url': false,
    'modules-cache-max-age': 7 * 24 * 60, // 7 days
    'dlx-cache-max-age': 24 * 60, // 1 day
    'node-linker': 'isolated',
    'package-lock': npmDefaults['package-lock'],
    pending: false,
    'package-manager-strict': process.env.COREPACK_ENABLE_STRICT !== '0',
    'package-manager-strict-version': false,
    'prefer-workspace-packages': false,
    'public-hoist-pattern': [
      '*eslint*',
      '*prettier*',
    ],
    'recursive-install': true,
    registry: npmDefaults.registry,
    'resolution-mode': 'highest',
    'resolve-peers-from-workspace-root': true,
    'save-peer': false,
    'save-workspace-protocol': 'rolling',
    'scripts-prepend-node-path': false,
    'side-effects-cache': true,
    symlink: true,
    'shared-workspace-lockfile': true,
    'shell-emulator': false,
    'strict-store-pkg-content-check': true,
    reverse: false,
    sort: true,
    'strict-peer-dependencies': false,
    'unsafe-perm': npmDefaults['unsafe-perm'],
    'use-beta-cli': false,
    userconfig: npmDefaults.userconfig,
    'verify-store-integrity': true,
    'virtual-store-dir': 'node_modules/.pnpm',
    'workspace-concurrency': 4,
    'workspace-prefix': opts.workspaceDir,
    'embed-readme': false,
    'registry-supports-time-field': false,
    'virtual-store-dir-max-length': 120,
    'peers-suffix-max-length': 1000,
  }

  const { config: npmConfig, warnings, failedToLoadBuiltInConfig } = loadNpmConf(cliOptions, rcOptionsTypes, defaultOptions)

  const configDir = getConfigDir(process)
  {
    const warn = npmConfig.addFile(path.join(configDir as string, 'rc'), 'pnpm-global')
    if (warn) warnings.push(warn)
  }
  {
    const warn = npmConfig.addFile(path.resolve(path.join(__dirname, 'pnpmrc')), 'pnpm-builtin')
    if (warn) warnings.push(warn)
  }

  delete cliOptions.prefix

  process.execPath = originalExecPath

  const rcOptions = Object.keys(rcOptionsTypes)

  const pnpmConfig: ConfigWithDeprecatedSettings = Object.fromEntries([
    ...rcOptions.map((configKey) => [camelcase(configKey, { locale: 'en-US' }), npmConfig.get(configKey)]) as any, // eslint-disable-line
    ...Object.entries(cliOptions).filter(([name, value]) => typeof value !== 'undefined').map(([name, value]) => [camelcase(name, { locale: 'en-US' }), value]),
  ]) as unknown as ConfigWithDeprecatedSettings
  // Resolving the current working directory to its actual location is crucial.
  // This prevents potential inconsistencies in the future, especially when processing or mapping subdirectories.
  const cwd = fs.realpathSync(betterPathResolve(cliOptions.dir ?? npmConfig.localPrefix))

  pnpmConfig.maxSockets = npmConfig.maxsockets
  // @ts-expect-error
  delete pnpmConfig['maxsockets']

  pnpmConfig.configDir = configDir
  pnpmConfig.workspaceDir = opts.workspaceDir
  pnpmConfig.workspaceRoot = cliOptions['workspace-root'] as boolean // This is needed to prevent pnpm reading workspaceRoot from env variables
  pnpmConfig.rawLocalConfig = Object.assign.apply(Object, [
    {},
    ...npmConfig.list.slice(3, pnpmConfig.workspaceDir && pnpmConfig.workspaceDir !== cwd ? 5 : 4).reverse(),
    cliOptions,
  ] as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  pnpmConfig.userAgent = pnpmConfig.rawLocalConfig['user-agent']
    ? pnpmConfig.rawLocalConfig['user-agent']
    : `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`
  pnpmConfig.rawConfig = Object.assign.apply(Object, [
    { registry: 'https://registry.npmjs.org/' },
    ...[...npmConfig.list].reverse(),
    cliOptions,
    { 'user-agent': pnpmConfig.userAgent },
  ] as any) // eslint-disable-line @typescript-eslint/no-explicit-any
  const networkConfigs = getNetworkConfigs(pnpmConfig.rawConfig)
  pnpmConfig.registries = {
    default: normalizeRegistryUrl(pnpmConfig.rawConfig.registry),
    ...networkConfigs.registries,
  }
  pnpmConfig.sslConfigs = networkConfigs.sslConfigs
  pnpmConfig.useLockfile = (() => {
    if (typeof pnpmConfig.lockfile === 'boolean') return pnpmConfig.lockfile
    if (typeof pnpmConfig.packageLock === 'boolean') return pnpmConfig.packageLock
    return false
  })()
  pnpmConfig.useGitBranchLockfile = (() => {
    if (typeof pnpmConfig.gitBranchLockfile === 'boolean') return pnpmConfig.gitBranchLockfile
    return false
  })()
  pnpmConfig.mergeGitBranchLockfiles = await (async () => {
    if (typeof pnpmConfig.mergeGitBranchLockfiles === 'boolean') return pnpmConfig.mergeGitBranchLockfiles
    if (pnpmConfig.mergeGitBranchLockfilesBranchPattern != null && pnpmConfig.mergeGitBranchLockfilesBranchPattern.length > 0) {
      const branch = await getCurrentBranch()
      if (branch) {
        const branchMatcher = createMatcher(pnpmConfig.mergeGitBranchLockfilesBranchPattern)
        return branchMatcher(branch)
      }
    }
    return undefined
  })()
  pnpmConfig.pnpmHomeDir = getDataDir(process)

  if (cliOptions['global']) {
    let globalDirRoot
    if (pnpmConfig.globalDir) {
      globalDirRoot = pnpmConfig.globalDir
    } else {
      globalDirRoot = path.join(pnpmConfig.pnpmHomeDir, 'global')
    }
    pnpmConfig.dir = path.join(globalDirRoot, LAYOUT_VERSION.toString())

    pnpmConfig.bin = npmConfig.get('global-bin-dir') ?? env.PNPM_HOME
    if (pnpmConfig.bin) {
      fs.mkdirSync(pnpmConfig.bin, { recursive: true })
      await checkGlobalBinDir(pnpmConfig.bin, { env, shouldAllowWrite: opts.globalDirShouldAllowWrite })
    }
    pnpmConfig.save = true
    pnpmConfig.allowNew = true
    pnpmConfig.ignoreCurrentPrefs = true
    pnpmConfig.saveProd = true
    pnpmConfig.saveDev = false
    pnpmConfig.saveOptional = false
    if ((pnpmConfig.hoistPattern != null) && (pnpmConfig.hoistPattern.length > 1 || pnpmConfig.hoistPattern[0] !== '*')) {
      if (opts.cliOptions['hoist-pattern']) {
        throw new PnpmError('CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL',
          'Configuration conflict. "hoist-pattern" may not be used with "global"')
      }
    }
    if (pnpmConfig.linkWorkspacePackages) {
      if (opts.cliOptions['link-workspace-packages']) {
        throw new PnpmError('CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL',
          'Configuration conflict. "link-workspace-packages" may not be used with "global"')
      }
      pnpmConfig.linkWorkspacePackages = false
    }
    if (pnpmConfig.sharedWorkspaceLockfile) {
      if (opts.cliOptions['shared-workspace-lockfile']) {
        throw new PnpmError('CONFIG_CONFLICT_SHARED_WORKSPACE_LOCKFILE_WITH_GLOBAL',
          'Configuration conflict. "shared-workspace-lockfile" may not be used with "global"')
      }
      pnpmConfig.sharedWorkspaceLockfile = false
    }
    if (pnpmConfig.lockfileDir) {
      if (opts.cliOptions['lockfile-dir']) {
        throw new PnpmError('CONFIG_CONFLICT_LOCKFILE_DIR_WITH_GLOBAL',
          'Configuration conflict. "lockfile-dir" may not be used with "global"')
      }
      delete pnpmConfig.lockfileDir
    }
    if (opts.cliOptions['virtual-store-dir']) {
      throw new PnpmError('CONFIG_CONFLICT_VIRTUAL_STORE_DIR_WITH_GLOBAL',
        'Configuration conflict. "virtual-store-dir" may not be used with "global"')
    }
    pnpmConfig.virtualStoreDir = '.pnpm'
  } else {
    pnpmConfig.dir = cwd
    pnpmConfig.bin = path.join(pnpmConfig.dir, 'node_modules', '.bin')
  }
  if (opts.cliOptions['save-peer']) {
    if (opts.cliOptions['save-prod']) {
      throw new PnpmError('CONFIG_CONFLICT_PEER_CANNOT_BE_PROD_DEP', 'A package cannot be a peer dependency and a prod dependency at the same time')
    }
    if (opts.cliOptions['save-optional']) {
      throw new PnpmError('CONFIG_CONFLICT_PEER_CANNOT_BE_OPTIONAL_DEP',
        'A package cannot be a peer dependency and an optional dependency at the same time')
    }
  }
  if (pnpmConfig.sharedWorkspaceLockfile && !pnpmConfig.lockfileDir && pnpmConfig.workspaceDir) {
    pnpmConfig.lockfileDir = pnpmConfig.workspaceDir
  }

  pnpmConfig.packageManager = packageManager

  if (env.NODE_ENV) {
    if (cliOptions.production) {
      pnpmConfig.only = 'production'
    }
    if (cliOptions.dev) {
      pnpmConfig.only = 'dev'
    }
  }

  if (pnpmConfig.only === 'prod' || pnpmConfig.only === 'production' || !pnpmConfig.only && pnpmConfig.production) {
    pnpmConfig.production = true
    pnpmConfig.dev = false
  } else if (pnpmConfig.only === 'dev' || pnpmConfig.only === 'development' || pnpmConfig.dev) {
    pnpmConfig.production = false
    pnpmConfig.dev = true
    pnpmConfig.optional = false
  } else {
    pnpmConfig.production = true
    pnpmConfig.dev = true
  }

  if (typeof pnpmConfig.filter === 'string') {
    pnpmConfig.filter = (pnpmConfig.filter as string).split(' ')
  }

  if (typeof pnpmConfig.filterProd === 'string') {
    pnpmConfig.filterProd = (pnpmConfig.filterProd as string).split(' ')
  }

  if (!pnpmConfig.ignoreScripts && pnpmConfig.workspaceDir) {
    pnpmConfig.extraBinPaths = [path.join(pnpmConfig.workspaceDir, 'node_modules', '.bin')]
  } else {
    pnpmConfig.extraBinPaths = []
  }

  if (pnpmConfig.preferSymlinkedExecutables && !isWindows()) {
    const cwd = pnpmConfig.lockfileDir ?? pnpmConfig.dir

    const virtualStoreDir = pnpmConfig.virtualStoreDir
      ? pnpmConfig.virtualStoreDir
      : pnpmConfig.modulesDir
        ? path.join(pnpmConfig.modulesDir, '.pnpm')
        : 'node_modules/.pnpm'

    pnpmConfig.extraEnv = {
      NODE_PATH: pathAbsolute(path.join(virtualStoreDir, 'node_modules'), cwd),
    }
  }

  if (pnpmConfig.shamefullyFlatten) {
    warnings.push('The "shamefully-flatten" setting has been renamed to "shamefully-hoist". Also, in most cases you won\'t need "shamefully-hoist". Since v4, a semistrict node_modules structure is on by default (via hoist-pattern=[*]).')
    pnpmConfig.shamefullyHoist = true
  }
  if (!pnpmConfig.cacheDir) {
    pnpmConfig.cacheDir = getCacheDir(process)
  }
  if (!pnpmConfig.stateDir) {
    pnpmConfig.stateDir = getStateDir(process)
  }
  // eslint-disable-next-line @typescript-eslint/no-unnecessary-boolean-literal-compare
  if (pnpmConfig.hoist === false) {
    delete pnpmConfig.hoistPattern
  }
  switch (pnpmConfig.shamefullyHoist) {
  case false:
    delete pnpmConfig.publicHoistPattern
    break
  case true:
    pnpmConfig.publicHoistPattern = ['*']
    break
  default:
    if (
      (pnpmConfig.publicHoistPattern == null) ||
        (pnpmConfig.publicHoistPattern === '') ||
        (
          Array.isArray(pnpmConfig.publicHoistPattern) &&
          pnpmConfig.publicHoistPattern.length === 1 &&
          pnpmConfig.publicHoistPattern[0] === ''
        )
    ) {
      delete pnpmConfig.publicHoistPattern
    }
    break
  }
  if (!pnpmConfig.symlink) {
    delete pnpmConfig.hoistPattern
    delete pnpmConfig.publicHoistPattern
  }
  if (typeof pnpmConfig['color'] === 'boolean') {
    switch (pnpmConfig['color']) {
    case true:
      pnpmConfig.color = 'always'
      break
    case false:
      pnpmConfig.color = 'never'
      break
    default:
      pnpmConfig.color = 'auto'
      break
    }
  }
  if (!pnpmConfig.httpsProxy) {
    pnpmConfig.httpsProxy = pnpmConfig.proxy ?? getProcessEnv('https_proxy')
  }
  if (!pnpmConfig.httpProxy) {
    pnpmConfig.httpProxy = pnpmConfig.httpsProxy ?? getProcessEnv('http_proxy') ?? getProcessEnv('proxy')
  }
  if (!pnpmConfig.noProxy) {
    // @ts-expect-error
    pnpmConfig.noProxy = pnpmConfig['noproxy'] ?? getProcessEnv('no_proxy')
  }
  switch (pnpmConfig.nodeLinker) {
  case 'pnp':
    pnpmConfig.enablePnp = pnpmConfig.nodeLinker === 'pnp'
    break
  case 'hoisted':
    if (pnpmConfig.preferSymlinkedExecutables == null) {
      pnpmConfig.preferSymlinkedExecutables = true
    }
    break
  }
  if (!pnpmConfig.userConfig) {
    pnpmConfig.userConfig = npmConfig.sources.user?.data
  }
  pnpmConfig.sideEffectsCacheRead = pnpmConfig.sideEffectsCache ?? pnpmConfig.sideEffectsCacheReadonly
  pnpmConfig.sideEffectsCacheWrite = pnpmConfig.sideEffectsCache

  if (opts.checkUnknownSetting) {
    const settingKeys = Object.keys({
      ...npmConfig?.sources?.workspace?.data,
      ...npmConfig?.sources?.project?.data,
    }).filter(key => key.trim() !== '')
    const unknownKeys = []
    for (const key of settingKeys) {
      if (!rcOptions.includes(key) && !key.startsWith('//') && !(key[0] === '@' && key.endsWith(':registry'))) {
        unknownKeys.push(key)
      }
    }
    if (unknownKeys.length > 0) {
      warnings.push(`Your .npmrc file contains unknown setting: ${unknownKeys.join(', ')}`)
    }
  }

  pnpmConfig.workspaceConcurrency = getWorkspaceConcurrency(pnpmConfig.workspaceConcurrency)

  if (!pnpmConfig.ignorePnpmfile) {
    pnpmConfig.hooks = requireHooks(pnpmConfig.lockfileDir ?? pnpmConfig.dir, pnpmConfig)
  }
  pnpmConfig.rootProjectManifestDir = pnpmConfig.lockfileDir ?? pnpmConfig.workspaceDir ?? pnpmConfig.dir
  pnpmConfig.rootProjectManifest = await safeReadProjectManifestOnly(pnpmConfig.rootProjectManifestDir) ?? undefined
  if (pnpmConfig.rootProjectManifest?.workspaces?.length && !pnpmConfig.workspaceDir) {
    warnings.push('The "workspaces" field in package.json is not supported by pnpm. Create a "pnpm-workspace.yaml" file instead.')
  }

  if (pnpmConfig.workspaceDir != null) {
    const workspaceManifest = await readWorkspaceManifest(pnpmConfig.workspaceDir)

    pnpmConfig.workspacePackagePatterns = cliOptions['workspace-packages'] as string[] ?? workspaceManifest?.packages
    pnpmConfig.catalogs = getCatalogsFromWorkspaceManifest(workspaceManifest)
  }

  pnpmConfig.failedToLoadBuiltInConfig = failedToLoadBuiltInConfig

  return { config: pnpmConfig, warnings }
}

function getProcessEnv (env: string): string | undefined {
  return process.env[env] ??
    process.env[env.toUpperCase()] ??
    process.env[env.toLowerCase()]
}
