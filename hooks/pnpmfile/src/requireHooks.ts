import type { PreResolutionHookContext, PreResolutionHookLogger } from '@pnpm/hooks.types'
import { hookLogger } from '@pnpm/core-loggers'
import { createHashFromFile } from '@pnpm/crypto.hash'
import pathAbsolute from 'path-absolute'
import type { CustomFetchers } from '@pnpm/fetcher-base'
import { type ImportIndexedPackageAsync } from '@pnpm/store-controller-types'
import { getPnpmfilePath } from './getPnpmfilePath'
import { requirePnpmfile } from './requirePnpmfile'
import { type HookContext, type Hooks } from './Hooks'

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
  importPackage?: ImportIndexedPackageAsync
  fetchers?: CustomFetchers
  calculatePnpmfileChecksum?: () => Promise<string | undefined>
}

export function requireHooks (
  prefix: string,
  opts: {
    globalPnpmfile?: string
    pnpmfile?: string
  }
): CookedHooks {
  const globalPnpmfile = opts.globalPnpmfile ? requirePnpmfile(pathAbsolute(opts.globalPnpmfile, prefix), prefix) : undefined
  let globalHooks: Hooks | undefined = globalPnpmfile?.hooks

  const pnpmfilePath = getPnpmfilePath(prefix, opts.pnpmfile)
  const pnpmFile = requirePnpmfile(pnpmfilePath, prefix)
  let hooks: Hooks | undefined = pnpmFile?.hooks

  if (!globalHooks && !hooks) return { afterAllResolved: [], filterLog: [], readPackage: [] }
  const calculatePnpmfileChecksum = hooks ? () => createHashFromFile(pnpmfilePath) : undefined
  globalHooks = globalHooks ?? {}
  hooks = hooks ?? {}
  const cookedHooks: CookedHooks & Required<Pick<CookedHooks, 'filterLog'>> = {
    afterAllResolved: [],
    filterLog: [],
    readPackage: [],
    calculatePnpmfileChecksum,
  }
  for (const hookName of ['readPackage', 'afterAllResolved'] as const) {
    if (globalHooks[hookName]) {
      const globalHook = globalHooks[hookName]
      const context = createReadPackageHookContext(globalPnpmfile!.filename, prefix, hookName)
      cookedHooks[hookName]!.push((pkg: object) => globalHook!(pkg as any, context)) // eslint-disable-line @typescript-eslint/no-explicit-any
    }
    if (hooks[hookName]) {
      const hook = hooks[hookName]
      const context = createReadPackageHookContext(pnpmFile!.filename, prefix, hookName)
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
