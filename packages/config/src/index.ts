import loadNpmConf = require('@zkochan/npm-conf')
import npmTypes = require('@zkochan/npm-conf/lib/types')
import camelcase = require('camelcase')
import path = require('path')
import whichcb = require('which')

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
  'frozen-shrinkwrap': Boolean,
  'global-path': path,
  'global-pnpmfile': String,
  'ignore-pnpmfile': Boolean,
  'ignore-stop-requests': Boolean,
  'ignore-upload-requests': Boolean,
  'independent-leaves': Boolean,
  'lock': Boolean,
  'lock-stale-duration': Number,
  'network-concurrency': Number,
  'offline': Boolean,
  'package-import-method': ['auto', 'hardlink', 'reflink', 'copy'],
  'pending': Boolean,
  'pnpmfile': String,
  'port': Number,
  'prefer-frozen-shrinkwrap': Boolean,
  'prefer-offline': Boolean,
  'production': [null, true],
  'protocol': ['auto', 'tcp', 'ipc'],
  'reporter': String,
  'scope': String,
  'shamefully-flatten': Boolean,
  'shrinkwrap-only': Boolean,
  'side-effects-cache': Boolean,
  'side-effects-cache-readonly': Boolean,
  'store': path,
  'use-running-store-server': Boolean,
  'use-store-server': Boolean,
  'verify-store-integrity': Boolean,
}, npmTypes.types)

export default async (
  opts: {
    cliArgs: object,
    packageManager: {
      name: string,
      version: string,
    },
  },
) => {
  const packageManager = opts && opts.packageManager || {name: 'pnpm', version: 'undefined'}
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

  const npmConfig = loadNpmConf(null, types, {
    'bail': true,
    'globalconfig': npmDefaults.globalconfig,
    'package-lock': npmDefaults['package-lock'],
    'prefix': npmDefaults.prefix,
    'registry': npmDefaults.registry,
    'shrinkwrap': npmDefaults.shrinkwrap,
    'unsafe-perm': npmDefaults['unsafe-perm'],
    'userconfig': npmDefaults.userconfig,
  })

  process.execPath = originalExecPath

  if (!cliArgs['user-agent']) {
    cliArgs['user-agent'] = `${packageManager.name}/${packageManager.version} npm/? node/${process.version} ${process.platform} ${process.arch}`
  }
  const pnpmConfig: any = Object.keys(types) // tslint:disable-line
    .reduce((acc, configKey) => {
      acc[camelcase(configKey)] = typeof cliArgs[configKey] !== 'undefined'
        ? cliArgs[configKey]
        : npmConfig.get(configKey)
      return acc
    }, {})
  pnpmConfig.rawNpmConfig = Object.assign.apply(Object, npmConfig.list.reverse().concat([cliArgs]))
  const npmGlobalPrefix = process.env.APPDATA
    ? path.join(process.env.APPDATA, 'npm')
    : npmConfig.globalPrefix
  pnpmConfig.globalBin = process.platform === 'win32'
    ? npmGlobalPrefix
    : path.resolve(npmGlobalPrefix, 'bin')
  pnpmConfig.globalPrefix = path.join(npmGlobalPrefix, 'pnpm-global')
  pnpmConfig.prefix = pnpmConfig.global
    ? pnpmConfig.globalPrefix
    : (cliArgs['prefix'] ? path.resolve(cliArgs['prefix']) : npmConfig.localPrefix) // tslint:disable-line
  pnpmConfig.bin = pnpmConfig.global
    ? pnpmConfig.globalBin
    : path.join(pnpmConfig.prefix, 'node_modules', '.bin')
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

  if (!pnpmConfig.packageLock && pnpmConfig.shrinkwrap) {
    pnpmConfig.shrinkwrap = false
  }

  return pnpmConfig
}
