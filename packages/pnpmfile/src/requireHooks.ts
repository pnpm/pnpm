import path from 'path'
import { hookLogger } from '@pnpm/core-loggers'
import pathAbsolute from 'path-absolute'
import type { Lockfile } from '@pnpm/lockfile-types'
import type { Log } from '@pnpm/core-loggers'
import { ImportIndexedPackage } from '@pnpm/store-controller-types'
import requirePnpmfile from './requirePnpmfile'

interface HookContext {
  log: (message: string) => void
}

interface Hooks {
  // eslint-disable-next-line
  readPackage?: (pkg: any, context: HookContext) => any
  afterAllResolved?: (lockfile: Lockfile, context: HookContext) => Lockfile | Promise<Lockfile>
  filterLog?: (log: Log) => boolean
  importPackage?: ImportIndexedPackage
}

// eslint-disable-next-line
type Cook<T extends (...args: any[]) => any> = (
  arg: Parameters<T>[0],
  // eslint-disable-next-line
  ...otherArgs: any[]
) => ReturnType<T>

export interface CookedHooks {
  readPackage?: Cook<Required<Hooks>['readPackage']>
  afterAllResolved?: Cook<Required<Hooks>['afterAllResolved']>
  filterLog?: Cook<Required<Hooks>['filterLog']>
  importPackage?: ImportIndexedPackage
}

export default function requireHooks (
  prefix: string,
  opts: {
    globalPnpmfile?: string
    pnpmfile?: string
  }
): CookedHooks {
  const globalPnpmfile = opts.globalPnpmfile && requirePnpmfile(pathAbsolute(opts.globalPnpmfile, prefix), prefix)
  let globalHooks: Hooks = globalPnpmfile?.hooks

  const pnpmFile = opts.pnpmfile && requirePnpmfile(pathAbsolute(opts.pnpmfile, prefix), prefix) ||
    requirePnpmfile(path.join(prefix, '.pnpmfile.cjs'), prefix)
  let hooks: Hooks = pnpmFile?.hooks

  if (!globalHooks && !hooks) return {}
  globalHooks = globalHooks || {}
  hooks = hooks || {}
  const cookedHooks: CookedHooks = {}
  for (const hookName of ['readPackage', 'afterAllResolved']) {
    if (globalHooks[hookName] && hooks[hookName]) {
      const globalHookContext = createReadPackageHookContext(globalPnpmfile.filename, prefix, hookName)
      const localHookContext = createReadPackageHookContext(pnpmFile.filename, prefix, hookName)
      // the `arg` is a package manifest in case of readPackage() and a lockfile object in case of afterAllResolved()
      cookedHooks[hookName] = async (arg: object) => {
        return hooks[hookName](
          await globalHooks[hookName](arg, globalHookContext),
          localHookContext
        )
      }
    } else if (globalHooks[hookName]) {
      const globalHook = globalHooks[hookName]
      const context = createReadPackageHookContext(globalPnpmfile.filename, prefix, hookName)
      cookedHooks[hookName] = (pkg: object) => globalHook(pkg, context)
    } else if (hooks[hookName]) {
      const hook = hooks[hookName]
      const context = createReadPackageHookContext(pnpmFile.filename, prefix, hookName)
      cookedHooks[hookName] = (pkg: object) => hook(pkg, context)
    }
  }
  const globalFilterLog = globalHooks.filterLog
  const filterLog = hooks.filterLog
  if (globalFilterLog != null && filterLog != null) {
    cookedHooks.filterLog = (log: Log) => globalFilterLog(log) && filterLog(log)
  } else {
    cookedHooks.filterLog = globalFilterLog ?? filterLog
  }
  cookedHooks.importPackage = hooks.importPackage ?? globalHooks.importPackage
  return cookedHooks
}

function createReadPackageHookContext (calledFrom: string, prefix: string, hook: string): HookContext {
  return {
    log: (message: string) => hookLogger.debug({
      from: calledFrom,
      hook,
      message,
      prefix,
    }),
  }
}
