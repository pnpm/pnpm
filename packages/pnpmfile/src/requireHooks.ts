import { hookLogger } from '@pnpm/core-loggers'
import logger from '@pnpm/logger'
import requirePnpmfile from './requirePnpmfile'
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')

export default function requireHooks (
  prefix: string,
  opts: {
    globalPnpmfile?: string
    pnpmfile?: string
  }
) {
  const globalPnpmfile = opts.globalPnpmfile && requirePnpmfile(pathAbsolute(opts.globalPnpmfile, prefix), prefix)
  let globalHooks = globalPnpmfile?.hooks

  const pnpmFile = opts.pnpmfile && requirePnpmfile(pathAbsolute(opts.pnpmfile, prefix), prefix) ||
    requirePnpmfile(path.join(prefix, 'pnpmfile.js'), prefix)
  let hooks = pnpmFile?.hooks

  if (!globalHooks && !hooks) return {}
  globalHooks = globalHooks || {}
  hooks = hooks || {}
  const cookedHooks = {}
  if (globalHooks.readPackage || hooks.readPackage) {
    logger.info({
      message: 'readPackage hook is declared. Manifests of dependencies might get overridden',
      prefix,
    })
  }
  for (const hookName of ['readPackage', 'afterAllResolved']) {
    if (globalHooks[hookName] && hooks[hookName]) {
      const globalHookContext = createReadPackageHookContext(globalPnpmfile.filename, prefix, hookName)
      const localHookContext = createReadPackageHookContext(pnpmFile.filename, prefix, hookName)
      // the `arg` is a package manifest in case of readPackage() and a lockfile object in case of afterAllResolved()
      cookedHooks[hookName] = (arg: object) => {
        return hooks[hookName](
          globalHooks[hookName](arg, globalHookContext),
          localHookContext
        )
      }
    } else if (globalHooks[hookName]) {
      cookedHooks[hookName] = R.partialRight(globalHooks[hookName], [createReadPackageHookContext(globalPnpmfile.filename, prefix, hookName)])
    } else if (hooks[hookName]) {
      cookedHooks[hookName] = R.partialRight(hooks[hookName], [createReadPackageHookContext(pnpmFile.filename, prefix, hookName)])
    }
  }
  return cookedHooks
}

function createReadPackageHookContext (calledFrom: string, prefix: string, hook: string) {
  return {
    log: (message: string) => hookLogger.debug({
      from: calledFrom,
      hook,
      message,
      prefix,
    }),
  }
}
