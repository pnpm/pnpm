import path from 'path'
import type { PreResolutioneHook, PreResolutionHookContext, PreResolutionHookLogger } from '@pnpm/core'
import { hookLogger } from '@pnpm/core-loggers'
import pathAbsolute from 'path-absolute'
import type { Lockfile } from '@pnpm/lockfile-types'
import type { Log } from '@pnpm/core-loggers'
import type { CustomFetchers } from '@pnpm/fetcher-base'
import { ImportIndexedPackage } from '@pnpm/store-controller-types'
import requirePnpmfile from './requirePnpmfile'

interface HookContext {
  log: (message: string) => void
}

interface Hooks {
  // eslint-disable-next-line
  readPackage?: (pkg: any, context: HookContext) => any
  preResolution?: PreResolutioneHook
  afterAllResolved?: (lockfile: Lockfile, context: HookContext) => Lockfile | Promise<Lockfile>
  filterLog?: (log: Log) => boolean
  importPackage?: ImportIndexedPackage
  fetchers?: CustomFetchers
}

// eslint-disable-next-line
type Cook<T extends (...args: any[]) => any> = (
  arg: Parameters<T>[0],
  // eslint-disable-next-line
  ...otherArgs: any[]
) => ReturnType<T>

export interface CookedHooks {
  readPackage?: Cook<Required<Hooks>['readPackage']>
  preResolution?: Cook<Required<Hooks>['preResolution']>
  afterAllResolved?: Cook<Required<Hooks>['afterAllResolved']>
  filterLog?: Cook<Required<Hooks>['filterLog']>
  importPackage?: ImportIndexedPackage
  fetchers?: CustomFetchers
}

export default function requireHooks (
  prefix: string,
  opts: {
    globalPnpmfile?: string
    pnpmfile?: string
    hooks?: Hooks
  }
): CookedHooks {
  const globalPnpmfile = opts.globalPnpmfile && requirePnpmfile(pathAbsolute(opts.globalPnpmfile, prefix), prefix)
  let globalHooks: Hooks = globalPnpmfile?.hooks

  const pnpmFile = opts.pnpmfile && requirePnpmfile(pathAbsolute(opts.pnpmfile, prefix), prefix) ||
    requirePnpmfile(path.join(prefix, '.pnpmfile.cjs'), prefix)
  let hooks: Hooks = pnpmFile?.hooks
  let optsHooks = opts?.hooks as Hooks

  if (!globalHooks && !hooks && !optsHooks) return {}
  globalHooks = globalHooks || {}
  hooks = hooks || {}
  optsHooks = optsHooks || {}
  const cookedHooks: CookedHooks = {}
  for (const hookName of ['readPackage', 'afterAllResolved']) {
    // eslint-disable-next-line
    const hookStack: Array<(arg: object) => Promise<any>> = []
    if (globalHooks[hookName]) {
      const globalHookContext = createReadPackageHookContext(globalPnpmfile.filename, prefix, hookName)
      // the `arg` is a package manifest in case of readPackage() and a lockfile object in case of afterAllResolved()
      hookStack.push(async (arg: object) => {
        return globalHooks[hookName](arg, globalHookContext)
      })
    }
    if (hooks[hookName]) {
      const localHookContext = createReadPackageHookContext(pnpmFile.filename, prefix, hookName)
      hookStack.push(async (arg: object) => {
        return hooks[hookName](arg, localHookContext)
      })
    }
    if (optsHooks[hookName]) {
      const optsHookContext = createReadPackageHookContext('opts', prefix, hookName)
      hookStack.push(async (arg: object) => {
        return optsHooks[hookName](arg, optsHookContext)
      })
    }
    if (hookStack.length === 3) {
      cookedHooks[hookName] = async (arg: object) => {
        return hookStack[2](
          await hookStack[1](
            await hookStack[0](arg)
          )
        )
      }
    } else if (hookStack.length === 2) {
      cookedHooks[hookName] = async (arg: object) => {
        return hookStack[1](
          await hookStack[0](arg)
        )
      }
    } else if (hookStack.length === 1) {
      cookedHooks[hookName] = hookStack[0]
    }
  }
  const filterLogStack: Array<(log: Log) => boolean> = []
  if (globalHooks.filterLog) {
    filterLogStack.push(globalHooks.filterLog)
  }
  if (hooks.filterLog) {
    filterLogStack.push(hooks.filterLog)
  }
  if (optsHooks.filterLog) {
    filterLogStack.push(optsHooks.filterLog)
  }
  if (filterLogStack.length === 3) {
    cookedHooks.filterLog = (log: Log) => filterLogStack[0](log) && filterLogStack[1](log) && filterLogStack[2](log)
  } if (filterLogStack.length === 2) {
    cookedHooks.filterLog = (log: Log) => filterLogStack[0](log) && filterLogStack[1](log)
  } if (filterLogStack.length === 1) {
    cookedHooks.filterLog = filterLogStack[0]
  }

  // `importPackage`, `preResolution` and `fetchers` can only be defined via a global pnpmfile

  cookedHooks.importPackage = globalHooks.importPackage

  const preResolutionHook = globalHooks.preResolution

  cookedHooks.preResolution = preResolutionHook
    ? (ctx: PreResolutionHookContext) => preResolutionHook(ctx, createPreResolutionHookLogger(prefix))
    : undefined

  cookedHooks.fetchers = globalHooks.fetchers

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

function createPreResolutionHookLogger (prefix: string): PreResolutionHookLogger {
  const hook = 'preResolution'

  return {
    info: (message: string) => hookLogger.info({ message, prefix, hook } as any), // eslint-disable-line
    warn: (message: string) => hookLogger.warn({ message, prefix, hook } as any), // eslint-disable-line
  }
}
