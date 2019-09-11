import { LAYOUT_VERSION } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import loadNpmConf = require('@zkochan/npm-conf')
import npmTypes = require('@zkochan/npm-conf/lib/types')
import camelcase from 'camelcase'
import findUp = require('find-up')
import path = require('path')
import whichcb = require('which')
import findBestGlobalPrefixOnWindows from './findBestGlobalPrefixOnWindows'
import getScopeRegistries, { normalizeRegistry } from './getScopeRegistries'
import { PnpmConfigs } from './PnpmConfigs'

export { PnpmConfigs }

const npmDefaults = loadNpmConf.defaults

function which (cmd: string) {
  return new Promise<string>((resolve, reject) => {
    whichcb(cmd, (err, resolvedPath) => err ? reject(err) : resolve(resolvedPath))
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
  'hoist': Boolean,
  'hoist-pattern': String,
  'ignore-pnpmfile': Boolean,
  'ignore-stop-requests': Boolean,
  'ignore-upload-requests': Boolean,
  'ignore-workspace-root-check': Boolean,
  'independent-leaves': Boolean,
  'latest': Boolean,
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
  'resolution-strategy': ['fast', 'fewer-dependencies'],
  'save-peer': Boolean,
  'shamefully-flatten': Boolean,
  'shamefully-hoist': Boolean,
  'shared-workspace-lockfile': Boolean,
  'shared-workspace-shrinkwrap': Boolean,
  'shrinkwrap-directory': path,
  'shrinkwrap-only': Boolean,
  'side-effects-cache': Boolean,
  'side-effects-cache-readonly': Boolean,
  'sort': Boolean,
  'store': path,
  'strict-peer-dependencies': Boolean,
  'use-beta-cli': Boolean,
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
    // The canonical names of commands. "pnpm i -r"=>"pnpm recursive install"
    command?: string[],
    packageManager: {
      name: string,
      version: string,
    },
  },
): Promise<{configs: PnpmConfigs, warnings: string[]}> => {
  const packageManager = opts && opts.packageManager || { name: 'pnpm', version: 'undefined' }
  const cliArgs = opts && opts.cliArgs || {}
  const command = opts.command || []
  const warnings = new Array<string>()

  switch (command[command.length - 1]) {
    case 'update':
      if (typeof cliArgs['frozen-lockfile'] !== 'undefined') {
        throw new PnpmError('CONFIG_BAD_OPTION', 'The "frozen-lockfile" option cannot be used with the "update" command')
      }
      if (typeof cliArgs['prefer-frozen-lockfile'] !== 'undefined') {
        throw new PnpmError('CONFIG_BAD_OPTION', 'The "prefer-frozen-lockfile" option cannot be used with the "update" command')
      }
      break
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
  } catch (err) {} // tslint:disable-line:no-empty

  const workspacePrefix = cliArgs['global'] // tslint:disable-line
    ? null
    : (
      await findWorkspacePrefix(cliArgs['prefix'] || process.cwd()) || null
    )
  const npmConfig = loadNpmConf(cliArgs, types, {
    'bail': true,
    'depth': (command[0] === 'list' || command[1] === 'list') ? 0 : Infinity,
    'fetch-retries': 2,
    'fetch-retry-factor': 10,
    'fetch-retry-maxtimeout': 60000,
    'fetch-retry-mintimeout': 10000,
    'globalconfig': npmDefaults.globalconfig,
    'hoist': true,
    'hoist-pattern': '*',
    'ignore-workspace-root-check': false,
    'latest': false,
    'link-workspace-packages': true,
    'lock': true,
    'package-lock': npmDefaults['package-lock'],
    'pending': false,
    'prefix': npmDefaults.prefix,
    'registry': npmDefaults.registry,
    'resolution-strategy': 'fast',
    'save-peer': false,
    'shamefully-hoist': false,
    'shared-workspace-shrinkwrap': true,
    'shrinkwrap': npmDefaults.shrinkwrap,
    'sort': true,
    'strict-peer-dependencies': false,
    'unsafe-perm': npmDefaults['unsafe-perm'],
    'use-beta-cli': false,
    'userconfig': npmDefaults.userconfig,
    'workspace-concurrency': 4,
    'workspace-prefix': workspacePrefix,
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
    default: normalizeRegistry(pnpmConfig.registry || 'https://registry.npmjs.org/'),
    ...getScopeRegistries(pnpmConfig.rawNpmConfig),
  }
  const npmGlobalPrefix: string = pnpmConfig.rawNpmConfig['pnpm-prefix'] ||
    (
      process.platform !== 'win32'
        ? npmConfig.globalPrefix
        : findBestGlobalPrefixOnWindows(npmConfig.globalPrefix, process.env)
    )
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

  pnpmConfig.localPrefix = (cliArgs['prefix'] ? path.resolve(cliArgs['prefix']) : npmConfig.localPrefix) // tslint:disable-line
  if (pnpmConfig.global) {
    pnpmConfig.prefix = path.join(pnpmConfig.globalPrefix, LAYOUT_VERSION.toString())
    pnpmConfig.bin = pnpmConfig.globalBin
    pnpmConfig.allowNew = true
    pnpmConfig.ignoreCurrentPrefs = true
    pnpmConfig.saveProd = true
    pnpmConfig.saveDev = false
    pnpmConfig.saveOptional = false
    if (pnpmConfig.independentLeaves) {
      if (opts.cliArgs['independent-leaves']) {
        throw new PnpmError('CONFIG_CONFLICT_INDEPENDENT_LEAVES_WITH_GLOBAL',
          'Configuration conflict. "independent-leaves" may not be used with "global"')
      }
      pnpmConfig.independentLeaves = false
    }
    if (pnpmConfig.hoistPattern !== '*') {
      if (opts.cliArgs['hoist-pattern']) {
        throw new PnpmError('CONFIG_CONFLICT_HOIST_PATTERN_WITH_GLOBAL',
          'Configuration conflict. "hoist-pattern" may not be used with "global"')
      }
      pnpmConfig.independentLeaves = false
    }
    if (pnpmConfig.linkWorkspacePackages) {
      if (opts.cliArgs['link-workspace-packages']) {
        throw new PnpmError('CONFIG_CONFLICT_LINK_WORKSPACE_PACKAGES_WITH_GLOBAL',
          'Configuration conflict. "link-workspace-packages" may not be used with "global"')
      }
      pnpmConfig.linkWorkspacePackages = false
    }
    if (pnpmConfig.sharedWorkspaceLockfile) {
      if (opts.cliArgs['shared-workspace-lockfile'] || opts.cliArgs['shared-workspace-shrinkwrap']) {
        throw new PnpmError('CONFIG_CONFLICT_SHARED_WORKSPACE_LOCKFILE_WITH_GLOBAL',
          'Configuration conflict. "shared-workspace-lockfile" may not be used with "global"')
      }
      pnpmConfig.sharedWorkspaceLockfile = false
    }
    if (pnpmConfig.lockfileDirectory) {
      if (opts.cliArgs['lockfile-directory'] || opts.cliArgs['shrinkwrap-directory']) {
        throw new PnpmError('CONFIG_CONFLICT_LOCKFILE_DIRECTORY_WITH_GLOBAL',
          'Configuration conflict. "lockfile-directory" may not be used with "global"')
      }
      delete pnpmConfig.lockfileDirectory
    }
  } else {
    pnpmConfig.prefix = pnpmConfig.localPrefix
    pnpmConfig.bin = path.join(pnpmConfig.prefix, 'node_modules', '.bin')
  }
  if (opts.cliArgs['save-peer']) {
    if (opts.cliArgs['save-prod']) {
      throw new PnpmError('CONFIG_CONFLICT_PEER_CANNOT_BE_PROD_DEP', 'A package cannot be a peer dependency and a prod dependency at the same time')
    }
    if (opts.cliArgs['save-optional']) {
      throw new PnpmError('CONFIG_CONFLICT_PEER_CANNOT_BE_OPTIONAL_DEP',
        'A package cannot be a peer dependency and an optional dependency at the same time')
    }
    pnpmConfig.saveDev = true
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
  pnpmConfig.include = {
    dependencies: pnpmConfig.production !== false,
    devDependencies: pnpmConfig.development !== false,
    optionalDependencies: pnpmConfig.optional !== false,
  }

  if (typeof pnpmConfig.filter === 'string') {
    pnpmConfig.filter = (pnpmConfig.filter as string).split(' ')
  }

  pnpmConfig.sideEffectsCacheRead = pnpmConfig.sideEffectsCache || pnpmConfig.sideEffectsCacheReadonly
  pnpmConfig.sideEffectsCacheWrite = pnpmConfig.sideEffectsCache

  if (!pnpmConfig.ignoreScripts && pnpmConfig.workspacePrefix) {
    pnpmConfig.extraBinPaths = [path.join(pnpmConfig.workspacePrefix, 'node_modules', '.bin')]
  } else {
    pnpmConfig.extraBinPaths = []
  }
  if (pnpmConfig['shamefullyFlatten']) {
    warnings.push('The "shamefully-flatten" setting is deprecated. Use "shamefully-hoist", "hoist" or "hoist-pattern" instead. Since v4, hoisting is on by default for all dependencies.')
    pnpmConfig.hoistPattern = '*'
    pnpmConfig.shamefullyHoist = true
  }
  if (pnpmConfig['hoist'] === false) {
    delete pnpmConfig.hoistPattern
  }

  return { configs: pnpmConfig, warnings }
}

export async function findWorkspacePrefix (prefix: string) {
  const workspaceManifestLocation = await findUp(WORKSPACE_MANIFEST_FILENAME, { cwd: prefix })
  return workspaceManifestLocation && path.dirname(workspaceManifestLocation)
}
