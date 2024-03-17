import path from 'node:path'
import type {
  PreResolutionHook,
  PreResolutionHookContext,
  PreResolutionHookLogger,
} from '@pnpm/hooks.types'
import { hookLogger } from '@pnpm/core-loggers'
import pathAbsolute from 'path-absolute'
import type { Lockfile } from '@pnpm/lockfile-types'
import type { Log } from '@pnpm/core-loggers'
import type { CustomFetchers } from '@pnpm/fetcher-base'
import type { ImportIndexedPackageAsync } from '@pnpm/store-controller-types'
import { requirePnpmfile } from './requirePnpmfile'

interface HookContext {
  log: (message: string) => void
}

interface Hooks {

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  readPackage?: ((pkg: any, context: HookContext) => any) | undefined
  preResolution?: PreResolutionHook | undefined
  afterAllResolved?: ((
    lockfile: Lockfile,
    context: HookContext
  ) => Lockfile | Promise<Lockfile>) | undefined
  filterLog?: ((log: Log) => boolean) | undefined
  importPackage?: ImportIndexedPackageAsync | undefined
  fetchers?: CustomFetchers | undefined
}

// eslint-disable-next-line
type Cook<T extends (...args: any[]) => any> = (
  arg: Parameters<T>[0],
  // eslint-disable-next-line
  ...otherArgs: any[]
) => ReturnType<T>

export interface CookedHooks {
  readPackage?: Array<Cook<Required<Hooks>['readPackage']>> | undefined
  preResolution?: Cook<Required<Hooks>['preResolution']> | undefined
  afterAllResolved?: Array<Cook<Required<Hooks>['afterAllResolved']>> | undefined
  filterLog?: Array<Cook<Required<Hooks>['filterLog']>> | undefined
  importPackage?: ImportIndexedPackageAsync | undefined
  fetchers?: CustomFetchers | undefined
}

export async function requireHooks(
  prefix: string,
  opts: {
    globalPnpmfile?: string | undefined
    pnpmfile?: string | undefined
  }
): Promise<CookedHooks> {
  const globalPnpmfile =
    typeof opts.globalPnpmfile === 'string' &&
    await requirePnpmfile(pathAbsolute(opts.globalPnpmfile, prefix), prefix)

  let globalHooks: Hooks = globalPnpmfile?.hooks

  const pnpmFile =
    (opts.pnpmfile &&
      await requirePnpmfile(pathAbsolute(opts.pnpmfile, prefix), prefix)) ||
    await requirePnpmfile(path.join(prefix, '.pnpmfile.cjs'), prefix)

  let hooks: Hooks = pnpmFile?.hooks

  if (!globalHooks && !hooks)
    return { afterAllResolved: [], filterLog: [], readPackage: [] }
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
      const context = createReadPackageHookContext(
        globalPnpmfile?.filename,
        prefix,
        hookName
      )
      cookedHooks[hookName]?.push((pkg: Lockfile) => {
        return globalHook?.(pkg, context);
      }
      )
    }
    if (hooks[hookName]) {
      const hook = hooks[hookName]
      const context = createReadPackageHookContext(
        pnpmFile?.filename,
        prefix,
        hookName
      )
      cookedHooks[hookName]?.push((pkg: Lockfile): Lockfile | Promise<Lockfile> => {
        return hook?.(pkg, context);
      })
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
    ? (ctx: PreResolutionHookContext) =>
      preResolutionHook(ctx, createPreResolutionHookLogger(prefix))
    : undefined

  cookedHooks.fetchers = globalHooks.fetchers

  return cookedHooks
}

function createReadPackageHookContext(
  calledFrom: string,
  prefix: string,
  hook: string
): HookContext {
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

function createPreResolutionHookLogger(
  prefix: string
): PreResolutionHookLogger {
  const hook = 'preResolution'

  return {
    info: (message: string) => hookLogger.info({ message, prefix, hook } as any), // eslint-disable-line
    warn: (message: string) => hookLogger.warn({ message, prefix, hook } as any), // eslint-disable-line
  }
}
