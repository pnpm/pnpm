import loadNpmConf = require('@zkochan/npm-conf')
import npmTypes = require('@zkochan/npm-conf/lib/types')
import camelcase from 'camelcase'
import findUp = require('find-up')
import path = require('path')
import whichcb = require('which')
import getScopeRegistries from './getScopeRegistries'
import { PnpmConfigs } from './PnpmConfigs'

export { PnpmConfigs }

const npmDefaults = loadNpmConf.defaults

function which (cmd: string) {
  return new Promise<string>((resolve, reject) => {
    whichcb(cmd, (err: Error, resolvedPath: string) => err ? reject(err) : resolve(resolvedPath))
  })
}

export const types = Object.assign({
  'background': Boolean,
  'bail': Boolean,
  'child-concurrency': Number,
  'dev': [null, true],
  'fetching-concurrency': Number,
  'filter': [String, Array],
  'frozen-lockfile': Boolean,
  'frozen-shrinkwrap': Boolean,
  'global-path': path,
  'global-pnpmfile': String,
  'ignore-pnpmfile': Boolean,
  'ignore-stop-requests': Boolean,
  'ignore-upload-requests': Boolean,
  'independent-leaves': Boolean,
  'link-workspace-packages': Boolean,
  'lock': Boolean,
  'lock-stale-duration': Number,
  'lockfile': Boolean,
  'lockfile-directory': path,
  'lockfile-only': Boolean,
  'network-concurrency': Number,
  'offline': Boolean,
  'package-import-method': ['auto', 'hardlink', 'reflink', 'copy'],
  'pending': Boolean,
  'pnpmfile': String,
  'port': Number,
  'prefer-frozen-lockfile': Boolean,
  'prefer-frozen-shrinkwrap': Boolean,
  'prefer-offline': Boolean,
  'production': [null, true],
  'protocol': ['auto', 'tcp', 'ipc'],
  'reporter': String,
  'shamefully-flatten': Boolean,
  'shared-workspace-lockfile': Boolean,
  'shared-workspace-shrinkwrap': Boolean,
  'shrinkwrap-directory': path,
  'shrinkwrap-only': Boolean,
  'side-effects-cache': Boolean,
  'side-effects-cache-readonly': Boolean,
  'sort': Boolean,
  'store': path,
  'strict-peer-dependencies': Boolean,
  'use-running-store-server': Boolean,
  'use-store-server': Boolean,
  'verify-store-integrity': Boolean,
  'workspace-concurrency': Number,
  'workspace-prefix': String,
}, npmTypes.types)

const WORKSPACE_MANIFEST_FILENAME = 'pnpm-workspace.yaml'

export default async (
  opts: {
    cliArgs: object,
    packageManager: {
      name: string,
      version: string,
    },
  },
): Promise<PnpmConfigs> => {
  const packageManager = opts && opts.packageManager || { name: 'pnpm', version: 'undefined' }
  const cliArgs = opts && opts.cliArgs || {}

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
  } catch (err) {} // tslint:disable-line:no-empty

  const workspaceManifestLocation = await findUp(WORKSPACE_MANIFEST_FILENAME, {
    cwd: cliArgs['prefix'] || process.cwd(), // tslint:disable-line
  })
  const npmConfig = loadNpmConf(cliArgs, types, {
    'bail': true,
    'fetch-retries': 2,
    'fetch-retry-factor': 10,
    'fetch-retry-maxtimeout': 60000,
    'fetch-retry-mintimeout': 10000,
    'globalconfig': npmDefaults.globalconfig,
    'link-workspace-packages': true,
    'lock': true,
    'package-lock': npmDefaults['package-lock'],
    'pending': false,
    'prefix': npmDefaults.prefix,
    'registry': npmDefaults.registry,
    'shared-workspace-shrinkwrap': true,
    'shrinkwrap': npmDefaults.shrinkwrap,
    'sort': true,
    'strict-peer-dependencies': false,
    'unsafe-perm': npmDefaults['unsafe-perm'],
    'userconfig': npmDefaults.userconfig,
    'workspace-concurrency': 4,
    'workspace-prefix': workspaceManifestLocation && path.dirname(workspaceManifestLocation),
  })

  process.execPath = originalExecPath

  if (!cliArgs['user-agent']) {
    cliArgs['user-agent'] = `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`
  }
  const pnpmConfig: PnpmConfigs = Object.keys(types) // tslint:disable-line
    .reduce((acc, configKey) => {
      acc[camelcase(configKey)] = typeof cliArgs[configKey] !== 'undefined'
        ? cliArgs[configKey]
        : npmConfig.get(configKey)
      return acc
    }, {} as PnpmConfigs)
  pnpmConfig.rawNpmConfig = Object.assign.apply(Object, npmConfig.list.reverse().concat([cliArgs]))
  pnpmConfig.registries = {
    default: pnpmConfig.registry || 'https://registry.npmjs.org/',
    ...getScopeRegistries(pnpmConfig.rawNpmConfig),
  }
  const npmGlobalPrefix: string = pnpmConfig.rawNpmConfig['pnpm-prefix'] ||
    (process.platform === 'win32' && process.env.APPDATA
      ? path.join(process.env.APPDATA, 'npm')
      : npmConfig.globalPrefix)
  pnpmConfig.globalBin = process.platform === 'win32'
    ? npmGlobalPrefix
    : path.resolve(npmGlobalPrefix, 'bin')
  pnpmConfig.globalPrefix = path.join(npmGlobalPrefix, 'pnpm-global')
  pnpmConfig.lockfileDirectory = typeof pnpmConfig['lockfileDirectory'] === 'undefined'
    ? pnpmConfig.shrinkwrapDirectory
    : pnpmConfig['lockfileDirectory']
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

  if (pnpmConfig.global) {
    const independentLeavesSuffix = pnpmConfig.independentLeaves ? '_independent_leaves' : ''
    const shamefullyFlattenSuffix = pnpmConfig.shamefullyFlatten ? '_shamefully_flatten' : ''
    const subfolder = '2' + independentLeavesSuffix + shamefullyFlattenSuffix
    pnpmConfig.prefix = path.join(pnpmConfig.globalPrefix, subfolder)
    pnpmConfig.bin = pnpmConfig.globalBin
    pnpmConfig.allowNew = true
    pnpmConfig.ignoreCurrentPrefs = true
    pnpmConfig.saveProd = true
    pnpmConfig.saveDev = false
    pnpmConfig.saveOptional = false
    if (pnpmConfig.linkWorkspacePackages) {
      if (opts.cliArgs['link-workspace-packages']) {
        const err = new Error('Configuration conflict. "link-workspace-packages" may not be used with "global"')
        err['code'] = 'ERR_PNPM_CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL' // tslint:disable-line:no-string-literal
        throw err
      }
      pnpmConfig.linkWorkspacePackages = false
    }
    if (pnpmConfig.sharedWorkspaceLockfile) {
      if (opts.cliArgs['shared-workspace-lockfile'] || opts.cliArgs['shared-workspace-shrinkwrap']) {
        const err = new Error('Configuration conflict. "shared-workspace-lockfile" may not be used with "global"')
        err['code'] = 'ERR_PNPM_CONFIG_CONFLICT_SHARED_WORKSPACE_LOCKFILE_WITH_GLOBAL' // tslint:disable-line:no-string-literal
        throw err
      }
      pnpmConfig.sharedWorkspaceLockfile = false
    }
    if (pnpmConfig.lockfileDirectory) {
      if (opts.cliArgs['lockfile-directory'] || opts.cliArgs['shrinkwrap-directory']) {
        const err = new Error('Configuration conflict. "lockfile-directory" may not be used with "global"')
        err['code'] = 'ERR_PNPM_CONFIG_CONFLICT_LOCKFILE_DIRECTORY_WITH_GLOBAL' // tslint:disable-line:no-string-literal
        throw err
      }
      delete pnpmConfig.lockfileDirectory
    }
  } else {
    pnpmConfig.prefix = (cliArgs['prefix'] ? path.resolve(cliArgs['prefix']) : npmConfig.localPrefix) // tslint:disable-line
    pnpmConfig.bin = path.join(pnpmConfig.prefix, 'node_modules', '.bin')
  }
  if (pnpmConfig.sharedWorkspaceLockfile && !pnpmConfig.lockfileDirectory) {
    pnpmConfig.lockfileDirectory = pnpmConfig.workspacePrefix || undefined
  }

  pnpmConfig.packageManager = packageManager

  if (pnpmConfig.only === 'prod' || pnpmConfig.only === 'production' || !pnpmConfig.only && pnpmConfig.production) {
    pnpmConfig.production = true
    pnpmConfig.development = false
  } else if (pnpmConfig.only === 'dev' || pnpmConfig.only === 'development' || pnpmConfig.dev) {
    pnpmConfig.production = false
    pnpmConfig.development = true
    pnpmConfig.optional = false
  } else {
    pnpmConfig.production = true
    pnpmConfig.development = true
  }

  if (typeof pnpmConfig.filter === 'string') {
    pnpmConfig.filter = (pnpmConfig.filter as string).split(' ')
  }

  pnpmConfig.sideEffectsCacheRead = pnpmConfig.sideEffectsCache || pnpmConfig.sideEffectsCacheReadonly
  pnpmConfig.sideEffectsCacheWrite = pnpmConfig.sideEffectsCache

  return pnpmConfig
}
