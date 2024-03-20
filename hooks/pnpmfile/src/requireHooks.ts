import path from 'node:path'

import { hookLogger } from '@pnpm/core-loggers'
import pathAbsolute from 'path-absolute'

import { requirePnpmfile } from './requirePnpmfile'
import { CookedHooks, HookContext, Lockfile, PreResolutionHookContext, PreResolutionHookLogger } from '@pnpm/types'

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
  // @ts-ignore
  let globalHooks: Hooks = globalPnpmfile?.hooks

  const pnpmFile =
    (opts.pnpmfile &&
      await requirePnpmfile(pathAbsolute(opts.pnpmfile, prefix), prefix)) ||
    await requirePnpmfile(path.join(prefix, '.pnpmfile.cjs'), prefix)
  // @ts-ignore
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
        // @ts-ignore
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
        // @ts-ignore
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
