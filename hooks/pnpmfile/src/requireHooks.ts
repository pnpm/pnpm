import path from 'path'
import type { PreResolutionHook, PreResolutionHookContext, PreResolutionHookLogger } from '@pnpm/core'
import { hookLogger } from '@pnpm/core-loggers'
import pathAbsolute from 'path-absolute'
import type { Lockfile } from '@pnpm/lockfile-types'
import type { Log } from '@pnpm/core-loggers'
import type { CustomFetchers } from '@pnpm/fetcher-base'
import { type ImportIndexedPackage } from '@pnpm/store-controller-types'
import { requirePnpmfile } from './requirePnpmfile'

interface HookContext {
  log: (message: string) => void
}

interface Hooks {
  // eslint-disable-next-line
  readPackage?: (pkg: any, context: HookContext) => any
  preResolution?: PreResolutionHook
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
  readPackage?: Array<Cook<Required<Hooks>['readPackage']>>
  preResolution?: Cook<Required<Hooks>['preResolution']>
  afterAllResolved?: Array<Cook<Required<Hooks>['afterAllResolved']>>
  filterLog?: Array<Cook<Required<Hooks>['filterLog']>>
  importPackage?: ImportIndexedPackage
  fetchers?: CustomFetchers
}

export function requireHooks (
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

  if (!globalHooks && !hooks) return { afterAllResolved: [], filterLog: [], readPackage: [] }
  globalHooks = globalHooks || {}
  hooks = hooks || {}
  const cookedHooks: CookedHooks & Required<Pick<CookedHooks, 'filterLog'>> = {
    afterAllResolved: [],
    filterLog: [],
    readPackage: [],
  }
  for (const hookName of ['readPackage', 'afterAllResolved'] as const) {
    if (globalHooks[hookName]) {
      const globalHook = globalHooks[hookName]
      const context = createReadPackageHookContext(globalPnpmfile.filename, prefix, hookName)
      cookedHooks[hookName]!.push((pkg: object) => globalHook!(pkg as any, context)) // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    if (hooks[hookName]) {
      const hook = hooks[hookName]
      const context = createReadPackageHookContext(pnpmFile.filename, prefix, hookName)
      cookedHooks[hookName]!.push((pkg: object) => hook!(pkg as any, context)) // eslint-disable-line @typescript-eslint/no-explicit-any
    }
  }
  if (globalHooks.filterLog != null) {
    cookedHooks.filterLog.push(globalHooks.filterLog)
  }
  if (hooks.filterLog != null) {
    cookedHooks.filterLog.push(hooks.filterLog)
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
    log: (message: string) => {
      hookLogger.debug({
        from: calledFrom,
        hook,
        message,
        prefix,
      })
    },
  }
}

function createPreResolutionHookLogger (prefix: string): PreResolutionHookLogger {
  const hook = 'preResolution'

  return {
    info: (message: string) => hookLogger.info({ message, prefix, hook } as any), // eslint-disable-line
    warn: (message: string) => hookLogger.warn({ message, prefix, hook } as any), // eslint-disable-line
  }
}
