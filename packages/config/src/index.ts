import path from 'path'
import fs from 'fs'
import os from 'os'
import { LAYOUT_VERSION } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import globalBinDir from '@pnpm/global-bin-dir'
import { requireHooks } from '@pnpm/pnpmfile'
import { safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import camelcase from 'camelcase'
import loadNpmConf from '@zkochan/npm-conf'
import npmTypes from '@zkochan/npm-conf/lib/types'
import { sync as canWriteToDir } from 'can-write-to-dir'
import normalizeRegistryUrl from 'normalize-registry-url'
import fromPairs from 'ramda/src/fromPairs'
import realpathMissing from 'realpath-missing'
import whichcb from 'which'
import getScopeRegistries from './getScopeRegistries'
import findBestGlobalPrefix from './findBestGlobalPrefix'
import { getCacheDir, getConfigDir, getDataDir, getStateDir } from './dirs'
import {
  Config,
  ConfigWithDeprecatedSettings,
  UniversalOptions,
} from './Config'
import { getWorkspaceConcurrency } from './concurrency'

export { Config, UniversalOptions }

const npmDefaults = loadNpmConf.defaults

const PNPM_GLOBAL = 'pnpm-global'

async function which (cmd: string) {
  return new Promise<string>((resolve, reject) => {
    whichcb(cmd, (err, resolvedPath) => (err != null) ? reject(err) : resolve(resolvedPath!))
  })
}

export const types = Object.assign({
  bail: Boolean,
  'cache-dir': String,
  'child-concurrency': Number,
  color: ['always', 'auto', 'never'],
  'config-dir': String,
  dev: [null, true],
  dir: String,
  'enable-modules-dir': Boolean,
  'enable-pre-post-scripts': Boolean,
  'extend-node-path': Boolean,
  'fetch-timeout': Number,
  'fetching-concurrency': Number,
  filter: [String, Array],
  'filter-prod': [String, Array],
  'frozen-lockfile': Boolean,
  'frozen-shrinkwrap': Boolean,
  'git-checks': Boolean,
  'global-bin-dir': String,
  'global-dir': String,
  'global-path': String,
  'global-pnpmfile': String,
  hoist: Boolean,
  'hoist-pattern': Array,
  'ignore-pnpmfile': Boolean,
  'ignore-workspace-root-check': Boolean,
  'link-workspace-packages': [Boolean, 'deep'],
  lockfile: Boolean,
  'lockfile-dir': String,
  'lockfile-directory': String, // TODO: deprecate
  'lockfile-only': Boolean,
  loglevel: ['silent', 'error', 'warn', 'info', 'debug'],
  maxsockets: Number,
  'modules-cache-max-age': Number,
  'modules-dir': String,
  'network-concurrency': Number,
  'node-linker': ['pnp', 'isolated', 'hoisted'],
  noproxy: String,
  'npm-path': String,
  offline: Boolean,
  'package-import-method': ['auto', 'hardlink', 'clone', 'copy'],
  pnpmfile: String,
  'prefer-frozen-lockfile': Boolean,
  'prefer-frozen-shrinkwrap': Boolean,
  'prefer-offline': Boolean,
  'prefer-workspace-packages': Boolean,
  production: [null, true],
  'public-hoist-pattern': Array,
  'publish-branch': String,
  'recursive-install': Boolean,
  reporter: String,
  'aggregate-output': Boolean,
  'save-peer': Boolean,
  'save-workspace-protocol': Boolean,
  'script-shell': String,
  'shamefully-flatten': Boolean,
  'shamefully-hoist': Boolean,
  'shared-workspace-lockfile': Boolean,
  'shared-workspace-shrinkwrap': Boolean,
  'shell-emulator': Boolean,
  'shrinkwrap-directory': String,
  'shrinkwrap-only': Boolean,
  'side-effects-cache': Boolean,
  'side-effects-cache-readonly': Boolean,
  symlink: Boolean,
  sort: Boolean,
  'state-dir': String,
  store: String, // TODO: deprecate
  'store-dir': String,
  stream: Boolean,
  'strict-peer-dependencies': Boolean,
  'use-beta-cli': Boolean,
  'use-node-version': String,
  'use-running-store-server': Boolean,
  'use-store-server': Boolean,
  'use-stderr': Boolean,
  'verify-store-integrity': Boolean,
  'virtual-store-dir': String,
  'workspace-concurrency': Number,
  'workspace-packages': [String, Array],
  'workspace-root': Boolean,
  'test-pattern': [String, Array],
  'changed-files-ignore-pattern': [String, Array],
}, npmTypes.types)

export type CliOptions = Record<string, unknown> & { dir?: string }

export default async (
  opts: {
    globalDirShouldAllowWrite?: boolean
    cliOptions: CliOptions
    packageManager: {
      name: string
      version: string
    }
    rcOptionsTypes?: Record<string, unknown>
    workspaceDir?: string | undefined
    checkUnknownSetting?: boolean
  }
): Promise<{ config: Config, warnings: string[] }> => {
  const packageManager = opts.packageManager ?? { name: 'pnpm', version: 'undefined' }
  const cliOptions = opts.cliOptions ?? {}
  const warnings = new Array<string>()

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
  const npmConfig = loadNpmConf(cliOptions, rcOptionsTypes, {
    bail: true,
    color: 'auto',
    'enable-modules-dir': true,
    'extend-node-path': true,
    'fetch-retries': 2,
    'fetch-retry-factor': 10,
    'fetch-retry-maxtimeout': 60000,
    'fetch-retry-mintimeout': 10000,
    'fetch-timeout': 60000,
    globalconfig: npmDefaults.globalconfig,
    hoist: true,
    'hoist-pattern': ['*'],
    'ignore-workspace-root-check': false,
    'link-workspace-packages': true,
    'modules-cache-max-age': 7 * 24 * 60, // 7 days
    'package-lock': npmDefaults['package-lock'],
    pending: false,
    'prefer-workspace-packages': false,
    'public-hoist-pattern': [
      // Packages like @types/node, @babel/types
      // should be publicly hoisted because TypeScript only searches in the root of node_modules
      '*types*',
      '*eslint*',
      '@prettier/plugin-*',
      '*prettier-plugin-*',
    ],
    'recursive-install': true,
    registry: npmDefaults.registry,
    'save-peer': false,
    'save-workspace-protocol': true,
    'scripts-prepend-node-path': false,
    symlink: true,
    'shared-workspace-lockfile': true,
    'shared-workspace-shrinkwrap': true,
    'shell-emulator': false,
    shrinkwrap: npmDefaults.shrinkwrap,
    reverse: false,
    sort: true,
    'strict-peer-dependencies': false,
    'unsafe-perm': npmDefaults['unsafe-perm'],
    'use-beta-cli': false,
    userconfig: npmDefaults.userconfig,
    'virtual-store-dir': 'node_modules/.pnpm',
    'workspace-concurrency': 4,
    'workspace-prefix': opts.workspaceDir,
  })

  npmConfig.addFile(path.resolve(path.join(__dirname, 'pnpmrc')), 'pnpm-builtin')

  delete cliOptions.prefix

  process.execPath = originalExecPath

  const rcOptions = Object.keys(rcOptionsTypes)

  const pnpmConfig: ConfigWithDeprecatedSettings = fromPairs([
    ...rcOptions.map((configKey) => [camelcase(configKey), npmConfig.get(configKey)]) as any, // eslint-disable-line
    ...Object.entries(cliOptions).filter(([name, value]) => typeof value !== 'undefined').map(([name, value]) => [camelcase(name), value]),
  ]) as unknown as ConfigWithDeprecatedSettings
  const cwd = (cliOptions.dir && path.resolve(cliOptions.dir)) ?? npmConfig.localPrefix

  pnpmConfig.maxSockets = npmConfig.maxsockets
  delete pnpmConfig['maxsockets']

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
  pnpmConfig.registries = {
    default: normalizeRegistryUrl(pnpmConfig.rawConfig.registry),
    ...getScopeRegistries(pnpmConfig.rawConfig),
  }
  pnpmConfig.lockfileDir = pnpmConfig.lockfileDir ?? pnpmConfig.lockfileDirectory ?? pnpmConfig.shrinkwrapDirectory
  pnpmConfig.useLockfile = (() => {
    if (typeof pnpmConfig['lockfile'] === 'boolean') return pnpmConfig['lockfile']
    if (typeof pnpmConfig['packageLock'] === 'boolean') return pnpmConfig['packageLock']
    if (typeof pnpmConfig['shrinkwrap'] === 'boolean') return pnpmConfig['shrinkwrap']
    return false
  })()
  pnpmConfig.lockfileOnly = typeof pnpmConfig['lockfileOnly'] === 'undefined'
    ? pnpmConfig.shrinkwrapOnly
    : pnpmConfig['lockfileOnly']
  pnpmConfig.frozenLockfile = typeof pnpmConfig['frozenLockfile'] === 'undefined'
    ? pnpmConfig.frozenShrinkwrap
    : pnpmConfig['frozenLockfile']
  pnpmConfig.preferFrozenLockfile = typeof pnpmConfig['preferFrozenLockfile'] === 'undefined'
    ? pnpmConfig.preferFrozenShrinkwrap
    : pnpmConfig['preferFrozenLockfile']
  pnpmConfig.sharedWorkspaceLockfile = typeof pnpmConfig['sharedWorkspaceLockfile'] === 'undefined'
    ? pnpmConfig.sharedWorkspaceShrinkwrap
    : pnpmConfig['sharedWorkspaceLockfile']
  pnpmConfig.pnpmHomeDir = getDataDir(process)

  if (cliOptions['global']) {
    const knownGlobalBinDirCandidates: string[] = []
    let globalDirRoot
    if (pnpmConfig['globalDir']) {
      globalDirRoot = pnpmConfig['globalDir']
    } else if (pnpmConfig.useBetaCli) {
      globalDirRoot = path.join(pnpmConfig.pnpmHomeDir, 'global-packages')
    } else {
      let npmGlobalPrefix: string = findBestGlobalPrefix(npmConfig.globalPrefix, process.env)
      const globalDirName = `${path.sep}${PNPM_GLOBAL}${path.sep}`
      if (npmGlobalPrefix.includes(globalDirName)) {
        npmGlobalPrefix = npmGlobalPrefix.substring(0, npmGlobalPrefix.indexOf(globalDirName))
      } else {
        const npmGlobalBinDir = process.platform === 'win32'
          ? npmGlobalPrefix
          : path.resolve(npmGlobalPrefix, 'bin')
        knownGlobalBinDirCandidates.push(npmGlobalBinDir)
      }
      globalDirRoot = path.join(firstWithWriteAccess([npmGlobalPrefix, os.homedir()]), PNPM_GLOBAL)
    }
    pnpmConfig.dir = path.join(globalDirRoot, LAYOUT_VERSION.toString())

    const npmConfigGlobalBinDir = npmConfig.get('global-bin-dir')
    if (typeof npmConfigGlobalBinDir === 'string') {
      fs.mkdirSync(npmConfigGlobalBinDir, { recursive: true })
      pnpmConfig.bin = npmConfigGlobalBinDir
    } else {
      pnpmConfig.bin = cliOptions.dir
        ? (
          process.platform === 'win32'
            ? cliOptions.dir
            : path.resolve(cliOptions.dir, 'bin')
        )
        : globalBinDir(knownGlobalBinDirCandidates, { shouldAllowWrite: opts.globalDirShouldAllowWrite === true })
    }
    pnpmConfig.save = true
    pnpmConfig.allowNew = true
    pnpmConfig.ignoreCurrentPrefs = true
    pnpmConfig.saveProd = true
    pnpmConfig.saveDev = false
    pnpmConfig.saveOptional = false
    pnpmConfig.extendNodePath = false
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
    delete pnpmConfig.virtualStoreDir
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
  if (pnpmConfig['shamefullyFlatten']) {
    warnings.push('The "shamefully-flatten" setting has been renamed to "shamefully-hoist". Also, in most cases you won\'t need "shamefully-hoist". Since v4, a semistrict node_modules structure is on by default (via hoist-pattern=[*]).')
    pnpmConfig.shamefullyHoist = true
  }
  if (!pnpmConfig.storeDir && pnpmConfig['store']) {
    warnings.push('The "store" setting has been renamed to "store-dir". Please use the new name.')
    pnpmConfig.storeDir = pnpmConfig['store']
  }
  if (!pnpmConfig.cacheDir) {
    pnpmConfig.cacheDir = getCacheDir(process)
  }
  if (!pnpmConfig.stateDir) {
    pnpmConfig.stateDir = getStateDir(process)
  }
  if (!pnpmConfig.configDir) {
    pnpmConfig.configDir = getConfigDir(process)
  }
  if (pnpmConfig['hoist'] === false) {
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
        // @ts-expect-error
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
    pnpmConfig.noProxy = pnpmConfig['noproxy'] ?? getProcessEnv('no_proxy')
  }
  pnpmConfig.enablePnp = pnpmConfig.nodeLinker === 'pnp'
  if (!pnpmConfig.userConfig) {
    pnpmConfig.userConfig = npmConfig.sources.user?.data
  }

  if (opts.checkUnknownSetting) {
    const settingKeys = Object.keys({
      ...npmConfig?.sources?.workspace?.data,
      ...npmConfig?.sources?.project?.data,
    }).filter(key => key.trim() !== '')
    const unknownKeys = []
    for (const key of settingKeys) {
      if (!rcOptions.includes(key) && !key.startsWith('//') && !(key.startsWith('@') && key.endsWith(':registry'))) {
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
  pnpmConfig.rootProjectManifest = await safeReadProjectManifestOnly(pnpmConfig.lockfileDir ?? pnpmConfig.dir) ?? undefined

  return { config: pnpmConfig, warnings }
}

function getProcessEnv (env: string) {
  return process.env[env] ??
    process.env[env.toUpperCase()] ??
    process.env[env.toLowerCase()]
}

function firstWithWriteAccess (dirs: string[]) {
  const first = dirs.find((dir) => {
    try {
      return canWriteToDir(dir)
    } catch (err: any) { // eslint-disable-line
      if (err.code !== 'ENOENT') throw err
    }
    try {
      fs.mkdirSync(dir, { recursive: true })
      return true
    } catch (err: any) { // eslint-disable-line
      return false
    }
  })
  if (first == null) {
    throw new PnpmError('NO_SUITABLE_GLOBAL_DIR', `pnpm has no write access to global direcotry. Tried locations: ${dirs.join(', ')}`)
  }
  return first
}
