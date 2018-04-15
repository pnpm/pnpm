import logger from '@pnpm/logger'
import {PackageManifest} from '@pnpm/types'
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')
import requirePnpmfile from './requirePnpmfile'

export default function requireHooks (
  prefix: string,
  opts: {
    globalPnpmfile?: string,
    pnpmfile?: string,
  },
) {
  const globalPnpmfile = opts.globalPnpmfile && requirePnpmfile(pathAbsolute(opts.globalPnpmfile, prefix))
  let globalHooks = globalPnpmfile && globalPnpmfile.hooks

  const pnpmFile = opts.pnpmfile && requirePnpmfile(pathAbsolute(opts.pnpmfile, prefix))
    || requirePnpmfile(path.join(prefix, 'pnpmfile.js'))
  let hooks = pnpmFile && pnpmFile.hooks

  if (!globalHooks && !hooks) return {}
  globalHooks = globalHooks || {}
  hooks = hooks || {}
  if (globalHooks.readPackage || hooks.readPackage) {
    logger.info('readPackage hook is declared. Manifests of dependencies might get overridden')
    if (globalHooks.readPackage && hooks.readPackage) {
      const globalHookContext = createReadPackageHookContext(globalPnpmfile.filename, prefix)
      const localHookContext = createReadPackageHookContext(pnpmFile.filename, prefix)
      return {
        readPackage: (pkg: PackageManifest) => {
          return hooks.readPackage(
            globalHooks.readPackage(pkg, globalHookContext),
            localHookContext,
          )
        },
      }
    }
    if (globalHooks.readPackage) {
      return {
        readPackage: R.partialRight(globalHooks.readPackage, [createReadPackageHookContext(globalPnpmfile.filename, prefix)]),
      }
    }
    return {
      readPackage: R.partialRight(hooks.readPackage, [createReadPackageHookContext(pnpmFile.filename, prefix)]),
    }
  }
  return {}
}

function createReadPackageHookContext (calledFrom: string, prefix: string) {
  const readPackageHookLogger = logger('hook')
  return {
    log: (message: string) => readPackageHookLogger.debug({
      from: calledFrom,
      hook: 'readPackage',
      message,
      prefix,
    }),
  }
}
